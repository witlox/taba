//! Core unit types: the fundamental taba primitive and all subtypes.
//!
//! A [`Unit`] is a self-describing, signed, typed entity carrying
//! capability declarations, behavioral contracts, and security
//! requirements. Every unit in the graph is signed by an author with
//! valid scope (INV-S3).
//!
//! The four unit kinds — Workload, Data, Policy, Governance — compose
//! through capability matching resolved by the solver. The six unit
//! states (Declared → Composed → Placed → Running → Draining →
//! Terminated) represent the lifecycle from authoring to teardown.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use taba_common::{
    AuthorId, DualClockEvent, LogicalClock, TrustDomainId, UnitId, ValidityWindow, Version,
    WallTime,
};

use crate::artifact::Artifact;
use crate::capability::Capability;
use crate::contract::{
    FailureSemantics, PlacementOnFailure, RecoveryRelationship, Scaling, StateRecovery, Tolerances,
    TrustDeclaration,
};
use crate::data::{
    Classification, ConsentScope, DataSchema, Provenance, RetentionPolicy, StorageRequirements,
};
use crate::delegation::SpawnContext;
use crate::health::HealthCheck;

// ===========================================================================
// Unit — the fundamental primitive
// ===========================================================================

/// The fundamental taba primitive.
///
/// A self-describing, signed, typed entity carrying capability
/// declarations, behavioral contracts, and security requirements.
/// Every unit in the graph is signed by an author with valid scope
/// (INV-S3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Unit {
    /// A compute process — runtime-agnostic (container, microVM, Wasm, native).
    Workload(WorkloadUnit),
    /// A dataset carrying schema, classification, provenance, and constraints.
    Data(DataUnit),
    /// Resolves a capability conflict between other units.
    Policy(PolicyUnit),
    /// Trust domain definitions, role assignments, or certifications.
    Governance(GovernanceUnit),
}

impl Unit {
    /// The header common to all unit types.
    #[must_use]
    pub const fn header(&self) -> &UnitHeader {
        match self {
            Self::Workload(w) => &w.header,
            Self::Data(d) => &d.header,
            Self::Policy(p) => &p.header,
            Self::Governance(g) => g.header(),
        }
    }

    /// Capabilities this unit requires from the environment.
    ///
    /// Only workload units declare needs. Data, policy, and governance
    /// units return an empty slice.
    #[must_use]
    pub fn needs(&self) -> &[Capability] {
        match self {
            Self::Workload(w) => &w.needs,
            _ => &[],
        }
    }

    /// Capabilities this unit provides to others.
    ///
    /// Workload, data, and cross-domain capability governance units
    /// declare provides. Policy and other governance units return
    /// an empty slice.
    #[must_use]
    pub fn provides(&self) -> &[Capability] {
        match self {
            Self::Workload(w) => &w.provides,
            Self::Data(d) => &d.provides,
            Self::Governance(GovernanceUnit::CrossDomainCapability(c)) => {
                std::slice::from_ref(&c.provides)
            }
            _ => &[],
        }
    }

    /// The kind discriminator for this unit.
    #[must_use]
    pub const fn kind(&self) -> UnitKind {
        UnitKind::from_unit(self)
    }

    /// The globally unique, immutable identifier for this unit.
    #[must_use]
    pub const fn id(&self) -> UnitId {
        self.header().id
    }
}

// ===========================================================================
// UnitKind — discriminator for scope checking and store queries
// ===========================================================================

/// Discriminator for the four unit kinds.
///
/// Used for author scope checking (INV-S5), store queries
/// ([`UnitStore::list_by_kind`](crate::store::UnitStore::list_by_kind)),
/// and error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum UnitKind {
    /// Compute process (container, microVM, Wasm, native).
    Workload,
    /// Dataset with constraints.
    Data,
    /// Capability conflict resolution.
    Policy,
    /// Trust domain definitions, roles, certifications.
    Governance,
}

