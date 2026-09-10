//! Composition graph entry types: graph entries, operations, merge
//! results, causal buffering, policy chains, and statistics.
//!
//! These types are the data model for the CRDT composition graph.
//! The graph is the single source of desired state (INV-C1).
//!
//! ## Timestamp resolution
//!
//! The spec data-models reference `Timestamp` which does not exist in
//! taba-common. Per M1 convention, creation and merge timestamps use
//! [`DualClockEvent`] (logical clock for ordering, wall time for
//! compliance), and the local clock uses [`LogicalClock`].

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use taba_common::{AuthorId, DualClockEvent, TrustDomainId, UnitId, Version};
use taba_core::{ConflictTuple, Unit};
use taba_security::SignedUnit;

// ===========================================================================
// GraphEntry
// ===========================================================================

/// A single entry in the composition graph.
///
/// Wraps a signed unit with graph-level metadata: merge timestamp,
/// incoming/outgoing reference edges, trust domain, and archival status.
///
/// Identity of a graph entry: `(UnitId, AuthorId, CreationTimestamp)`.
/// The graph is a set — no duplicates with the same identity triple.
///
/// Note: `PartialEq` is intentionally NOT derived because
/// [`SignedUnit`] does not implement it (the [`SignatureContext`]
/// field lacks `PartialEq`). Use [`GraphEntry::unit_id`] for identity
/// comparison.
///
/// [`SignatureContext`]: taba_security::SignatureContext
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEntry {
    /// The signed unit payload.
    pub signed_unit: SignedUnit<Unit>,
    /// When this entry was merged into the local graph (dual clock —
    /// logical for ordering, wall time for compliance).
    pub merged_at: DualClockEvent,
    /// Incoming edges: units that reference this one.
    pub referenced_by: BTreeSet<UnitId>,
    /// Outgoing edges: units this one references.
    pub references: BTreeSet<UnitId>,
    /// Which trust domain this entry belongs to (for future sharding).
    pub trust_domain: TrustDomainId,
    /// Whether this entry has been archived (moved to cold storage).
    pub archived: bool,
}

impl GraphEntry {
    /// Returns the [`UnitId`] of the wrapped unit.
    #[must_use]
    pub const fn unit_id(&self) -> UnitId {
        self.signed_unit.unit.id()
    }

    /// Returns a reference to the wrapped [`Unit`].
    #[must_use]
    pub const fn unit(&self) -> &Unit {
        &self.signed_unit.unit
    }

    /// Creates a new `GraphEntry` from a [`SignedUnit`] with the given
    /// merge timestamp and reference edges.
    ///
    /// The trust domain is taken from the unit's header. The entry is
    /// not archived by default.
    #[must_use]
    pub const fn from_signed_unit(
        signed_unit: SignedUnit<Unit>,
        merged_at: DualClockEvent,
        references: BTreeSet<UnitId>,
    ) -> Self {
        let trust_domain = signed_unit.unit.header().trust_domain;
        Self {
            signed_unit,
            merged_at,
            referenced_by: BTreeSet::new(),
            references,
            trust_domain,
            archived: false,
        }
    }

    /// Rough memory estimate for this entry in bytes.
    ///
    /// This is a heuristic — it counts the key fields (signed unit
    /// serialized size, reference set sizes) without requiring actual
    /// serialization. Used by the memory monitor (INV-R6).
    #[must_use]
    pub fn memory_estimate(&self) -> u64 {
        // Base overhead for the struct itself.
        let base = 128u64;

        // Reference sets: each UnitId is 16 bytes (UUID).
        let refs_size = u64::try_from(self.references.len() + self.referenced_by.len())
            .unwrap_or(u64::MAX)
            * 16;

        // Signed unit: approximate by header size + signature/context overhead.
        // Header has ~5 UUIDs (80 bytes) + string fields.
        // Signature is 64 bytes, public key is 32 bytes, context has 2 UUIDs (32 bytes).
        let signed_unit_size = 80u64 + 64u64 + 32u64 + 32u64 + 64u64; // ~272 bytes

        base + refs_size + signed_unit_size
    }
}

