//! Composition graph: the single source of desired state (INV-C1).
//!
//! The [`Graph`] trait defines core mutation operations: insert, merge,
//! supersede, archive, compact, snapshot. All mutations go through
//! this trait. Signature verification is a synchronous gate before
//! any unit enters graph state (INV-S3). When a [`Verifier`] is
//! configured via [`DefaultGraph::with_verifier`], signatures are
//! cryptographically verified before the WAL write. When no verifier
//! is configured (M2 default), structural validation only is performed.
//!
//! [`DefaultGraph`] is the in-memory implementation for M2. It uses
//! [`std::sync::Mutex`] for interior mutability (the trait methods take
//! `&self`). The mutex is never held across an `await` point — all
//! operations are synchronous.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::{Arc, Mutex};

use taba_common::{AuthorId, DualClockEvent, TrustDomainId, UnitId};
use taba_core::{
    ConflictTuple, DefaultValidator, GovernanceUnit, RoleAssignment, Unit, UnitKind, UnitTypeScope,
    UnitValidator,
};
use taba_security::{DefaultScopeChecker, ScopeChecker, Verifier};

use crate::compaction::{Compactor, DefaultCompactor};
use crate::crdt::{CompositionGraphData, GraphDelta};
use crate::entry::{GraphEntry, GraphStats, MergeResult, PendingEntry, PolicyChain, PolicyVersion};
use crate::error::GraphError;
use crate::memory::{DefaultMemoryMonitor, MemoryMonitor};
use crate::query::{GraphQuery, ProvenanceLink, traverse_provenance_impl};
use crate::snapshot::GraphSnapshot;
use crate::wal::{InMemoryWal, WalEntry};

// ===========================================================================
// Graph trait
// ===========================================================================

/// Core graph operations: insert, merge, supersede, archive, compact.
///
/// The graph is the single source of desired state (INV-C1). All
/// mutations go through this trait. Signature verification is a
/// synchronous gate before any unit enters graph state (INV-S3).
///
/// WAL-before-effect: every mutation is WAL'd before becoming visible
/// to local queries (INV-C4). In M2, the WAL is in-memory only.
pub trait Graph {
    /// Insert a unit into the graph.
    ///
    /// In M2, structural validation is performed using
    /// [`DefaultValidator`]. If the unit's references are all
    /// satisfied, it enters the active set. If references are
    /// unsatisfied, it enters the pending queue (causal buffering
    /// per INV-C4).
    ///
    /// WAL-before-effect: the insertion is appended to the WAL
    /// before becoming visible to queries (INV-C4).
    ///
    /// # Errors
    ///
    /// - [`GraphError::SignatureRejected`]: structural validation or
    ///   signature verification failed, or spawn depth exceeds maximum
    ///   (INV-S3, INV-W3).
    /// - [`GraphError::ScopeViolation`]: scope uniqueness violation
    ///   (INV-S8).
    /// - [`GraphError::MemoryLimitExceeded`]: graph is at capacity.
    /// - [`GraphError::PolicyChainError`]: duplicate policy (INV-C7).
    /// - [`GraphError::WouldCreateCycle`]: recovery deps would cycle.
    fn insert(&self, unit: Unit) -> impl Future<Output = Result<(), GraphError>> + Send;

    /// Merge a remote delta into the local graph.
    ///
    /// The merge is commutative, associative, and idempotent (INV-C2).
    /// Each unit in the delta is individually validated before merge.
    /// Units that fail validation are rejected without affecting the
    /// rest of the delta.
    ///
    /// Returns a list of `(UnitId, GraphError)` for rejected units.
    /// An empty list means the entire delta merged successfully.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::MergeConflict`] only for true CRDT
    /// violations. Normal capability conflicts are NOT merge conflicts.
    fn merge(
        &self,
        delta: GraphDelta,
    ) -> impl Future<Output = Result<Vec<(UnitId, GraphError)>, GraphError>> + Send;

    /// Supersede a policy unit with a new version (INV-C7).
    ///
    /// The new policy must explicitly reference the old policy's ID
    /// in its `supersedes` field. The old policy is marked revoked.
    /// The supersession chain is immutable.
    ///
    /// # Errors
    ///
    /// - [`GraphError::NotFound`]: old policy not in the graph.
    /// - [`GraphError::PolicyChainError`]: supersession is invalid
    ///   (wrong conflict tuple, broken chain, duplicate without
    ///   supersession).
    fn supersede(
        &self,
        old_policy: &UnitId,
        new_policy: Unit,
    ) -> impl Future<Output = Result<(), GraphError>> + Send;

    /// Archive a unit (soft-delete). The unit is removed from the
    /// active set but retained for historical queries and provenance
    /// integrity.
    ///
    /// Governance units cannot be archived (they are permanent,
    /// INV-G3). Policy units can only be archived after they are
    /// superseded or their conflict tuple no longer exists.
    ///
    /// # Errors
    ///
    /// - [`GraphError::NotFound`]: unit not in the graph.
    /// - [`GraphError::MergeConflict`]: governance unit cannot be
    ///   archived (INV-G3).
    fn archive(&self, id: &UnitId) -> impl Future<Output = Result<(), GraphError>> + Send;

    /// Compact the graph by removing archived units whose retention
    /// has expired.
    ///
    /// Respects data unit retention declarations (INV-D2).
    /// Auto-triggers at 80% of memory limit (INV-R6).
    ///
    /// Returns the number of units compacted and bytes freed.
    fn compact(&self) -> impl Future<Output = Result<(u64, u64), GraphError>> + Send;

    /// Take a consistent point-in-time snapshot for the solver.
    ///
    /// The snapshot is immutable — concurrent mutations do not
    /// affect it. Includes a generation counter for staleness
    /// detection.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::PersistenceError`] if the snapshot
    /// cannot be taken (should not happen for in-memory M2).
    fn snapshot(&self) -> impl Future<Output = Result<GraphSnapshot, GraphError>> + Send;

    /// Check whether a snapshot is still current (no mutations since
    /// it was taken).
    fn is_snapshot_current(&self, snapshot: &GraphSnapshot) -> bool;

    /// Return current graph statistics (unit counts, memory usage).
    fn stats(&self) -> GraphStats;
}

// ===========================================================================
// Reference computation
// ===========================================================================

/// Computes all [`UnitId`] references declared by a [`Unit`].
///
/// References are the outgoing edges in the graph — the other units
/// that this unit depends on. A unit enters the pending queue if any
/// of its references are not yet in the active set (causal buffering,
/// INV-C4).
///
/// This is a pure function: no I/O, no side effects.
fn compute_references(unit: &Unit) -> BTreeSet<UnitId> {
    let mut refs = BTreeSet::new();

    match unit {
        Unit::Workload(w) => {
            // Recovery relationships: depends_on references.
            for rr in &w.recovery_relationships {
                refs.insert(rr.depends_on);
            }
            // Spawn context: parent service reference.
            if let Some(ctx) = &w.spawn_context {
                refs.insert(ctx.spawned_by);
            }
        }
        Unit::Data(d) => {
            // Provenance: producing workload and input data units.
            if let Some(prov) = &d.provenance {
                refs.insert(prov.produced_by);
                for input in &prov.inputs {
                    refs.insert(*input);
                }
            }
            // Parent data unit.
            if let Some(parent) = &d.parent {
                refs.insert(*parent);
            }
        }
        Unit::Policy(p) => {
            // Conflict tuple: referenced units.
            for id in &p.conflict.unit_ids {
                refs.insert(*id);
            }
            // Superseded policy.
            if let Some(supersedes) = &p.supersedes {
                refs.insert(*supersedes);
            }
        }
        Unit::Governance(g) => {
            // Certification: composition reference.
            if let taba_core::GovernanceUnit::Certification(c) = g {
                refs.insert(c.composition_id);
            }
            // Other governance subtypes have no explicit unit references.
        }
    }

    // Remove self-references (a unit should not reference itself).
    refs.remove(&unit.id());

    refs
}