impl UnitKind {
    /// Returns the kind of the given unit.
    #[must_use]
    pub const fn from_unit(unit: &Unit) -> Self {
        match unit {
            Unit::Workload(_) => Self::Workload,
            Unit::Data(_) => Self::Data,
            Unit::Policy(_) => Self::Policy,
            Unit::Governance(_) => Self::Governance,
        }
    }
}

impl std::fmt::Display for UnitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workload => write!(f, "workload"),
            Self::Data => write!(f, "data"),
            Self::Policy => write!(f, "policy"),
            Self::Governance => write!(f, "governance"),
        }
    }
}

/// Which unit types an author is authorized to create (INV-S5, INV-S8).
///
/// Role assignments use this enum to scope author authority. For
/// state-producing unit types (workload, data), no two distinct
/// authors may have identical scope tuples (INV-S8). For
/// decision-making types (policy, governance), overlapping scopes
/// are permitted (INV-S8a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UnitTypeScope {
    /// Author can create workload units.
    Workload,
    /// Author can create data units.
    Data,
    /// Author can create policy units.
    Policy,
    /// Author can create governance units.
    Governance,
}

impl From<UnitKind> for UnitTypeScope {
    fn from(kind: UnitKind) -> Self {
        match kind {
            UnitKind::Workload => Self::Workload,
            UnitKind::Data => Self::Data,
            UnitKind::Policy => Self::Policy,
            UnitKind::Governance => Self::Governance,
        }
    }
}

// ===========================================================================
// UnitHeader — common metadata
// ===========================================================================

/// Metadata common to all unit types.
///
/// Every unit carries a header with its identity, author, trust
/// domain, creation event (dual clock), optional validity window,
/// lifecycle state, and optional git version ref.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitHeader {
    /// Globally unique, immutable identifier.
    pub id: UnitId,
    /// Author who signed this unit (or delegation token for spawned tasks).
    pub author: AuthorId,
    /// Trust domain this unit belongs to.
    pub trust_domain: TrustDomainId,
    /// When this unit was created (dual clock — logical for ordering,
    /// wall time for compliance).
    pub created_at: DualClockEvent,
    /// Validity window. Optional for services (INV-W1), set for bounded
    /// tasks (INV-W2).
    pub validity: Option<ValidityWindow>,
    /// Current lifecycle state.
    pub state: UnitState,
    /// Git-native version ref (commit SHA or tag). Optional for
    /// non-git sources (A9).
    pub version: Option<String>,
}

// ===========================================================================
// UnitState — lifecycle
// ===========================================================================

/// Lifecycle states of a unit from authoring to termination.
///
/// Progression: `Declared` → `Composed` → `Placed` → `Running` →
/// `Draining` → `Terminated`. The ordering is significant: each
/// state is strictly greater than the previous, enabling
/// comparisons for state-transition validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UnitState {
    /// Unit has been authored and signed but not yet composed.
    Declared,
    /// Solver has resolved this unit's capabilities into a composition.
    Composed,
    /// Solver has assigned this unit to a node.
    Placed,
    /// Unit is actively running on its assigned node.
    Running,
    /// Unit is gracefully shutting down.
    Draining,
    /// Unit has completed its lifecycle.
    Terminated,
}

// ===========================================================================
// Workload unit
// ===========================================================================

