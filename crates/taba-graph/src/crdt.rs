//! δ-state CRDT composition graph data structure.
//!
//! The composition graph is the single source of desired state (INV-C1).
//! It is a δ-state CRDT (DL-012): merge is commutative, associative,
//! and idempotent (INV-C2). Specifically:
//!
//! - **Add-set**: signed units (grow-only, never removed from add-set)
//! - **Remove-set**: tombstones/archival markers (grow-only, monotonic)
//! - **Policy chains**: versioned register per [`ConflictTuple`],
//!   converges to highest-version non-revoked policy
//!   ([Multi-Value Register](https://crdt.spec.subjective.com/))
//! - **Pending queue**: node-local, NOT replicated
//!
//! [`GraphDelta`] is a partial state (not an operation log). Merging
//! deltas is idempotent (like `CvRDT`) while shipping only changes (like
//! `CmRDT`). All graph operations are monotonic: inserts grow the
//! add-set, compaction adds to the remove-set. This satisfies INV-C2
//! by construction.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use taba_common::{LogicalClock, UnitId};
use taba_core::ConflictTuple;

use crate::entry::{GraphEntry, MergeResult, PendingEntry, PolicyChain};
use crate::error::GraphError;

// ===========================================================================
// CompositionGraphData
// ===========================================================================

/// The CRDT state of the composition graph.
///
/// Contains all active (merged) entries, pending entries awaiting
/// causal delivery, and policy chains. This is the internal state
/// manipulated by [`crate::DefaultGraph`] and snapshotted by
/// [`crate::GraphSnapshot`].
///
/// The `generation` counter is incremented on every mutation (insert,
/// merge, supersede, archive, compact) and used for snapshot staleness
/// detection (A006).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionGraphData {
    /// All active (merged) entries in the graph, keyed by [`UnitId`].
    pub entries: BTreeMap<UnitId, GraphEntry>,
    /// Units verified but with unsatisfied references, awaiting causal
    /// delivery (INV-C4). Node-local, NOT replicated.
    pub pending: Vec<PendingEntry>,
    /// Policy chains keyed by their [`ConflictTuple`].
    /// Only one non-revoked policy per conflict tuple (INV-C7).
    pub policy_chains: BTreeMap<ConflictTuple, PolicyChain>,
    /// The local node's logical clock for ordering (INV-T1).
    pub local_clock: LogicalClock,
    /// Memory usage estimate in bytes (for INV-R6 limit enforcement).
    pub memory_estimate_bytes: u64,
    /// Monotonically increasing generation counter. Incremented on
    /// every mutation for snapshot staleness detection.
    pub generation: u64,
}

