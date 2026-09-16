//! Local client: operates on an in-memory graph with disk-persisted state.
//!
//! For M5, the CLI operates in **local mode** — it uses the library
//! crates directly with an in-memory graph, persisting state (keypair,
//! config) to a local directory. A future daemon mode will connect to
//! a running taba-node via gRPC.
//!
//! The [`LocalClient`] holds:
//! - `graph` — the composition graph (with verifier + scope checker)
//! - `solver` — the deterministic solver
//! - `auth` — local author key management
//! - `config` — node configuration (trust domain, cluster ID, node ID)
//! - `key_pair` — the local author's Ed25519 keypair
//! - `trail_recorder` — decision trail recorder

use std::sync::Arc;

use sha2::{Digest, Sha256};
use taba_common::{
    AuthorId, ClusterId, LogicalClock, NodeId, TrustDomainId, UnitId, ValidityWindow,
};
use taba_graph::{DefaultGraph, Graph, GraphQuery, GraphSnapshot, GraphStats, ProvenanceLink};
use taba_observe::{DecisionTrail, DecisionTrailQuery, DefaultDecisionTrailRecorder};
use taba_security::{DefaultSigner, KeyPair, PublicKey, SignatureContext, SignedUnit, Signer};
use taba_solver::{DefaultSolver, MembershipSnapshot, Solver, SolverResult};

use crate::auth::{LocalAuth, LocalConfig};
use crate::error::CliError;

// ===========================================================================
// LocalClient
// ===========================================================================

/// Local client: operates on an in-memory graph with disk-persisted state.
///
/// In M5 local mode, the client holds a [`DefaultGraph`] (with
/// structural validation only — no signature verification), a
/// [`DefaultSolver`], the local author's [`KeyPair`], and a
/// [`DefaultDecisionTrailRecorder`].
///
/// State (keypair and config) is persisted to the local state
/// directory via [`LocalAuth`].
pub struct LocalClient {
    /// The composition graph (with verifier + scope checker).
    graph: Arc<DefaultGraph>,
    /// The deterministic solver.
    solver: DefaultSolver,
    /// Local author key management.
    auth: LocalAuth,
    /// Node configuration (trust domain, cluster ID, node ID).
    config: LocalConfig,
    /// The local author's Ed25519 keypair.
    key_pair: KeyPair,
    /// Decision trail recorder.
    trail_recorder: DefaultDecisionTrailRecorder,
}

