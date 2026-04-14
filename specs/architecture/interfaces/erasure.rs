// taba-erasure: Reed-Solomon erasure coding, shard distribution, and reconstruction.
//
// This crate provides k-of-n Reed-Solomon erasure coding over GF(2^8) (DL-013)
// for graph shards. Implementation: `reed-solomon-erasure` crate (SIMD-accelerated).
// It is NOT replication — it encodes data into n shards such that any k shards
// can reconstruct the original. Parameters adapt to fleet size:
// k = ceil(N * (1 - R/100)) where R is the resilience percentage. Default R=33.
// GF(2^8) limits n to 256; for clusters >128 nodes, distribute to a subset.
//
// Reconstruction has backpressure (INV-R1): throttled rate, priority queue,
// circuit breaker.

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct NodeId(/* opaque */);
pub struct UnitId(/* opaque */);

/// A single erasure-coded shard.
pub struct Shard {
    /// Which shard index within the encoding (0..n-1).
    pub index: u32,
    /// The encoded data.
    pub data: Vec<u8>,
}

/// Parameters for erasure coding: k data shards, m parity shards, n = k + m total.
#[derive(Debug, Clone, Copy)]
pub struct ErasureParams {
    /// Number of data shards (minimum for reconstruction).
    pub k: u32,
    /// Number of parity shards.
    pub m: u32,
}

/// Identifier for a shard group (all shards encoding the same data).
pub struct ShardGroupId(/* opaque */);

/// Assignment of a shard to a node.
pub struct ShardAssignment {
    pub group: ShardGroupId,
    pub shard_index: u32,
    pub node: NodeId,
}

/// Priority tier for reconstruction (INV-R1).
/// Higher priority = reconstructed first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReconstructionPriority {
    /// Governance unit shards — highest priority.
    Governance,
    /// Policy unit shards.
    Policy,
    /// Data constraint/classification shards.
    DataConstraint,
    /// Workload unit shards — lowest priority.
    Workload,
}

/// Status of a reconstruction job.
pub enum ReconstructionStatus {
    /// Queued, waiting for processing.
    Queued { position: usize },
    /// Currently being reconstructed.
    InProgress { shards_received: u32, shards_needed: u32 },
    /// Successfully reconstructed.
    Complete,
    /// Failed (insufficient shards available).
    Failed { reason: String },
    /// Paused by circuit breaker.
    CircuitBroken,
}