// ===========================================================================
// GraphOp
// ===========================================================================

/// Operations that can be applied to the composition graph.
///
/// All operations are WAL'd before their effects become visible
/// (INV-C4). This enum is extensible — new operation types may be
/// added in future milestones.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GraphOp {
    /// Insert a new unit into the graph.
    ///
    /// Signature verified synchronously before insertion (INV-S3).
    Insert {
        /// The signed unit to insert.
        signed_unit: Box<SignedUnit<Unit>>,
    },
    /// Compose two or more units through capability matching.
    ///
    /// Produces edges between matched units.
    Compose {
        /// The units being composed.
        unit_ids: BTreeSet<UnitId>,
        /// The resulting capability matches.
        matches: Vec<taba_core::CapabilityMatch>,
    },
    /// A policy unit supersedes a previous policy for the same conflict.
    ///
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

// ===========================================================================
// Merge result
// ===========================================================================

/// Result of merging two graph states (CRDT merge).
///
/// Merge is commutative, associative, and idempotent (INV-C2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeResult {
    /// Units that were newly added to this node's graph.
    pub new_entries: Vec<UnitId>,
    /// Pending entries that were promoted to active after references arrived.
    pub promoted: Vec<UnitId>,
    /// Entries that were already present (idempotent merge — no-op).
    pub duplicates: Vec<UnitId>,
    /// Units that failed verification and were rejected.
    pub rejected: Vec<RejectedEntry>,
    /// Conflicts detected that require policy resolution.
    pub new_conflicts: Vec<ConflictTuple>,
}

impl MergeResult {
    /// Creates an empty `MergeResult` (no changes).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            new_entries: Vec::new(),
            promoted: Vec::new(),
            duplicates: Vec::new(),
            rejected: Vec::new(),
            new_conflicts: Vec::new(),
        }
    }

    /// Returns `true` if the merge produced no changes at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.new_entries.is_empty()
            && self.promoted.is_empty()
            && self.duplicates.is_empty()
            && self.rejected.is_empty()
            && self.new_conflicts.is_empty()
    }

    /// Combines two `MergeResult`s by concatenating all fields.
    ///
    /// Used when merging multiple deltas in sequence.
    #[must_use]
    pub fn merge_with(mut self, other: Self) -> Self {
        self.new_entries.extend(other.new_entries);
        self.promoted.extend(other.promoted);
        self.duplicates.extend(other.duplicates);
        self.rejected.extend(other.rejected);
        self.new_conflicts.extend(other.new_conflicts);
        self
    }
}

impl Default for MergeResult {
    fn default() -> Self {
        Self::empty()
    }
}

// ===========================================================================
// Rejection
// ===========================================================================

/// A unit rejected during graph merge, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedEntry {
    /// The unit that was rejected.
    pub unit_id: UnitId,
    /// Why the unit was rejected.
    pub reason: RejectionReason,
}

/// Reason a unit was rejected during graph merge.
///
/// This enum is extensible — new rejection reasons may be added in
/// future milestones as the verification pipeline expands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

// ===========================================================================
// Causal buffering (pending entries)
// ===========================================================================

/// A unit that has been verified but whose references are not yet satisfied.
///
/// Held in the pending queue until referenced units arrive (INV-C4).
/// WAL'd as `Pending(unit, missing_refs)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingEntry {
    /// The verified unit awaiting reference satisfaction.
    pub signed_unit: SignedUnit<Unit>,
    /// References that are not yet present in the local graph.
    pub missing_refs: BTreeSet<UnitId>,
    /// When this entry was received and verified (dual clock).
    pub received_at: DualClockEvent,
}

impl PendingEntry {
    /// Returns the [`UnitId`] of the pending unit.
    #[must_use]
    pub const fn unit_id(&self) -> UnitId {
        self.signed_unit.unit.id()
    }

    /// Returns a reference to the pending [`Unit`].
    #[must_use]
    pub const fn unit(&self) -> &Unit {
        &self.signed_unit.unit
    }

    /// Returns `true` if all references are now satisfied (no missing refs).
    #[must_use]
    pub fn is_satisfied(&self) -> bool {
        self.missing_refs.is_empty()
    }
}