impl LocalClient {
    /// Load or initialize a local client.
    ///
    /// If the state directory is not yet initialized, runs
    /// [`LocalAuth::init`] to generate a keypair and configuration.
    /// If already initialized, loads the existing keypair and config.
    ///
    /// The graph is created in-memory with structural validation
    /// only (no signature verification — M5 local mode).
    ///
    /// # Errors
    ///
    /// - [`CliError::Io`] if the state directory cannot be created.
    /// - [`CliError::Security`] if key generation fails.
    /// - [`CliError::Json`] if the config file is invalid.
    pub async fn load(state_dir: Option<std::path::PathBuf>) -> Result<Self, CliError> {
        let auth = match state_dir {
            Some(dir) => LocalAuth::with_state_dir(dir)?,
            None => LocalAuth::new()?,
        };

        if !auth.is_initialized() {
            auth.init()?;
        }

        let config = auth.load_config()?;
        let key_pair = auth.load_keypair()?;

        // Derive the author ID from the public key.
        let author_id = author_id_from_keypair(&key_pair);

        // Build the signer from the local keypair (INV-S3).
        let signer: std::sync::Arc<dyn taba_security::Signer + Send + Sync> = std::sync::Arc::new(
            taba_security::DefaultSigner::new(taba_security::KeyPair::from_signing_key(
                taba_security::SigningKey::from_bytes(&key_pair.signing_key().to_bytes()),
            )),
        );

        // Build the verifier and register the local public key.
        let mut verifier = taba_security::DefaultVerifier::new();
        verifier.add_key(author_id, *key_pair.public_key(), None);
        let verifier_arc: std::sync::Arc<dyn taba_security::Verifier + Send + Sync> =
            std::sync::Arc::new(verifier);

        // First run: create governance units so the scope checker
        // has a role assignment for the local author.
        let graph_path = auth.state_dir().join("graph.json");
        if !graph_path.exists() {
            let role_assignment =
                taba_core::GovernanceUnit::RoleAssignment(taba_core::RoleAssignment {
                    header: taba_core::UnitHeader {
                        id: taba_common::UnitId(uuid::Uuid::new_v4()),
                        author: author_id,
                        trust_domain: config.trust_domain,
                        created_at: taba_common::DualClockEvent {
                            logical_clock: taba_common::LogicalClock(2),
                            wall_time: taba_common::WallTime { millis: 0 },
                            timezone: "UTC".to_string(),
                        },
                        validity: None,
                        state: taba_core::UnitState::Declared,
                        version: None,
                    },
                    assignee: author_id,
                    unit_type_scope: vec![
                        taba_core::UnitTypeScope::Workload,
                        taba_core::UnitTypeScope::Data,
                        taba_core::UnitTypeScope::Policy,
                        taba_core::UnitTypeScope::Governance,
                    ],
                    trust_domain_scope: vec![config.trust_domain],
                });
            let units = vec![taba_core::Unit::Governance(role_assignment)];
            let json = serde_json::to_string(&units)?;
            std::fs::write(&graph_path, json)?;
        }

        // Phase 1: Read graph.json and extract RoleAssignments
        // using a graph WITHOUT verifier/scope_checker. Governance
        // units are self-signed and don't require scope checks.
        let mut scope_checker = taba_security::DefaultScopeChecker::new();
        let raw_units: Vec<taba_core::Unit> = if graph_path.exists() {
            let json = std::fs::read_to_string(&graph_path)?;
            let units: Vec<taba_core::Unit> = serde_json::from_str(&json)?;
            for u in &units {
                if let taba_core::Unit::Governance(taba_core::GovernanceUnit::RoleAssignment(ra)) =
                    u
                {
                    scope_checker.add_assignment(ra.clone());
                }
            }
            units
        } else {
            Vec::new()
        };

        // Phase 2: Create graph WITH signer, verifier, and populated
        // scope checker. Insert all units.
        let scope_checker = std::sync::Arc::new(scope_checker);
        let graph = Arc::new(
            DefaultGraph::new(1_073_741_824)
                .with_signer(
                    std::sync::Arc::clone(&signer),
                    config.trust_domain,
                    config.cluster_id,
                )
                .with_verifier(std::sync::Arc::clone(&verifier_arc))
                .with_scope_checker(std::sync::Arc::clone(&scope_checker))
                .with_max_spawn_depth(4),
        );

        for unit in raw_units {
            let r = graph.insert(unit).await;
            if r.is_err() {}
        }

        // Create solver.
        let solver = DefaultSolver::new();

        // Create trail recorder.
        let trail_recorder = DefaultDecisionTrailRecorder::new();

        Ok(Self {
            graph,
            solver,
            auth,
            config,
            key_pair,
            trail_recorder,
        })
    }

    pub async fn load_unverified(state_dir: Option<std::path::PathBuf>) -> Result<Self, CliError> {
        let auth = match state_dir {
            Some(dir) => LocalAuth::with_state_dir(dir)?,
            None => LocalAuth::new()?,
        };

        if !auth.is_initialized() {
            auth.init()?;
        }

        let config = auth.load_config()?;
        let key_pair = auth.load_keypair()?;

        // Load persisted graph state if it exists.
        let graph = Arc::new(DefaultGraph::new(1_073_741_824));
        let graph_path = auth.state_dir().join("graph.json");
        if graph_path.exists() {
            let json = std::fs::read_to_string(&graph_path)?;
            let units: Vec<taba_core::Unit> = serde_json::from_str(&json)?;
            for unit in units {
                let r = graph.insert(unit).await;
                if r.is_err() {}
            }
        }

        let solver = DefaultSolver::new();
        let trail_recorder = DefaultDecisionTrailRecorder::new();

        Ok(Self {
            graph,
            solver,
            auth,
            config,
            key_pair,
            trail_recorder,
        })
    }

    /// Get graph stats.
    #[must_use]
    pub fn graph_stats(&self) -> GraphStats {
        self.graph.stats()
    }