/// A reconstruction job in the priority queue.
pub struct ReconstructionJob {
    pub group: ShardGroupId,
    pub priority: ReconstructionPriority,
    pub status: ReconstructionStatus,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum ErasureError {
    /// Not enough shards to reconstruct (need k, have fewer).
    InsufficientShards { need: u32, have: u32 },
    /// Shard data is corrupted (checksum mismatch).
    CorruptShard { group: ShardGroupId, index: u32 },
    /// Encoding parameters are invalid (k=0, m=0, k > max, etc.).
    InvalidParams { reason: String },
    /// No nodes available to distribute shards to.
    NoAvailableNodes,
    /// Reconstruction circuit breaker tripped (queue depth exceeded threshold).
    CircuitBreakerTripped { queue_depth: usize, threshold: usize },
    /// Network or storage error during shard transfer.
    TransferError { node: NodeId, reason: String },
    /// Re-coding failed (insufficient healthy shards for new parameters).
    RecodingFailed { reason: String },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Core erasure coding: encode data into shards and decode from shards.
///
/// This is the pure coding layer — no I/O, no distribution logic. It
/// operates on byte slices and produces/consumes `Shard` values.
pub trait ErasureCoder {
    /// Encode data into n shards (k data + m parity).
    ///
    /// The original data can be reconstructed from any k of the n shards.
    /// Each shard includes its index for reconstruction ordering.
    ///
    /// Returns `ErasureError::InvalidParams` if the parameters are
    /// out of range.
    fn encode(&self, data: &[u8], params: ErasureParams) -> Result<Vec<Shard>, ErasureError>;

    /// Reconstruct original data from at least k shards.
    ///
    /// Shards can arrive in any order — index is used for positioning.
    /// Returns `ErasureError::InsufficientShards` if fewer than k shards
    /// are provided. Returns `ErasureError::CorruptShard` if any shard
    /// fails integrity checks.
    fn decode(&self, shards: &[Shard], params: ErasureParams) -> Result<Vec<u8>, ErasureError>;

    /// Compute the erasure parameters for a given fleet size and resilience
    /// percentage.
    ///
    /// k = ceil(N * (1 - R/100)) where R is the resilience percentage and
    /// N is the number of active nodes. m = N - k.
    ///
    /// Returns `ErasureError::InvalidParams` if N is too small or R is out
    /// of range (0-100).
    fn compute_params(&self, node_count: u32, resilience_pct: u32) -> Result<ErasureParams, ErasureError>;
}

/// Manages shard distribution across nodes and re-coding when fleet changes.
///
/// This trait handles the distributed aspects: deciding which node holds
/// which shard, triggering re-coding when nodes join/leave, and fetching
/// shards from remote nodes for reconstruction.
///
/// Governance units are actively replicated (full copies on N nodes), not
/// just erasure-coded (INV-R6).
pub trait ShardManager {
    /// Distribute shards for a newly encoded shard group across available nodes.
    ///
    /// Selects target nodes to maximize fault isolation (different racks,
    /// availability zones where known). Governance unit shards are fully
    /// replicated in addition to erasure coding (INV-R6).
    ///
    /// Returns the shard assignments. Returns `ErasureError::NoAvailableNodes`
    /// if there are fewer available nodes than shards.
    async fn distribute(
        &self,
        group: ShardGroupId,
        shards: Vec<Shard>,
    ) -> Result<Vec<ShardAssignment>, ErasureError>;

    /// Fetch shards from remote nodes for reconstruction.
    ///
    /// Contacts nodes holding shards for the given group and retrieves at
    /// least k shards. Applies backpressure: respects the rate limits set
    /// by `ReconstructionScheduler`.
    ///
    /// Returns `ErasureError::InsufficientShards` if too many nodes are
    /// unreachable. Returns `ErasureError::TransferError` on network failure.
    async fn fetch_shards(
        &self,
        group: &ShardGroupId,
        min_shards: u32,
    ) -> Result<Vec<Shard>, ErasureError>;

    /// Re-code a shard group with new erasure parameters.
    ///
    /// Triggered when fleet size changes significantly (nodes join/leave).
    /// Fetches existing shards, decodes, re-encodes with new parameters,
    /// and distributes the new shards.
    ///
    /// Returns `ErasureError::RecodingFailed` if reconstruction of the
    /// original data fails.
    async fn recode(
        &self,
        group: &ShardGroupId,
        new_params: ErasureParams,
    ) -> Result<Vec<ShardAssignment>, ErasureError>;

    /// Get current shard assignments for a group.
    async fn assignments(
        &self,
        group: &ShardGroupId,
    ) -> Result<Vec<ShardAssignment>, ErasureError>;
}

/// Priority queue for reconstruction jobs with backpressure (INV-R1).
///
/// When nodes fail, their shards must be reconstructed on surviving nodes.
/// This can cause a reconstruction storm (FM-13). The scheduler throttles
/// reconstruction rate and prioritizes by shard criticality:
/// governance > policy > data constraints > workload.
pub trait ReconstructionScheduler {
    /// Enqueue a reconstruction job with the given priority.
    ///
    /// If the circuit breaker is tripped (queue depth exceeds threshold),
    /// returns `ErasureError::CircuitBreakerTripped` and the job is NOT
    /// enqueued. The operator must be alerted.
    async fn enqueue(
        &self,
        group: ShardGroupId,
        priority: ReconstructionPriority,
    ) -> Result<(), ErasureError>;

    /// Dequeue the next highest-priority reconstruction job.
    ///
    /// Returns `None` if the queue is empty. Jobs are dequeued in priority
    /// order (governance first), then FIFO within the same priority.
    async fn dequeue(&self) -> Option<ReconstructionJob>;

    /// Get the current status of a reconstruction job.
    async fn status(&self, group: &ShardGroupId) -> Option<ReconstructionStatus>;

    /// Get current queue depth (total pending jobs).
    fn queue_depth(&self) -> usize;

    /// Check whether the circuit breaker is tripped.
    ///
    /// The circuit breaker trips when queue depth exceeds the configured
    /// threshold. While tripped, no new jobs are accepted and an operator
    /// alert should be raised.
    fn is_circuit_broken(&self) -> bool;

    /// Reset the circuit breaker (operator action).
    ///
    /// Allows new reconstruction jobs to be enqueued. Should only be called
    /// after the underlying cause (e.g., cascading failures) is resolved.
    async fn reset_circuit_breaker(&self) -> Result<(), ErasureError>;

    /// Set the maximum reconstruction rate (jobs per second).
    ///
    /// Used to prevent reconstruction I/O from overwhelming surviving nodes.
    fn set_rate_limit(&self, jobs_per_second: u32);
}