// ===========================================================================
// Policy chain
// ===========================================================================

/// A versioned lineage chain of policies for a single conflict tuple.
///
/// Only one non-revoked policy is active at any time (INV-C7).
/// The solver uses the latest non-revoked version in the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyChain {
    /// The conflict this chain resolves.
    pub conflict: ConflictTuple,
    /// Ordered list of policy versions (oldest first).
    pub versions: Vec<PolicyVersion>,
    /// The currently active (latest non-revoked) policy.
    pub active_policy: Option<UnitId>,
}

impl PolicyChain {
    /// Creates a new empty policy chain for the given conflict.
    #[must_use]
    pub const fn new(conflict: ConflictTuple) -> Self {
        Self {
            conflict,
            versions: Vec::new(),
            active_policy: None,
        }
    }

    /// Adds a new policy version to the chain.
    ///
    /// The active policy is updated to the latest non-revoked version.
    /// If all versions are revoked, `active_policy` becomes `None`.
    pub fn add_version(&mut self, version: PolicyVersion) {
        self.versions.push(version);
        self.recompute_active();
    }

    /// Revokes a policy by its unit ID.
    ///
    /// The policy is marked as revoked in the chain. The active policy
    /// is recomputed — if the revoked policy was active, the next
    /// latest non-revoked policy (if any) becomes active.
    pub fn revoke(&mut self, policy_id: UnitId) {
        for v in &mut self.versions {
            if v.policy_id == policy_id {
                v.revoked = true;
            }
        }
        self.recompute_active();
    }

    /// Recomputes the active policy as the latest non-revoked version.
    fn recompute_active(&mut self) {
        self.active_policy = self
            .versions
            .iter()
            .rev()
            .find(|v| !v.revoked)
            .map(|v| v.policy_id);
    }
}

/// A single version in a policy supersession chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyVersion {
    /// The policy unit ID.
    pub policy_id: UnitId,
    /// Version number in the chain.
    pub version: Version,
    /// Whether this version has been revoked.
    pub revoked: bool,
    /// Who authored this version.
    pub author: AuthorId,
    /// When this version was created (dual clock — logical for
    /// ordering, wall time for compliance).
    pub created_at: DualClockEvent,
}

// ===========================================================================
// Graph statistics
// ===========================================================================