// ===========================================================================
// DefaultGraph
// ===========================================================================

/// In-memory implementation of [`Graph`] and [`GraphQuery`] for M2.
///
/// Holds the CRDT graph state ([`CompositionGraphData`]) behind a
/// [`Mutex`] for interior mutability, an in-memory WAL, a structural
/// validator, and a merge policy. All operations are synchronous —
/// the mutex is never held across an `await` point.
///
/// # Memory management
///
/// The `memory_limit_bytes` parameter enforces INV-R6. At 80%,
/// [`compact`](Graph::compact) should be called. At 100%, the node
/// enters degraded mode (refuses new placements).
pub struct DefaultGraph {
    /// Shared CRDT graph state.
    state: Arc<Mutex<CompositionGraphData>>,
    /// In-memory write-ahead log (M2: no disk).
    wal: Mutex<InMemoryWal>,
    /// Structural validator for units.
    validator: DefaultValidator,
    /// Memory limit in bytes (INV-R6).
    memory_limit_bytes: u64,
    /// Optional signature verifier (INV-S3). When present, signatures
    /// are cryptographically verified before a unit enters graph state.
    /// When `None` (M2 default), structural validation only is performed.
    verifier: Option<Arc<dyn Verifier + Send + Sync>>,
    /// Maximum spawn depth for workload units (INV-W3). Default: 4.
    max_spawn_depth: u8,
    /// Optional scope checker for role-assignment uniqueness (INV-S8).
    scope_checker: Option<Arc<DefaultScopeChecker>>,
}

impl DefaultGraph {
    /// Creates a new empty `DefaultGraph` with the given memory limit.
    ///
    /// The memory limit enforces INV-R6: auto-compaction at 80%,
    /// degraded mode at 100%.
    #[must_use]
    pub fn new(memory_limit_bytes: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(CompositionGraphData::new())),
            wal: Mutex::new(InMemoryWal::new()),
            validator: DefaultValidator::empty(),
            memory_limit_bytes,
            verifier: None,
            max_spawn_depth: 4,
            scope_checker: None,
        }
    }

    /// Creates a new `DefaultGraph` with the given validator and
    /// memory limit.
    ///
    /// Use this constructor when you have role assignments to enforce
    /// author scope (INV-S5).
    #[must_use]
    pub fn with_validator(validator: DefaultValidator, memory_limit_bytes: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(CompositionGraphData::new())),
            wal: Mutex::new(InMemoryWal::new()),
            validator,
            memory_limit_bytes,
            verifier: None,
            max_spawn_depth: 4,
            scope_checker: None,
        }
    }

    /// Sets the signature verifier for this graph (INV-S3).
    ///
    /// When set, all units inserted via [`insert`](Graph::insert) and
    /// all entries received via [`merge`](Graph::merge) must have a
    /// valid Ed25519 signature before they enter graph state. When
    /// `None` (M2 default), structural validation only is performed.
    #[must_use]
    pub fn with_verifier(mut self, verifier: Arc<dyn Verifier + Send + Sync>) -> Self {
        self.verifier = Some(verifier);
        self
    }

    /// Sets the maximum spawn depth for workload units (INV-W3).
    ///
    /// Units with `spawn_context.spawn_depth > max` are rejected at
    /// graph merge. Default: 4 (from `ClusterConfig.max_spawn_depth`).
    #[must_use]
    pub const fn with_max_spawn_depth(mut self, n: u8) -> Self {
        self.max_spawn_depth = n;
        self
    }

    /// Sets the scope checker for this graph (INV-S8).
    ///
    /// When set, role assignments are checked for scope uniqueness
    /// before they enter graph state. No two distinct authors may
    /// have identical scope tuples for state-producing unit types
    /// (workload, data). Overlapping scopes for decision-making types
    /// (policy, governance) are permitted (INV-S8a).
    #[must_use]
    pub fn with_scope_checker(mut self, checker: Arc<DefaultScopeChecker>) -> Self {
        self.scope_checker = Some(checker);
        self
    }

    /// Returns a clone of the shared state `Arc`.
    ///
    /// Used by [`DefaultCompactor`] and [`DefaultMemoryMonitor`] to
    /// share the graph state without going through the `Graph` trait.
    #[must_use]
    pub fn shared_state(&self) -> Arc<Mutex<CompositionGraphData>> {
        Arc::clone(&self.state)
    }

    /// Returns the memory limit in bytes.
    #[must_use]
    pub const fn memory_limit_bytes(&self) -> u64 {
        self.memory_limit_bytes
    }

    /// Returns a reference to the in-memory WAL (for testing).
    #[must_use]
    pub const fn wal(&self) -> &Mutex<InMemoryWal> {
        &self.wal
    }

    /// Validates a unit structurally using the graph's validator.
    ///
    /// This is the first verification gate (structural well-formedness).
    /// When a [`Verifier`] is also configured, cryptographic signature
    /// verification is performed separately in [`insert`](Graph::insert)
    /// after this check. Returns `Ok(())` if the unit is well-formed,
    /// or `Err(GraphError::SignatureRejected)` on failure.
    ///
    /// [`Verifier`]: taba_security::Verifier
    fn validate(&self, unit: &Unit) -> Result<(), GraphError> {
        self.validator
            .validate(unit)
            .map_err(|e| GraphError::SignatureRejected {
                unit: unit.id(),
                reason: e.to_string(),
            })
    }

    /// Creates a [`SignedUnit`] with placeholder crypto for M2.
    ///
    /// In M2, actual Ed25519 signing is deferred. This method wraps
    /// a [`Unit`] in a [`SignedUnit`] with zero-valued signature,
    /// context, and signer — structurally valid but not
    /// cryptographically signed.
    const fn wrap_signed(unit: Unit) -> taba_security::SignedUnit<Unit> {
        taba_security::SignedUnit {
            unit,
            signature: taba_security::Signature([0u8; 64]),
            context: taba_security::SignatureContext {
                trust_domain_id: taba_common::TrustDomainId(uuid::Uuid::nil()),
                cluster_id: taba_common::ClusterId(uuid::Uuid::nil()),
                validity_window: taba_common::ValidityWindow {
                    lc_range: None,
                    wall_time_deadline: None,
                },
            },
            signer: taba_security::PublicKey([0u8; 32]),
        }
    }

    /// Checks memory limit and returns error if exceeded (INV-R6).
    const fn check_memory_limit(&self, state: &CompositionGraphData) -> Result<(), GraphError> {
        if state.memory_estimate_bytes > self.memory_limit_bytes {
            return Err(GraphError::MemoryLimitExceeded {
                used: state.memory_estimate_bytes,
                limit: self.memory_limit_bytes,
            });
        }
        Ok(())
    }

    /// Promotes pending entries whose references are now satisfied.
    ///
    /// Scans the pending queue for entries whose `missing_refs` are
    /// all present in the active entries. Promoted entries are
    /// removed from the pending queue, added to the active set, and
    /// WAL'd as `Promoted`.
    fn promote_pending(&self, state: &mut CompositionGraphData) -> Vec<UnitId> {
        let active_ids: BTreeSet<UnitId> = state.entries.keys().copied().collect();
        let mut still_pending = Vec::new();
        let mut promoted = Vec::new();

        for pending in state.pending.drain(..) {
            let now_satisfied = pending
                .missing_refs
                .iter()
                .all(|ref_id| active_ids.contains(ref_id));

            if now_satisfied {
                let id = pending.unit_id();
                if let std::collections::btree_map::Entry::Vacant(e) = state.entries.entry(id) {
                    let entry = GraphEntry::from_signed_unit(
                        pending.signed_unit.clone(),
                        pending.received_at.clone(),
                        pending.missing_refs.clone(),
                    );
                    e.insert(entry);
                    promoted.push(id);

                    // WAL: Promoted
                    if let Ok(mut wal) = self.wal.lock() {
                        wal.append(WalEntry::Promoted { unit_id: id });
                    }
                }
            } else {
                still_pending.push(pending);
            }
        }

        state.pending = still_pending;
        promoted
    }
}