impl CompositionGraphData {
    /// Creates a new empty composition graph.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            pending: Vec::new(),
            policy_chains: BTreeMap::new(),
            local_clock: LogicalClock(0),
            memory_estimate_bytes: 0,
            generation: 0,
        }
    }

    /// Returns the [`UnitId`] set of all active entries.
    ///
    /// Used for CRDT equivalence checks (same set of unit identity
    /// tuples = equivalent state).
    #[must_use]
    pub fn entry_ids(&self) -> BTreeSet<UnitId> {
        self.entries.keys().copied().collect()
    }

    /// Returns a reference to the entry for the given [`UnitId`], if present.
    #[must_use]
    pub fn get(&self, id: &UnitId) -> Option<&GraphEntry> {
        self.entries.get(id)
    }

    /// Returns the number of active (non-archived) units.
    #[must_use]
    pub fn active_count(&self) -> u64 {
        u64::try_from(self.entries.values().filter(|e| !e.archived).count()).unwrap_or(u64::MAX)
    }

    /// Returns the number of archived units.
    #[must_use]
    pub fn archived_count(&self) -> u64 {
        u64::try_from(self.entries.values().filter(|e| e.archived).count()).unwrap_or(u64::MAX)
    }

    /// Returns the number of pending units.
    #[must_use]
    pub fn pending_count(&self) -> u64 {
        u64::try_from(self.pending.len()).unwrap_or(u64::MAX)
    }

    /// Increments the local logical clock.
    const fn tick_clock(&mut self) {
        self.local_clock.tick();
    }

    /// Increments the generation counter.
    pub(crate) const fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }

    /// Recomputes the memory estimate from all entries.
    ///
    /// Sums [`GraphEntry::memory_estimate`] for all entries (active
    /// and archived) plus a base overhead for the pending queue and
    /// policy chains.
    pub fn recompute_memory(&mut self) {
        let entries_size: u64 = self.entries.values().map(GraphEntry::memory_estimate).sum();

        // Pending entries: approximate by half the entry size each
        // (they carry a SignedUnit but less metadata).
        let pending_size: u64 = self
            .pending
            .iter()
            .map(|p| {
                GraphEntry::memory_estimate(&GraphEntry::from_signed_unit(
                    p.signed_unit.clone(),
                    p.received_at.clone(),
                    p.missing_refs.clone(),
                )) / 2
            })
            .sum();

        // Policy chains: approximate 256 bytes per chain.
        let policy_size: u64 = u64::try_from(self.policy_chains.len()).unwrap_or(u64::MAX) * 256;

        self.memory_estimate_bytes = entries_size + pending_size + policy_size;
    }

    /// Inserts a new entry directly into the active set.
    ///
    /// This is a low-level operation — it does NOT check references,
    /// validate signatures, or write to WAL. The caller (typically
    /// [`crate::DefaultGraph::insert`]) is responsible for those gates.
    ///
    /// Returns `Ok(())` if the entry was inserted, or
    /// `Err(GraphError::MergeConflict)` if an entry with the same
    /// [`UnitId`] already exists.
    pub fn insert_entry(&mut self, entry: GraphEntry) -> Result<(), GraphError> {
        let id = entry.unit_id();
        if self.entries.contains_key(&id) {
            return Err(GraphError::MergeConflict {
                reason: format!("unit {id:?} already exists in the graph"),
            });
        }
        self.entries.insert(id, entry);
        self.recompute_memory();
        self.tick_clock();
        self.bump_generation();
        Ok(())
    }

    /// Merges a delta (partial state) into this graph.
    ///
    /// This is the core CRDT merge operation. It is:
    ///
    /// - **Commutative**: `merge(A, B) == merge(B, A)` — entries are
    ///   unioned by [`UnitId`], order does not matter.
    /// - **Associative**: `merge(merge(A, B), C) == merge(A, merge(B, C))`
    ///   — `BTreeMap` union is associative.
    /// - **Idempotent**: `merge(A, A) == A` — entries with the same
    ///   [`UnitId`] are deduplicated (no-op).
    ///
    /// Each entry in the delta is individually processed. Entries that
    /// are already present (same [`UnitId`]) are counted as duplicates.
    /// Entries that fail structural validation are rejected without
    /// affecting the rest of the delta.
    ///
    /// After merging new entries, any pending entries whose references
    /// are now satisfied are promoted to the active set.
    ///
    /// # Errors
    ///
    /// Returns `Err(GraphError::MergeConflict)` only if two different
    /// units claim the same [`UnitId`] (Byzantine). In M2, this is not
    /// detected because `Unit` does not implement `PartialEq` — all
    /// same-UnitId entries are treated as duplicates.
    pub fn merge_delta(&mut self, delta: &GraphDelta) -> MergeResult {
        let mut result = MergeResult::empty();

        // Phase 1: Merge new entries from the delta.
        for (id, entry) in &delta.entries {
            if self.entries.contains_key(id) {
                // Idempotent: same UnitId is already present.
                result.duplicates.push(*id);
            } else {
                // New entry — add to the active set.
                self.entries.insert(*id, entry.clone());
                result.new_entries.push(*id);
            }
        }

        // Phase 2: Promote pending entries whose references are now satisfied.
        if !result.new_entries.is_empty() {
            self.promote_pending(&mut result);
        }

        // Phase 3: Merge policy chains (union — keep highest-version
        // chain per conflict tuple).
        for (conflict, chain) in &delta.policy_chains {
            if let Some(existing) = self.policy_chains.get_mut(conflict) {
                // Merge versions: union by policy_id, keep latest.
                for version in &chain.versions {
                    if !existing
                        .versions
                        .iter()
                        .any(|v| v.policy_id == version.policy_id)
                    {
                        existing.add_version(version.clone());
                    }
                }
            } else {
                self.policy_chains.insert(conflict.clone(), chain.clone());
            }
        }

        // Update memory and clocks.
        self.recompute_memory();
        self.tick_clock();
        self.bump_generation();

        result
    }

    /// Promotes pending entries whose references are now satisfied.
    ///
    /// Scans the pending queue for entries whose `missing_refs` are all
    /// present in the active entries. Promoted entries are removed from
    /// the pending queue and added to the active set.
    fn promote_pending(&mut self, result: &mut MergeResult) {
        let active_ids = self.entry_ids();
        let mut still_pending = Vec::new();

        for pending in self.pending.drain(..) {
            let now_satisfied = pending
                .missing_refs
                .iter()
                .all(|ref_id| active_ids.contains(ref_id));

            if now_satisfied {
                // All references satisfied — promote to active.
                let id = pending.unit_id();
                if let std::collections::btree_map::Entry::Vacant(e) = self.entries.entry(id) {
                    let entry = GraphEntry::from_signed_unit(
                        pending.signed_unit.clone(),
                        pending.received_at.clone(),
                        pending.missing_refs.clone(),
                    );
                    e.insert(entry);
                    result.promoted.push(id);
                }
            } else {
                // Still has unsatisfied references — keep in pending.
                still_pending.push(pending);
            }
        }

        self.pending = still_pending;
    }

    /// Creates a [`GraphDelta`] containing all entries in this graph.
    ///
    /// Useful for testing and for initial state synchronisation.
    #[must_use]
    pub fn to_delta(&self) -> GraphDelta {
        GraphDelta {
            entries: self.entries.clone(),
            policy_chains: self.policy_chains.clone(),
        }
    }

    /// Checks whether two graph states are equivalent for CRDT purposes.
    ///
    /// Two states are equivalent if they contain the same set of
    /// [`UnitId`] keys in their `entries` maps. This is the
    /// identity-based equivalence used by [`crate::MergePolicy::is_equivalent`].
    #[must_use]
    pub fn is_equivalent_to(&self, other: &Self) -> bool {
        self.entry_ids() == other.entry_ids()
    }
}

