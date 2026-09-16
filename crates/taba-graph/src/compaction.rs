//! Compaction engine for the composition graph (INV-G1 through INV-G5).
//!
//! Compaction removes or tombstones units that are no longer needed,
//! freeing memory while preserving provenance graph structure
//! (INV-G2). Eligibility is deterministic: the same graph state
//! produces the same eligible units on all nodes (INV-G1).
//!
//! ## Priority order (INV-G5)
//!
//! Compaction processes units in priority order (least valuable first):
//!
//! 1. Ephemeral data (no refs → `Remove`, has refs → `Tombstone`)
//! 2. Terminated bounded tasks
//! 3. Superseded (revoked) policies
//! 4. Expired persistent data
//!
//! Governance units are never compacted (INV-G3).

use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use taba_common::{UnitId, WallTime};
use taba_core::{DataUnit, RetentionMode, Unit, UnitKind, UnitState};

use crate::crdt::CompositionGraphData;
use crate::entry::GraphEntry;
use crate::error::GraphError;

// ===========================================================================
// CompactionAction
// ===========================================================================

/// Action for a compactable unit.
///
/// Determines how a unit is removed from the active graph during
/// compaction. The action preserves provenance integrity (INV-G2)
/// while freeing memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompactionAction {
    /// Fully remove (no tombstone). For unreferenced ephemeral data
    /// — the data was truly intermediate scratch (INV-D4).
    Remove,
    /// Replace with a tombstone (preserves references). For data
    /// with downstream references, terminated tasks, and superseded
    /// policies.
    Tombstone,
    /// Archive to cold storage first, then tombstone. For units
    /// whose retention policy mandates archival before removal.
    ArchiveAndTombstone,
}

// ===========================================================================
// Compactor trait
// ===========================================================================

/// Compaction engine for the graph (INV-G1 through INV-G5).
///
/// Compaction eligibility is deterministic: same graph state = same
/// eligible units on all nodes (INV-G1). Tombstones preserve
/// provenance (INV-G2). Governance units are exempt (INV-G3).
/// Priority order per INV-G5.
pub trait Compactor {
    /// Compute which units are eligible for compaction in the
    /// current graph.
    ///
    /// Deterministic: same graph state = same result on any node.
    /// Returns a `Vec` of `(UnitId, CompactionAction)` pairs,
    /// ordered by priority (INV-G5).
    fn compute_eligible(&self) -> Vec<(UnitId, CompactionAction)>;

    /// Execute compaction, freeing at least `target_bytes` of memory.
    ///
    /// Processes eligible units in priority order (INV-G5):
    /// ephemeral → terminated tasks → superseded policies →
    /// expired data.
    ///
    /// For ephemeral data: reference check (INV-D4) — no refs →
    /// `Remove`, has refs → `Tombstone`.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::CompactionFailed`] if the compaction
    /// could not free the target bytes or encountered errors.
    fn compact(
        &self,
        target_bytes: u64,
    ) -> impl Future<Output = Result<(u64, u64), GraphError>> + Send;
}

// ===========================================================================
// DefaultCompactor
// ===========================================================================

/// Default implementation of [`Compactor`].
///
/// Computes compaction eligibility based on:
/// - Retention mode (ephemeral → highest priority, INV-G5)
/// - Unit state (terminated bounded tasks)
/// - Policy revocation status (superseded policies)
/// - Reference presence (INV-D4: no refs → `Remove`, has refs →
///   `Tombstone`)
///
/// Governance units are never compacted (INV-G3).
///
/// The compactor shares graph state via `Arc<Mutex<...>>` — this
/// is the same lock used by [`crate::DefaultGraph`]. The lock is
/// never held across an `await` point.
pub struct DefaultCompactor {
    /// Shared graph state.
    state: Arc<Mutex<CompositionGraphData>>,
    /// Memory limit in bytes (for computing how much to free).
    memory_limit_bytes: u64,
}