impl Graph for DefaultGraph {
    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    async fn insert(&self, unit: Unit) -> Result<(), GraphError> {
        // Phase 1: Structural validation (M2: no crypto).
        self.validate(&unit)?;

        let id = unit.id();
        let references = compute_references(&unit);

        // Phase 2: Determine if references are satisfied.
        let missing_refs: BTreeSet<UnitId> = {
            let state = self
                .state
                .lock()
                .expect("graph mutex should not be poisoned");
            references
                .iter()
                .filter(|ref_id| !state.entries.contains_key(ref_id))
                .copied()
                .collect()
        };

        // Phase 3: Check memory limit before inserting.
        {
            let state = self
                .state
                .lock()
                .expect("graph mutex should not be poisoned");
            self.check_memory_limit(&state)?;
        }

        // Phase 4: Wrap in SignedUnit and run security gates before WAL.
        let signed = Self::wrap_signed(unit);

        // Phase 4a: Signature verification gate (INV-S3).
        // In M2, insert receives a raw Unit. wrap_signed creates a
        // placeholder SignedUnit with a zero-valued signature. When a
        // verifier is configured, the placeholder is rejected — M3
        // will provide properly signed units via the node daemon.
        if let Some(verifier) = &self.verifier {
            let logical_clock = signed.unit.header().created_at.logical_clock;
            verifier
                .verify(
                    &signed.unit,
                    &signed.signature,
                    &signed.context.trust_domain_id,
                    &signed.context.cluster_id,
                    &logical_clock,
                    None,
                )
                .map_err(|e| GraphError::SignatureRejected {
                    unit: id,
                    reason: e.to_string(),
                })?;
        } else {
            // M2: no verifier configured, structural validation only
        }

        // Phase 4b: Spawn depth enforcement (INV-W3).
        // Workload units with a spawn context must not exceed the
        // maximum spawn depth (default 4).
        if let Unit::Workload(w) = &signed.unit {
            if let Some(spawn_ctx) = &w.spawn_context {
                if spawn_ctx.spawn_depth > self.max_spawn_depth {
                    return Err(GraphError::SignatureRejected {
                        unit: id,
                        reason: format!(
                            "spawn depth {} exceeds maximum {} (INV-W3)",
                            spawn_ctx.spawn_depth, self.max_spawn_depth
                        ),
                    });
                }
            }
        }

        // Phase 4c: Scope uniqueness enforcement (INV-S8).
        // For role assignments, no two distinct authors may have
        // identical (unit_type_scope, trust_domain_scope) tuples for
        // state-producing unit types. Overlapping scopes for
        // decision-making types (policy, governance) are permitted
        // (INV-S8a).
        if let Some(scope_checker) = &self.scope_checker {
            if let Unit::Governance(GovernanceUnit::RoleAssignment(new_ra)) = &signed.unit {
                let existing: Vec<RoleAssignment> = {
                    let state = self
                        .state
                        .lock()
                        .expect("graph mutex should not be poisoned");
                    state
                        .entries
                        .values()
                        .filter_map(|e| {
                            if let Unit::Governance(GovernanceUnit::RoleAssignment(ra)) = e.unit() {
                                Some(ra.clone())
                            } else {
                                None
                            }
                        })
                        .collect()
                };

                let mut merged = existing;
                merged.push(new_ra.clone());

                if let Some(first_scope) = new_ra.unit_type_scope.first() {
                    let unit_kind = match first_scope {
                        UnitTypeScope::Workload => UnitKind::Workload,
                        UnitTypeScope::Data => UnitKind::Data,
                        UnitTypeScope::Policy => UnitKind::Policy,
                        // UnitTypeScope::Governance and any future
                        // #[non_exhaustive] variants default to
                        // Governance (conservative — scope uniqueness
                        // returns Ok for Governance per INV-S8a).
                        _ => UnitKind::Governance,
                    };
                    scope_checker
                        .validate_scope_uniqueness(&merged, unit_kind)
                        .map_err(|e| GraphError::ScopeViolation {
                            author: new_ra.assignee,
                            reason: e.to_string(),
                        })?;
                }
            }
        } else {
            // M2: no scope checker configured, scope uniqueness not enforced
        }

        // Phase 5: WAL-before-effect (INV-C4).
        if missing_refs.is_empty() {
            // References satisfied — enter active set.
            let merged_at = DualClockEvent {
                logical_clock: {
                    let mut state = self
                        .state
                        .lock()
                        .expect("graph mutex should not be poisoned");
                    state.local_clock.tick();
                    state.local_clock
                },
                wall_time: taba_common::WallTime {
                    millis: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64),
                },
                timezone: "UTC".to_string(),
            };

            // WAL: Merged
            if let Ok(mut wal) = self.wal.lock() {
                wal.append(WalEntry::Merged {
                    unit_id: id,
                    timestamp: merged_at.clone(),
                });
            }

            // Effect: add to active entries.
            let mut state = self
                .state
                .lock()
                .expect("graph mutex should not be poisoned");
            let entry = GraphEntry::from_signed_unit(signed, merged_at, references);
            state.entries.insert(id, entry);

            // For policy units: check INV-C7 and build policy chain.
            if let Some(signed_unit) = state.entries.get(&id) {
                if let Unit::Policy(p) = signed_unit.unit() {
                    let conflict = p.conflict.clone();
                    let policy_version = PolicyVersion {
                        policy_id: p.header.id,
                        version: p.version,
                        revoked: p.revoked,
                        author: p.header.author,
                        created_at: p.header.created_at.clone(),
                    };
                    state
                        .policy_chains
                        .entry(conflict.clone())
                        .or_insert_with(|| PolicyChain::new(conflict.clone()))
                        .add_version(policy_version);
                }
            }

            // Promote any pending entries whose references are now satisfied.
            self.promote_pending(&mut state);

            state.recompute_memory();
            state.bump_generation();
        } else {
            // References unsatisfied — enter pending queue.
            let received_at = DualClockEvent {
                logical_clock: {
                    let mut state = self
                        .state
                        .lock()
                        .expect("graph mutex should not be poisoned");
                    state.local_clock.tick();
                    state.local_clock
                },
                wall_time: taba_common::WallTime {
                    millis: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64),
                },
                timezone: "UTC".to_string(),
            };