/// A compute process with full behavioral contracts.
///
/// Runtime-agnostic: the solver matches [`ArtifactType`](crate::ArtifactType)
/// to node [`RuntimeCapability`](crate::node_capability::RuntimeCapability)
/// (INV-N2). Services run indefinitely (INV-W1); bounded tasks
/// auto-terminate on completion, failure, or deadline (INV-W2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// Service (indefinite) or `BoundedTask` (lifecycle-limited).
    pub kind: WorkloadKind,
    /// Artifact packaging: OCI image, native binary, Wasm module, K8s manifest.
    pub artifact: Artifact,
    /// Capabilities this workload requires from the environment.
    pub needs: Vec<Capability>,
    /// Capabilities this workload exposes to other units.
    pub provides: Vec<Capability>,
    /// Latency and failure budgets this workload can tolerate.
    pub tolerates: Tolerances,
    /// Identity-based trust declarations.
    pub trusts: Vec<TrustDeclaration>,
    /// Scaling parameters (min/max instances, triggers).
    pub scaling: Scaling,
    /// What happens when this workload fails.
    pub failure_semantics: FailureSemantics,
    /// Dependency ordering on failure recovery (cycles fail closed per INV-K5).
    pub recovery_relationships: Vec<RecoveryRelationship>,
    /// How this workload recovers state after restart.
    pub state_recovery: StateRecovery,
    /// What to do when the hosting node fails (INV-N5).
    pub placement_on_failure: Option<PlacementOnFailure>,
    /// Optional health check endpoint (INV-O3, progressive).
    pub health_check: Option<HealthCheck>,
    /// If this is a spawned bounded task, the parent service and delegation token.
    pub spawn_context: Option<SpawnContext>,
}

/// Whether a workload is a long-running service or a bounded task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkloadKind {
    /// Long-running, indefinite lifetime. No validity window (INV-W1).
    Service,
    /// Lifecycle-limited. Auto-terminates on completion/failure/deadline (INV-W2).
    BoundedTask,
}

// ===========================================================================
// Data unit
// ===========================================================================

/// A dataset carrying constraints. Hierarchical: parent contains children.
///
/// Granularity is demand-driven — children exist only where
/// constraints diverge from parent (INV-D3). Maximum hierarchy
/// depth: 16 levels (enforced at graph merge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// Typed schema declaration for this dataset.
    pub schema: DataSchema,
    /// Data classification in the lattice (INV-S4, INV-S7).
    pub classification: Classification,
    /// How this data was produced (provenance chain, INV-D1).
    pub provenance: Option<Provenance>,
    /// How long this data must be retained and on what legal basis (INV-D2).
    pub retention: RetentionPolicy,
    /// What this data may be used for (purpose-qualified capabilities).
    pub consent_scope: Vec<ConsentScope>,
    /// Storage requirements (encryption, jurisdiction).
    pub storage_requirements: StorageRequirements,
    /// Parent data unit, if this is a child in a hierarchy (INV-D3).
    pub parent: Option<UnitId>,
    /// Capabilities this data unit provides (e.g., `"provides dataset X"`).
    pub provides: Vec<Capability>,
}

// ===========================================================================
// Policy unit
// ===========================================================================

/// Resolves a capability conflict between other units.
///
/// Required whenever the solver detects incompatible declarations
/// (INV-S2). Only one non-revoked policy per conflict tuple (INV-C7).
/// Supersession creates an immutable chain — the solver uses the
/// latest non-revoked version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The specific conflict this policy resolves (set of unit IDs + capability name).
    pub conflict: ConflictTuple,
    /// The resolution: allow, deny, or conditional.
    pub resolution: PolicyResolution,
    /// Trust domain scope of this policy.
    pub scope: TrustDomainId,
    /// Human-readable justification for this policy decision.
    pub rationale: String,
    /// If this policy supersedes an earlier one, the ID of the replaced policy.
    /// Creates a versioned lineage chain (INV-C7).
    pub supersedes: Option<UnitId>,
    /// Version in the supersession chain. First policy is version 1.
    pub version: Version,
    /// Whether this policy has been revoked.
    pub revoked: bool,
}

/// The conflict tuple that a policy resolves: a set of unit IDs plus
/// the capability name where the conflict was detected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConflictTuple {
    /// The units involved in the conflict (sorted for determinism).
    pub unit_ids: BTreeSet<UnitId>,
    /// The capability name where the conflict was detected.
    pub capability_name: String,
}

/// How a policy resolves a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PolicyResolution {
    /// Allow the composition unconditionally.
    Allow,
    /// Deny the composition.
    Deny,
    /// Allow under specific conditions.
    Conditional {
        /// Conditions that must hold for this resolution to apply.
        conditions: Vec<String>,
    },
}