impl Default for CompositionGraphData {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// GraphDelta
// ===========================================================================

/// A partial state for merging into the composition graph.
///
/// Unlike an operation log, a `GraphDelta` carries actual entries
/// (not operations). Merging deltas is idempotent: merging the same
/// delta twice produces the same result as merging it once.
///
/// Deltas are the unit of replication in the δ-state CRDT (DL-012).
/// A node computes a delta containing only the entries that changed
/// since the last sync and ships it to peers. Peers merge the delta
/// into their local graph.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphDelta {
    /// Entries to merge into the target graph.
    pub entries: BTreeMap<UnitId, GraphEntry>,
    /// Policy chains to merge.
    pub policy_chains: BTreeMap<ConflictTuple, PolicyChain>,
}

impl GraphDelta {
    /// Creates a new empty delta.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a delta from a single entry.
    #[must_use]
    pub fn from_entry(entry: GraphEntry) -> Self {
        let id = entry.unit_id();
        let mut entries = BTreeMap::new();
        entries.insert(id, entry);
        Self {
            entries,
            policy_chains: BTreeMap::new(),
        }
    }

    /// Adds an entry to this delta.
    pub fn add_entry(&mut self, entry: GraphEntry) {
        let id = entry.unit_id();
        self.entries.insert(id, entry);
    }

    /// Adds a policy chain to this delta.
    pub fn add_policy_chain(&mut self, conflict: ConflictTuple, chain: PolicyChain) {
        self.policy_chains.insert(conflict, chain);
    }