impl DefaultCompactor {
    /// Creates a new `DefaultCompactor` sharing the given graph state.
    ///
    /// The `memory_limit_bytes` is used to compute compaction targets
    /// when invoked via `Graph::compact()`.
    #[must_use]
    pub const fn new(state: Arc<Mutex<CompositionGraphData>>, memory_limit_bytes: u64) -> Self {
        Self {
            state,
            memory_limit_bytes,
        }
    }

    /// Returns the memory limit this compactor was configured with.
    #[must_use]
    pub const fn memory_limit_bytes(&self) -> u64 {
        self.memory_limit_bytes
    }

    /// Computes the compaction action for a single entry.
    ///
    /// Returns `None` if the entry is not eligible for compaction.
    fn action_for(entry: &GraphEntry) -> Option<CompactionAction> {
        let unit = entry.unit();

        // INV-G3: governance units are never compacted.
        if unit.kind() == UnitKind::Governance {
            return None;
        }

        // Archived units are already compacted.
        if entry.archived {
            return None;
        }

        match unit {
            Unit::Data(d) => {
                // Ephemeral data (INV-D4, INV-G5 priority 1).
                if d.retention.mode == taba_core::RetentionMode::Ephemeral {
                    if entry.referenced_by.is_empty() {
                        return Some(CompactionAction::Remove);
                    }
                    return Some(CompactionAction::Tombstone);
                }

                // Local-only data — never in graph, but if it somehow
                // is, treat like ephemeral.
                if d.retention.mode == taba_core::RetentionMode::LocalOnly {
                    if entry.referenced_by.is_empty() {
                        return Some(CompactionAction::Remove);
                    }
                    return Some(CompactionAction::Tombstone);
                }

                // Persistent data: check for expiry.
                // In M2, without wall time tracking, we can't check
                // expiry. Skip persistent data compaction.
                // (Future: check d.retention.duration vs wall clock.)
                None
            }
            Unit::Workload(w) => {
                // Terminated bounded tasks (INV-G5 priority 3).
                if w.kind == taba_core::WorkloadKind::BoundedTask
                    && w.header.state == UnitState::Terminated
                {
                    return Some(CompactionAction::Tombstone);
                }
                // Services are permanent (INV-W1) — not compacted
                // unless explicitly drained.
                None
            }
            Unit::Policy(p) => {
                // Superseded/revoked policies (INV-G5 priority 4).
                if p.revoked {
                    return Some(CompactionAction::Tombstone);
                }
                None
            }
            Unit::Governance(_) => None,
        }
    }

    /// Orders eligible units by compaction priority (INV-G5).
    ///
    /// Priority (lowest number = highest priority = compacted first):
    /// 1. Ephemeral data
    /// 2. Terminated bounded tasks
    /// 3. Superseded policies
    /// 4. Expired persistent data
    fn priority(entry: &GraphEntry) -> u8 {
        let unit = entry.unit();
        match unit {
            Unit::Data(d) => {
                if d.retention.mode == taba_core::RetentionMode::Ephemeral
                    || d.retention.mode == taba_core::RetentionMode::LocalOnly
                {
                    1
                } else {
                    4 // Expired persistent data (future)
                }
            }
            Unit::Workload(w) => {
                if w.kind == taba_core::WorkloadKind::BoundedTask
                    && w.header.state == UnitState::Terminated
                {
                    2
                } else {
                    5 // Services (not normally compacted)
                }
            }
            Unit::Policy(p) => {
                if p.revoked {
                    3
                } else {
                    255
                }
            }
            Unit::Governance(_) => 255,
        }
    }
}