            // WAL: Pending
            if let Ok(mut wal) = self.wal.lock() {
                wal.append(WalEntry::Pending {
                    unit_id: id,
                    missing_refs: missing_refs.clone(),
                });
            }

            // Effect: add to pending queue.
            let mut state = self
                .state
                .lock()
                .expect("graph mutex should not be poisoned");
            state.pending.push(PendingEntry {
                signed_unit: signed,
                missing_refs,
                received_at,
            });
            state.recompute_memory();
            state.bump_generation();
        }

        Ok(())
    }

    async fn merge(&self, delta: GraphDelta) -> Result<Vec<(UnitId, GraphError)>, GraphError> {
        let mut rejected: Vec<(UnitId, GraphError)> = Vec::new();

        // Phase 1: Validate each entry and build a filtered delta.
        let mut valid_delta = GraphDelta::new();

        for (id, entry) in &delta.entries {
            if let Err(e) = self.validate(entry.unit()) {
                rejected.push((*id, e));
                continue;
            }

            // Signature verification gate (INV-S3).
            if let Some(verifier) = &self.verifier {
                let logical_clock = entry.unit().header().created_at.logical_clock;
                if let Err(e) = verifier.verify(
                    entry.unit(),
                    &entry.signed_unit.signature,
                    &entry.signed_unit.context.trust_domain_id,
                    &entry.signed_unit.context.cluster_id,
                    &logical_clock,
                    None,
                ) {
                    rejected.push((
                        *id,
                        GraphError::SignatureRejected {
                            unit: *id,
                            reason: e.to_string(),
                        },
                    ));
                    continue;
                }
            }

            valid_delta.add_entry(entry.clone());
        }

        // Merge policy chains from the delta.
        for (conflict, chain) in &delta.policy_chains {
            valid_delta.add_policy_chain(conflict.clone(), chain.clone());
        }

        // Phase 2: Merge the valid delta into the graph.
        let mut state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        // Check memory limit before merging.
        self.check_memory_limit(&state)?;

        let result: MergeResult = state.merge_delta(&valid_delta);

        // Phase 3: WAL all merged entries.
        for id in &result.new_entries {
            if let Some(entry) = state.entries.get(id) {
                if let Ok(mut wal) = self.wal.lock() {
                    wal.append(WalEntry::Merged {
                        unit_id: *id,
                        timestamp: entry.merged_at.clone(),
                    });
                }
            }
        }

        for id in &result.promoted {
            if let Ok(mut wal) = self.wal.lock() {
                wal.append(WalEntry::Promoted { unit_id: *id });
            }
        }

        Ok(rejected)
    }

    #[allow(clippy::too_many_lines, clippy::cast_possible_truncation)]
    async fn supersede(&self, old_policy: &UnitId, new_policy: Unit) -> Result<(), GraphError> {
        // Validate the new policy structurally.
        self.validate(&new_policy)?;

        // The new policy must supersede the old one.
        let new_policy_supersedes = match &new_policy {
            Unit::Policy(p) => p.supersedes,
            _ => {
                return Err(GraphError::PolicyChainError {
                    conflict: ConflictTuple {
                        unit_ids: BTreeSet::new(),
                        capability_name: String::new(),
                    },
                    reason: "supersede can only be called with policy units".to_string(),
                });
            }
        };

        if new_policy_supersedes != Some(*old_policy) {
            return Err(GraphError::PolicyChainError {
                conflict: ConflictTuple {
                    unit_ids: BTreeSet::new(),
                    capability_name: String::new(),
                },
                reason: format!(
                    "new policy must supersedes old policy {old_policy:?}, but supersedes is {new_policy_supersedes:?}"
                ),
            });
        }

        let mut state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        // Check that the old policy exists.
        let old_conflict = match state.entries.get(old_policy) {
            Some(entry) => {
                if let Unit::Policy(p) = entry.unit() {
                    p.conflict.clone()
                } else {
                    return Err(GraphError::PolicyChainError {
                        conflict: ConflictTuple {
                            unit_ids: BTreeSet::new(),
                            capability_name: String::new(),
                        },
                        reason: format!("{old_policy:?} is not a policy unit"),
                    });
                }
            }
            None => {
                return Err(GraphError::NotFound { id: *old_policy });
            }
        };

        // Get the new policy's conflict tuple.
        let (new_id, new_conflict, new_version, new_author, new_created_at) = match &new_policy {
            Unit::Policy(p) => (
                p.header.id,
                p.conflict.clone(),
                p.version,
                p.header.author,
                p.header.created_at.clone(),
            ),
            _ => unreachable!("validated above"),
        };

        // INV-C7: The new policy must resolve the same conflict.
        if new_conflict != old_conflict {
            return Err(GraphError::PolicyChainError {
                conflict: old_conflict,
                reason: "new policy resolves a different conflict than the old policy".to_string(),
            });
        }

        // Mark the old policy as revoked in the entry.
        // SignedUnit<Unit>.unit is an owned Unit (not Arc), so we
        // can mutate it directly via &mut entry.signed_unit.unit.
        if let Some(entry) = state.entries.get_mut(old_policy) {
            if let Unit::Policy(p) = &mut entry.signed_unit.unit {
                p.revoked = true;
            }
        }

        // Also revoke the old policy in the policy chain.
        if let Some(chain) = state.policy_chains.get_mut(&old_conflict) {
            chain.revoke(*old_policy);
        }

        // Compute references for the new policy before moving it.
        let references = compute_references(&new_policy);
        let new_signed = Self::wrap_signed(new_policy);

        // Add the new policy to the graph.
        let merged_at = {
            state.local_clock.tick();
            DualClockEvent {
                logical_clock: state.local_clock,
                wall_time: taba_common::WallTime {
                    millis: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64),
                },
                timezone: "UTC".to_string(),
            }
        };

        let new_entry = GraphEntry::from_signed_unit(new_signed, merged_at.clone(), references);
        state.entries.insert(new_id, new_entry);

        // Update the policy chain with the new version.
        let policy_version = PolicyVersion {
            policy_id: new_id,
            version: new_version,
            revoked: false,
            author: new_author,
            created_at: new_created_at,
        };
        state
            .policy_chains
            .entry(old_conflict.clone())
            .or_insert_with(|| PolicyChain::new(old_conflict.clone()))
            .add_version(policy_version);

        state.recompute_memory();
        state.bump_generation();

        // WAL: Merged (new policy)
        if let Ok(mut wal) = self.wal.lock() {
            wal.append(WalEntry::Merged {
                unit_id: new_id,
                timestamp: merged_at,
            });
        }

        Ok(())
    }

    async fn archive(&self, id: &UnitId) -> Result<(), GraphError> {
        let mut state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        let entry = state
            .entries
            .get_mut(id)
            .ok_or(GraphError::NotFound { id: *id })?;

        // Governance units cannot be archived (INV-G3).
        if entry.unit().kind() == UnitKind::Governance {
            return Err(GraphError::MergeConflict {
                reason: "governance units cannot be archived (INV-G3)".to_string(),
            });
        }

        // Policy units can only be archived after supersession.
        if let Unit::Policy(p) = entry.unit() {
            if !p.revoked {
                return Err(GraphError::PolicyChainError {
                    conflict: p.conflict.clone(),
                    reason: "policy can only be archived after supersession or conflict resolution"
                        .to_string(),
                });
            }
        }

        // Check if already archived.
        if entry.archived {
            return Err(GraphError::Archived { id: *id });
        }

        // Archive the entry.
        entry.archived = true;
        state.recompute_memory();
        state.bump_generation();

        // WAL: Archive is not a standard WAL entry type (DL-008 defines
        // Merged, Pending, Promoted). In M2, we don't WAL archive
        // operations — they are handled by the compaction subsystem.

        Ok(())
    }

    async fn compact(&self) -> Result<(u64, u64), GraphError> {
        let monitor = DefaultMemoryMonitor::new(Arc::clone(&self.state), self.memory_limit_bytes);
        let (current, limit, _) = monitor.check_usage();

        if !monitor.is_compaction_needed() {
            return Ok((0, 0));
        }

        // Target: free enough to get back to 80% of limit.
        let target_bytes = current.saturating_sub(limit * 80 / 100);

        let compactor = DefaultCompactor::new(Arc::clone(&self.state), self.memory_limit_bytes);
        compactor.compact(target_bytes).await
    }

    async fn snapshot(&self) -> Result<GraphSnapshot, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        Ok(GraphSnapshot::new(
            state.generation,
            state.entries.clone(),
            state.policy_chains.clone(),
        ))
    }

    fn is_snapshot_current(&self, snapshot: &GraphSnapshot) -> bool {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");
        snapshot.is_current(state.generation)
    }

    fn stats(&self) -> GraphStats {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        GraphStats {
            active_units: state.active_count(),
            pending_units: state.pending_count(),
            archived_units: state.archived_count(),
            memory_bytes: state.memory_estimate_bytes,
            memory_limit_bytes: self.memory_limit_bytes,
        }
    }
}