// ===========================================================================
// Governance unit
// ===========================================================================

/// Trust domain definitions, role scope assignments, certification
/// attestations, operational commands, and promotion gates.
///
/// Created through multi-party agreement (INV-S6, INV-S10) or
/// self-signed in Tier 0. Governance units are actively replicated
/// (full copies on N nodes, INV-R6) and never compacted (INV-G3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceUnit {
    /// Defines a trust domain boundary.
    TrustDomainDef(TrustDomainDef),
    /// Assigns an author to a scoped role.
    RoleAssignment(RoleAssignment),
    /// Attests that a composition meets a standard.
    Certification(Certification),
    /// Fleet-wide administrative instruction (refresh-capabilities, etc.).
    OperationalCommand(OperationalCommand),
    /// Declares auto-promote vs human-approval per environment transition.
    PromotionGate(PromotionGateDef),
    /// Publishes a capability for cross-domain consumption (INV-X5).
    CrossDomainCapability(CrossDomainCapabilityDef),
    /// Key revocation (causal model, INV-S3).
    KeyRevocation(KeyRevocationDef),
}

impl GovernanceUnit {
    /// The header common to all governance unit types.
    #[must_use]
    pub const fn header(&self) -> &UnitHeader {
        match self {
            Self::TrustDomainDef(t) => &t.header,
            Self::RoleAssignment(r) => &r.header,
            Self::Certification(c) => &c.header,
            Self::OperationalCommand(o) => &o.header,
            Self::PromotionGate(p) => &p.header,
            Self::CrossDomainCapability(c) => &c.header,
            Self::KeyRevocation(k) => &k.header,
        }
    }
}

// ---------------------------------------------------------------------------
// Governance subtypes
// ---------------------------------------------------------------------------

/// Trust domain boundary definition.
///
/// Creation requires multi-party signing (minimum 2 distinct authors,
/// INV-S10). Once created, a trust domain scopes author permissions
/// for all units authored within it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustDomainDef {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The trust domain this unit defines.
    pub domain_id: TrustDomainId,
    /// Human-readable name for the trust domain.
    pub name: String,
    /// Description of this domain's purpose.
    pub description: String,
    /// Authors who co-signed the domain creation.
    pub signers: Vec<AuthorId>,
    /// When this trust domain expires (if ever). Wall-clock deadline
    /// for compliance purposes (INV-T2).
    pub expires_at: Option<WallTime>,
}

/// Assigns an author to a scoped role within a trust domain.
///
/// No two distinct authors may have identical scope tuples for
/// state-producing unit types (INV-S8). For decision-making types
/// (policy, governance), overlapping scopes are permitted (INV-S8a).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The author receiving the role.
    pub assignee: AuthorId,
    /// Which unit types this author can create.
    pub unit_type_scope: Vec<UnitTypeScope>,
    /// Which trust domains this author can operate in.
    pub trust_domain_scope: Vec<TrustDomainId>,
}

/// Attestation that a composition meets a particular standard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certification {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The composition being certified.
    pub composition_id: UnitId,
    /// Name of the standard or regulation.
    pub standard: String,
    /// Details of the certification.
    pub details: String,
    /// When this certification expires. Wall-clock deadline for
    /// compliance purposes (INV-T2).
    pub expires_at: Option<WallTime>,
}

/// Fleet-wide administrative instruction propagated via gossip.
///
/// Operational commands are rate-limited: one command per type per
/// gossip convergence window (F-A314). Examples: refresh-capabilities,
/// enter-degraded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalCommand {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The specific command.
    pub command_type: OperationalCommandType,
}