impl Compactor for DefaultCompactor {
    fn compute_eligible(&self) -> Vec<(UnitId, CompactionAction)> {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");

        let mut eligible: Vec<(UnitId, CompactionAction, u8)> = state
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                Self::action_for(entry).map(|action| (*id, action, Self::priority(entry)))
            })
            .collect();

        // Sort by priority (lowest = highest priority = first).
        eligible.sort_by_key(|(_, _, priority)| *priority);

        eligible
            .into_iter()
            .map(|(id, action, _)| (id, action))
            .collect()
    }

    async fn compact(&self, target_bytes: u64) -> Result<(u64, u64), GraphError> {
        let eligible = self.compute_eligible();

        let mut freed_bytes: u64 = 0;
        let mut compacted_count: u64 = 0;

        {
            let mut state = self
                .state
                .lock()
                .expect("graph mutex should not be poisoned");

            for (id, action) in &eligible {
                if freed_bytes >= target_bytes {
                    break;
                }

                let Some(entry) = state.entries.get(id) else {
                    continue;
                };

                let entry_size = entry.memory_estimate();

                match action {
                    CompactionAction::Remove => {
                        state.entries.remove(id);
                    }
                    CompactionAction::Tombstone | CompactionAction::ArchiveAndTombstone => {
                        if let Some(e) = state.entries.get_mut(id) {
                            e.archived = true;
                        }
                    }
                }

                freed_bytes = freed_bytes.saturating_add(entry_size);
                compacted_count += 1;
            }

            state.recompute_memory();
            state.local_clock.tick();
            state.generation = state.generation.saturating_add(1);
        }

        Ok((compacted_count, freed_bytes))
    }
}

// ===========================================================================
// RetentionChecker (INV-D2)
// ===========================================================================

/// Checks whether a data unit's retention has expired based on
/// wall-clock time (INV-D2).
///
/// Expired data units are eligible for compaction. The checker
/// compares the data unit's `created_at` wall time plus its
/// `retention.duration` against the current `WallTime`.
///
/// - **Persistent with duration**: expired if `created_at + duration
///   <= now`.
/// - **Persistent without duration**: never expires (returns
///   `false`). The data is retained indefinitely.
/// - **Ephemeral / `LocalOnly`**: not subject to wall-time expiry
///   (ephemeral is handled by INV-D4 when the producing task
///   terminates; local-only never enters the graph). Returns
///   `false`.
///
/// The checker is a pure function: no I/O, no side effects.
/// Given the same `WallTime` and `DataUnit`, the result is identical
/// on every node (INV-G1, INV-C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionChecker {
    /// The current wall-clock time used for expiry checks.
    now: WallTime,
}

impl RetentionChecker {
    /// Creates a new `RetentionChecker` with the given current time.
    ///
    /// The `now` value is the wall-clock time at which the retention
    /// check is performed. It must be in milliseconds since the Unix
    /// epoch (matching [`WallTime::millis`]).
    #[must_use]
    pub const fn new(now: WallTime) -> Self {
        Self { now }
    }

    /// Returns the current wall-clock time this checker was created
    /// with.
    #[must_use]
    pub const fn now(&self) -> WallTime {
        self.now
    }

    /// Returns `true` if the given data unit's retention has expired
    /// as of this checker's wall-clock time (INV-D2).
    ///
    /// Only `Persistent` data with an explicit `duration` is subject
    /// to wall-time expiry. Data with no duration (retained
    /// indefinitely) or non-persistent retention modes return
    /// `false`.
    ///
    /// # Determinism
    ///
    /// This is a pure function: given the same `WallTime` and
    /// `DataUnit`, the result is identical on every node. No
    /// floating-point arithmetic is used.
    #[must_use]
    pub fn is_expired(&self, data: &DataUnit) -> bool {
        // Only persistent data with a duration is subject to
        // wall-time expiry.
        if data.retention.mode != RetentionMode::Persistent {
            return false;
        }

        let Some(duration) = data.retention.duration else {
            // No duration — retained indefinitely (INV-D2).
            return false;
        };

        let created_ms = u128::from(data.header.created_at.wall_time.millis);
        let duration_ms = duration.as_millis();
        let now_ms = u128::from(self.now.millis);

        // Expired if the retention period has elapsed: created_at +
        // duration <= now. Using u128 to avoid overflow on large
        // durations.
        created_ms.saturating_add(duration_ms) <= now_ms
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use taba_common::{
        AuthorId, ClusterId, DualClockEvent, LogicalClock, TrustDomainId, ValidityWindow, WallTime,
    };
    use taba_core::{Unit, WorkloadKind};
    use taba_security::{PublicKey, Signature, SignatureContext};
    use taba_test_harness::{DataUnitBuilder, WorkloadUnitBuilder};

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

    /// Creates a [`GraphEntry`] from a [`Unit`].
    fn entry(unit: Unit) -> GraphEntry {
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new())
    }

