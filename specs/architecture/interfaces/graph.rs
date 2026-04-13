// taba-graph: CRDT composition graph — the single source of desired state.
//
// This crate implements the distributed graph that holds all units and their
// relationships. It owns merge semantics (CRDT), causal buffering, policy
// supersession, and query operations. The graph is sharded via erasure coding
// (taba-erasure) and synchronized via gossip (taba-gossip).

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct UnitId(/* opaque */);
pub struct Unit(/* opaque */);
pub struct AuthorId(/* opaque */);
pub struct TrustDomainId(/* opaque */);
pub struct Capability(/* opaque */);
pub struct Classification(/* opaque */);

/// A delta (set of changes) to be merged into the graph.
/// Contains one or more units and their relationships.
pub struct GraphDelta(/* opaque */);

/// Consistent point-in-time snapshot of the graph.
///
/// Immutable — concurrent mutations do not affect it. Includes a generation
/// counter to detect staleness. The solver should check `is_current()` before
/// applying placement results.
///
/// IMMUTABILITY CONTRACT (A006): GraphSnapshot is wrapped in Arc<> at
/// creation time. Once taken, the snapshot is frozen — concurrent graph
/// mutations do not affect it. The solver holds an Arc<GraphSnapshot>
/// and can safely compute placements while the graph continues to receive
/// merges. Type defined in taba-core (A009). Implemented by taba-graph.
pub struct GraphSnapshot {
    /// Monotonically increasing generation counter. Incremented on every
    /// mutation (insert, merge, supersede, archive, compact).
    pub generation: u64,
    /* ... opaque internal state ... */
}

/// A policy supersession chain: ordered sequence of policy units for a
/// single conflict tuple, from oldest to newest.
pub struct PolicyChain(/* opaque */);

/// The conflict tuple: set of unit IDs + capability name that a policy resolves.
pub struct ConflictTuple(/* opaque */);

/// Provenance link: which unit produced which data unit, from what inputs.
pub struct ProvenanceLink {
    pub producer: UnitId,
    pub inputs: Vec<UnitId>,
    pub output: UnitId,
    /// Dual clock event (INV-T2). Logical clock for ordering, wall time for display.
    pub timestamp: DualClockEvent,
}

pub struct DualClockEvent(/* from taba-common */);