/// Type of operational command (progressive disclosure).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OperationalCommandType {
    /// All nodes re-probe their capabilities (INV-N1).
    RefreshCapabilities,
    /// Operator-triggered degraded mode.
    EnterDegraded {
        /// Reason for entering degraded mode.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Promotion
// ---------------------------------------------------------------------------

/// Environment promotion gate for a trust domain (INV-E3).
///
/// Declares which environment transitions auto-promote and which
/// require human approval. If no `PromotionGate` exists, all
/// transitions default to auto-promote (progressive disclosure:
/// zero config = full auto).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionGateDef {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// Environment transition rules.
    pub transitions: Vec<PromotionTransition>,
}

/// A single environment transition rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionTransition {
    /// Source environment (e.g., `"test"`).
    pub from_env: String,
    /// Target environment (e.g., `"prod"`).
    pub to_env: String,
    /// Promotion mode: auto or human approval.
    pub mode: PromotionMode,
}

/// Promotion mode for an environment transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PromotionMode {
    /// CI or automation can promote automatically.
    Auto,
    /// Human must explicitly approve.
    HumanApproval,
}

/// Promotion policy: gates workload placement by environment (INV-E1).
///
/// A signed, auditable declaration: "unit X version Y is approved
/// for env:Z." Promotion policies are cumulative and non-exclusive
/// (INV-E2): promoting to `env:prod` does not remove the unit from
/// `env:test`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionPolicy {
    /// The policy unit wrapping this promotion.
    pub header: UnitHeader,
    /// The workload unit being promoted.
    pub unit_ref: UnitId,
    /// The version being promoted (git ref or content-addressable ID).
    pub version: String,
    /// The target environment (e.g., `"test"`, `"prod"`).
    pub target_environment: String,
    /// Human-readable rationale.
    pub rationale: String,
}

// ---------------------------------------------------------------------------
// Cross-domain capability
// ---------------------------------------------------------------------------

/// Cross-domain capability advertisement (INV-X5).
///
/// A governance unit publishing capabilities a trust domain is
/// willing to share. A domain only discovers another domain's
/// capabilities through a bridge or operator configuration — no
/// global discovery mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDomainCapabilityDef {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The capability being advertised.
    pub provides: Capability,
    /// Conditions for cross-domain access.
    pub conditions: String,
}

// ---------------------------------------------------------------------------
// Key revocation
// ---------------------------------------------------------------------------