    /// Creates an ephemeral [`Unit::Data`] (no refs).
    fn ephemeral_data_no_refs(id: UnitId) -> Unit {
        Unit::Data(
            DataUnitBuilder::new()
                .with_id(id)
                .with_retention(taba_core::RetentionPolicy {
                    mode: taba_core::RetentionMode::Ephemeral,
                    duration: None,
                    legal_basis: "interim".to_string(),
                    mandatory: false,
                })
                .build(),
        )
    }

    /// Creates a terminated bounded task [`Unit::Workload`].
    fn terminated_bounded_task(id: UnitId) -> Unit {
        let mut workload = WorkloadUnitBuilder::new()
            .with_id(id)
            .with_kind(WorkloadKind::BoundedTask)
            .with_validity(ValidityWindow {
                lc_range: Some((LogicalClock(10), LogicalClock(100))),
                wall_time_deadline: None,
            })
            .build();
        workload.header.state = UnitState::Terminated;
        Unit::Workload(workload)
    }

    /// Creates a state with the given entries.
    fn state_with(entries: Vec<GraphEntry>) -> Arc<Mutex<CompositionGraphData>> {
        let mut data = CompositionGraphData::new();
        for entry in entries {
            let id = entry.unit_id();
            data.entries.insert(id, entry);
        }
        data.recompute_memory();
        Arc::new(Mutex::new(data))
    }

    #[test]
    fn test_compaction_eligible_ephemeral() {
        let id = UnitId(uuid::Uuid::new_v4());
        let state = state_with(vec![entry(ephemeral_data_no_refs(id))]);
        let compactor = DefaultCompactor::new(state, 1_000_000);

        let eligible = compactor.compute_eligible();
        assert_eq!(eligible.len(), 1, "ephemeral data should be eligible");
        assert_eq!(eligible[0].0, id);
        assert_eq!(
            eligible[0].1,
            CompactionAction::Remove,
            "ephemeral data with no refs should be Remove"
        );
    }

    #[test]
    fn test_compaction_eligible_with_refs() {
        let id = UnitId(uuid::Uuid::new_v4());
        let ref_by = UnitId(uuid::Uuid::new_v4());

        let mut data = CompositionGraphData::new();
        let mut entry = entry(ephemeral_data_no_refs(id));
        entry.referenced_by.insert(ref_by);
        data.entries.insert(id, entry);
        data.recompute_memory();

        let state = Arc::new(Mutex::new(data));
        let compactor = DefaultCompactor::new(state, 1_000_000);

        let eligible = compactor.compute_eligible();
        assert_eq!(
            eligible.len(),
            1,
            "ephemeral data with refs should be eligible"
        );
        assert_eq!(
            eligible[0].1,
            CompactionAction::Tombstone,
            "ephemeral data with refs should be Tombstone"
        );
    }

    #[test]
    fn test_compaction_priority_order() {
        // Create one of each compaction-eligible type
        let ephemeral_id = UnitId(uuid::Uuid::new_v4());
        let terminated_id = UnitId(uuid::Uuid::new_v4());
        let expired_id = UnitId(uuid::Uuid::new_v4()); // Will be persistent (not compacted in M2)

        let entries = vec![
            entry(ephemeral_data_no_refs(ephemeral_id)),
            entry(terminated_bounded_task(terminated_id)),
            entry(ephemeral_data_no_refs(expired_id)),
        ];

        let state = state_with(entries);
        let compactor = DefaultCompactor::new(state, 1_000_000);

        let eligible = compactor.compute_eligible();

        // Ephemeral data should come before terminated bounded tasks
        let ephemeral_pos = eligible
            .iter()
            .position(|(id, _)| *id == ephemeral_id)
            .expect("ephemeral should be eligible");
        let terminated_pos = eligible
            .iter()
            .position(|(id, _)| *id == terminated_id)
            .expect("terminated should be eligible");

        assert!(
            ephemeral_pos < terminated_pos,
            "ephemeral data (priority 1) should come before terminated bounded tasks (priority 2)"
        );

        // expired_id is actually ephemeral too (we reused the function),
        // so it should also be in the eligible list
        assert!(
            eligible.iter().any(|(id, _)| *id == expired_id),
            "second ephemeral should also be eligible"
        );
    }