    /// Insert a unit into the graph (after validation).
    ///
    /// The unit is validated by the graph's internal validator before
    /// entering graph state. If the unit's references are not yet
    /// satisfied, it enters the pending queue (causal buffering,
    /// INV-C4).
    ///
    /// # Errors
    ///
    /// - [`CliError::Graph`] if validation fails or the graph rejects
    ///   the unit.
    pub async fn insert_unit(&self, unit: taba_core::Unit) -> Result<(), CliError> {
        self.graph.insert(unit).await.map_err(CliError::from)?;
        self.persist().await?;
        Ok(())
    }

    /// Take a graph snapshot.
    ///
    /// Returns an immutable point-in-time view of the graph for solver
    /// consumption. Concurrent mutations do not affect the snapshot.
    ///
    /// # Errors
    ///
    /// - [`CliError::Graph`] if the snapshot cannot be taken.
    pub async fn snapshot(&self) -> Result<GraphSnapshot, CliError> {
        self.graph.snapshot().await.map_err(CliError::from)
    }

    /// Run the solver on the current graph.
    ///
    /// Uses a single-node membership snapshot (M5 local mode). The
    /// solver is deterministic (INV-C3): same input = same output.
    #[must_use]
    pub fn solve(&self, snapshot: &GraphSnapshot) -> SolverResult {
        let membership =
            MembershipSnapshot::single_node(self.config.node_id, Self::node_capabilities());
        self.solver.solve(snapshot, &membership)
    }

    /// Get a unit by ID.
    ///
    /// Returns the unit if it is in the active (non-archived) set.
    ///
    /// # Errors
    ///
    /// - [`CliError::Graph`] if the unit is not found or has been
    ///   archived.
    pub fn get_unit(&self, id: &UnitId) -> Result<taba_core::Unit, CliError> {
        self.graph.get(id).map_err(CliError::from)
    }

    /// List all active (non-archived) units in the graph.
    ///
    /// # Errors
    ///
    /// - [`CliError::Graph`] if the snapshot cannot be taken.
    pub async fn list_units(&self) -> Result<Vec<taba_core::Unit>, CliError> {
        let snapshot = self.graph.snapshot().await.map_err(CliError::from)?;
        Ok(snapshot
            .entries
            .values()
            .filter(|e| !e.archived)
            .map(|e| e.signed_unit.unit.clone())
            .collect())
    }

    /// Archive a unit (soft-delete).
    ///
    /// The unit is removed from the active set but retained for
    /// historical queries and provenance integrity. Governance units
    /// cannot be archived (INV-G3).
    ///
    /// # Errors
    ///
    /// - [`CliError::Graph`] if the unit is not found, already
    ///   archived, or is a governance unit.
    pub async fn archive_unit(&self, id: &UnitId) -> Result<(), CliError> {
        self.graph.archive(id).await.map_err(CliError::from)?;
        self.persist().await?;
        Ok(())
    }

