//! In-memory write-ahead log for M2 (no disk persistence).
//!
//! The WAL records three entry types (DL-008, INV-C4):
//!
//! - [`WalEntry::Merged`] — a unit was verified and merged into the
//!   active graph.
//! - [`WalEntry::Pending`] — a unit was verified but its references are
//!   not yet satisfied; it is buffered in the pending queue.
//! - [`WalEntry::Promoted`] — a pending unit was promoted to active
//!   after its references arrived.
//!
//! **WAL-before-effect**: every mutation is WAL'd before its effects
//! become visible to local queries. In M2, the WAL is in-memory only
//! — real disk persistence arrives in M3 (taba-node).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, UnitId};

// ===========================================================================
// WalEntry
// ===========================================================================

/// A single entry in the write-ahead log (DL-008, INV-C4).
///
/// Records graph mutations for durability and replay. The three
/// variants correspond to the three mutation types:
///
/// 1. `Merged` — a verified unit enters the active graph.
/// 2. `Pending` — a verified unit enters the pending queue (causal
///    buffering).
/// 3. `Promoted` — a pending unit is activated after references arrive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalEntry {
    /// A unit was verified and merged into the active graph.
    Merged {
        /// The ID of the unit that was merged.
        unit_id: UnitId,
        /// When the merge occurred (dual clock — logical for ordering,
        /// wall time for compliance).
        timestamp: DualClockEvent,
    },
    /// A unit was verified but its references are not yet satisfied.
    ///
    /// The unit is buffered in the pending queue until its missing
    /// references arrive (INV-C4).
    Pending {
        /// The ID of the pending unit.
        unit_id: UnitId,
        /// References that are not yet present in the local graph.
        missing_refs: BTreeSet<UnitId>,
    },
    /// A pending unit was promoted to active after its references arrived.
    Promoted {
        /// The ID of the unit that was promoted.
        unit_id: UnitId,
    },
}

// ===========================================================================
// InMemoryWal
// ===========================================================================

/// In-memory write-ahead log for M2.
///
/// Stores [`WalEntry`] values in a [`Vec`]. The log is append-only and
/// preserves insertion order. On startup (in a real implementation),
/// `replay()` would re-apply all entries to reconstruct graph state.
///
/// **M2 limitation**: this WAL is in-memory only. If the process
/// crashes, the log is lost. Real disk persistence is implemented in
/// M3 (taba-node). The WAL-before-effect contract (INV-C4) is still
/// honoured — entries are appended before mutations become visible —
/// just without disk durability.
#[derive(Debug, Clone, Default)]
pub struct InMemoryWal {
    /// Ordered list of WAL entries.
    entries: Vec<WalEntry>,
}

impl InMemoryWal {
    /// Creates a new empty in-memory WAL.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an entry to the WAL.
    ///
    /// This is the WAL-before-effect gate: the entry MUST be appended
    /// before the corresponding mutation becomes visible to local
    /// queries (INV-C4).
    pub fn append(&mut self, entry: WalEntry) {
        self.entries.push(entry);
    }

    /// Returns all WAL entries in insertion order.
    ///
    /// On startup, replay re-applies all entries to reconstruct graph
    /// state. The returned `Vec` is a clone — the caller can iterate
    /// without holding a borrow on the WAL.
    #[must_use]
    pub fn replay(&self) -> Vec<WalEntry> {
        self.entries.clone()
    }

    /// Returns the number of entries in the WAL.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the WAL contains no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all entries from the WAL.
    ///
    /// In a real implementation, this would be called after a
    /// checkpoint (e.g., after snapshotting the graph to disk). For M2,
    /// it is primarily used in tests.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{LogicalClock, WallTime};