/// Statistics about graph size and health.
///
/// Returned by [`Graph::stats`](crate::Graph::stats) and used by the
/// memory monitor to determine compaction and degraded mode triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStats {
    /// Number of active (non-archived, non-pending) units.
    pub active_units: u64,
    /// Number of units in the pending queue awaiting causal delivery.
    pub pending_units: u64,
    /// Number of archived units (retained for provenance).
    pub archived_units: u64,
    /// Current memory usage estimate in bytes.
    pub memory_bytes: u64,
    /// Configured memory limit in bytes (INV-R6).
    pub memory_limit_bytes: u64,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{ClusterId, ValidityWindow};
    use taba_common::{LogicalClock, WallTime};
    use taba_core::UnitKind;
    use taba_security::{PublicKey, Signature, SignatureContext};
    use taba_test_harness::WorkloadUnitBuilder;

    /// Creates a minimal [`DualClockEvent`] for testing.
    fn test_dual_clock() -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1000 },
            timezone: "UTC".to_string(),
        }
    }

    /// Wraps a [`Unit`] in a [`SignedUnit`] with placeholder crypto
    /// values. In M2, actual signature verification is deferred —
    /// this helper creates a structurally valid `SignedUnit`.
    fn wrap_signed(unit: Unit) -> SignedUnit<Unit> {
        SignedUnit {
            unit,
            signature: Signature([0u8; 64]),
            context: SignatureContext {
                trust_domain_id: taba_common::TrustDomainId(uuid::Uuid::nil()),
                cluster_id: ClusterId(uuid::Uuid::nil()),
                validity_window: ValidityWindow {
                    lc_range: None,
                    wall_time_deadline: None,
                },
            },
            signer: PublicKey([0u8; 32]),
        }
    }

    /// Creates a minimal [`GraphEntry`] for testing.
    fn test_entry() -> GraphEntry {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new())
    }

    // -- GraphEntry ---------------------------------------------------------

    #[test]
    fn test_entry_unit_id() {
        let entry = test_entry();
        let expected_id = entry.signed_unit.unit.id();
        assert_eq!(entry.unit_id(), expected_id);
    }

    #[test]
    fn test_entry_unit_ref() {
        let entry = test_entry();
        assert_eq!(entry.unit().kind(), UnitKind::Workload);
    }

    #[test]
    fn test_entry_from_signed_unit_trust_domain() {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let expected_td = unit.header().trust_domain;
        let signed = wrap_signed(unit);
        let entry = GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new());
        assert_eq!(entry.trust_domain, expected_td);
    }

    #[test]
    fn test_entry_from_signed_unit_not_archived() {
        let entry = test_entry();
        assert!(!entry.archived, "new entry should not be archived");
    }

    #[test]
    fn test_entry_memory_estimate_positive() {
        let entry = test_entry();
        assert!(
            entry.memory_estimate() > 0,
            "memory estimate should be positive"
        );
    }

    #[test]
    fn test_entry_memory_estimate_increases_with_refs() {
        let entry_no_refs = test_entry();

        let refs: BTreeSet<UnitId> = (0..10)
            .map(|i: i32| {
                let u = u32::try_from(i).unwrap_or(0);
                UnitId(uuid::Uuid::from_u128(u128::from(u)))
            })
            .collect();
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        let entry_with_refs = GraphEntry::from_signed_unit(signed, test_dual_clock(), refs);

        assert!(
            entry_with_refs.memory_estimate() > entry_no_refs.memory_estimate(),
            "entry with references should have higher memory estimate"
        );
    }

    // -- GraphOp ------------------------------------------------------------

    #[test]
    fn test_graph_op_insert() {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        let op = GraphOp::Insert {
            signed_unit: Box::new(signed),
        };
        assert!(matches!(op, GraphOp::Insert { .. }));
    }

    #[test]
    fn test_graph_op_compose() {
        let ids = BTreeSet::from([UnitId(uuid::Uuid::new_v4())]);
        let op = GraphOp::Compose {
            unit_ids: ids,
            matches: Vec::new(),
        };
        assert!(matches!(op, GraphOp::Compose { .. }));
    }

    #[test]
    fn test_graph_op_supersede() {
        let op = GraphOp::Supersede {
            new_policy: UnitId(uuid::Uuid::new_v4()),
            old_policy: UnitId(uuid::Uuid::new_v4()),
        };
        assert!(matches!(op, GraphOp::Supersede { .. }));
    }

    #[test]
    fn test_graph_op_archive() {
        let op = GraphOp::Archive {
            root_unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
        };
        assert!(matches!(op, GraphOp::Archive { .. }));
    }

    // -- MergeResult --------------------------------------------------------

    #[test]
    fn test_merge_result_empty() {
        let result = MergeResult::empty();
        assert!(result.is_empty());
        assert!(result.new_entries.is_empty());
        assert!(result.promoted.is_empty());
        assert!(result.duplicates.is_empty());
        assert!(result.rejected.is_empty());
        assert!(result.new_conflicts.is_empty());
    }

    #[test]
    fn test_merge_result_default_is_empty() {
        let result = MergeResult::default();
        assert!(result.is_empty());
    }

    #[test]
    fn test_merge_result_not_empty_with_new_entries() {
        let result = MergeResult {
            new_entries: vec![UnitId(uuid::Uuid::new_v4())],
            ..MergeResult::empty()
        };
        assert!(!result.is_empty());
    }

    #[test]
    fn test_merge_result_merge_with() {
        let a = MergeResult {
            new_entries: vec![UnitId(uuid::Uuid::new_v4())],
            duplicates: vec![UnitId(uuid::Uuid::new_v4())],
            ..MergeResult::empty()
        };
        let b = MergeResult {
            promoted: vec![UnitId(uuid::Uuid::new_v4())],
            ..MergeResult::empty()
        };

        let combined = a.merge_with(b);
        assert_eq!(combined.new_entries.len(), 1);
        assert_eq!(combined.duplicates.len(), 1);
        assert_eq!(combined.promoted.len(), 1);
    }

    // -- RejectedEntry / RejectionReason ------------------------------------

    #[test]
    fn test_rejected_entry_construction() {
        let id = UnitId(uuid::Uuid::new_v4());
        let entry = RejectedEntry {
            unit_id: id,
            reason: RejectionReason::InvalidSignature,
        };
        assert_eq!(entry.unit_id, id);
        assert_eq!(entry.reason, RejectionReason::InvalidSignature);
    }

    #[test]
    fn test_rejection_reason_all_variants() {
        let reasons = [
            RejectionReason::InvalidSignature,
            RejectionReason::ScopeViolation,
            RejectionReason::KeyRevoked,
            RejectionReason::ContextMismatch,
            RejectionReason::DuplicatePolicy,
        ];
        // Each variant should serialize and deserialize
        for reason in &reasons {
            let json = serde_json::to_string(reason).expect("serialize RejectionReason");
            let decoded: RejectionReason =
                serde_json::from_str(&json).expect("deserialize RejectionReason");
            assert_eq!(*reason, decoded);
        }
    }

    // -- PendingEntry -------------------------------------------------------

    #[test]
    fn test_pending_entry_unit_id() {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        let pending = PendingEntry {
            signed_unit: signed,
            missing_refs: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            received_at: test_dual_clock(),
        };
        assert_eq!(pending.unit_id(), pending.signed_unit.unit.id());
    }

    #[test]
    fn test_pending_entry_is_satisfied_false() {
        let pending = PendingEntry {
            signed_unit: wrap_signed(Unit::Workload(WorkloadUnitBuilder::new().build())),
            missing_refs: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            received_at: test_dual_clock(),
        };
        assert!(
            !pending.is_satisfied(),
            "entry with missing refs is not satisfied"
        );
    }

    #[test]
    fn test_pending_entry_is_satisfied_true() {
        let pending = PendingEntry {
            signed_unit: wrap_signed(Unit::Workload(WorkloadUnitBuilder::new().build())),
            missing_refs: BTreeSet::new(),
            received_at: test_dual_clock(),
        };
        assert!(
            pending.is_satisfied(),
            "entry with no missing refs is satisfied"
        );
    }

    // -- PolicyChain / PolicyVersion ----------------------------------------

    #[test]
    fn test_policy_chain_new_empty() {
        let conflict = ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        };
        let chain = PolicyChain::new(conflict.clone());
        assert_eq!(chain.conflict, conflict);
        assert!(chain.versions.is_empty());
        assert!(chain.active_policy.is_none());
    }

    #[test]
    fn test_policy_chain_add_version_sets_active() {
        let mut chain = PolicyChain::new(ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        });

        let policy_id = UnitId(uuid::Uuid::new_v4());
        chain.add_version(PolicyVersion {
            policy_id,
            version: Version(1),
            revoked: false,
            author: AuthorId(uuid::Uuid::new_v4()),
            created_at: test_dual_clock(),
        });

        assert_eq!(chain.active_policy, Some(policy_id));
        assert_eq!(chain.versions.len(), 1);
    }

    #[test]
    fn test_policy_chain_revoke_updates_active() {
        let mut chain = PolicyChain::new(ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        });

        let policy_id = UnitId(uuid::Uuid::new_v4());
        chain.add_version(PolicyVersion {
            policy_id,
            version: Version(1),
            revoked: false,
            author: AuthorId(uuid::Uuid::new_v4()),
            created_at: test_dual_clock(),
        });
        assert_eq!(chain.active_policy, Some(policy_id));

        // Revoke the only policy → active becomes None
        chain.revoke(policy_id);
        assert_eq!(chain.active_policy, None);
    }

    #[test]
    fn test_policy_chain_revoke_promotes_previous() {
        let mut chain = PolicyChain::new(ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        });

        let v1_id = UnitId(uuid::Uuid::new_v4());
        let v2_id = UnitId(uuid::Uuid::new_v4());

        chain.add_version(PolicyVersion {
            policy_id: v1_id,
            version: Version(1),
            revoked: false,
            author: AuthorId(uuid::Uuid::new_v4()),
            created_at: test_dual_clock(),
        });
        chain.add_version(PolicyVersion {
            policy_id: v2_id,
            version: Version(2),
            revoked: false,
            author: AuthorId(uuid::Uuid::new_v4()),
            created_at: test_dual_clock(),
        });

        // v2 is active (latest non-revoked)
        assert_eq!(chain.active_policy, Some(v2_id));

        // Revoke v2 → v1 becomes active
        chain.revoke(v2_id);
        assert_eq!(chain.active_policy, Some(v1_id));
    }

    #[test]
    fn test_policy_chain_multiple_revokes() {
        let mut chain = PolicyChain::new(ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        });

        let v1_id = UnitId(uuid::Uuid::new_v4());
        let v2_id = UnitId(uuid::Uuid::new_v4());

        chain.add_version(PolicyVersion {
            policy_id: v1_id,
            version: Version(1),
            revoked: false,
            author: AuthorId(uuid::Uuid::new_v4()),
            created_at: test_dual_clock(),
        });
        chain.add_version(PolicyVersion {
            policy_id: v2_id,
            version: Version(2),
            revoked: false,
            author: AuthorId(uuid::Uuid::new_v4()),
            created_at: test_dual_clock(),
        });

        // Revoke both → active becomes None
        chain.revoke(v1_id);
        chain.revoke(v2_id);
        assert_eq!(chain.active_policy, None);
    }

    // -- GraphStats ---------------------------------------------------------

    #[test]
    fn test_graph_stats_construction() {
        let stats = GraphStats {
            active_units: 10,
            pending_units: 5,
            archived_units: 2,
            memory_bytes: 1_000_000,
            memory_limit_bytes: 2_000_000,
        };
        assert_eq!(stats.active_units, 10);
        assert_eq!(stats.pending_units, 5);
        assert_eq!(stats.archived_units, 2);
        assert_eq!(stats.memory_bytes, 1_000_000);
        assert_eq!(stats.memory_limit_bytes, 2_000_000);
    }

    #[test]
    fn test_graph_stats_equality() {
        let a = GraphStats {
            active_units: 1,
            pending_units: 0,
            archived_units: 0,
            memory_bytes: 100,
            memory_limit_bytes: 200,
        };
        let b = a;
        assert_eq!(a, b);
    }

    // -- Serialization roundtrips -------------------------------------------

    #[test]
    fn test_graph_stats_serialization_roundtrip() {
        let stats = GraphStats {
            active_units: 10,
            pending_units: 5,
            archived_units: 2,
            memory_bytes: 1_000_000,
            memory_limit_bytes: 2_000_000,
        };
        let json = serde_json::to_string(&stats).expect("serialize GraphStats");
        let decoded: GraphStats = serde_json::from_str(&json).expect("deserialize GraphStats");
        assert_eq!(stats, decoded);
    }

    #[test]
    fn test_merge_result_serialization_roundtrip() {
        let result = MergeResult {
            new_entries: vec![UnitId(uuid::Uuid::new_v4())],
            promoted: vec![UnitId(uuid::Uuid::new_v4())],
            duplicates: vec![UnitId(uuid::Uuid::new_v4())],
            rejected: vec![RejectedEntry {
                unit_id: UnitId(uuid::Uuid::new_v4()),
                reason: RejectionReason::InvalidSignature,
            }],
            new_conflicts: vec![ConflictTuple {
                unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
                capability_name: "storage".to_string(),
            }],
        };
        let json = serde_json::to_string(&result).expect("serialize MergeResult");
        let decoded: MergeResult = serde_json::from_str(&json).expect("deserialize MergeResult");
        assert_eq!(result, decoded);
    }

    #[test]
    fn test_policy_chain_serialization_roundtrip() {
        let chain = PolicyChain::new(ConflictTuple {
            unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "compute".to_string(),
        });
        let json = serde_json::to_string(&chain).expect("serialize PolicyChain");
        let decoded: PolicyChain = serde_json::from_str(&json).expect("deserialize PolicyChain");
        assert_eq!(chain, decoded);
    }
}