/// Key revocation governance unit (causal model per INV-S3).
///
/// Revocation takes effect when this governance unit is merged into
/// a node's local graph — not at clock-comparison time. Units from
/// the revoked author that arrived before the merge are grandfathered.
/// The gossip convergence window is the inherent security boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRevocationDef {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The author whose key is being revoked.
    pub revoked_author: AuthorId,
    /// Logical clock at which revocation was issued.
    pub revocation_lc: LogicalClock,
    /// Reason for revocation.
    pub reason: String,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ArtifactType, CrashBehavior, OomBehavior, ShutdownBehavior};

    /// Creates a minimal [`UnitHeader`] for testing.
    fn test_header() -> UnitHeader {
        UnitHeader {
            id: UnitId(uuid::Uuid::new_v4()),
            author: AuthorId(uuid::Uuid::new_v4()),
            trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
            created_at: DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: UnitState::Declared,
            version: None,
        }
    }

    /// Creates a [`UnitHeader`] with the given validity window.
    fn test_header_with_validity(validity: Option<ValidityWindow>) -> UnitHeader {
        let mut h = test_header();
        h.validity = validity;
        h
    }

    /// Creates a minimal valid [`WorkloadUnit`] for testing.
    fn test_workload_unit() -> WorkloadUnit {
        WorkloadUnit {
            header: test_header(),
            kind: WorkloadKind::Service,
            artifact: Artifact {
                artifact_type: ArtifactType::Oci,
                artifact_ref: "registry.example.com/app:v1".to_string(),
                digest: taba_common::ContentDigest("sha256:abc123".to_string()),
                requires: Vec::new(),
                kernel_ref: None,
                rootfs_ref: None,
            },
            needs: Vec::new(),
            provides: vec![Capability::new("compute", "http")],
            tolerates: Tolerances {
                max_latency: Some(std::time::Duration::from_millis(100)),
                failure_modes: vec!["timeout".to_string()],
                consistency: None,
            },
            trusts: Vec::new(),
            scaling: Scaling {
                min_instances: 1,
                max_instances: 3,
                triggers: Vec::new(),
            },
            failure_semantics: FailureSemantics {
                on_oom: OomBehavior::Restart,
                on_crash: CrashBehavior::Unexpected,
                on_shutdown: ShutdownBehavior::Immediate,
            },
            recovery_relationships: Vec::new(),
            state_recovery: StateRecovery::Stateless,
            placement_on_failure: None,
            health_check: None,
            spawn_context: None,
        }
    }

    /// Creates a minimal valid [`DataUnit`] for testing.
    fn test_data_unit() -> DataUnit {
        DataUnit {
            header: test_header(),
            schema: DataSchema {
                format: "json-schema".to_string(),
                definition: "{}".to_string(),
            },
            classification: Classification::Public,
            provenance: None,
            retention: RetentionPolicy {
                mode: crate::data::RetentionMode::Persistent,
                duration: Some(std::time::Duration::from_secs(86400)),
                legal_basis: "consent".to_string(),
                mandatory: false,
            },
            consent_scope: Vec::new(),
            storage_requirements: StorageRequirements {
                encrypted_at_rest: false,
                jurisdictions: Vec::new(),
                min_replicas: None,
            },
            parent: None,
            provides: vec![Capability::new("storage", "dataset-x")],
        }
    }

    /// Creates a minimal valid [`PolicyUnit`] for testing.
    fn test_policy_unit() -> PolicyUnit {
        PolicyUnit {
            header: test_header(),
            conflict: ConflictTuple {
                unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
                capability_name: "storage".to_string(),
            },
            resolution: PolicyResolution::Allow,
            scope: TrustDomainId(uuid::Uuid::new_v4()),
            rationale: "compatible providers".to_string(),
            supersedes: None,
            version: Version(1),
            revoked: false,
        }
    }

    // -- UnitState ordering ------------------------------------------------

    #[test]
    fn test_unit_state_ordering() {
        assert!(UnitState::Declared < UnitState::Composed);
        assert!(UnitState::Composed < UnitState::Placed);
        assert!(UnitState::Placed < UnitState::Running);
        assert!(UnitState::Running < UnitState::Draining);
        assert!(UnitState::Draining < UnitState::Terminated);

        // Full chain: Declared < Composed < Placed < Running < Draining < Terminated
        assert!(UnitState::Declared < UnitState::Terminated);
    }

    // -- WorkloadKind and validity -----------------------------------------

    #[test]
    fn test_unit_kind_service_has_no_validity() {
        // Services don't require validity windows (INV-W1).
        let header = test_header_with_validity(None);
        let unit = Unit::Workload(WorkloadUnit {
            kind: WorkloadKind::Service,
            header,
            ..test_workload_unit()
        });

        // A service with no validity window should have validity: None.
        assert!(
            unit.header().validity.is_none(),
            "service should not require a validity window (INV-W1)"
        );
    }

    #[test]
    fn test_unit_kind_bounded_task_has_validity() {
        // Bounded tasks have validity windows (INV-W2).
        let validity = ValidityWindow {
            lc_range: Some((LogicalClock(10), LogicalClock(100))),
            wall_time_deadline: Some(WallTime { millis: 200_000 }),
        };
        let header = test_header_with_validity(Some(validity));
        let unit = Unit::Workload(WorkloadUnit {
            kind: WorkloadKind::BoundedTask,
            header,
            ..test_workload_unit()
        });

        // A bounded task should have a validity window.
        assert!(
            unit.header().validity.is_some(),
            "bounded task should have a validity window (INV-W2)"
        );
    }

    // -- Serialization roundtrips ------------------------------------------

    #[test]
    fn test_workload_unit_serialization_roundtrip() {
        let unit = test_workload_unit();

        let json1 = serde_json::to_string(&unit).expect("serialize WorkloadUnit");
        let decoded: WorkloadUnit = serde_json::from_str(&json1).expect("deserialize WorkloadUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize WorkloadUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_data_unit_serialization_roundtrip() {
        let unit = test_data_unit();

        let json1 = serde_json::to_string(&unit).expect("serialize DataUnit");
        let decoded: DataUnit = serde_json::from_str(&json1).expect("deserialize DataUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize DataUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_policy_unit_serialization_roundtrip() {
        let unit = test_policy_unit();

        let json1 = serde_json::to_string(&unit).expect("serialize PolicyUnit");
        let decoded: PolicyUnit = serde_json::from_str(&json1).expect("deserialize PolicyUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize PolicyUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    // -- Governance unit construction and serialization --------------------

    #[test]
    fn test_governance_unit_trust_domain_def() {
        let def = TrustDomainDef {
            header: test_header(),
            domain_id: TrustDomainId(uuid::Uuid::new_v4()),
            name: "production".to_string(),
            description: "Production trust domain".to_string(),
            signers: vec![
                AuthorId(uuid::Uuid::new_v4()),
                AuthorId(uuid::Uuid::new_v4()),
            ],
            expires_at: Some(WallTime {
                millis: 1_735_689_600_000,
            }),
        };

        let gov = GovernanceUnit::TrustDomainDef(def);

        let json1 = serde_json::to_string(&gov).expect("serialize GovernanceUnit::TrustDomainDef");
        let decoded: GovernanceUnit =
            serde_json::from_str(&json1).expect("deserialize GovernanceUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize GovernanceUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_governance_unit_role_assignment() {
        let assignment = RoleAssignment {
            header: test_header(),
            assignee: AuthorId(uuid::Uuid::new_v4()),
            unit_type_scope: vec![UnitTypeScope::Workload, UnitTypeScope::Data],
            trust_domain_scope: vec![TrustDomainId(uuid::Uuid::new_v4())],
        };

        let gov = GovernanceUnit::RoleAssignment(assignment);

        let json1 = serde_json::to_string(&gov).expect("serialize GovernanceUnit::RoleAssignment");
        let decoded: GovernanceUnit =
            serde_json::from_str(&json1).expect("deserialize GovernanceUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize GovernanceUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_governance_unit_key_revocation() {
        let revocation = KeyRevocationDef {
            header: test_header(),
            revoked_author: AuthorId(uuid::Uuid::new_v4()),
            revocation_lc: LogicalClock(42),
            reason: "key compromised".to_string(),
        };

        let gov = GovernanceUnit::KeyRevocation(revocation);

        let json1 = serde_json::to_string(&gov).expect("serialize GovernanceUnit::KeyRevocation");
        let decoded: GovernanceUnit =
            serde_json::from_str(&json1).expect("deserialize GovernanceUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize GovernanceUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_governance_unit_promotion_gate() {
        let gate = PromotionGateDef {
            header: test_header(),
            transitions: vec![PromotionTransition {
                from_env: "test".to_string(),
                to_env: "prod".to_string(),
                mode: PromotionMode::HumanApproval,
            }],
        };

        let gov = GovernanceUnit::PromotionGate(gate);

        let json1 = serde_json::to_string(&gov).expect("serialize GovernanceUnit::PromotionGate");
        let decoded: GovernanceUnit =
            serde_json::from_str(&json1).expect("deserialize GovernanceUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize GovernanceUnit");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    // -- Unit helper methods -----------------------------------------------

    #[test]
    fn test_unit_kind_discrimination() {
        assert_eq!(
            Unit::Workload(test_workload_unit()).kind(),
            UnitKind::Workload
        );
        assert_eq!(Unit::Data(test_data_unit()).kind(), UnitKind::Data);
        assert_eq!(Unit::Policy(test_policy_unit()).kind(), UnitKind::Policy);
        assert_eq!(
            Unit::Governance(GovernanceUnit::KeyRevocation(KeyRevocationDef {
                header: test_header(),
                revoked_author: AuthorId(uuid::Uuid::new_v4()),
                revocation_lc: LogicalClock(1),
                reason: "test".to_string(),
            }))
            .kind(),
            UnitKind::Governance
        );
    }

    #[test]
    fn test_unit_provides_extraction() {
        let workload = Unit::Workload(test_workload_unit());
        assert!(!workload.provides().is_empty());

        let data = Unit::Data(test_data_unit());
        assert!(!data.provides().is_empty());

        let policy = Unit::Policy(test_policy_unit());
        assert!(policy.provides().is_empty());
    }

    #[test]
    fn test_unit_needs_extraction() {
        let mut workload = test_workload_unit();
        workload.needs = vec![Capability::new("storage", "redis")];
        let unit = Unit::Workload(workload);
        assert_eq!(unit.needs().len(), 1);

        let data = Unit::Data(test_data_unit());
        assert!(data.needs().is_empty(), "data units have no needs");
    }

    // -- Property tests ----------------------------------------------------

    use proptest::prelude::*;

    fn arb_workload_unit() -> impl Strategy<Value = WorkloadUnit> {
        (
            any::<bool>(),
            "[a-z]{1,8}",
            "[a-z]{1,12}",
            "[a-z]{1,12}",
            "[a-z]{1,16}",
            proptest::collection::vec("[a-z]{1,8}", 0..5),
        )
            .prop_map(
                |(is_bounded, cap_type, cap_name, need_name, provide_name, requires)| {
                    WorkloadUnit {
                        header: UnitHeader {
                            id: UnitId(uuid::Uuid::new_v4()),
                            author: AuthorId(uuid::Uuid::new_v4()),
                            trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
                            created_at: DualClockEvent {
                                logical_clock: LogicalClock(1),
                                wall_time: WallTime { millis: 1000 },
                                timezone: "UTC".to_string(),
                            },
                            validity: if is_bounded {
                                Some(ValidityWindow {
                                    lc_range: Some((LogicalClock(10), LogicalClock(100))),
                                    wall_time_deadline: None,
                                })
                            } else {
                                None
                            },
                            state: UnitState::Declared,
                            version: None,
                        },
                        kind: if is_bounded {
                            WorkloadKind::BoundedTask
                        } else {
                            WorkloadKind::Service
                        },
                        artifact: Artifact {
                            artifact_type: ArtifactType::Oci,
                            artifact_ref: format!("registry.example.com/{cap_name}:v1"),
                            digest: taba_common::ContentDigest("sha256:abc123".to_string()),
                            requires,
                            kernel_ref: None,
                            rootfs_ref: None,
                        },
                        needs: vec![Capability::new(&cap_type, &need_name)],
                        provides: vec![Capability::new(&cap_type, &provide_name)],
                        tolerates: Tolerances {
                            max_latency: Some(std::time::Duration::from_millis(100)),
                            failure_modes: vec!["timeout".to_string()],
                            consistency: None,
                        },
                        trusts: Vec::new(),
                        scaling: Scaling {
                            min_instances: 1,
                            max_instances: 3,
                            triggers: Vec::new(),
                        },
                        failure_semantics: FailureSemantics {
                            on_oom: OomBehavior::Restart,
                            on_crash: CrashBehavior::Unexpected,
                            on_shutdown: ShutdownBehavior::Immediate,
                        },
                        recovery_relationships: Vec::new(),
                        state_recovery: StateRecovery::Stateless,
                        placement_on_failure: None,
                        health_check: None,
                        spawn_context: None,
                    }
                },
            )
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_workload_unit_serialization_roundtrip(
            unit in arb_workload_unit(),
        ) {
            let json1 = serde_json::to_string(&unit).expect("serialize");
            let decoded: WorkloadUnit = serde_json::from_str(&json1).expect("deserialize");
            let json2 = serde_json::to_string(&decoded).expect("re-serialize");
            prop_assert_eq!(json1, json2, "JSON must be identical after round-trip");
        }
    }
}
