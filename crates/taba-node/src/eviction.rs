//! Eviction: node-local memory relief (INV-G4).
//!
//! Eviction drops the full content of non-active units from local
//! memory while preserving references (`UnitId` + shard location) for
//! later reconstruction. This is distinct from compaction (which
//! removes from the graph entirely and creates tombstones).
//!
//! Eviction is a cache operation, not a lifecycle event. The unit
//! remains live in the graph — eviction only affects the node's local
//! copy. Evicted content is reconstructable from peers (erasure
//! coding) or archive.
//!
//! ## INV-G4 summary
//!
//! | Property | Eviction | Compaction |
//! |----------|----------|------------|
//! | Scope | Node-local | Graph-wide |
//! | Tombstone | No | Yes |
//! | Unit in graph | Remains live | Removed |
//! | Content | Dropped locally | Dropped everywhere |
//! | Reconstruction | From peers/archive | Not possible |

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use taba_common::UnitId;
use taba_core::Unit;

use crate::error::NodeError;

// ===========================================================================
// ShardLocation
// ===========================================================================

/// Location of an evicted unit's content for later reconstruction.
///
/// Preserved when a unit is evicted so the node knows where to find
/// the content again — from peers (erasure-coded shards), a local
/// archive, or another source identified by `location_hint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShardLocation {
    /// The evicted unit's ID.
    pub unit_id: UnitId,
    /// Hint for locating the content (peer node ID, shard ID, archive
    /// path, etc.). The format is intentionally freeform to support
    /// different reconstruction sources.
    pub location_hint: String,
}

// ===========================================================================
// EvictionPolicy
// ===========================================================================

/// Node-local eviction policy (INV-G4).
///
/// Manages local memory by evicting full content of non-active units
/// while preserving references for reconstruction. Eviction is a cache
/// operation — the unit remains live in the graph.
///
/// ## Operations
///
/// - [`store`](Self::store): place full content in local memory.
/// - [`evict`](Self::evict): drop full content, keep a reference
///   ([`ShardLocation`]) for reconstruction.
/// - [`content`](Self::content): retrieve full content if not evicted.
/// - [`shard_location`](Self::shard_location): retrieve the reference
///   for an evicted unit.
///
/// Thread-safe via interior mutability. The mutex is never held across
/// an `await` point — all operations are synchronous.
pub struct EvictionPolicy {
    /// Full content of units currently in local memory.
    content: Mutex<BTreeMap<UnitId, Unit>>,
    /// References for evicted units (`UnitId` → shard location).
    evicted: Mutex<BTreeMap<UnitId, ShardLocation>>,
}

impl EvictionPolicy {
    /// Creates a new empty eviction policy.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            content: Mutex::new(BTreeMap::new()),
            evicted: Mutex::new(BTreeMap::new()),
        }
    }

    /// Stores full content for a unit in local memory.
    ///
    /// If the unit was previously evicted, the eviction entry is
    /// removed and the full content is restored.
    pub fn store(&self, unit: Unit) {
        let id = unit.id();
        self.content
            .lock()
            .expect("content mutex should not be poisoned")
            .insert(id, unit);
        self.evicted
            .lock()
            .expect("evicted mutex should not be poisoned")
            .remove(&id);
    }

    /// Returns the full content of a unit if it is currently in local
    /// memory (not evicted).
    #[must_use]
    pub fn content(&self, unit_id: &UnitId) -> Option<Unit> {
        self.content
            .lock()
            .expect("content mutex should not be poisoned")
            .get(unit_id)
            .cloned()
    }

    /// Evicts a unit from local memory, dropping its full content
    /// while preserving a [`ShardLocation`] reference for later
    /// reconstruction (INV-G4).
    ///
    /// After eviction:
    /// - The unit's full content is NOT in local memory.
    /// - The unit's [`ShardLocation`] IS preserved (for reconstruction).
    /// - The unit remains live in the graph (eviction is node-local,
    ///   not a lifecycle event).
    ///
    /// # Errors
    ///
    /// - [`NodeError::UnitNotFound`] if the unit is not in local
    ///   memory (already evicted or never stored).
    pub fn evict(&self, unit_id: &UnitId, location_hint: &str) -> Result<ShardLocation, NodeError> {
        let mut content = self
            .content
            .lock()
            .expect("content mutex should not be poisoned");

        // Remove the full content. If not present, the unit was never
        // stored or already evicted.
        let unit = content
            .remove(unit_id)
            .ok_or(NodeError::UnitNotFound { id: *unit_id })?;

        // Drop the full content — only the reference is preserved.
        let unit_id = unit.id();
        drop(unit);

        let shard = ShardLocation {
            unit_id,
            location_hint: location_hint.to_string(),
        };

        self.evicted
            .lock()
            .expect("evicted mutex should not be poisoned")
            .insert(unit_id, shard.clone());

        Ok(shard)
    }

    /// Returns `true` if the unit's content has been evicted from
    /// local memory (reference-only).
    #[must_use]
    pub fn is_evicted(&self, unit_id: &UnitId) -> bool {
        self.evicted
            .lock()
            .expect("evicted mutex should not be poisoned")
            .contains_key(unit_id)
    }

    /// Returns the [`ShardLocation`] for an evicted unit, if
    /// available.
    ///
    /// Used for reconstruction: the node fetches the content from
    /// peers (erasure coding) or archive using this hint.
    #[must_use]
    pub fn shard_location(&self, unit_id: &UnitId) -> Option<ShardLocation> {
        self.evicted
            .lock()
            .expect("evicted mutex should not be poisoned")
            .get(unit_id)
            .cloned()
    }

    /// Returns the number of units with full content in local memory.
    #[must_use]
    pub fn content_count(&self) -> usize {
        self.content
            .lock()
            .expect("content mutex should not be poisoned")
            .len()
    }

    /// Returns the number of evicted units (references only).
    #[must_use]
    pub fn evicted_count(&self) -> usize {
        self.evicted
            .lock()
            .expect("evicted mutex should not be poisoned")
            .len()
    }
}