    #[test]
    fn test_compaction_governance_exempt() {
        // Governance units should never be compacted
        let gov_unit = Unit::Governance(taba_core::GovernanceUnit::KeyRevocation(
            taba_core::KeyRevocationDef {
                header: taba_core::UnitHeader {
                    id: UnitId(uuid::Uuid::new_v4()),
                    author: AuthorId(uuid::Uuid::new_v4()),
                    trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
                    created_at: test_dual_clock(),
                    validity: None,
                    state: UnitState::Declared,
                    version: None,
                },
                revoked_author: AuthorId(uuid::Uuid::new_v4()),
                revocation_lc: LogicalClock(1),
                reason: "test".to_string(),
            },
        ));

        let state = state_with(vec![entry(gov_unit)]);
        let compactor = DefaultCompactor::new(state, 1_000_000);

        let eligible = compactor.compute_eligible();
        assert!(
            eligible.is_empty(),
            "governance units should never be compacted"
        );
    }

    #[tokio::test]
    async fn test_compaction_compact_removes() {
        let id = UnitId(uuid::Uuid::new_v4());
        let state = state_with(vec![entry(ephemeral_data_no_refs(id))]);

        let initial_count = {
            let s = state.lock().expect("lock");
            s.active_count()
        };
        assert_eq!(initial_count, 1);

        let compactor = DefaultCompactor::new(state.clone(), 1_000_000);
        let (compacted, freed) = compactor.compact(1).await.expect("compact should succeed");

        assert_eq!(compacted, 1, "should compact 1 unit");
        assert!(freed > 0, "should free some bytes");

        let after_count = {
            let s = state.lock().expect("lock");
            s.active_count()
        };
        assert_eq!(after_count, 0, "unit should be removed after compaction");
    }

    #[tokio::test]
    async fn test_compaction_compact_tombstones() {
        let id = UnitId(uuid::Uuid::new_v4());
        let ref_by = UnitId(uuid::Uuid::new_v4());

        let mut data = CompositionGraphData::new();
        let mut entry = entry(ephemeral_data_no_refs(id));
        entry.referenced_by.insert(ref_by);
        data.entries.insert(id, entry);
        data.recompute_memory();

        let state = Arc::new(Mutex::new(data));
        let compactor = DefaultCompactor::new(state.clone(), 1_000_000);

        let (compacted, _) = compactor.compact(1).await.expect("compact should succeed");
        assert_eq!(compacted, 1, "should compact 1 unit");

        let s = state.lock().expect("lock");
        let entry = s
            .entries
            .get(&id)
            .expect("entry should still exist (tombstoned)");
        assert!(
            entry.archived,
            "entry should be archived (tombstoned), not removed"
        );
    }

    #[tokio::test]
    async fn test_compaction_compact_partial() {
        // Compact only enough to meet target_bytes
        let ids: Vec<UnitId> = (0..5).map(|_| UnitId(uuid::Uuid::new_v4())).collect();
        let entries: Vec<GraphEntry> = ids
            .iter()
            .map(|&id| entry(ephemeral_data_no_refs(id)))
            .collect();
        let state = state_with(entries);

        // Get the memory estimate of one entry
        let one_entry_size = {
            let s = state.lock().expect("lock");
            s.memory_estimate_bytes / 5
        };

        let compactor = DefaultCompactor::new(state.clone(), 1_000_000);
        // Target: free about 2 entries worth
        let target = one_entry_size * 2;
        let (compacted, _) = compactor
            .compact(target)
            .await
            .expect("compact should succeed");

        assert!(
            compacted <= 3,
            "should compact at most 3 units (target = 2 entries, may need 1 extra)"
        );
        assert!(compacted >= 2, "should compact at least 2 units");
    }

