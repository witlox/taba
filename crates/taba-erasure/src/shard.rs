//! Shard types: domain [`Shard`], criticality tiers, and shard
//! assignments.
//!
//! A [`Shard`] is an erasure-coded fragment of the composition graph.
//! Shards are distributed across nodes (see [`crate::distribution`])
//! and can be reconstructed from surviving peers when a node fails
//! (see [`crate::reconstruction`]).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use taba_common::{DualClockEvent, LogicalClock, NodeId, ShardId, UnitId, WallTime};
use uuid::Uuid;

use crate::error::ShardGroupId;
use crate::params::ErasureParams;

// ---------------------------------------------------------------------------
// ShardCriticality
// ---------------------------------------------------------------------------

/// Criticality tier for reconstruction priority ordering.
///
/// Higher-criticality shards are reconstructed first (INV-R1, FM-13).
/// Order: Governance > Policy > `DataConstraints` > Workload.
///
/// The derived `Ord` places `Governance` as the greatest value, which
/// is convenient for max-heap priority queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ShardCriticality {
    /// Workload-related shards — lowest reconstruction priority.
    Workload = 0,
    /// Data constraint shards.
    DataConstraints = 1,
    /// Policy shards.
    Policy = 2,
    /// Governance shards — highest reconstruction priority.
    /// Governance shards are also fully replicated (INV-R6).
    Governance = 3,
}

// ---------------------------------------------------------------------------
// Shard
// ---------------------------------------------------------------------------

/// An erasure-coded shard of the composition graph.
///
/// Distributed across nodes. Each node holds a subset of shards.
/// Any `k` of the `n` shards (where `k = params.data_shards`) can
/// reconstruct the original data.
///
/// Governance-criticality shards are an exception: they are actively
/// replicated (full copies on N nodes), not just erasure-coded
/// (INV-R6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shard {
    /// Unique identifier for this shard.
    pub id: ShardId,
    /// Index of this shard in the erasure coding scheme (0-based).
    pub index: u32,
    /// The encoded shard data.
    pub data: Vec<u8>,
    /// Which units' data is included in this shard.
    pub covers_units: BTreeSet<UnitId>,
    /// The erasure parameters used to encode this shard.
    pub params: ErasureParams,
    /// When this shard was last re-encoded (A002: `DualClockEvent`, not
    /// the deprecated `Timestamp` type).
    pub encoded_at: DualClockEvent,
    /// The node currently holding this shard.
    pub held_by: NodeId,
    /// Criticality tier for reconstruction priority (INV-R1).
    pub criticality: ShardCriticality,
}

impl Shard {
    /// Creates a new shard with the given index, data, params, and
    /// criticality.
    ///
    /// The shard ID is randomly generated. `covers_units` is empty,
    /// `encoded_at` is a minimal `DualClockEvent`, and `held_by` is
    /// `NodeId::nil()`. The caller is expected to fill in these
    /// fields after distribution.
    #[must_use]
    pub fn new(
        index: u32,
        data: Vec<u8>,
        params: ErasureParams,
        criticality: ShardCriticality,
    ) -> Self {
        Self {
            id: ShardId(Uuid::new_v4()),
            index,
            data,
            covers_units: BTreeSet::new(),
            params,
            encoded_at: minimal_dual_clock(),
            held_by: NodeId(Uuid::nil()),
            criticality,
        }
    }

    /// Returns `true` if this is a data shard (index < k).
    #[must_use]
    pub const fn is_data(&self) -> bool {
        self.index < self.params.data_shards
    }

    /// Returns `true` if this is a parity shard (index >= k).
    #[must_use]
    pub const fn is_parity(&self) -> bool {
        self.index >= self.params.data_shards
    }
}

impl PartialEq for Shard {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.index == other.index
            && self.data == other.data
            && self.covers_units == other.covers_units
            && self.params == other.params
            && self.encoded_at == other.encoded_at
            && self.held_by == other.held_by
            && self.criticality == other.criticality
    }
}

impl Eq for Shard {}

/// Creates a minimal `DualClockEvent` for shard construction.
///
/// Production callers should overwrite `encoded_at` with the actual
/// event timestamp after encoding.
fn minimal_dual_clock() -> DualClockEvent {
    DualClockEvent {
        logical_clock: LogicalClock(0),
        wall_time: WallTime { millis: 0 },
        timezone: "UTC".to_string(),
    }
}