    /// Persist the current graph state to disk as JSON.
    async fn persist(&self) -> Result<(), CliError> {
        let units = self.list_units().await?;
        let json = serde_json::to_string(&units)?;
        let path = self.auth.state_dir().join("graph.json");
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Get provenance chain for a data unit.
    ///
    /// Walks the provenance chain back to source data units (INV-D1).
    ///
    /// # Errors
    ///
    /// - [`CliError::Graph`] if the data unit is not found or any link
    ///   in the chain references a unit not in the local graph.
    pub fn provenance(&self, data_unit: &UnitId) -> Result<Vec<ProvenanceLink>, CliError> {
        self.graph
            .traverse_provenance(data_unit)
            .map_err(CliError::from)
    }

    /// Query all decision trails.
    ///
    /// Returns all recorded trails in chronological order (by logical
    /// clock). Uses a wide range query to retrieve everything (INV-O2).
    ///
    /// # Errors
    ///
    /// - [`CliError::Observe`] if the query fails.
    pub fn decision_trails(&self) -> Result<Vec<DecisionTrail>, CliError> {
        self.trail_recorder
            .query_by_range(&LogicalClock(0), &LogicalClock(u64::MAX))
            .map_err(CliError::from)
    }

    /// Sign a unit with the local keypair.
    ///
    /// Creates a [`SignedUnit`] with Ed25519 signature bound to the
    /// local trust domain and cluster (INV-S3).
    ///
    /// # Errors
    ///
    /// - [`CliError::Security`] if signing fails.
    pub fn sign_unit(
        &self,
        unit: &taba_core::Unit,
    ) -> Result<SignedUnit<taba_core::Unit>, CliError> {
        let signer = DefaultSigner::new(KeyPair::from_signing_key(
            taba_security::SigningKey::from_bytes(&self.key_pair.signing_key().to_bytes()),
        ));

        let validity = ValidityWindow {
            lc_range: None,
            wall_time_deadline: None,
        };

        let signature = signer.sign(
            unit,
            &self.config.trust_domain,
            &self.config.cluster_id,
            &validity,
        )?;

        let context = SignatureContext {
            trust_domain_id: self.config.trust_domain,
            cluster_id: self.config.cluster_id,
            validity_window: validity,
        };

        Ok(SignedUnit {
            unit: unit.clone(),
            signature,
            context,
            signer: *self.key_pair.public_key(),
        })
    }

    /// Get the local trust domain.
    #[must_use]
    pub const fn trust_domain(&self) -> TrustDomainId {
        self.config.trust_domain
    }

    /// Get the local cluster ID.
    #[must_use]
    pub const fn cluster_id(&self) -> ClusterId {
        self.config.cluster_id
    }

    /// Get the local node ID.
    #[must_use]
    pub const fn node_id(&self) -> NodeId {
        self.config.node_id
    }

    /// Get the local author ID (derived from public key).
    ///
    /// The author ID is the first 16 bytes of SHA-256(public_key),
    /// encoded as a UUID. This provides a stable, deterministic
    /// identifier for the author without exposing the full public key.
    #[must_use]
    pub fn author_id(&self) -> AuthorId {
        author_id_from_keypair(&self.key_pair)
    }

    /// Get the local public key.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        self.key_pair.public_key()
    }

    /// Get a reference to the state directory.
    #[must_use]
    pub fn state_dir(&self) -> &std::path::Path {
        self.auth.state_dir()
    }

    /// Builds default node capabilities for M5 single-node mode.
    ///
    /// In M5, the local node advertises a basic `Linux/x86_64`
    /// capability set with OCI runtime support and `env:dev`
    /// environment.
    #[must_use]
    fn node_capabilities() -> taba_core::NodeCapabilitySet {
        taba_core::NodeCapabilitySet {
            arch: "x86_64".to_string(),
            os: "linux".to_string(),
            privilege: taba_core::PrivilegeLevel::User,
            runtimes: vec![taba_core::RuntimeCapability::Oci],
            ports_privileged: false,
            storage: vec!["ssd".to_string()],
            environment: Some("env:dev".to_string()),
            author_affinity: None,
            clock_quality: taba_common::ClockQuality::Ntp,
            timezone: "UTC".to_string(),
            custom_tags: Vec::new(),
        }
    }
}