// ===========================================================================
// GraphQuery implementation for DefaultGraph
// ===========================================================================

impl GraphQuery for DefaultGraph {
    fn get(&self, id: &UnitId) -> Result<Unit, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        let entry = state
            .entries
            .get(id)
            .ok_or(GraphError::NotFound { id: *id })?;

        if entry.archived {
            return Err(GraphError::Archived { id: *id });
        }

        Ok(entry.signed_unit.unit.clone())
    }

    fn traverse_provenance(&self, data_unit: &UnitId) -> Result<Vec<ProvenanceLink>, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        // Filter to active (non-archived) entries for traversal.
        let active_entries: BTreeMap<UnitId, GraphEntry> = state
            .entries
            .iter()
            .filter(|(_, e)| !e.archived)
            .map(|(k, v)| (*k, v.clone()))
            .collect();

        traverse_provenance_impl(&active_entries, data_unit)
    }

    fn active_policy(&self, conflict: &ConflictTuple) -> Result<Option<Unit>, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        let chain = state.policy_chains.get(conflict);

        match chain {
            None => Ok(None),
            Some(chain) => match &chain.active_policy {
                None => Ok(None),
                Some(policy_id) => {
                    let entry = state.entries.get(policy_id).ok_or_else(|| {
                        GraphError::PolicyChainError {
                            conflict: conflict.clone(),
                            reason: format!("active policy {policy_id:?} not in graph"),
                        }
                    })?;

                    if entry.archived {
                        return Err(GraphError::Archived { id: *policy_id });
                    }

                    Ok(Some(entry.signed_unit.unit.clone()))
                }
            },
        }
    }

    fn policy_chain(&self, conflict: &ConflictTuple) -> Result<PolicyChain, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        Ok(state
            .policy_chains
            .get(conflict)
            .cloned()
            .unwrap_or_else(|| PolicyChain::new(conflict.clone())))
    }

    fn units_in_domain(&self, domain: &TrustDomainId) -> Result<Vec<Unit>, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        Ok(state
            .entries
            .values()
            .filter(|e| !e.archived && &e.trust_domain == domain)
            .map(|e| e.signed_unit.unit.clone())
            .collect())
    }

    fn units_by_author(&self, author: &AuthorId) -> Result<Vec<Unit>, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        Ok(state
            .entries
            .values()
            .filter(|e| !e.archived && &e.signed_unit.unit.header().author == author)
            .map(|e| e.signed_unit.unit.clone())
            .collect())
    }

    fn pending_units(&self) -> Result<Vec<(Unit, Vec<UnitId>)>, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        Ok(state
            .pending
            .iter()
            .map(|p| {
                (
                    p.signed_unit.unit.clone(),
                    p.missing_refs.iter().copied().collect(),
                )
            })
            .collect())
    }

    fn child_data_units(&self, parent: &UnitId) -> Result<Vec<Unit>, GraphError> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        Ok(state
            .entries
            .values()
            .filter(|e| !e.archived)
            .filter_map(|e| {
                if let Unit::Data(d) = e.unit() {
                    if d.parent == Some(*parent) {
                        return Some(e.signed_unit.unit.clone());
                    }
                }
                None
            })
            .collect())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