impl Default for EvictionPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EvictionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvictionPolicy").finish_non_exhaustive()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_test_harness::WorkloadUnitBuilder;
    use uuid::Uuid;

    fn test_unit(id: UnitId) -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build())
    }

    #[test]
    fn scenario_evict_drops_content_but_keeps_reference() {
        // INV-G4: After eviction, the unit is still tracked (reference
        // preserved) but its content is NOT in local memory.
        let policy = EvictionPolicy::new();
        let id = UnitId(Uuid::new_v4());
        let unit = test_unit(id);

        // Store the unit's full content.
        policy.store(unit);
        assert_eq!(policy.content_count(), 1);
        assert!(!policy.is_evicted(&id));
        assert!(
            policy.content(&id).is_some(),
            "content should be available before eviction"
        );

        // Evict the unit.
        let shard = policy
            .evict(&id, "peer:node-abc/shard-42")
            .expect("eviction should succeed");

        assert_eq!(shard.unit_id, id);
        assert_eq!(shard.location_hint, "peer:node-abc/shard-42");

        // After eviction:
        // 1. Content is NOT in local memory.
        assert!(
            policy.content(&id).is_none(),
            "content should NOT be in local memory after eviction (INV-G4)"
        );
        assert_eq!(
            policy.content_count(),
            0,
            "content count should be 0 after eviction"
        );

        // 2. The unit is still tracked (reference preserved).
        assert!(
            policy.is_evicted(&id),
            "unit should be marked as evicted (reference preserved, INV-G4)"
        );
        assert_eq!(policy.evicted_count(), 1);

        // 3. The shard location is available for reconstruction.
        let retrieved = policy
            .shard_location(&id)
            .expect("shard location should be available for evicted unit");
        assert_eq!(retrieved, shard);
    }

    #[test]
    fn scenario_evict_unknown_unit_rejected() {
        // Evicting a unit that was never stored should fail.
        let policy = EvictionPolicy::new();
        let unknown_id = UnitId(Uuid::new_v4());

        let result = policy.evict(&unknown_id, "peer:node-abc");
        assert!(
            matches!(result, Err(NodeError::UnitNotFound { id }) if id == unknown_id),
            "evicting unknown unit should return UnitNotFound"
        );
    }

    #[test]
    fn scenario_double_evict_rejected() {
        // Evicting an already-evicted unit should fail (content is
        // already gone).
        let policy = EvictionPolicy::new();
        let id = UnitId(Uuid::new_v4());

        policy.store(test_unit(id));
        policy
            .evict(&id, "peer:node-abc")
            .expect("first eviction should succeed");

        let result = policy.evict(&id, "peer:node-abc");
        assert!(
            matches!(result, Err(NodeError::UnitNotFound { .. })),
            "double eviction should fail — content already dropped"
        );
    }

    #[test]
    fn scenario_store_after_evict_restores_content() {
        // After eviction, storing the unit again should restore full
        // content and remove the eviction entry.
        let policy = EvictionPolicy::new();
        let id = UnitId(Uuid::new_v4());

        policy.store(test_unit(id));
        policy
            .evict(&id, "peer:node-abc")
            .expect("eviction should succeed");

        assert!(policy.is_evicted(&id));
        assert!(policy.content(&id).is_none());

        // Restore by storing again.
        policy.store(test_unit(id));

        assert!(
            !policy.is_evicted(&id),
            "unit should no longer be marked as evicted after restore"
        );
        assert!(
            policy.content(&id).is_some(),
            "content should be available again after restore"
        );
        assert_eq!(policy.content_count(), 1);
        assert_eq!(policy.evicted_count(), 0);
    }

    #[test]
    fn scenario_eviction_does_not_affect_other_units() {
        // Evicting one unit should not affect other units' content.
        let policy = EvictionPolicy::new();
        let id_a = UnitId(Uuid::new_v4());
        let id_b = UnitId(Uuid::new_v4());

        policy.store(test_unit(id_a));
        policy.store(test_unit(id_b));

        assert_eq!(policy.content_count(), 2);

        // Evict only A.
        policy
            .evict(&id_a, "peer:node-abc")
            .expect("eviction of A should succeed");

        // B should still have content.
        assert!(
            policy.content(&id_a).is_none(),
            "A content should be evicted"
        );
        assert!(
            policy.content(&id_b).is_some(),
            "B content should be unaffected by A's eviction"
        );
        assert_eq!(policy.content_count(), 1);
        assert_eq!(policy.evicted_count(), 1);
    }

    #[test]
    fn scenario_shard_location_serialization_roundtrip() {
        let shard = ShardLocation {
            unit_id: UnitId(Uuid::new_v4()),
            location_hint: "peer:node-xyz/shard-7".to_string(),
        };

        let json = serde_json::to_string(&shard).expect("serialize");
        let decoded: ShardLocation = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(shard, decoded);
    }
}
