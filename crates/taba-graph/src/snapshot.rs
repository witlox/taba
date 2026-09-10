//! Immutable graph snapshot for solver consumption.
//!
//! The [`GraphSnapshot`] is a frozen, point-in-time view of the
//! composition graph. Once taken, concurrent mutations to the live
//! graph do not affect it. The solver holds a snapshot and can safely
//! compute placements while the graph continues to receive merges.
//!
//! ## Immutability contract (A006)
//!
//! `GraphSnapshot` is immutable — it has no mutation methods. The
//! `generation` counter allows staleness detection: call
//! [`GraphSnapshot::is_current`] with the live graph's current
//! generation to determine whether the snapshot is still fresh.
//!
//! ## Relationship to taba-core
//!
//! taba-core defines an opaque `GraphSnapshot` placeholder (in
//! `store.rs`) for DAG cleanliness. This module defines the REAL
//! `GraphSnapshot` with concrete graph state. The solver imports
//! this type from taba-graph. The placeholder in taba-core stays for
//! the `UnitStore` trait definition.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use taba_common::UnitId;
use taba_core::ConflictTuple;

use crate::entry::{GraphEntry, PolicyChain};

// ===========================================================================
// GraphSnapshot
// ===========================================================================

/// Immutable point-in-time view of the composition graph.
///
/// Contains a copy of all active entries and policy chains at the
/// moment the snapshot was taken. The `generation` counter corresponds
/// to the live graph's generation at snapshot time — incrementing the
/// live generation (via any mutation) makes the snapshot stale.
///
/// The solver should check [`GraphSnapshot::is_current`] before
/// applying placement results to avoid acting on stale state.
///
/// # Immutability
///
/// This type has no mutation methods. Once created, the snapshot is
/// frozen. Clone the snapshot if you need a separate copy — both
/// copies remain immutable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// Monotonically increasing generation counter. Incremented on
    /// every mutation (insert, merge, supersede, archive, compact).
    /// Compares against the live graph's generation for staleness
    /// detection.
    pub generation: u64,
    /// All entries in the graph at snapshot time (active and archived).
    pub entries: BTreeMap<UnitId, GraphEntry>,
    /// All policy chains at snapshot time.
    pub policy_chains: BTreeMap<ConflictTuple, PolicyChain>,
}

impl GraphSnapshot {
    /// Creates a new snapshot from the given generation, entries, and
    /// policy chains.
    ///
    /// This is typically called by [`crate::DefaultGraph::snapshot`]
    /// after locking the graph's internal state.
    #[must_use]
    pub const fn new(
        generation: u64,
        entries: BTreeMap<UnitId, GraphEntry>,
        policy_chains: BTreeMap<ConflictTuple, PolicyChain>,
    ) -> Self {
        Self {
            generation,
            entries,
            policy_chains,
        }
    }

    /// Returns `true` if this snapshot's generation matches the live
    /// graph's generation.
    ///
    /// A snapshot is "current" if no mutations have occurred since it
    /// was taken. The solver should call this before applying placement
    /// results.
    ///
    /// # Example
    ///
    /// ```
    /// use taba_graph::GraphSnapshot;
    ///
    /// let snapshot = GraphSnapshot::new(
    ///     5,
    ///     std::collections::BTreeMap::new(),
    ///     std::collections::BTreeMap::new(),
    /// );
    /// assert!(snapshot.is_current(5));   // no mutations since
    /// assert!(!snapshot.is_current(6));  // a mutation occurred
    /// ```
    #[must_use]
    pub const fn is_current(&self, live_generation: u64) -> bool {
        self.generation == live_generation
    }

    /// Returns the [`UnitId`] set of all entries in this snapshot.
    #[must_use]
    pub fn entry_ids(&self) -> std::collections::BTreeSet<UnitId> {
        self.entries.keys().copied().collect()
    }

    /// Returns a reference to the entry for the given [`UnitId`], if
    /// present in this snapshot.
    #[must_use]
    pub fn get(&self, id: &UnitId) -> Option<&GraphEntry> {
        self.entries.get(id)
    }

    /// Returns the number of entries in this snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if this snapshot contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of active (non-archived) units in this snapshot.
    #[must_use]
    pub fn active_count(&self) -> u64 {
        u64::try_from(self.entries.values().filter(|e| !e.archived).count()).unwrap_or(u64::MAX)
    }