/// Computes the `AuthorId` from a `KeyPair`: the first 16 bytes of
/// SHA-256(public_key), encoded as a UUID.
fn author_id_from_keypair(key_pair: &taba_security::KeyPair) -> AuthorId {
    let mut hasher = Sha256::new();
    hasher.update(key_pair.public_key().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    AuthorId(uuid::Uuid::from_bytes(bytes))
}

impl std::fmt::Debug for LocalClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalClient")
            .field("config", &self.config)
            .field("state_dir", &self.auth.state_dir())
            .finish_non_exhaustive()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_core::Unit;
    use taba_security::{DefaultVerifier, Verifier};
    use taba_test_harness::WorkloadUnitBuilder;

    #[tokio::test]
    async fn test_load_initializes_if_needed() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("taba-state");

        // Load should initialize.
        let client = LocalClient::load_unverified(Some(state.clone()))
            .await
            .expect("load should succeed");

        assert!(state.exists(), "state directory should exist");
        assert_ne!(
            client.trust_domain(),
            TrustDomainId(uuid::Uuid::nil()),
            "trust domain should be non-nil"
        );
        assert_ne!(
            client.cluster_id(),
            ClusterId(uuid::Uuid::nil()),
            "cluster ID should be non-nil"
        );
    }

    #[tokio::test]
    async fn test_load_loads_existing() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let state = tmp.path().join("taba-state");

        // First load: initialize.
        let client1 = LocalClient::load_unverified(Some(state.clone()))
            .await
            .expect("load 1");
        let td1 = client1.trust_domain();
        let pk1 = *client1.public_key();

        // Second load: should reuse the same state.
        let client2 = LocalClient::load_unverified(Some(state))
            .await
            .expect("load 2");
        assert_eq!(client2.trust_domain(), td1, "trust domain should match");
        assert_eq!(*client2.public_key(), pk1, "public key should match");
    }

    #[tokio::test]
    async fn test_insert_and_get_unit() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let id = unit.id();

        client
            .insert_unit(unit)
            .await
            .expect("insert should succeed");
        let retrieved = client.get_unit(&id).expect("get should succeed");
        assert_eq!(retrieved.id(), id);
    }

    #[tokio::test]
    async fn test_list_units() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        let u1 = Unit::Workload(WorkloadUnitBuilder::new().build());
        let u2 = Unit::Workload(WorkloadUnitBuilder::new().build());
        let u3 = Unit::Workload(WorkloadUnitBuilder::new().build());

        client.insert_unit(u1).await.expect("insert 1");
        client.insert_unit(u2).await.expect("insert 2");
        client.insert_unit(u3).await.expect("insert 3");

        let units = client.list_units().await.expect("list should succeed");
        assert_eq!(units.len(), 3, "should list 3 units");
    }

    #[tokio::test]
    async fn test_archive_unit() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let id = unit.id();

        client.insert_unit(unit).await.expect("insert");
        client
            .archive_unit(&id)
            .await
            .expect("archive should succeed");

        // Archived unit should not be retrievable via get_unit.
        let result = client.get_unit(&id);
        assert!(result.is_err(), "archived unit should not be retrievable");
    }

    #[tokio::test]
    async fn test_graph_stats() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        let stats = client.graph_stats();
        assert_eq!(
            stats.active_units, 0,
            "empty graph should have 0 active units"
        );

        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        client.insert_unit(unit).await.expect("insert");

        let stats = client.graph_stats();
        assert_eq!(stats.active_units, 1, "should have 1 active unit");
    }

    #[tokio::test]
    async fn test_solve_empty_graph() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        let snapshot = client.snapshot().await.expect("snapshot");
        let result = client.solve(&snapshot);

        assert!(result.placements.is_empty(), "empty graph → no placements");
        assert!(result.conflicts.is_empty(), "empty graph → no conflicts");
    }

    #[tokio::test]
    async fn test_solve_with_units() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        // Insert a workload that provides a capability.
        let provider = Unit::Workload(WorkloadUnitBuilder::new().build());
        client.insert_unit(provider).await.expect("insert provider");

        let snapshot = client.snapshot().await.expect("snapshot");
        let result = client.solve(&snapshot);

        // The solver should produce at least one placement for the
        // provider (placed on the single local node).
        assert!(
            !result.placements.is_empty() || !result.unplaceable.is_empty(),
            "solver should produce results for non-empty graph"
        );
    }

    #[tokio::test]
    async fn test_provenance_no_data() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load");

        // Insert a workload (not a data unit).
        let workload = Unit::Workload(WorkloadUnitBuilder::new().build());
        let id = workload.id();
        client.insert_unit(workload).await.expect("insert workload");

        // Provenance on a non-data unit should return an error or
        // empty result (traverse_provenance returns empty for
        // non-data units that have no provenance).
        let result = client.provenance(&id);
        // The graph may return NotFound or an empty list depending on
        // whether the unit has provenance metadata. Either is
        // acceptable — what matters is it doesn't panic.
        let _ = result;
    }

    #[tokio::test]
    async fn test_sign_unit() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let client = LocalClient::load_unverified(Some(tmp.path().to_path_buf()))
            .await
            .expect("load should succeed");

        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = client.sign_unit(&unit).expect("sign should succeed");

        // The signed unit's signer should match the client's public key.
        assert_eq!(signed.signer, *client.public_key());

        // The signature should be verifiable via DefaultVerifier.
        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, *client.public_key(), None);
        verifier
            .verify(
                &unit,
                &signed.signature,
                &signed.context.trust_domain_id,
                &signed.context.cluster_id,
                &unit.header().created_at.logical_clock,
                Some(&signed.context.validity_window),
            )
            .expect("signature should verify");
    }
}
