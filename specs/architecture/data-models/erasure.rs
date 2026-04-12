//! Erasure coding types: shard management, reconstruction, and backpressure.
//!
//! taba uses erasure coding (not replication) for graph resilience. Shards
//! are distributed across active nodes. Parameters adapt to fleet size:
//! k = ceil(N * (1 - R/100)) where R is the configured resilience percentage
//! (INV-R4). Reconstruction has backpressure to prevent cascading failures
//! (INV-R1, FM-13).
//!
//! Governance units are an exception: they are actively replicated (full
//! copies on N nodes), not just erasure-coded (INV-R6).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::common::{NodeId, ShardId, Timestamp, UnitId};

// ---------------------------------------------------------------------------
// Erasure parameters
// ---------------------------------------------------------------------------

/// Erasure coding parameters for the current cluster configuration.
/// k-of-n coding where parameters adapt to fleet size.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ErasureParams {
    /// Total number of shards (n). Equals the number of active nodes
    /// participating in erasure coding.
    pub total_shards: u32,
    /// Minimum shards required for reconstruction (k).
    /// k = ceil(N * (1 - resilience_pct / 100)).
    pub data_shards: u32,
    /// Number of parity shards (n - k). Cluster can tolerate this many
    /// simultaneous node failures without data loss.
    pub parity_shards: u32,
    /// The configured resilience percentage from ClusterConfig.
    pub resilience_pct: u8,
}

// ---------------------------------------------------------------------------
// Shards
// ---------------------------------------------------------------------------

/// An erasure-coded shard of the composition graph.
/// Distributed across nodes. Each node holds a subset of shards.
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
    /// When this shard was last re-encoded.
    pub encoded_at: Timestamp,
    /// The node currently holding this shard.
    pub held_by: NodeId,
    /// Criticality tier for reconstruction priority (INV-R1).
    pub criticality: ShardCriticality,
}

/// Criticality tier for reconstruction priority ordering.
/// Higher-criticality shards are reconstructed first (INV-R1, FM-13).
/// Order: Governance > Policy > DataConstraints > Workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ShardCriticality {
    /// Workload-related shards -- lowest reconstruction priority.
    Workload = 0,
    /// Data constraint shards.
    DataConstraints = 1,
    /// Policy shards.
    Policy = 2,
    /// Governance shards -- highest reconstruction priority.
    Governance = 3,
}

// ---------------------------------------------------------------------------
// Reconstruction
// ---------------------------------------------------------------------------

/// A request to reconstruct a lost shard from surviving shards.
/// Queued and processed with backpressure (INV-R1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionRequest {
    /// The shard to reconstruct.
    pub shard_id: ShardId,
    /// Nodes that hold surviving shards needed for reconstruction.
    pub source_nodes: Vec<NodeId>,
    /// The node that will hold the reconstructed shard.
    pub target_node: NodeId,
    /// Why reconstruction is needed.
    pub reason: ReconstructionReason,
    /// Priority based on shard criticality.
    pub priority: ReconstructionPriority,
    /// When this request was created.
    pub requested_at: Timestamp,
}

/// Why a shard needs reconstruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ReconstructionReason {
    /// The node holding this shard has failed.
    NodeFailure { failed_node: NodeId },
    /// The node holding this shard is draining.
    NodeDraining { draining_node: NodeId },
    /// Shard data was found to be corrupted (post-reconstruction re-verification).
    Corruption,
    /// Rebalancing after a new node joined.
    Rebalance,
}

/// Priority level for shard reconstruction.
/// Derived from ShardCriticality plus urgency factors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReconstructionPriority {
    /// Base criticality from shard content type.
    pub criticality: ShardCriticality,
    /// Urgency: how many parity shards remain for this data.
    /// Lower = more urgent (closer to data loss threshold).
    pub remaining_parity: u32,
}

// ---------------------------------------------------------------------------
// Backpressure
// ---------------------------------------------------------------------------

/// State of the reconstruction backpressure mechanism (INV-R1, FM-13).
/// Prevents cascading failures when multiple nodes fail in rapid succession.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureState {
    /// Current reconstruction queue depth.
    pub queue_depth: u32,
    /// Maximum queue depth before circuit breaker trips.
    pub circuit_breaker_threshold: u32,
    /// Whether the circuit breaker is currently tripped.
    pub circuit_breaker_open: bool,
    /// Current throttle rate: reconstructions permitted per second.
    pub throttle_rate: u32,
    /// Number of reconstructions currently in progress.
    pub in_progress: u32,
    /// Number of reconstructions completed since last reset.
    pub completed: u64,
    /// Number of reconstructions that failed.
    pub failed: u64,
    /// When the circuit breaker last changed state.
    pub last_state_change: Timestamp,
}