/// Statistics about graph size and health.
pub struct GraphStats {
    pub active_units: u64,
    pub pending_units: u64,
    pub archived_units: u64,
    pub memory_bytes: u64,
    pub memory_limit_bytes: u64,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum GraphError {
    /// Signature verification failed (synchronous gate per INV-S3).
    SignatureRejected { unit: UnitId, reason: String },
    /// Unit references entities not yet in the graph (enters pending queue).
    UnsatisfiedReferences { unit: UnitId, missing: Vec<UnitId> },
    /// Merge would violate CRDT properties.
    MergeConflict { reason: String },
    /// Unit not found in the graph.
    NotFound { id: UnitId },
    /// Policy supersession chain is broken or ambiguous.
    PolicyChainError { conflict: ConflictTuple, reason: String },
    /// Graph memory limit exceeded (INV-R6). Node should enter degraded mode.
    MemoryLimitExceeded { used: u64, limit: u64 },
    /// WAL or storage persistence error.
    PersistenceError { reason: String },
    /// Query references an archived or compacted unit.
    Archived { id: UnitId },
    /// Inserting this unit would create a cyclic recovery dependency (INV-K5).
    WouldCreateCycle { unit: UnitId, cycle: Vec<UnitId> },
    /// Compaction failed (partially or fully).
    CompactionFailed { units_attempted: u64, units_failed: u64 },
    /// Author scope violation (INV-S5, INV-S8).
    ScopeViolation { author: AuthorId, reason: String },
    /// Declassification policy missing required multi-party signatures (INV-S9).
    DeclassificationDenied { policy: UnitId, reason: String },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Core graph operations: insert, merge, supersede, archive, compact.
///
/// The graph is the single source of desired state (INV-C1). All mutations
/// go through this trait. Signature verification is a synchronous gate
/// before any unit enters graph state (INV-S3).
pub trait Graph {
    /// Insert a unit into the graph.
    ///
    /// Precondition: the unit's signature MUST be verified by
    /// `security::Verifier` before calling this. The graph performs a final
    /// signature check as a defense-in-depth measure.
    ///
    /// If the unit's references are all satisfied, it enters the active set.
    /// If references are unsatisfied, it enters the pending queue (causal
    /// buffering per INV-C4). When a pending unit's references are later
    /// satisfied, it is automatically promoted.
    ///
    /// WAL-before-effect: the insertion is fsync'd to WAL before becoming
    /// visible to queries (INV-C4). The WAL write guarantees durability —
    /// method returns only after fsync() or equivalent.
    ///
    /// For policy units: validates INV-C7 (no duplicate non-revoked policy
    /// for the same conflict tuple) and INV-S9 (declassification policies
    /// require multi-party signing) before accepting.
    ///
    /// Returns `GraphError::SignatureRejected` if the defense-in-depth check
    /// fails. Returns `GraphError::MemoryLimitExceeded` if the graph is at
    /// capacity (triggers degraded mode per INV-R6).
    /// Returns `GraphError::PolicyChainError` if a duplicate policy exists.
    /// Returns `GraphError::WouldCreateCycle` if recovery deps would cycle.
    async fn insert(&self, unit: Unit) -> Result<(), GraphError>;

    /// Merge a remote delta into the local graph.
    ///
    /// The merge is commutative, associative, and idempotent (INV-C2). Each
    /// unit in the delta is individually verified before merge. Units that
    /// fail verification are rejected without affecting the rest of the delta.
    ///
    /// Returns a list of units that were rejected (if any), with reasons.
    /// An empty list means the entire delta merged successfully.
    async fn merge(&self, delta: GraphDelta) -> Result<Vec<(UnitId, GraphError)>, GraphError>;

    /// Supersede a policy unit with a new version.
    ///
    /// Enforces INV-C7: only one non-revoked policy per conflict tuple.
    /// The new policy must explicitly reference the old policy's ID in its
    /// `supersedes` field. The old policy is marked revoked. The supersession
    /// chain is immutable.
    ///
    /// Returns `GraphError::PolicyChainError` if the supersession is invalid
    /// (wrong conflict tuple, chain is broken, etc.).
    async fn supersede(
        &self,
        old_policy: &UnitId,
        new_policy: Unit,
    ) -> Result<(), GraphError>;

    /// Archive a unit (soft-delete). The unit is removed from the active set
    /// but retained for historical queries and provenance integrity.
    ///
    /// Governance units cannot be archived (they are permanent). Policy units
    /// can only be archived after they are superseded or their conflict tuple
    /// no longer exists.
    async fn archive(&self, id: &UnitId) -> Result<(), GraphError>;

    /// Compact the graph by removing archived units whose retention has expired.
    ///
    /// Respects data unit retention declarations (INV-D2). Auto-triggers at
    /// 80% of memory limit (INV-R6). Provenance links to compacted units are
    /// replaced with tombstone markers.
    ///
    /// Returns the number of units compacted and bytes freed.
    async fn compact(&self) -> Result<(u64, u64), GraphError>;

    /// Take a consistent point-in-time snapshot for the solver.
    ///
    /// The snapshot is immutable — concurrent mutations do not affect it.
    /// Used by `solver::Solver::solve` as input. Includes a generation
    /// counter for staleness detection.
    ///
    /// Snapshot is taken AFTER all pending signature verifications complete —
    /// no unverified units are included.
    async fn snapshot(&self) -> Result<GraphSnapshot, GraphError>;

    /// Check whether a snapshot is still current (no mutations since it was taken).
    fn is_snapshot_current(&self, snapshot: &GraphSnapshot) -> bool;

    /// Return current graph statistics (unit counts, memory usage).
    fn stats(&self) -> GraphStats;
}

/// Read-only query operations over the graph.
///
/// Separated from `Graph` to allow read-only access patterns (e.g., the
/// solver only needs `GraphQuery`, not mutation). All queries operate over
/// the active set unless explicitly requesting archived or pending units.
pub trait GraphQuery {
    /// Retrieve a single unit by ID.
    ///
    /// Returns `GraphError::NotFound` if the unit is not in the active set.
    /// Returns `GraphError::Archived` if the unit was archived (caller can
    /// decide whether to accept archived units).
    fn get(&self, id: &UnitId) -> Result<Unit, GraphError>;

    /// Traverse the provenance chain of a data unit.
    ///
    /// Returns the full provenance path: all producing workloads and their
    /// input data units, recursively, back to source data units with no
    /// provenance (root data). Used by `security::TaintComputer`.
    ///
    /// Enforces INV-D1: provenance chain must be unbroken. Returns
    /// `GraphError::NotFound` if any link in the chain references a unit
    /// not in the local graph (may be pending causal delivery).
    fn traverse_provenance(&self, data_unit: &UnitId) -> Result<Vec<ProvenanceLink>, GraphError>;

    /// Get the active (non-revoked) policy for a conflict tuple.
    ///
    /// Enforces INV-C7: returns exactly zero or one policy. If the
    /// supersession chain is broken, returns `GraphError::PolicyChainError`.
    ///
    /// Orphaned policies (referencing non-existent conflicts) are detected
    /// here and flagged for archival (INV-C5).
    fn active_policy(&self, conflict: &ConflictTuple) -> Result<Option<Unit>, GraphError>;

    /// Get the full supersession chain for a conflict tuple.
    ///
    /// Returns all policy versions in order (oldest to newest), including
    /// revoked ones. Used for audit trails.
    fn policy_chain(&self, conflict: &ConflictTuple) -> Result<PolicyChain, GraphError>;

    /// List all units within a trust domain.
    fn units_in_domain(&self, domain: &TrustDomainId) -> Result<Vec<Unit>, GraphError>;

    /// List all units authored by a specific author.
    fn units_by_author(&self, author: &AuthorId) -> Result<Vec<Unit>, GraphError>;

    /// Find all pending units and their missing references.
    fn pending_units(&self) -> Result<Vec<(Unit, Vec<UnitId>)>, GraphError>;

    /// Find all data units that are children of a given parent.
    ///
    /// Returns only direct children (one level). Used for validating
    /// hierarchical constraint inheritance (INV-S7).
    fn child_data_units(&self, parent: &UnitId) -> Result<Vec<Unit>, GraphError>;
}

/// CRDT merge semantics. Defines how conflicting states are resolved.
///
/// Any implementation MUST satisfy INV-C2:
/// - Commutative: merge(A, B) == merge(B, A)
/// - Associative: merge(merge(A, B), C) == merge(A, merge(B, C))
/// - Idempotent: merge(A, A) == A
///
/// These properties are load-bearing for partition tolerance (FM-03).
/// Property-based tests MUST verify all three.
pub trait MergePolicy {
    /// Merge two graph states, producing a new graph state.
    ///
    /// The unit identity tuple is (UnitId, Author, CreationTimestamp). Units
    /// with the same identity are deduplicated (idempotent). Units with
    /// different identities are unioned (commutative). Ordering does not
    /// matter (associative).
    ///
    /// Signature verification is a synchronous gate: every unit in both inputs
    /// must be verified before merge proceeds.
    ///
    /// Returns `GraphError::MergeConflict` only for true violations (e.g.,
    /// two different units claim the same UnitId — Byzantine). Normal
    /// capability conflicts are NOT merge conflicts; they are surfaced by
    /// the solver.
    fn merge(&self, local: &GraphSnapshot, remote: &GraphDelta) -> Result<GraphDelta, GraphError>;

    /// Check whether two graph states are equivalent after merge.
    ///
    /// Used in property tests to verify CRDT laws. Two states are equivalent
    /// if they contain the same set of unit identity tuples.
    fn is_equivalent(&self, a: &GraphSnapshot, b: &GraphSnapshot) -> bool;
}

// ---------------------------------------------------------------------------
// Memory monitoring and compaction (A024, INV-R6, INV-G1-G5)
// ---------------------------------------------------------------------------

/// Monitors graph memory usage and triggers compaction (INV-R6).
///
/// Auto-compaction at 80% of limit. Degraded mode at 100%.
pub trait MemoryMonitor {
    /// Check current memory usage against configured limit.
    ///
    /// Returns (current_bytes, limit_bytes, pressure_pct).
    fn check_usage(&self) -> (u64, u64, u8);

    /// Returns true if memory usage exceeds 80% of limit (compaction trigger).
    fn is_compaction_needed(&self) -> bool;

    /// Returns true if memory usage exceeds 100% of limit (degraded trigger).
    fn is_degraded_needed(&self) -> bool;
}

/// Compaction engine for the graph (INV-G1 through INV-G5).
///
/// Compaction eligibility is deterministic: same graph state = same eligible
/// units on all nodes (INV-G1). Tombstones preserve provenance (INV-G2).
/// Governance units are exempt (INV-G3). Priority order per INV-G5.
pub trait Compactor {
    /// Compute which units are eligible for compaction in the current graph.
    ///
    /// Deterministic: same graph state = same result on any node.
    fn compute_eligible(&self) -> Vec<(UnitId, CompactionAction)>;

    /// Execute compaction, freeing at least `target_bytes` of memory.
    ///
    /// Processes eligible units in priority order (INV-G5):
    /// ephemeral → trails → terminated tasks → superseded policies →
    /// terminated services → expired data.
    ///
    /// For ephemeral data: reference check (INV-D4) — no refs → remove,
    /// has refs → tombstone.
    ///
    /// For archival-mandated units: archive first, then tombstone (FM-21).
    /// Returns ArchivalFailed if archive backend unavailable and unit
    /// requires archival.
    ///
    /// Returns (units_compacted, bytes_freed).
    async fn compact(&self, target_bytes: u64) -> Result<(u64, u64), GraphError>;
}

/// Action for a compactable unit.
pub enum CompactionAction {
    /// Fully remove (no tombstone). For unreferenced ephemeral data.
    Remove,
    /// Replace with tombstone (preserves references).
    Tombstone,
    /// Archive to cold storage first, then tombstone.
    ArchiveAndTombstone,
}