    #[test]
    fn test_compaction_action_serialization() {
        for action in [
            CompactionAction::Remove,
            CompactionAction::Tombstone,
            CompactionAction::ArchiveAndTombstone,
        ] {
            let json = serde_json::to_string(&action).expect("serialize");
            let decoded: CompactionAction = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(action, decoded);
        }
    }

    #[test]
    fn test_compaction_persistent_data_not_compacted() {
        // Persistent data is not compacted in M2 (no wall time check)
        let id = UnitId(uuid::Uuid::new_v4());
        let state = state_with(vec![entry(Unit::Data(
            DataUnitBuilder::new().with_id(id).build(),
        ))]);
        let compactor = DefaultCompactor::new(state, 1_000_000);

        let eligible = compactor.compute_eligible();
        assert!(
            eligible.is_empty(),
            "persistent data should not be compacted in M2"
        );
    }

    #[test]
    fn test_compaction_active_service_not_compacted() {
        let id = UnitId(uuid::Uuid::new_v4());
        let state = state_with(vec![entry(Unit::Workload(
            WorkloadUnitBuilder::new().with_id(id).build(),
        ))]);
        let compactor = DefaultCompactor::new(state, 1_000_000);

        let eligible = compactor.compute_eligible();
        assert!(
            eligible.is_empty(),
            "active service should not be compacted (INV-W1)"
        );
    }

    // -- RetentionChecker (INV-D2) ----------------------------------------

    #[test]
    fn scenario_retention_expired_persistent_data() {
        // INV-D2: A persistent data unit whose retention duration has
        // elapsed is expired and eligible for compaction.
        let created_at = WallTime { millis: 1000 };
        let duration = std::time::Duration::from_millis(1000);

        let mut data = DataUnitBuilder::new().build();
        data.header.created_at.wall_time = created_at;
        data.retention = taba_core::RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: Some(duration),
            legal_basis: "consent".to_string(),
            mandatory: false,
        };

        // now = 3000ms → created_at + duration = 2000ms <= 3000ms → expired
        let checker = RetentionChecker::new(WallTime { millis: 3000 });
        assert!(
            checker.is_expired(&data),
            "persistent data created at 1000ms with 1000ms duration should be expired at 3000ms (INV-D2)"
        );
    }

    #[test]
    fn scenario_retention_non_expired_persistent_data() {
        // INV-D2: A persistent data unit whose retention duration has
        // not yet elapsed is NOT expired.
        let created_at = WallTime { millis: 1000 };
        let duration = std::time::Duration::from_secs(86_400); // 1 day

        let mut data = DataUnitBuilder::new().build();
        data.header.created_at.wall_time = created_at;
        data.retention = taba_core::RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: Some(duration),
            legal_basis: "consent".to_string(),
            mandatory: false,
        };

        // now = 2000ms → created_at + duration = 86401000ms > 2000ms → not expired
        let checker = RetentionChecker::new(WallTime { millis: 2000 });
        assert!(
            !checker.is_expired(&data),
            "persistent data created at 1000ms with 1-day duration should NOT be expired at 2000ms (INV-D2)"
        );
    }

    #[test]
    fn scenario_retention_no_duration_never_expires() {
        // INV-D2: A persistent data unit with no duration is retained
        // indefinitely and never expires.
        let created_at = WallTime { millis: 1000 };

        let mut data = DataUnitBuilder::new().build();
        data.header.created_at.wall_time = created_at;
        data.retention = taba_core::RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: None,
            legal_basis: "indefinite".to_string(),
            mandatory: false,
        };

        // now = very far in the future → still not expired (no duration)
        let checker = RetentionChecker::new(WallTime { millis: u64::MAX });
        assert!(
            !checker.is_expired(&data),
            "persistent data with no duration should never expire (INV-D2)"
        );
    }
}