// ---------------------------------------------------------------------------
// ShardAssignment
// ---------------------------------------------------------------------------

/// Assignment of a shard to a node.
///
/// Produced by [`crate::distribution::ShardManager::distribute`] and
/// tracked for re-coding and reconstruction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShardAssignment {
    /// The shard group this assignment belongs to.
    pub group: ShardGroupId,
    /// Index of the shard within the group (0-based).
    pub shard_index: u32,
    /// The node assigned to hold this shard.
    pub node: NodeId,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_params() -> ErasureParams {
        ErasureParams {
            total_shards: 3,
            data_shards: 2,
            parity_shards: 1,
            resilience_pct: 33,
        }
    }

    #[test]
    fn test_shard_serialization_roundtrip() {
        let shard = Shard {
            id: ShardId(Uuid::new_v4()),
            index: 1,
            data: vec![0xDE, 0xAD, 0xBE, 0xEF],
            covers_units: {
                let mut set = BTreeSet::new();
                set.insert(UnitId(Uuid::new_v4()));
                set.insert(UnitId(Uuid::new_v4()));
                set
            },
            params: test_params(),
            encoded_at: minimal_dual_clock(),
            held_by: NodeId(Uuid::new_v4()),
            criticality: ShardCriticality::Policy,
        };

        let json = serde_json::to_string(&shard).expect("serialize Shard");
        let decoded: Shard = serde_json::from_str(&json).expect("deserialize Shard");
        assert_eq!(shard, decoded);
    }

    #[test]
    fn test_shard_criticality_ordering() {
        use ShardCriticality::*;

        // Governance > Policy > DataConstraints > Workload
        assert!(Governance > Policy);
        assert!(Policy > DataConstraints);
        assert!(DataConstraints > Workload);
        assert!(Governance > Workload);

        // min / max: Workload is the smallest, Governance is the largest.
        assert_eq!(Workload.min(Policy), Workload);
        assert_eq!(Governance.max(Policy), Governance);
    }

    #[test]
    fn test_shard_new() {
        let params = test_params();
        let shard = Shard::new(0, vec![1, 2, 3], params, ShardCriticality::Workload);

        assert_eq!(shard.index, 0);
        assert_eq!(shard.data, vec![1, 2, 3]);
        assert_eq!(shard.params, params);
        assert_eq!(shard.criticality, ShardCriticality::Workload);
        assert!(shard.covers_units.is_empty());
        assert!(shard.is_data());
        assert!(!shard.is_parity());
    }

    #[test]
    fn test_shard_is_data_is_parity() {
        let params = test_params(); // k=2, m=1, n=3

        let data_shard = Shard::new(0, vec![1], params, ShardCriticality::Workload);
        assert!(data_shard.is_data());
        assert!(!data_shard.is_parity());

        let parity_shard = Shard::new(2, vec![1], params, ShardCriticality::Workload);
        assert!(!parity_shard.is_data());
        assert!(parity_shard.is_parity());
    }

    #[test]
    fn test_shard_assignment_serialization_roundtrip() {
        let assignment = ShardAssignment {
            group: ShardGroupId::new(),
            shard_index: 5,
            node: NodeId(Uuid::new_v4()),
        };

        let json = serde_json::to_string(&assignment).expect("serialize ShardAssignment");
        let decoded: ShardAssignment =
            serde_json::from_str(&json).expect("deserialize ShardAssignment");
        assert_eq!(assignment, decoded);
    }

    #[test]
    fn test_shard_assignment_equality() {
        let group = ShardGroupId::new();
        let node = NodeId(Uuid::new_v4());

        let a = ShardAssignment {
            group,
            shard_index: 3,
            node,
        };
        let b = ShardAssignment {
            group,
            shard_index: 3,
            node,
        };
        let c = ShardAssignment {
            group,
            shard_index: 4,
            node,
        };

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_shard_criticality_serde_roundtrip() {
        for variant in [
            ShardCriticality::Workload,
            ShardCriticality::DataConstraints,
            ShardCriticality::Policy,
            ShardCriticality::Governance,
        ] {
            let json = serde_json::to_string(&variant).expect("serialize criticality");
            let decoded: ShardCriticality =
                serde_json::from_str(&json).expect("deserialize criticality");
            assert_eq!(variant, decoded);
        }
    }
}
