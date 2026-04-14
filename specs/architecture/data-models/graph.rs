//! Composition graph types: the CRDT, graph operations, merge semantics, and causal buffering.
//!
//! The composition graph is the single source of desired state (INV-C1).
//! It is a δ-state CRDT (delta-state, DL-012) -- merge is commutative,
//! associative, and idempotent (INV-C2). Specifically:
//!
//! - **Add-set**: signed units (grow-only, never removed from add-set)
//! - **Remove-set**: tombstones (grow-only, monotonic)
//! - **Policy chains**: versioned register per ConflictTuple, converges to
//!   highest-version non-revoked policy (Multi-Value Register)
//! - **Pending queue**: node-local, NOT replicated
//!
//! `GraphDelta` is a partial state (not an operation log). Merging deltas is
//! idempotent (like CvRDT) while shipping only changes (like CmRDT). All
//! graph operations are monotonic: inserts grow the add-set, compaction adds
//! to the remove-set. This satisfies INV-C2 by construction.
//!
//! Every entry is signed and verified before merge (INV-S3). Units with
//! unsatisfied references enter the pending queue for causal buffering (INV-C4).
//!
//! Phase 1-2: full graph per node, memory-bounded (INV-R6).
//! Phase 3+: trust domain sharding with cross-domain forwarding.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::common::{AuthorId, NodeId, ShardId, Timestamp, TrustDomainId, UnitId, Version};
use crate::core::{ConflictTuple, Unit, UnitState};
use crate::security::SignedUnit;

// ---------------------------------------------------------------------------
// Composition graph
// ---------------------------------------------------------------------------

/// The composition graph -- a CRDT containing all units and their relationships.
/// Distributed across all nodes via erasure coding. No masters, no etcd.
///
/// Identity of a graph entry: (UnitId, AuthorId, CreationTimestamp).
/// The graph is a set -- no duplicates with the same identity triple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionGraph {
    /// All active (merged) entries in the graph, keyed by UnitId.
    pub entries: BTreeMap<UnitId, GraphEntry>,
    /// Units verified but with unsatisfied references, awaiting causal delivery (INV-C4).
    pub pending: Vec<PendingEntry>,
    /// Policy chains keyed by their conflict tuple.
    /// Only one non-revoked policy per conflict tuple (INV-C7).
    pub policy_chains: BTreeMap<ConflictTuple, PolicyChain>,
    /// The local node's logical clock for ordering.
    pub local_clock: Timestamp,
    /// Memory usage estimate in bytes (for INV-R6 limit enforcement).
    pub memory_estimate_bytes: u64,
}

/// A single entry in the composition graph.
/// Wraps a signed unit with graph-level metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEntry {
    /// The signed unit payload.
    pub signed_unit: SignedUnit<Unit>,
    /// When this entry was merged into the local graph.
    pub merged_at: Timestamp,
    /// Incoming edges: units that reference this one.
    pub referenced_by: BTreeSet<UnitId>,
    /// Outgoing edges: units this one references.
    pub references: BTreeSet<UnitId>,
    /// Which trust domain this entry belongs to (for future sharding).
    pub trust_domain: TrustDomainId,
    /// Whether this entry has been archived (moved to cold storage).
    pub archived: bool,
}

// ---------------------------------------------------------------------------
// Graph operations
// ---------------------------------------------------------------------------

/// Operations that can be applied to the composition graph.
/// All operations are WAL'd before their effects become visible (INV-C4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GraphOp {
    /// Insert a new unit into the graph.
    /// Signature verified synchronously before insertion.
    Insert {
        signed_unit: SignedUnit<Unit>,
    },
    /// Compose two or more units through capability matching.
    /// Produces edges between matched units.
    Compose {
        /// The units being composed.
        unit_ids: BTreeSet<UnitId>,
        /// The resulting capability matches.
        matches: Vec<crate::core::CapabilityMatch>,
    },
    /// A policy unit supersedes a previous policy for the same conflict.
    /// Creates a versioned lineage chain (INV-C7).
    Supersede {
        /// The new policy unit.
        new_policy: UnitId,
        /// The policy being superseded.
        old_policy: UnitId,
    },
    /// Move a subgraph to cold storage while preserving provenance chain.
    Archive {
        /// Root units of the subgraph to archive.
        root_unit_ids: BTreeSet<UnitId>,
    },
}

// ---------------------------------------------------------------------------
// Merge and conflict resolution
// ---------------------------------------------------------------------------

/// Result of merging two graph states (CRDT merge).
/// Merge is commutative, associative, and idempotent (INV-C2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    /// Units that were newly added to this node's graph.
    pub new_entries: Vec<UnitId>,
    /// Pending entries that were promoted to active after references arrived.
    pub promoted: Vec<UnitId>,
    /// Entries that were already present (idempotent merge -- no-op).
    pub duplicates: Vec<UnitId>,
    /// Units that failed verification and were rejected.
    pub rejected: Vec<RejectedEntry>,
    /// Conflicts detected that require policy resolution.
    pub new_conflicts: Vec<ConflictTuple>,
}

/// A unit rejected during merge, with the reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedEntry {
    /// The unit that was rejected.
    pub unit_id: UnitId,
    /// Why the unit was rejected.
    pub reason: RejectionReason,
}

/// Reason a unit was rejected during graph merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RejectionReason {
    /// Signature is cryptographically invalid.
    InvalidSignature,
    /// Author did not have valid scope at creation time.
    ScopeViolation,
    /// Author's key was revoked before the unit's creation timestamp.
    KeyRevoked,
    /// Signature context (trust domain, cluster) does not match.
    ContextMismatch,
    /// Duplicate policy for the same conflict without supersession.
    DuplicatePolicy,
}

// ---------------------------------------------------------------------------
// Causal buffering (pending entries)
// ---------------------------------------------------------------------------

/// A unit that has been verified but whose references are not yet satisfied.
/// Held in the pending queue until referenced units arrive (INV-C4).
/// WAL'd as Pending(unit, missing_refs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEntry {
    /// The verified unit awaiting reference satisfaction.
    pub signed_unit: SignedUnit<Unit>,
    /// References that are not yet present in the local graph.
    pub missing_refs: BTreeSet<UnitId>,
    /// When this entry was received and verified.
    pub received_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Policy chain
// ---------------------------------------------------------------------------

/// A versioned lineage chain of policies for a single conflict tuple.
/// Only one non-revoked policy is active at any time (INV-C7).
/// Solver uses the latest non-revoked version in the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyChain {
    /// The conflict this chain resolves.
    pub conflict: ConflictTuple,
    /// Ordered list of policy versions (oldest first).
    pub versions: Vec<PolicyVersion>,
    /// The currently active (latest non-revoked) policy.
    pub active_policy: Option<UnitId>,
}

/// A single version in a policy supersession chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyVersion {
    /// The policy unit ID.
    pub policy_id: UnitId,
    /// Version number in the chain.
    pub version: Version,
    /// Whether this version has been revoked.
    pub revoked: bool,
    /// Who authored this version.
    pub author: AuthorId,
    /// When this version was created.
    pub created_at: Timestamp,
}