    /// Creates a minimal [`DualClockEvent`] for testing.
    fn test_dual_clock() -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1000 },
            timezone: "UTC".to_string(),
        }
    }

    // -- WalEntry variants --------------------------------------------------

    #[test]
    fn test_wal_entry_types() {
        let id = UnitId(uuid::Uuid::new_v4());

        let merged = WalEntry::Merged {
            unit_id: id,
            timestamp: test_dual_clock(),
        };
        assert!(matches!(merged, WalEntry::Merged { .. }));

        let pending = WalEntry::Pending {
            unit_id: id,
            missing_refs: BTreeSet::new(),
        };
        assert!(matches!(pending, WalEntry::Pending { .. }));

        let promoted = WalEntry::Promoted { unit_id: id };
        assert!(matches!(promoted, WalEntry::Promoted { .. }));
    }

    #[test]
    fn test_wal_entry_serialization_roundtrip() {
        let id = UnitId(uuid::Uuid::new_v4());

        let merged = WalEntry::Merged {
            unit_id: id,
            timestamp: test_dual_clock(),
        };
        let json = serde_json::to_string(&merged).expect("serialize WalEntry::Merged");
        let decoded: WalEntry = serde_json::from_str(&json).expect("deserialize WalEntry");
        assert_eq!(merged, decoded);

        let pending = WalEntry::Pending {
            unit_id: id,
            missing_refs: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
        };
        let json = serde_json::to_string(&pending).expect("serialize WalEntry::Pending");
        let decoded: WalEntry = serde_json::from_str(&json).expect("deserialize WalEntry");
        assert_eq!(pending, decoded);

        let promoted = WalEntry::Promoted { unit_id: id };
        let json = serde_json::to_string(&promoted).expect("serialize WalEntry::Promoted");
        let decoded: WalEntry = serde_json::from_str(&json).expect("deserialize WalEntry");
        assert_eq!(promoted, decoded);
    }

    // -- InMemoryWal --------------------------------------------------------

    #[test]
    fn test_wal_new_is_empty() {
        let wal = InMemoryWal::new();
        assert!(wal.is_empty());
        assert_eq!(wal.len(), 0);
        assert!(wal.replay().is_empty());
    }

    #[test]
    fn test_wal_default_is_empty() {
        let wal = InMemoryWal::default();
        assert!(wal.is_empty());
    }

    #[test]
    fn test_wal_append_and_replay() {
        let mut wal = InMemoryWal::new();

        let id1 = UnitId(uuid::Uuid::new_v4());
        let id2 = UnitId(uuid::Uuid::new_v4());

        wal.append(WalEntry::Merged {
            unit_id: id1,
            timestamp: test_dual_clock(),
        });
        wal.append(WalEntry::Promoted { unit_id: id2 });

        assert_eq!(wal.len(), 2);
        assert!(!wal.is_empty());

        let replayed = wal.replay();
        assert_eq!(
            replayed.len(),
            2,
            "replay should return all entries in order"
        );

        // Verify order is preserved
        assert!(matches!(replayed[0], WalEntry::Merged { unit_id, .. } if unit_id == id1));
        assert!(matches!(replayed[1], WalEntry::Promoted { unit_id } if unit_id == id2));
    }

    #[test]
    fn test_wal_append_preserves_order() {
        let mut wal = InMemoryWal::new();
        let ids: Vec<UnitId> = (0..10).map(|_| UnitId(uuid::Uuid::new_v4())).collect();

        for id in &ids {
            wal.append(WalEntry::Promoted { unit_id: *id });
        }

        let replayed = wal.replay();
        assert_eq!(replayed.len(), 10);

        for (i, entry) in replayed.iter().enumerate() {
            match entry {
                WalEntry::Promoted { unit_id } => {
                    assert_eq!(*unit_id, ids[i], "order must be preserved at index {i}");
                }
                _ => panic!("expected WalEntry::Promoted at index {i}"),
            }
        }
    }

    #[test]
    fn test_wal_clear() {
        let mut wal = InMemoryWal::new();
        wal.append(WalEntry::Promoted {
            unit_id: UnitId(uuid::Uuid::new_v4()),
        });
        assert!(!wal.is_empty());

        wal.clear();
        assert!(wal.is_empty());
        assert_eq!(wal.len(), 0);
    }

    #[test]
    fn test_wal_replay_returns_clone() {
        let mut wal = InMemoryWal::new();
        wal.append(WalEntry::Promoted {
            unit_id: UnitId(uuid::Uuid::new_v4()),
        });

        let replayed = wal.replay();
        // Modifying the replay result should not affect the WAL
        drop(replayed);

        assert_eq!(
            wal.len(),
            1,
            "WAL should be unaffected by replay result lifecycle"
        );
    }

    #[test]
    fn test_wal_mixed_entry_types() {
        let mut wal = InMemoryWal::new();

        let merged_id = UnitId(uuid::Uuid::new_v4());
        let pending_id = UnitId(uuid::Uuid::new_v4());
        let promoted_id = UnitId(uuid::Uuid::new_v4());

        wal.append(WalEntry::Merged {
            unit_id: merged_id,
            timestamp: test_dual_clock(),
        });
        wal.append(WalEntry::Pending {
            unit_id: pending_id,
            missing_refs: BTreeSet::from([merged_id]),
        });
        wal.append(WalEntry::Promoted {
            unit_id: promoted_id,
        });

        assert_eq!(wal.len(), 3);

        let replayed = wal.replay();
        assert!(matches!(&replayed[0], WalEntry::Merged { unit_id, .. } if *unit_id == merged_id));
        assert!(
            matches!(&replayed[1], WalEntry::Pending { unit_id, missing_refs } if *unit_id == pending_id && missing_refs.contains(&merged_id))
        );
        assert!(matches!(&replayed[2], WalEntry::Promoted { unit_id } if *unit_id == promoted_id));
    }
}