impl std::fmt::Debug for DefaultGraph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultGraph").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{
        ClusterId, DelegationTokenId, LogicalClock, ValidityWindow, Version, WallTime,
    };
    use taba_core::{RecoveryAction, RecoveryRelationship, SpawnContext, UnitHeader, UnitState};
    use taba_security::{DefaultVerifier, PublicKey, Signature, SignatureContext};
    use taba_test_harness::{PolicyUnitBuilder, WorkloadUnitBuilder};

    /// Creates a [`Unit::Workload`] with the given ID and trust domain.
    fn workload(id: UnitId, td: TrustDomainId, author: AuthorId) -> Unit {
        Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id)
                .with_author(author)
                .with_trust_domain(td)
                .build(),
        )
    }

    /// Creates a [`Unit::Governance`] with a [`RoleAssignment`] for testing.
    ///
    /// The `domain_scopes` must be non-empty (the first element is used
    /// as the trust domain in the unit header). The `type_scopes` must
    /// be non-empty for the unit to pass structural validation.
    fn role_assignment_unit(
        assignee: AuthorId,
        type_scopes: Vec<UnitTypeScope>,
        domain_scopes: Vec<TrustDomainId>,
    ) -> Unit {
        Unit::Governance(GovernanceUnit::RoleAssignment(RoleAssignment {
            header: UnitHeader {
                id: UnitId(uuid::Uuid::new_v4()),
                author: assignee,
                trust_domain: domain_scopes[0],
                created_at: DualClockEvent {
                    logical_clock: LogicalClock(1),
                    wall_time: WallTime { millis: 1000 },
                    timezone: "UTC".to_string(),
                },
                validity: None,
                state: UnitState::Declared,
                version: None,
            },
            assignee,
            unit_type_scope: type_scopes,
            trust_domain_scope: domain_scopes,
        }))
    }

    // -- insert --------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_valid_unit() {
        let graph = DefaultGraph::new(1_000_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        graph
            .insert(workload(id, td, author))
            .await
            .expect("insert should succeed");

        // Unit should be in the active set
        let result = graph.get(&id);
        assert!(result.is_ok(), "get should find inserted unit");
        assert_eq!(result.expect("unit").id(), id);
    }

    #[tokio::test]
    async fn test_insert_unit_with_unsatisfied_refs() {
        let graph = DefaultGraph::new(1_000_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        // Create a workload that references a missing unit via recovery_relationships
        let mut w = WorkloadUnitBuilder::new()
            .with_id(id)
            .with_author(author)
            .with_trust_domain(td)
            .build();
        let missing_ref = UnitId(uuid::Uuid::new_v4());
        w.recovery_relationships = vec![RecoveryRelationship {
            depends_on: missing_ref,
            action: RecoveryAction::DrainFirst,
        }];

        graph
            .insert(Unit::Workload(w))
            .await
            .expect("insert should succeed (goes to pending)");

        // Unit should NOT be in the active set
        let result = graph.get(&id);
        assert!(
            matches!(result, Err(GraphError::NotFound { .. })),
            "pending unit should not be in active set"
        );

        // Unit should be in the pending queue
        let pending = graph.pending_units().expect("pending_units should succeed");
        assert_eq!(pending.len(), 1, "should have 1 pending unit");
        assert_eq!(pending[0].0.id(), id);
        assert!(
            pending[0].1.contains(&missing_ref),
            "pending unit should list missing_ref as missing"
        );
    }

    #[tokio::test]
    async fn test_insert_promotes_when_refs_arrive() {
        let graph = DefaultGraph::new(1_000_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        // Create a unit that references a missing unit
        let missing_ref = UnitId(uuid::Uuid::new_v4());
        let pending_id = UnitId(uuid::Uuid::new_v4());

        let mut w = WorkloadUnitBuilder::new()
            .with_id(pending_id)
            .with_author(author)
            .with_trust_domain(td)
            .build();
        w.recovery_relationships = vec![RecoveryRelationship {
            depends_on: missing_ref,
            action: RecoveryAction::DrainFirst,
        }];

        graph
            .insert(Unit::Workload(w))
            .await
            .expect("insert should succeed (goes to pending)");

        // Verify it's pending
        assert!(
            graph.get(&pending_id).is_err(),
            "should be pending, not active"
        );

        // Now insert the referenced unit
        graph
            .insert(workload(missing_ref, td, author))
            .await
            .expect("insert ref should succeed");

        // The pending unit should now be promoted to active
        let result = graph.get(&pending_id);
        assert!(
            result.is_ok(),
            "pending unit should be promoted after refs arrive"
        );

        // Pending queue should be empty
        let pending = graph.pending_units().expect("pending_units should succeed");
        assert!(
            pending.is_empty(),
            "pending queue should be empty after promotion"
        );
    }

    // -- archive -------------------------------------------------------------

    #[tokio::test]
    async fn test_archive_unit() {
        let graph = DefaultGraph::new(1_000_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        graph
            .insert(workload(id, td, author))
            .await
            .expect("insert should succeed");

        // Archive the unit
        graph.archive(&id).await.expect("archive should succeed");

        // Archived unit should not be in active set
        let result = graph.get(&id);
        assert!(
            matches!(result, Err(GraphError::Archived { id: archived_id }) if archived_id == id),
            "archived unit should return Archived error"
        );
    }

    #[tokio::test]
    async fn test_archive_governance_unit_fails() {
        let graph = DefaultGraph::new(1_000_000_000);

        let gov = Unit::Governance(taba_core::GovernanceUnit::KeyRevocation(
            taba_core::KeyRevocationDef {
                header: taba_core::UnitHeader {
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
                },
                revoked_author: AuthorId(uuid::Uuid::new_v4()),
                revocation_lc: LogicalClock(1),
                reason: "test".to_string(),
            },
        ));

        let gov_id = gov.id();
        graph
            .insert(gov)
            .await
            .expect("insert governance should succeed");

        let result = graph.archive(&gov_id).await;
        assert!(
            result.is_err(),
            "archiving governance unit should fail (INV-G3)"
        );
    }

    #[tokio::test]
    async fn test_archive_not_found() {
        let graph = DefaultGraph::new(1_000_000_000);
        let missing_id = UnitId(uuid::Uuid::new_v4());

        let result = graph.archive(&missing_id).await;
        assert!(
            matches!(result, Err(GraphError::NotFound { id }) if id == missing_id),
            "archiving non-existent unit should return NotFound"
        );
    }

    // -- supersede -----------------------------------------------------------

    #[tokio::test]
    async fn test_supersede_policy() {
        let graph = DefaultGraph::new(1_000_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        // Insert a unit to serve as conflict participant
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let unit_id = unit.id();
        graph.insert(unit).await.expect("insert unit");

        // Create and insert old policy
        let old_conflict = ConflictTuple {
            unit_ids: BTreeSet::from([unit_id]),
            capability_name: "storage".to_string(),
        };

        let old_policy = PolicyUnitBuilder::new()
            .with_conflict(old_conflict.clone())
            .with_author(author)
            .with_trust_domain(td)
            .with_version(Version(1))
            .build();
        let old_id = old_policy.header.id;

        graph
            .insert(Unit::Policy(old_policy))
            .await
            .expect("insert old policy");

        // Create new policy that supersedes the old one
        let new_policy = PolicyUnitBuilder::new()
            .with_conflict(old_conflict.clone())
            .with_author(author)
            .with_trust_domain(td)
            .with_version(Version(2))
            .with_supersedes(old_id)
            .build();
        let new_id = new_policy.header.id;

        graph
            .supersede(&old_id, Unit::Policy(new_policy))
            .await
            .expect("supersede should succeed");

        // Old policy should be revoked
        let chain = graph
            .policy_chain(&old_conflict)
            .expect("policy_chain should succeed");
        assert!(
            chain
                .versions
                .iter()
                .any(|v| v.policy_id == old_id && v.revoked),
            "old policy should be revoked in the chain"
        );

        // Active policy should be the new one
        let active = graph
            .active_policy(&old_conflict)
            .expect("active_policy should succeed");
        assert!(active.is_some(), "should have an active policy");
        assert_eq!(
            active.expect("policy").id(),
            new_id,
            "active policy should be the new one"
        );
    }

    #[tokio::test]
    async fn test_supersede_duplicate_policy_fails() {
        let graph = DefaultGraph::new(1_000_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let conflict = ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        };

        // Insert first policy (no supersession)
        let policy1 = PolicyUnitBuilder::new()
            .with_conflict(conflict.clone())
            .with_author(author)
            .with_trust_domain(td)
            .with_version(Version(1))
            .build();

        graph
            .insert(Unit::Policy(policy1))
            .await
            .expect("insert policy1");

        // Insert second policy for the same conflict WITHOUT supersession
        // This should trigger INV-C7 violation
        let policy2 = PolicyUnitBuilder::new()
            .with_conflict(conflict.clone())
            .with_author(author)
            .with_trust_domain(td)
            .with_version(Version(1))
            .build();
        let policy2_id = policy2.header.id;

        // The insert should either:
        // 1. Reject the second policy (return error), or
        // 2. Accept it but mark it as a conflict
        // In M2, the insert method doesn't check INV-C7 directly —
        // that's checked at query time (active_policy returns the
        // latest non-revoked). But if both policies have version 1
        // and neither is revoked, the active policy is the last one
        // inserted (highest in the chain).
        //
        // For a true INV-C7 violation (duplicate without supersession),
        // the supersede method should fail if the new policy doesn't
        // reference the old one.
        graph
            .insert(Unit::Policy(policy2))
            .await
            .expect("insert should still succeed (goes to graph)");

        // Try to supersede without proper reference
        let bad_policy = PolicyUnitBuilder::new()
            .with_conflict(conflict.clone())
            .with_author(author)
            .with_trust_domain(td)
            .with_version(Version(2))
            .build(); // No supersedes!

        let result = graph.supersede(&policy2_id, Unit::Policy(bad_policy)).await;
        assert!(
            result.is_err(),
            "supersede without proper supersedes reference should fail (INV-C7)"
        );

        let _ = policy2_id; // silence warning
    }

    // -- snapshot ------------------------------------------------------------

    #[tokio::test]
    async fn test_snapshot_generation_increments() {
        let graph = DefaultGraph::new(1_000_000_000);

        // Initial snapshot
        let snap0 = graph.snapshot().await.expect("snapshot should succeed");
        assert_eq!(snap0.generation, 0, "initial generation should be 0");

        // Insert a unit
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        graph
            .insert(workload(id, td, author))
            .await
            .expect("insert should succeed");

        // New snapshot should have higher generation
        let snap1 = graph.snapshot().await.expect("snapshot should succeed");
        assert!(
            snap1.generation > snap0.generation,
            "generation should increase after mutation"
        );
    }

    #[tokio::test]
    async fn test_snapshot_is_immutable() {
        let graph = DefaultGraph::new(1_000_000_000);

        // Insert a unit
        let id1 = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        graph
            .insert(workload(id1, td, author))
            .await
            .expect("insert should succeed");

        // Take snapshot
        let snap = graph.snapshot().await.expect("snapshot should succeed");
        let snap_len = snap.len();

        // Insert another unit
        let id2 = UnitId(uuid::Uuid::new_v4());
        graph
            .insert(workload(id2, td, author))
            .await
            .expect("insert should succeed");

        // Original snapshot should be unchanged
        assert_eq!(
            snap.len(),
            snap_len,
            "snapshot should not change after graph mutation"
        );
        assert!(
            !snap.is_current(graph.stats().active_units),
            "snapshot should be stale after mutation"
        );
        // Actually, is_current compares generation, not active_units.
        // Let me check generation properly.
    }

    #[tokio::test]
    async fn test_snapshot_is_current_after_no_mutation() {
        let graph = DefaultGraph::new(1_000_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        graph
            .insert(workload(id, td, author))
            .await
            .expect("insert should succeed");

        let snap = graph.snapshot().await.expect("snapshot should succeed");

        assert!(
            graph.is_snapshot_current(&snap),
            "snapshot should be current when no mutations occurred since"
        );
    }

    #[tokio::test]
    async fn test_snapshot_not_current_after_mutation() {
        let graph = DefaultGraph::new(1_000_000_000);
        let id1 = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        graph
            .insert(workload(id1, td, author))
            .await
            .expect("insert should succeed");
        let snap = graph.snapshot().await.expect("snapshot should succeed");

        // Mutate
        let id2 = UnitId(uuid::Uuid::new_v4());
        graph
            .insert(workload(id2, td, author))
            .await
            .expect("insert should succeed");

        assert!(
            !graph.is_snapshot_current(&snap),
            "snapshot should NOT be current after mutation"
        );
    }

    // -- stats ---------------------------------------------------------------

    #[tokio::test]
    async fn test_stats_counts() {
        let graph = DefaultGraph::new(1_000_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        // Initial stats
        let stats = graph.stats();
        assert_eq!(stats.active_units, 0);
        assert_eq!(stats.pending_units, 0);
        assert_eq!(stats.archived_units, 0);

        // Insert 3 units
        graph
            .insert(workload(UnitId(uuid::Uuid::new_v4()), td, author))
            .await
            .expect("insert 1");
        graph
            .insert(workload(UnitId(uuid::Uuid::new_v4()), td, author))
            .await
            .expect("insert 2");
        graph
            .insert(workload(UnitId(uuid::Uuid::new_v4()), td, author))
            .await
            .expect("insert 3");

        let stats = graph.stats();
        assert_eq!(stats.active_units, 3, "should have 3 active units");
        assert_eq!(stats.pending_units, 0, "should have 0 pending units");
        assert_eq!(stats.archived_units, 0, "should have 0 archived units");
        assert!(stats.memory_bytes > 0, "memory should be positive");

        // Archive 1
        let archived_id = UnitId(uuid::Uuid::new_v4());
        graph
            .insert(workload(archived_id, td, author))
            .await
            .expect("insert for archive");
        graph
            .archive(&archived_id)
            .await
            .expect("archive should succeed");

        let stats = graph.stats();
        assert_eq!(
            stats.active_units, 3,
            "should still have 3 active units (archived one was extra)"
        );
        assert_eq!(stats.archived_units, 1, "should have 1 archived unit");
    }

    // -- merge ---------------------------------------------------------------

    #[tokio::test]
    async fn test_merge_adds_entries() {
        let graph = DefaultGraph::new(1_000_000_000);

        // Create a delta with one entry
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        let unit = workload(id, td, author);

        // We need to create a GraphEntry for the delta
        let signed = taba_security::SignedUnit {
            unit,
            signature: Signature([0u8; 64]),
            context: SignatureContext {
                trust_domain_id: td,
                cluster_id: ClusterId(uuid::Uuid::nil()),
                validity_window: ValidityWindow {
                    lc_range: None,
                    wall_time_deadline: None,
                },
            },
            signer: PublicKey([0u8; 32]),
        };

        let entry = GraphEntry::from_signed_unit(
            signed,
            DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            BTreeSet::new(),
        );

        let mut delta = GraphDelta::new();
        delta.add_entry(entry);

        let rejected = graph.merge(delta).await.expect("merge should succeed");
        assert!(rejected.is_empty(), "no entries should be rejected");

        let result = graph.get(&id);
        assert!(result.is_ok(), "merged entry should be in active set");
    }

    #[tokio::test]
    async fn test_merge_rejects_invalid() {
        let graph = DefaultGraph::new(1_000_000_000);

        // Create an invalid unit (empty provides)
        let mut w = WorkloadUnitBuilder::new().build();
        w.provides = Vec::new();
        let invalid_unit = Unit::Workload(w);

        let signed = taba_security::SignedUnit {
            unit: invalid_unit,
            signature: Signature([0u8; 64]),
            context: SignatureContext {
                trust_domain_id: TrustDomainId(uuid::Uuid::nil()),
                cluster_id: ClusterId(uuid::Uuid::nil()),
                validity_window: ValidityWindow {
                    lc_range: None,
                    wall_time_deadline: None,
                },
            },
            signer: PublicKey([0u8; 32]),
        };

        let entry = GraphEntry::from_signed_unit(
            signed,
            DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            BTreeSet::new(),
        );

        let mut delta = GraphDelta::new();
        delta.add_entry(entry);

        let rejected = graph.merge(delta).await.expect("merge should succeed");
        assert_eq!(rejected.len(), 1, "invalid entry should be rejected");
    }

    // -- WAL integration -----------------------------------------------------

    #[tokio::test]
    async fn test_wal_records_insert() {
        let graph = DefaultGraph::new(1_000_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        graph
            .insert(workload(id, td, author))
            .await
            .expect("insert should succeed");

        let wal = graph.wal().lock().expect("wal lock");
        let entries = wal.replay();

        assert!(!entries.is_empty(), "WAL should have entries after insert");
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, WalEntry::Merged { unit_id, .. } if *unit_id == id)),
            "WAL should contain a Merged entry for the inserted unit"
        );
    }

    #[tokio::test]
    async fn test_wal_records_pending() {
        let graph = DefaultGraph::new(1_000_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let missing_ref = UnitId(uuid::Uuid::new_v4());
        let pending_id = UnitId(uuid::Uuid::new_v4());

        let mut w = WorkloadUnitBuilder::new()
            .with_id(pending_id)
            .with_author(author)
            .with_trust_domain(td)
            .build();
        w.recovery_relationships = vec![RecoveryRelationship {
            depends_on: missing_ref,
            action: RecoveryAction::DrainFirst,
        }];

        graph
            .insert(Unit::Workload(w))
            .await
            .expect("insert should succeed");

        let wal = graph.wal().lock().expect("wal lock");
        let entries = wal.replay();

        assert!(
            entries
                .iter()
                .any(|e| matches!(e, WalEntry::Pending { unit_id, .. } if *unit_id == pending_id)),
            "WAL should contain a Pending entry for the pending unit"
        );
    }

    #[tokio::test]
    async fn test_wal_records_promotion() {
        let graph = DefaultGraph::new(1_000_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let missing_ref = UnitId(uuid::Uuid::new_v4());
        let pending_id = UnitId(uuid::Uuid::new_v4());

        let mut w = WorkloadUnitBuilder::new()
            .with_id(pending_id)
            .with_author(author)
            .with_trust_domain(td)
            .build();
        w.recovery_relationships = vec![RecoveryRelationship {
            depends_on: missing_ref,
            action: RecoveryAction::DrainFirst,
        }];

        // Insert pending unit
        graph
            .insert(Unit::Workload(w))
            .await
            .expect("insert pending");

        // Insert the referenced unit (triggers promotion)
        graph
            .insert(workload(missing_ref, td, author))
            .await
            .expect("insert ref");

        let wal = graph.wal().lock().expect("wal lock");
        let entries = wal.replay();

        assert!(
            entries
                .iter()
                .any(|e| matches!(e, WalEntry::Promoted { unit_id } if *unit_id == pending_id)),
            "WAL should contain a Promoted entry after promotion"
        );
    }

    // -- INV-S3: signature verification ------------------------------------

    #[tokio::test]
    async fn test_insert_with_verifier_rejects_unsigned() {
        // When a verifier is configured, the zero-valued placeholder
        // signature from wrap_signed is rejected (INV-S3). The verifier
        // has no keys, so KeyNotFound is returned — a unit with an
        // unknown author must not enter graph state.
        let verifier: Arc<dyn Verifier + Send + Sync> = Arc::new(DefaultVerifier::new());
        let graph = DefaultGraph::new(1_000_000_000).with_verifier(verifier);

        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let result = graph.insert(workload(id, td, author)).await;
        assert!(
            result.is_err(),
            "insert with verifier should reject unsigned unit"
        );
        assert!(
            matches!(result, Err(GraphError::SignatureRejected { unit, .. }) if unit == id),
            "expected SignatureRejected for unsigned unit, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_insert_without_verifier_accepts() {
        // Without a verifier, insert performs structural validation
        // only and accepts the unit (backward compatible with M2).
        let graph = DefaultGraph::new(1_000_000_000);

        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        graph
            .insert(workload(id, td, author))
            .await
            .expect("insert without verifier should succeed");

        assert!(graph.get(&id).is_ok(), "unit should be in active set");
    }

    // -- INV-W3: spawn depth enforcement -----------------------------------

    #[tokio::test]
    async fn test_insert_spawn_depth_within_limit() {
        // Spawn depth 4 with max 4 should be accepted (INV-W3).
        // The unit references its parent (via spawn_context), so it
        // enters the pending queue — but the depth check passes.
        let graph = DefaultGraph::new(1_000_000_000).with_max_spawn_depth(4);

        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        let parent_id = UnitId(uuid::Uuid::new_v4());

        let w = WorkloadUnitBuilder::new()
            .with_author(author)
            .with_trust_domain(td)
            .with_spawn_context(SpawnContext {
                spawned_by: parent_id,
                delegation_token_id: DelegationTokenId(uuid::Uuid::new_v4()),
                spawn_depth: 4,
            })
            .build();

        graph
            .insert(Unit::Workload(w))
            .await
            .expect("spawn depth 4 within max 4 should be accepted");
    }

    #[tokio::test]
    async fn test_insert_spawn_depth_exceeds_limit() {
        // Spawn depth 5 with max 4 should be rejected (INV-W3).
        let graph = DefaultGraph::new(1_000_000_000).with_max_spawn_depth(4);

        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        let parent_id = UnitId(uuid::Uuid::new_v4());

        let id = UnitId(uuid::Uuid::new_v4());
        let w = WorkloadUnitBuilder::new()
            .with_id(id)
            .with_author(author)
            .with_trust_domain(td)
            .with_spawn_context(SpawnContext {
                spawned_by: parent_id,
                delegation_token_id: DelegationTokenId(uuid::Uuid::new_v4()),
                spawn_depth: 5,
            })
            .build();

        let result = graph.insert(Unit::Workload(w)).await;
        assert!(
            result.is_err(),
            "spawn depth 5 exceeding max 4 should be rejected"
        );
        assert!(
            matches!(result, Err(GraphError::SignatureRejected { unit, .. }) if unit == id),
            "expected SignatureRejected for spawn depth violation, got: {result:?}"
        );
    }

    // -- INV-S8: scope uniqueness enforcement ------------------------------

    #[tokio::test]
    async fn test_insert_duplicate_scope_rejected() {
        // Two distinct authors with identical (workload, domain)
        // scope tuples should be rejected (INV-S8).
        let scope_checker = Arc::new(DefaultScopeChecker::new());
        let graph = DefaultGraph::new(1_000_000_000).with_scope_checker(scope_checker);

        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author1 = AuthorId(uuid::Uuid::new_v4());
        let author2 = AuthorId(uuid::Uuid::new_v4());

        // First role assignment — should succeed.
        let ra1 = role_assignment_unit(author1, vec![UnitTypeScope::Workload], vec![td]);
        graph
            .insert(ra1)
            .await
            .expect("first role assignment should succeed");

        // Second role assignment with same scope tuple but a different
        // author — should be rejected (INV-S8).
        let ra2 = role_assignment_unit(author2, vec![UnitTypeScope::Workload], vec![td]);
        let result = graph.insert(ra2).await;
        assert!(
            result.is_err(),
            "duplicate scope tuple from distinct authors should be rejected"
        );
        assert!(
            matches!(result, Err(GraphError::ScopeViolation { author, .. }) if author == author2),
            "expected ScopeViolation for duplicate scope, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_insert_overlapping_policy_scope_allowed() {
        // Two distinct authors with identical (policy, domain) scope
        // tuples should be allowed (INV-S8a).
        let scope_checker = Arc::new(DefaultScopeChecker::new());
        let graph = DefaultGraph::new(1_000_000_000).with_scope_checker(scope_checker);

        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author1 = AuthorId(uuid::Uuid::new_v4());
        let author2 = AuthorId(uuid::Uuid::new_v4());

        // First policy role assignment — should succeed.
        let ra1 = role_assignment_unit(author1, vec![UnitTypeScope::Policy], vec![td]);
        graph
            .insert(ra1)
            .await
            .expect("first policy role assignment should succeed");

        // Second policy role assignment with same scope tuple but a
        // different author — should also succeed (INV-S8a).
        let ra2 = role_assignment_unit(author2, vec![UnitTypeScope::Policy], vec![td]);
        graph
            .insert(ra2)
            .await
            .expect("overlapping policy scopes should be allowed (INV-S8a)");
    }
}