    /// Returns the number of archived units in this snapshot.
    #[must_use]
    pub fn archived_count(&self) -> u64 {
        u64::try_from(self.entries.values().filter(|e| e.archived).count()).unwrap_or(u64::MAX)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{
        ClusterId, DualClockEvent, LogicalClock, TrustDomainId, ValidityWindow, WallTime,
    };
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

    /// Creates a minimal [`GraphEntry`] for testing.
    fn fresh_entry() -> GraphEntry {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), std::collections::BTreeSet::new())
    }

    #[test]
    fn test_snapshot_new_empty() {
        let snapshot = GraphSnapshot::new(0, BTreeMap::new(), BTreeMap::new());
        assert_eq!(snapshot.generation, 0);
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.len(), 0);
        assert_eq!(snapshot.active_count(), 0);
        assert_eq!(snapshot.archived_count(), 0);
    }

    #[test]
    fn test_snapshot_is_current_matching() {
        let snapshot = GraphSnapshot::new(5, BTreeMap::new(), BTreeMap::new());
        assert!(
            snapshot.is_current(5),
            "snapshot should be current when generation matches"
        );
    }

    #[test]
    fn test_snapshot_is_current_not_matching() {
        let snapshot = GraphSnapshot::new(5, BTreeMap::new(), BTreeMap::new());
        assert!(
            !snapshot.is_current(6),
            "snapshot should NOT be current when generation differs"
        );
        assert!(
            !snapshot.is_current(4),
            "snapshot should NOT be current when generation differs"
        );
    }

    #[test]
    fn test_snapshot_entry_ids() {
        let entry = fresh_entry();
        let id = entry.unit_id();
        let mut entries = BTreeMap::new();
        entries.insert(id, entry);

        let snapshot = GraphSnapshot::new(1, entries, BTreeMap::new());
        let ids = snapshot.entry_ids();
        assert_eq!(ids.len(), 1);
        assert!(ids.contains(&id));
    }

    #[test]
    fn test_snapshot_get() {
        let entry = fresh_entry();
        let id = entry.unit_id();
        let mut entries = BTreeMap::new();
        entries.insert(id, entry);

        let snapshot = GraphSnapshot::new(1, entries, BTreeMap::new());
        assert!(snapshot.get(&id).is_some(), "should find entry by ID");
        assert!(
            snapshot.get(&UnitId(uuid::Uuid::new_v4())).is_none(),
            "should not find unknown ID"
        );
    }

    #[test]
    fn test_snapshot_active_and_archived_count() {
        let entry1 = fresh_entry();
        let id1 = entry1.unit_id();
        let mut entry2 = fresh_entry();
        entry2.archived = true;
        let id2 = entry2.unit_id();

        let mut entries = BTreeMap::new();
        entries.insert(id1, entry1);
        entries.insert(id2, entry2);

        let snapshot = GraphSnapshot::new(1, entries, BTreeMap::new());
        assert_eq!(snapshot.active_count(), 1);
        assert_eq!(snapshot.archived_count(), 1);
        assert_eq!(snapshot.len(), 2);
    }

    #[test]
    fn test_snapshot_clone_is_independent() {
        let entry = fresh_entry();
        let id = entry.unit_id();
        let mut entries = BTreeMap::new();
        entries.insert(id, entry);

        let snapshot = GraphSnapshot::new(1, entries, BTreeMap::new());
        let cloned = snapshot.clone();

        // Both should have the same data
        assert_eq!(snapshot.generation, cloned.generation);
        assert_eq!(snapshot.len(), cloned.len());

        // The clone is independent — modifying the original entries map
        // after clone doesn't affect the clone (since BTreeMap is clone-by-value)
    }

    #[test]
    fn test_snapshot_serialization_roundtrip() {
        let entry = fresh_entry();
        let mut entries = BTreeMap::new();
        entries.insert(entry.unit_id(), entry);

        let snapshot = GraphSnapshot::new(42, entries, BTreeMap::new());

        let json = serde_json::to_string(&snapshot).expect("serialize GraphSnapshot");
        let decoded: GraphSnapshot =
            serde_json::from_str(&json).expect("deserialize GraphSnapshot");

        assert_eq!(decoded.generation, 42);
        assert_eq!(decoded.len(), 1);
    }
}