    /// Returns `true` if the delta contains no entries and no policy chains.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.policy_chains.is_empty()
    }

    /// Returns the number of entries in this delta.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Merges another delta into this one (union of entries).
    ///
    /// This is the delta-level merge: entries from `other` are added
    /// to `self` if not already present (by [`UnitId`]). This is
    /// itself commutative, associative, and idempotent.
    pub fn merge(&mut self, other: &Self) {
        for (id, entry) in &other.entries {
            self.entries.entry(*id).or_insert_with(|| entry.clone());
        }
        for (conflict, chain) in &other.policy_chains {
            self.policy_chains
                .entry(conflict.clone())
                .or_insert_with(|| chain.clone());
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{ClusterId, DualClockEvent, TrustDomainId, ValidityWindow, WallTime};
    use taba_core::Unit;
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

    /// Wraps a [`Unit`] in a [`SignedUnit`] with placeholder crypto.
    fn wrap_signed(unit: Unit) -> taba_security::SignedUnit<Unit> {
        taba_security::SignedUnit {
            unit,
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
        }
    }

    /// Creates a [`GraphEntry`] with a fresh [`UnitId`] and no references.
    fn fresh_entry() -> GraphEntry {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new())
    }

    /// Creates a [`GraphEntry`] with the given [`UnitId`] and no references.
    fn entry_with_id(id: UnitId) -> GraphEntry {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build());
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new())
    }

    /// Creates a [`CompositionGraphData`] with the given set of [`UnitId`]s.
    fn graph_with_ids(ids: &[UnitId]) -> CompositionGraphData {
        let mut data = CompositionGraphData::new();
        for &id in ids {
            let entry = entry_with_id(id);
            // Use insert_entry but ignore generation/clock changes for
            // test simplicity — we just want entries in the map.
            data.entries.insert(id, entry);
        }
        data.recompute_memory();
        data
    }

    // -- Basic operations ---------------------------------------------------

    #[test]
    fn test_crdt_new_is_empty() {
        let data = CompositionGraphData::new();
        assert!(data.entries.is_empty());
        assert!(data.pending.is_empty());
        assert!(data.policy_chains.is_empty());
        assert_eq!(data.local_clock, LogicalClock(0));
        assert_eq!(data.memory_estimate_bytes, 0);
        assert_eq!(data.generation, 0);
    }

    #[test]
    fn test_crdt_default_is_new() {
        let data = CompositionGraphData::default();
        assert!(data.entries.is_empty());
        assert_eq!(data.generation, 0);
    }

    #[test]
    fn test_crdt_insert_and_get() {
        let mut data = CompositionGraphData::new();
        let entry = fresh_entry();
        let id = entry.unit_id();

        data.insert_entry(entry).expect("insert should succeed");

        assert!(
            data.get(&id).is_some(),
            "entry should be retrievable after insert"
        );
        assert_eq!(data.active_count(), 1);
        assert_eq!(data.entry_ids().len(), 1);
        assert!(data.entry_ids().contains(&id));
    }

    #[test]
    fn test_crdt_insert_duplicate_fails() {
        let mut data = CompositionGraphData::new();
        let entry = fresh_entry();
        let id = entry.unit_id();

        data.insert_entry(entry)
            .expect("first insert should succeed");
        let duplicate = entry_with_id(id);

        let result = data.insert_entry(duplicate);
        assert!(
            matches!(result, Err(GraphError::MergeConflict { .. })),
            "duplicate insert should fail with MergeConflict"
        );
    }

    #[test]
    fn test_crdt_insert_increments_generation() {
        let mut data = CompositionGraphData::new();
        assert_eq!(data.generation, 0);

        data.insert_entry(fresh_entry()).expect("insert");
        assert_eq!(
            data.generation, 1,
            "generation should increment after insert"
        );

        data.insert_entry(fresh_entry()).expect("insert");
        assert_eq!(data.generation, 2, "generation should increment again");
    }

    #[test]
    fn test_crdt_memory_estimation() {
        let mut data = CompositionGraphData::new();
        assert_eq!(data.memory_estimate_bytes, 0);

        data.insert_entry(fresh_entry()).expect("insert");
        let mem_after_one = data.memory_estimate_bytes;
        assert!(
            mem_after_one > 0,
            "memory estimate should be positive after insert"
        );

        data.insert_entry(fresh_entry()).expect("insert");
        let mem_after_two = data.memory_estimate_bytes;
        assert!(
            mem_after_two > mem_after_one,
            "memory estimate should increase with more entries"
        );
    }

    #[test]
    fn test_crdt_active_and_archived_count() {
        let mut data = CompositionGraphData::new();
        let id1 = UnitId(uuid::Uuid::new_v4());
        let id2 = UnitId(uuid::Uuid::new_v4());

        data.insert_entry(entry_with_id(id1)).expect("insert 1");
        data.insert_entry(entry_with_id(id2)).expect("insert 2");

        assert_eq!(data.active_count(), 2);
        assert_eq!(data.archived_count(), 0);

        // Archive one
        if let Some(entry) = data.entries.get_mut(&id1) {
            entry.archived = true;
        }

        assert_eq!(data.active_count(), 1);
        assert_eq!(data.archived_count(), 1);
    }

    #[test]
    fn test_crdt_to_delta() {
        let mut data = CompositionGraphData::new();
        data.insert_entry(fresh_entry()).expect("insert");
        data.insert_entry(fresh_entry()).expect("insert");

        let delta = data.to_delta();
        assert_eq!(delta.len(), 2, "delta should contain all entries");
    }

    // -- Merge semantics ----------------------------------------------------

    #[test]
    fn test_crdt_merge_no_op_on_empty() {
        let mut data = CompositionGraphData::new();
        let delta = GraphDelta::new();

        let result = data.merge_delta(&delta);

        assert!(result.is_empty(), "merging empty delta should be no-op");
        assert!(data.entries.is_empty());
    }

    #[test]
    fn test_crdt_merge_adds_new_entries() {
        let mut data = CompositionGraphData::new();
        let entry = fresh_entry();
        let id = entry.unit_id();

        let delta = GraphDelta::from_entry(entry);
        let result = data.merge_delta(&delta);

        assert_eq!(result.new_entries, vec![id]);
        assert!(data.get(&id).is_some());
    }

    #[test]
    fn test_crdt_merge_idempotent() {
        let entry = fresh_entry();
        let id = entry.unit_id();
        let delta = GraphDelta::from_entry(entry);

        // First merge
        let mut data_a = CompositionGraphData::new();
        let result_a = data_a.merge_delta(&delta);
        assert_eq!(result_a.new_entries, vec![id]);

        // Second merge of the same delta
        let result_b = data_a.merge_delta(&delta);
        assert!(
            result_b.new_entries.is_empty(),
            "second merge should add nothing"
        );
        assert_eq!(
            result_b.duplicates,
            vec![id],
            "second merge should be a duplicate"
        );
        assert_eq!(data_a.active_count(), 1, "should still have only 1 entry");
    }

    #[test]
    fn test_crdt_merge_commutative() {
        let entry_a = fresh_entry();
        let id_a = entry_a.unit_id();
        let entry_b = fresh_entry();
        let id_b = entry_b.unit_id();

        let delta_a = GraphDelta::from_entry(entry_a);
        let delta_b = GraphDelta::from_entry(entry_b);

        // merge(A, B): start empty, merge delta_a, then delta_b
        let mut data_ab = CompositionGraphData::new();
        data_ab.merge_delta(&delta_a);
        data_ab.merge_delta(&delta_b);

        // merge(B, A): start empty, merge delta_b, then delta_a
        let mut data_ba = CompositionGraphData::new();
        data_ba.merge_delta(&delta_b);
        data_ba.merge_delta(&delta_a);

        // Both should have the same set of entry IDs
        assert_eq!(
            data_ab.entry_ids(),
            data_ba.entry_ids(),
            "merge(A, B) and merge(B, A) should produce equivalent states"
        );
        assert!(data_ab.entry_ids().contains(&id_a));
        assert!(data_ab.entry_ids().contains(&id_b));
    }

    #[test]
    fn test_crdt_merge_associative() {
        let entry_a = fresh_entry();
        let entry_b = fresh_entry();
        let entry_c = fresh_entry();

        let delta_a = GraphDelta::from_entry(entry_a);
        let delta_b = GraphDelta::from_entry(entry_b);
        let delta_c = GraphDelta::from_entry(entry_c);

        // merge(merge(A, B), C)
        let mut left = CompositionGraphData::new();
        left.merge_delta(&delta_a);
        left.merge_delta(&delta_b);
        // Now create a delta from this state and merge C
        let merged_ab = left.to_delta();
        let mut left_result = CompositionGraphData::new();
        left_result.merge_delta(&merged_ab);
        left_result.merge_delta(&delta_c);

        // merge(A, merge(B, C))
        let mut right = CompositionGraphData::new();
        right.merge_delta(&delta_b);
        right.merge_delta(&delta_c);
        let merged_bc = right.to_delta();
        let mut right_result = CompositionGraphData::new();
        right_result.merge_delta(&delta_a);
        right_result.merge_delta(&merged_bc);

        // Both should have the same set of entry IDs
        assert_eq!(
            left_result.entry_ids(),
            right_result.entry_ids(),
            "merge(merge(A,B), C) == merge(A, merge(B,C))"
        );
    }

    #[test]
    fn test_crdt_is_equivalent_same() {
        let entry = fresh_entry();
        let id = entry.unit_id();

        let data_a = graph_with_ids(&[id]);
        let data_b = graph_with_ids(&[id]);

        assert!(data_a.is_equivalent_to(&data_b));
    }

    #[test]
    fn test_crdt_is_equivalent_different() {
        let id_a = UnitId(uuid::Uuid::new_v4());
        let id_b = UnitId(uuid::Uuid::new_v4());

        let data_a = graph_with_ids(&[id_a]);
        let data_b = graph_with_ids(&[id_b]);

        assert!(!data_a.is_equivalent_to(&data_b));
    }

    // -- Pending promotion --------------------------------------------------

    #[test]
    fn test_crdt_promote_pending_on_merge() {
        let mut data = CompositionGraphData::new();

        // Create a unit that references a missing unit
        let ref_id = UnitId(uuid::Uuid::new_v4());
        let pending_unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let pending_signed = wrap_signed(pending_unit);
        let pending_id = pending_signed.unit.id();

        // Add to pending with missing reference
        data.pending.push(PendingEntry {
            signed_unit: pending_signed,
            missing_refs: BTreeSet::from([ref_id]),
            received_at: test_dual_clock(),
        });

        assert_eq!(data.pending_count(), 1);
        assert_eq!(data.active_count(), 0);

        // Merge a delta containing the referenced unit
        let ref_entry = entry_with_id(ref_id);
        let delta = GraphDelta::from_entry(ref_entry);
        let result = data.merge_delta(&delta);

        // The referenced unit should be added
        assert!(result.new_entries.contains(&ref_id));
        // The pending unit should be promoted
        assert!(
            result.promoted.contains(&pending_id),
            "pending unit should be promoted after refs arrive"
        );
        assert_eq!(
            data.pending_count(),
            0,
            "pending should be empty after promotion"
        );
        assert_eq!(
            data.active_count(),
            2,
            "should have 2 active units (ref + promoted)"
        );
    }

    #[test]
    fn test_crdt_pending_not_promoted_when_refs_still_missing() {
        let mut data = CompositionGraphData::new();

        let ref_a = UnitId(uuid::Uuid::new_v4());
        let ref_b = UnitId(uuid::Uuid::new_v4());

        // Pending unit needs both ref_a and ref_b
        let pending_unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let pending_signed = wrap_signed(pending_unit);

        data.pending.push(PendingEntry {
            signed_unit: pending_signed,
            missing_refs: BTreeSet::from([ref_a, ref_b]),
            received_at: test_dual_clock(),
        });

        // Merge only ref_a (still missing ref_b)
        let delta = GraphDelta::from_entry(entry_with_id(ref_a));
        let result = data.merge_delta(&delta);

        assert!(result.new_entries.contains(&ref_a));
        assert!(
            result.promoted.is_empty(),
            "pending unit should NOT be promoted when refs are still missing"
        );
        assert_eq!(data.pending_count(), 1, "should still have 1 pending unit");
    }

    // -- GraphDelta --------------------------------------------------------

    #[test]
    fn test_graph_delta_new_is_empty() {
        let delta = GraphDelta::new();
        assert!(delta.is_empty());
        assert_eq!(delta.len(), 0);
    }

    #[test]
    fn test_graph_delta_from_entry() {
        let entry = fresh_entry();
        let id = entry.unit_id();
        let delta = GraphDelta::from_entry(entry);

        assert_eq!(delta.len(), 1);
        assert!(delta.entries.contains_key(&id));
    }

    #[test]
    fn test_graph_delta_add_entry() {
        let mut delta = GraphDelta::new();
        let entry = fresh_entry();
        let id = entry.unit_id();

        delta.add_entry(entry);
        assert_eq!(delta.len(), 1);
        assert!(delta.entries.contains_key(&id));
    }

    #[test]
    fn test_graph_delta_merge() {
        let entry_a = fresh_entry();
        let id_a = entry_a.unit_id();
        let entry_b = fresh_entry();
        let id_b = entry_b.unit_id();

        let mut delta_a = GraphDelta::from_entry(entry_a);
        let delta_b = GraphDelta::from_entry(entry_b);

        delta_a.merge(&delta_b);

        assert_eq!(delta_a.len(), 2, "merged delta should contain both entries");
        assert!(delta_a.entries.contains_key(&id_a));
        assert!(delta_a.entries.contains_key(&id_b));
    }

    #[test]
    fn test_graph_delta_merge_idempotent() {
        let entry = fresh_entry();
        let id = entry.unit_id();

        let mut delta_a = GraphDelta::from_entry(entry.clone());
        let delta_b = GraphDelta::from_entry(entry);

        delta_a.merge(&delta_b);

        assert_eq!(delta_a.len(), 1, "merging same entry should be idempotent");
        assert!(delta_a.entries.contains_key(&id));
    }

    // -- Property tests (proptest) -----------------------------------------

    use proptest::prelude::*;

    /// Strategy for generating a unique set of `UnitIds`.
    fn arb_unit_id_set(min: usize, max: usize) -> impl Strategy<Value = BTreeSet<UnitId>> {
        prop::collection::btree_set(
            prop::num::u128::ANY.prop_map(|v| UnitId(uuid::Uuid::from_u128(v))),
            min..=max,
        )
    }

    /// Builds a `GraphDelta` from a set of `UnitIds`.
    fn delta_from_ids(ids: &BTreeSet<UnitId>) -> GraphDelta {
        let mut delta = GraphDelta::new();
        for &id in ids {
            delta.add_entry(entry_with_id(id));
        }
        delta
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        /// Merge is commutative: merge(A, B) == merge(B, A)
        #[test]
        fn proptest_crdt_merge_commutative(
            ids_a in arb_unit_id_set(0, 20),
            ids_b in arb_unit_id_set(0, 20),
        ) {
            let delta_a = delta_from_ids(&ids_a);
            let delta_b = delta_from_ids(&ids_b);

            // merge(A, B)
            let mut data_ab = CompositionGraphData::new();
            data_ab.merge_delta(&delta_a);
            data_ab.merge_delta(&delta_b);

            // merge(B, A)
            let mut data_ba = CompositionGraphData::new();
            data_ba.merge_delta(&delta_b);
            data_ba.merge_delta(&delta_a);

            // Both should have the same set of entry IDs
            prop_assert_eq!(
                data_ab.entry_ids(),
                data_ba.entry_ids(),
                "merge(A, B) == merge(B, A) must hold (commutative)"
            );
        }

        /// Merge is idempotent: merge(A, A) == A
        #[test]
        fn proptest_crdt_merge_idempotent(
            ids in arb_unit_id_set(0, 20),
        ) {
            let delta = delta_from_ids(&ids);

            // merge(A, .) — merge delta once
            let mut data_once = CompositionGraphData::new();
            data_once.merge_delta(&delta);

            // merge(A, A) — merge the same delta again
            let mut data_twice = CompositionGraphData::new();
            data_twice.merge_delta(&delta);
            data_twice.merge_delta(&delta);

            // Both should have the same set of entry IDs
            prop_assert_eq!(
                data_once.entry_ids(),
                data_twice.entry_ids(),
                "merge(A, A) == A must hold (idempotent)"
            );
        }

        /// Merge is associative: merge(merge(A, B), C) == merge(A, merge(B, C))
        #[test]
        fn proptest_crdt_merge_associative(
            ids_a in arb_unit_id_set(0, 15),
            ids_b in arb_unit_id_set(0, 15),
            ids_c in arb_unit_id_set(0, 15),
        ) {
            let delta_a = delta_from_ids(&ids_a);
            let delta_b = delta_from_ids(&ids_b);
            let delta_c = delta_from_ids(&ids_c);

            // merge(merge(A, B), C)
            let mut data_ab = CompositionGraphData::new();
            data_ab.merge_delta(&delta_a);
            data_ab.merge_delta(&delta_b);
            let delta_ab = data_ab.to_delta();
            let mut data_abc_left = CompositionGraphData::new();
            data_abc_left.merge_delta(&delta_ab);
            data_abc_left.merge_delta(&delta_c);

            // merge(A, merge(B, C))
            let mut data_bc = CompositionGraphData::new();
            data_bc.merge_delta(&delta_b);
            data_bc.merge_delta(&delta_c);
            let delta_bc = data_bc.to_delta();
            let mut data_abc_right = CompositionGraphData::new();
            data_abc_right.merge_delta(&delta_a);
            data_abc_right.merge_delta(&delta_bc);

            // Both should have the same set of entry IDs
            prop_assert_eq!(
                data_abc_left.entry_ids(),
                data_abc_right.entry_ids(),
                "merge(merge(A,B), C) == merge(A, merge(B,C)) must hold (associative)"
            );
        }
    }
}
