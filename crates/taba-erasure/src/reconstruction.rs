//! Reconstruction request types, priority scheduling, and
//! backpressure (INV-R1, FM-13).
//!
//! When nodes fail, their shards must be reconstructed on surviving
//! nodes. The [`ReconstructionScheduler`] throttles reconstruction
//! rate and prioritizes by shard criticality:
//! governance > policy > data constraints > workload.
//!
//! A circuit breaker prevents reconstruction storms (FM-13): when the
//! queue depth exceeds a configured threshold, no new jobs are
//! accepted and an operator alert should be raised.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use taba_common::{DualClockEvent, LogicalClock, NodeId, ShardId, WallTime};

use crate::error::{ErasureError, ShardGroupId};
use crate::shard::ShardCriticality;

// ---------------------------------------------------------------------------
// ReconstructionReason
// ---------------------------------------------------------------------------

/// Why a shard needs reconstruction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReconstructionReason {
    /// The node holding this shard has failed.
    NodeFailure {
        /// The node that failed.
        failed_node: NodeId,
    },
    /// The node holding this shard is draining.
    NodeDraining {
        /// The node that is draining.
        draining_node: NodeId,
    },
    /// Shard data was found to be corrupted (post-reconstruction
    /// re-verification).
    Corruption,
    /// Rebalancing after a new node joined.
    Rebalance,
}

// ---------------------------------------------------------------------------
// ReconstructionPriority
// ---------------------------------------------------------------------------

/// Priority level for shard reconstruction.
///
/// Derived from [`ShardCriticality`] plus urgency factors.
/// The derived `Ord` orders first by criticality (ascending:
/// Workload < `DataConstraints` < Policy < Governance) then by
/// `remaining_parity` (ascending). The [`DefaultReconstructionScheduler`]
/// uses a custom ordering that prioritizes higher criticality and
/// lower `remaining_parity` (more urgent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReconstructionPriority {
    /// Base criticality from shard content type.
    pub criticality: ShardCriticality,
    /// Urgency: how many parity shards remain for this data.
    /// Lower = more urgent (closer to data loss threshold).
    pub remaining_parity: u32,
}

impl ReconstructionPriority {
    /// Creates a new `ReconstructionPriority` from the given
    /// criticality and remaining parity count.
    #[must_use]
    pub const fn new(criticality: ShardCriticality, remaining_parity: u32) -> Self {
        Self {
            criticality,
            remaining_parity,
        }
    }
}

// ---------------------------------------------------------------------------
// ReconstructionRequest
// ---------------------------------------------------------------------------

/// A request to reconstruct a lost shard from surviving shards.
///
/// Queued and processed with backpressure (INV-R1). The
/// [`ReconstructionScheduler`] manages the lifecycle of
/// reconstruction requests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// When this request was created (A002: `DualClockEvent`).
    pub requested_at: DualClockEvent,
}

// ---------------------------------------------------------------------------
// BackpressureState
// ---------------------------------------------------------------------------

/// State of the reconstruction backpressure mechanism (INV-R1, FM-13).
///
/// Prevents cascading failures when multiple nodes fail in rapid
/// succession. The circuit breaker trips when `queue_depth` exceeds
/// `circuit_breaker_threshold`, at which point no new reconstruction
/// jobs are accepted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// When the circuit breaker last changed state (A002: `DualClockEvent`).
    pub last_state_change: DualClockEvent,
}

// ---------------------------------------------------------------------------
// ReconstructionStatus
// ---------------------------------------------------------------------------

/// Status of a reconstruction job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReconstructionStatus {
    /// Queued, waiting for processing.
    Queued {
        /// Position in the queue (0-based, lower = earlier).
        position: usize,
    },
    /// Currently being reconstructed.
    InProgress {
        /// Number of shards received so far.
        shards_received: u32,
        /// Total shards needed for reconstruction.
        shards_needed: u32,
    },
    /// Successfully reconstructed.
    Complete,
    /// Failed (insufficient shards available).
    Failed {
        /// Human-readable reason for the failure.
        reason: String,
    },
    /// Paused by circuit breaker.
    CircuitBroken,
}

// ---------------------------------------------------------------------------
// ReconstructionJob
// ---------------------------------------------------------------------------

/// A reconstruction job in the priority queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconstructionJob {
    /// The shard group to reconstruct.
    pub group: ShardGroupId,
    /// Priority of this job.
    pub priority: ReconstructionPriority,
    /// Current status of this job.
    pub status: ReconstructionStatus,
}

// ---------------------------------------------------------------------------
// Internal: PriorityQueueEntry
// ---------------------------------------------------------------------------

/// Internal priority queue entry with a sequence number for FIFO
/// tiebreaking.
///
/// The custom `Ord` implements:
/// 1. Higher criticality = greater (popped first in max-heap)
/// 2. Lower `remaining_parity` = greater (more urgent)
/// 3. Lower sequence = greater (FIFO within same priority)
struct PriorityQueueEntry {
    group: ShardGroupId,
    priority: ReconstructionPriority,
    sequence: u64,
}

impl PartialEq for PriorityQueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}

impl Eq for PriorityQueueEntry {}

impl PartialOrd for PriorityQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityQueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // 1. Higher criticality = greater (popped first in max-heap)
        let crit_cmp = self.priority.criticality.cmp(&other.priority.criticality);
        if crit_cmp != Ordering::Equal {
            return crit_cmp;
        }
        // 2. Lower remaining_parity = greater (more urgent)
        let parity_cmp = other
            .priority
            .remaining_parity
            .cmp(&self.priority.remaining_parity);
        if parity_cmp != Ordering::Equal {
            return parity_cmp;
        }
        // 3. Lower sequence = greater (FIFO)
        other.sequence.cmp(&self.sequence)
    }
}

// ---------------------------------------------------------------------------
// Helper: minimal DualClockEvent
// ---------------------------------------------------------------------------

/// Creates a minimal `DualClockEvent` for internal use.
///
/// Production callers should overwrite timestamps with actual event
/// times.
fn minimal_dual_clock() -> DualClockEvent {
    DualClockEvent {
        logical_clock: LogicalClock(0),
        wall_time: WallTime { millis: 0 },
        timezone: "UTC".to_string(),
    }
}

// ---------------------------------------------------------------------------
// ReconstructionScheduler trait
// ---------------------------------------------------------------------------

/// Priority queue for reconstruction jobs with backpressure (INV-R1).
///
/// When nodes fail, their shards must be reconstructed on surviving
/// nodes. This can cause a reconstruction storm (FM-13). The
/// scheduler throttles reconstruction rate and prioritizes by shard
/// criticality: governance > policy > data constraints > workload.
///
/// Within the same priority, jobs are dequeued FIFO.
pub trait ReconstructionScheduler: Send + Sync {
    /// Enqueue a reconstruction job with the given priority.
    ///
    /// If the circuit breaker is tripped (queue depth exceeds
    /// threshold), returns [`ErasureError::CircuitBreakerTripped`]
    /// and the job is NOT enqueued. The operator must be alerted.
    ///
    /// # Errors
    ///
    /// - [`ErasureError::CircuitBreakerTripped`] if the circuit
    ///   breaker is open or the queue depth exceeds the threshold.
    async fn enqueue(
        &self,
        group: ShardGroupId,
        priority: ReconstructionPriority,
    ) -> Result<(), ErasureError>;

    /// Dequeue the next highest-priority reconstruction job.
    ///
    /// Returns `None` if the queue is empty. Jobs are dequeued in
    /// priority order (governance first), then FIFO within the same
    /// priority.
    async fn dequeue(&self) -> Option<ReconstructionJob>;

    /// Get the current status of a reconstruction job.
    ///
    /// Returns `None` if the group has no known reconstruction job.
    async fn status(&self, group: &ShardGroupId) -> Option<ReconstructionStatus>;

    /// Get current queue depth (total pending jobs).
    fn queue_depth(&self) -> usize;

    /// Check whether the circuit breaker is tripped.
    ///
    /// The circuit breaker trips when queue depth exceeds the
    /// configured threshold. While tripped, no new jobs are accepted
    /// and an operator alert should be raised.
    fn is_circuit_broken(&self) -> bool;

    /// Reset the circuit breaker (operator action).
    ///
    /// Allows new reconstruction jobs to be enqueued. Should only be
    /// called after the underlying cause (e.g., cascading failures)
    /// is resolved.
    ///
    /// # Errors
    ///
    /// This method is infallible for the default implementation but
    /// returns `Result` for future extensibility.
    async fn reset_circuit_breaker(&self) -> Result<(), ErasureError>;

    /// Set the maximum reconstruction rate (jobs per second).
    ///
    /// Used to prevent reconstruction I/O from overwhelming
    /// surviving nodes.
    fn set_rate_limit(&self, jobs_per_second: u32);
}

// ---------------------------------------------------------------------------
// DefaultReconstructionScheduler
// ---------------------------------------------------------------------------

/// Default in-memory implementation of [`ReconstructionScheduler`].
///
/// Uses a [`BinaryHeap`] with a custom ordering that prioritizes
/// governance > policy > data constraints > workload (INV-R1), then
/// by urgency (lower `remaining_parity` = more urgent), then FIFO
/// within the same priority.
///
/// The circuit breaker trips when the queue depth exceeds the
/// configured threshold (FM-13). Once tripped, no new jobs are
/// accepted until [`reset_circuit_breaker`](ReconstructionScheduler::reset_circuit_breaker)
/// is called.
pub struct DefaultReconstructionScheduler {
    /// Priority queue of pending reconstruction jobs.
    queue: Mutex<BinaryHeap<PriorityQueueEntry>>,
    /// Status of each group's reconstruction job.
    statuses: Mutex<HashMap<ShardGroupId, ReconstructionStatus>>,
    /// Next sequence number (for FIFO tiebreaking).
    next_seq: Mutex<u64>,
    /// Circuit breaker threshold: queue depth at which the breaker trips.
    circuit_breaker_threshold: Mutex<u32>,
    /// Whether the circuit breaker is currently open (tripped).
    circuit_breaker_open: Mutex<bool>,
    /// Throttle rate: reconstructions permitted per second.
    throttle_rate: Mutex<u32>,
    /// Number of reconstructions currently in progress.
    in_progress: Mutex<u32>,
    /// Number of reconstructions completed since last reset.
    completed: Mutex<u64>,
    /// Number of reconstructions that failed.
    failed: Mutex<u64>,
    /// When the circuit breaker last changed state.
    last_state_change: Mutex<DualClockEvent>,
}

impl DefaultReconstructionScheduler {
    /// Creates a new scheduler with the given circuit breaker
    /// threshold and default throttle rate of 10 jobs/second.
    ///
    /// # Panics
    ///
    /// This constructor never panics — mutexes are created fresh.
    #[must_use]
    pub fn new(circuit_breaker_threshold: u32) -> Self {
        Self {
            queue: Mutex::new(BinaryHeap::new()),
            statuses: Mutex::new(HashMap::new()),
            next_seq: Mutex::new(0),
            circuit_breaker_threshold: Mutex::new(circuit_breaker_threshold),
            circuit_breaker_open: Mutex::new(false),
            throttle_rate: Mutex::new(10),
            in_progress: Mutex::new(0),
            completed: Mutex::new(0),
            failed: Mutex::new(0),
            last_state_change: Mutex::new(minimal_dual_clock()),
        }
    }

    /// Returns the current backpressure state (INV-R1, FM-13).
    #[must_use]
    pub fn backpressure_state(&self) -> BackpressureState {
        let queue_depth = u32::try_from(self.queue_depth()).unwrap_or(u32::MAX);
        let threshold = *self
            .circuit_breaker_threshold
            .lock()
            .expect("circuit_breaker_threshold mutex should not be poisoned");
        let circuit_breaker_open = *self
            .circuit_breaker_open
            .lock()
            .expect("circuit_breaker_open mutex should not be poisoned");
        let throttle_rate = *self
            .throttle_rate
            .lock()
            .expect("throttle_rate mutex should not be poisoned");
        let in_progress = *self
            .in_progress
            .lock()
            .expect("in_progress mutex should not be poisoned");
        let completed = *self
            .completed
            .lock()
            .expect("completed mutex should not be poisoned");
        let failed = *self
            .failed
            .lock()
            .expect("failed mutex should not be poisoned");
        let last_state_change = self
            .last_state_change
            .lock()
            .expect("last_state_change mutex should not be poisoned")
            .clone();

        BackpressureState {
            queue_depth,
            circuit_breaker_threshold: threshold,
            circuit_breaker_open,
            throttle_rate,
            in_progress,
            completed,
            failed,
            last_state_change,
        }
    }

    /// Marks a reconstruction job as complete.
    ///
    /// Increments the completed counter and decrements the
    /// in-progress counter.
    pub fn mark_complete(&self, group: &ShardGroupId) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.insert(*group, ReconstructionStatus::Complete);
        }
        if let Ok(mut completed) = self.completed.lock() {
            *completed += 1;
        }
        if let Ok(mut in_progress) = self.in_progress.lock() {
            if *in_progress > 0 {
                *in_progress -= 1;
            }
        }
    }

    /// Marks a reconstruction job as failed with the given reason.
    ///
    /// Increments the failed counter and decrements the
    /// in-progress counter.
    pub fn mark_failed(&self, group: &ShardGroupId, reason: &str) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.insert(
                *group,
                ReconstructionStatus::Failed {
                    reason: reason.to_string(),
                },
            );
        }
        if let Ok(mut failed) = self.failed.lock() {
            *failed += 1;
        }
        if let Ok(mut in_progress) = self.in_progress.lock() {
            if *in_progress > 0 {
                *in_progress -= 1;
            }
        }
    }
}

impl ReconstructionScheduler for DefaultReconstructionScheduler {
    async fn enqueue(
        &self,
        group: ShardGroupId,
        priority: ReconstructionPriority,
    ) -> Result<(), ErasureError> {
        let threshold = *self
            .circuit_breaker_threshold
            .lock()
            .expect("circuit_breaker_threshold mutex should not be poisoned");

        // Check and trip the circuit breaker if needed.
        {
            let queue_depth = self.queue_depth();
            let threshold_usize = usize::try_from(threshold).unwrap_or(usize::MAX);
            let mut circuit_open = self
                .circuit_breaker_open
                .lock()
                .expect("circuit_breaker_open mutex should not be poisoned");

            if *circuit_open || queue_depth > threshold_usize {
                // Trip the breaker if not already open.
                if !*circuit_open {
                    *circuit_open = true;
                    if let Ok(mut last_change) = self.last_state_change.lock() {
                        *last_change = minimal_dual_clock();
                    }
                }
                return Err(ErasureError::CircuitBreakerTripped {
                    queue_depth,
                    threshold: threshold_usize,
                });
            }
        }

        // Get the next sequence number.
        let seq = {
            let mut next = self
                .next_seq
                .lock()
                .expect("next_seq mutex should not be poisoned");
            let s = *next;
            *next += 1;
            s
        };

        // Enqueue the job.
        let position = {
            let mut queue = self
                .queue
                .lock()
                .expect("queue mutex should not be poisoned");
            queue.push(PriorityQueueEntry {
                group,
                priority,
                sequence: seq,
            });
            queue.len() - 1
        };

        // Set status to Queued.
        {
            let mut statuses = self
                .statuses
                .lock()
                .expect("statuses mutex should not be poisoned");
            statuses.insert(group, ReconstructionStatus::Queued { position });
        }

        Ok(())
    }

    async fn dequeue(&self) -> Option<ReconstructionJob> {
        let entry = {
            let mut queue = self
                .queue
                .lock()
                .expect("queue mutex should not be poisoned");
            queue.pop()
        }?;

        // Set status to InProgress.
        {
            let mut statuses = self
                .statuses
                .lock()
                .expect("statuses mutex should not be poisoned");
            statuses.insert(
                entry.group,
                ReconstructionStatus::InProgress {
                    shards_received: 0,
                    shards_needed: 0,
                },
            );
        }

        // Increment in-progress.
        {
            let mut in_progress = self
                .in_progress
                .lock()
                .expect("in_progress mutex should not be poisoned");
            *in_progress += 1;
        }

        Some(ReconstructionJob {
            group: entry.group,
            priority: entry.priority,
            status: ReconstructionStatus::InProgress {
                shards_received: 0,
                shards_needed: 0,
            },
        })
    }

    async fn status(&self, group: &ShardGroupId) -> Option<ReconstructionStatus> {
        let statuses = self
            .statuses
            .lock()
            .expect("statuses mutex should not be poisoned");
        statuses.get(group).cloned()
    }

    fn queue_depth(&self) -> usize {
        self.queue
            .lock()
            .expect("queue mutex should not be poisoned")
            .len()
    }

    fn is_circuit_broken(&self) -> bool {
        *self
            .circuit_breaker_open
            .lock()
            .expect("circuit_breaker_open mutex should not be poisoned")
    }

    async fn reset_circuit_breaker(&self) -> Result<(), ErasureError> {
        let mut circuit_open = self
            .circuit_breaker_open
            .lock()
            .expect("circuit_breaker_open mutex should not be poisoned");
        *circuit_open = false;
        drop(circuit_open);

        if let Ok(mut last_change) = self.last_state_change.lock() {
            *last_change = minimal_dual_clock();
        }

        Ok(())
    }

    fn set_rate_limit(&self, jobs_per_second: u32) {
        let mut rate = self
            .throttle_rate
            .lock()
            .expect("throttle_rate mutex should not be poisoned");
        *rate = jobs_per_second;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn gov_priority(remaining: u32) -> ReconstructionPriority {
        ReconstructionPriority::new(ShardCriticality::Governance, remaining)
    }

    fn policy_priority(remaining: u32) -> ReconstructionPriority {
        ReconstructionPriority::new(ShardCriticality::Policy, remaining)
    }

    fn data_priority(remaining: u32) -> ReconstructionPriority {
        ReconstructionPriority::new(ShardCriticality::DataConstraints, remaining)
    }

    fn workload_priority(remaining: u32) -> ReconstructionPriority {
        ReconstructionPriority::new(ShardCriticality::Workload, remaining)
    }

    fn group() -> ShardGroupId {
        ShardGroupId::new()
    }

    #[tokio::test]
    async fn test_enqueue_dequeue_priority() {
        // Enqueue jobs in mixed priority order; verify governance
        // is dequeued before policy before data before workload.
        let scheduler = DefaultReconstructionScheduler::new(1000);

        let g_workload = group();
        let g_data = group();
        let g_policy = group();
        let g_gov = group();

        // Enqueue in reverse priority order.
        scheduler
            .enqueue(g_workload, workload_priority(3))
            .await
            .unwrap();
        scheduler.enqueue(g_data, data_priority(3)).await.unwrap();
        scheduler
            .enqueue(g_policy, policy_priority(3))
            .await
            .unwrap();
        scheduler.enqueue(g_gov, gov_priority(3)).await.unwrap();

        // Dequeue in priority order.
        let first = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(first.group, g_gov, "governance should be dequeued first");

        let second = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(second.group, g_policy, "policy should be dequeued second");

        let third = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(
            third.group, g_data,
            "data constraints should be dequeued third"
        );

        let fourth = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(fourth.group, g_workload, "workload should be dequeued last");

        // Queue should now be empty.
        assert!(scheduler.dequeue().await.is_none());
    }

    #[tokio::test]
    async fn test_enqueue_dequeue_priority_within_criticality() {
        // Within the same criticality, lower remaining_parity = higher
        // priority (more urgent).
        let scheduler = DefaultReconstructionScheduler::new(1000);

        let g_low_urgency = group(); // remaining_parity = 5 (less urgent)
        let g_high_urgency = group(); // remaining_parity = 1 (more urgent)

        scheduler
            .enqueue(g_low_urgency, policy_priority(5))
            .await
            .unwrap();
        scheduler
            .enqueue(g_high_urgency, policy_priority(1))
            .await
            .unwrap();

        let first = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(
            first.group, g_high_urgency,
            "lower remaining_parity (more urgent) should be dequeued first"
        );

        let second = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(second.group, g_low_urgency);
    }

    #[tokio::test]
    async fn test_enqueue_dequeue_fifo_within_same_priority() {
        // Within the exact same priority, jobs are dequeued FIFO.
        let scheduler = DefaultReconstructionScheduler::new(1000);

        let g_first = group();
        let g_second = group();

        scheduler
            .enqueue(g_first, policy_priority(3))
            .await
            .unwrap();
        scheduler
            .enqueue(g_second, policy_priority(3))
            .await
            .unwrap();

        let first = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(
            first.group, g_first,
            "first enqueued should be first dequeued (FIFO)"
        );

        let second = scheduler.dequeue().await.expect("should not be empty");
        assert_eq!(second.group, g_second);
    }

    #[tokio::test]
    async fn test_enqueue_circuit_breaker() {
        // Threshold = 3. Enqueue 3 jobs (OK). The 4th should trip
        // the breaker.
        let scheduler = DefaultReconstructionScheduler::new(3);

        for _ in 0..3 {
            scheduler
                .enqueue(group(), workload_priority(1))
                .await
                .unwrap();
        }
        assert_eq!(scheduler.queue_depth(), 3);

        // 4th enqueue: queue_depth (3) is not > threshold (3), so this
        // should still succeed. Wait, the check is queue_depth > threshold.
        // 3 > 3 is false, so it should succeed.
        scheduler
            .enqueue(group(), workload_priority(1))
            .await
            .unwrap();
        assert_eq!(scheduler.queue_depth(), 4);

        // Now queue_depth (4) > threshold (3). The next enqueue should fail.
        let result = scheduler.enqueue(group(), workload_priority(1)).await;
        assert!(matches!(
            result,
            Err(ErasureError::CircuitBreakerTripped {
                queue_depth: 4,
                threshold: 3
            })
        ));

        // Subsequent enqueues should also fail (breaker is open).
        let result = scheduler.enqueue(group(), workload_priority(1)).await;
        assert!(matches!(
            result,
            Err(ErasureError::CircuitBreakerTripped {
                queue_depth: 4,
                threshold: 3
            })
        ));
    }

    #[tokio::test]
    async fn test_dequeue_empty() {
        let scheduler = DefaultReconstructionScheduler::new(100);
        assert!(scheduler.dequeue().await.is_none());
    }

    #[tokio::test]
    async fn test_status_queued() {
        let scheduler = DefaultReconstructionScheduler::new(100);
        let g = group();

        scheduler.enqueue(g, workload_priority(2)).await.unwrap();

        let status = scheduler.status(&g).await.expect("should have status");
        assert!(matches!(
            status,
            ReconstructionStatus::Queued { position: 0 }
        ));
    }

    #[tokio::test]
    async fn test_status_in_progress() {
        let scheduler = DefaultReconstructionScheduler::new(100);
        let g = group();

        scheduler.enqueue(g, workload_priority(2)).await.unwrap();
        let _job = scheduler.dequeue().await.expect("should dequeue");

        let status = scheduler.status(&g).await.expect("should have status");
        assert!(matches!(
            status,
            ReconstructionStatus::InProgress {
                shards_received: 0,
                shards_needed: 0
            }
        ));
    }

    #[tokio::test]
    async fn test_status_complete() {
        let scheduler = DefaultReconstructionScheduler::new(100);
        let g = group();

        scheduler.enqueue(g, workload_priority(2)).await.unwrap();
        let _job = scheduler.dequeue().await.expect("should dequeue");
        scheduler.mark_complete(&g);

        let status = scheduler.status(&g).await.expect("should have status");
        assert_eq!(status, ReconstructionStatus::Complete);
    }

    #[tokio::test]
    async fn test_status_failed() {
        let scheduler = DefaultReconstructionScheduler::new(100);
        let g = group();

        scheduler.enqueue(g, workload_priority(2)).await.unwrap();
        let _job = scheduler.dequeue().await.expect("should dequeue");
        scheduler.mark_failed(&g, "insufficient shards");

        let status = scheduler.status(&g).await.expect("should have status");
        assert!(matches!(
            status,
            ReconstructionStatus::Failed { ref reason } if reason == "insufficient shards"
        ));
    }

    #[tokio::test]
    async fn test_status_nonexistent() {
        let scheduler = DefaultReconstructionScheduler::new(100);
        let g = group();

        assert!(scheduler.status(&g).await.is_none());
    }

    #[tokio::test]
    async fn test_is_circuit_broken() {
        // Threshold = 2. Enqueue 2 jobs (OK). The 3rd: depth=2, 2 > 2 is false → OK.
        // 4th: depth=3, 3 > 2 → trips.
        let scheduler = DefaultReconstructionScheduler::new(2);

        assert!(
            !scheduler.is_circuit_broken(),
            "breaker should start closed"
        );

        // Enqueue 2 jobs (OK), then 3rd (OK, depth=2 not > 2), then 4th trips (3 > 2).
        for _ in 0..3 {
            scheduler
                .enqueue(group(), workload_priority(1))
                .await
                .unwrap();
        }
        let result = scheduler.enqueue(group(), workload_priority(1)).await;
        assert!(matches!(
            result,
            Err(ErasureError::CircuitBreakerTripped { .. })
        ));

        assert!(
            scheduler.is_circuit_broken(),
            "breaker should be open after tripping"
        );
    }

    #[tokio::test]
    async fn test_reset_circuit_breaker() {
        let scheduler = DefaultReconstructionScheduler::new(2);

        // Trip the breaker.
        for _ in 0..3 {
            scheduler
                .enqueue(group(), workload_priority(1))
                .await
                .unwrap();
        }
        let result = scheduler.enqueue(group(), workload_priority(1)).await;
        assert!(matches!(
            result,
            Err(ErasureError::CircuitBreakerTripped { .. })
        ));

        // Reset the breaker.
        scheduler.reset_circuit_breaker().await.unwrap();
        assert!(
            !scheduler.is_circuit_broken(),
            "breaker should be closed after reset"
        );

        // But the queue still has jobs. If we dequeue some, enqueue
        // should work again (queue_depth < threshold).
        scheduler.dequeue().await.unwrap();
        scheduler.dequeue().await.unwrap();
        assert_eq!(scheduler.queue_depth(), 1);

        scheduler
            .enqueue(group(), workload_priority(1))
            .await
            .expect("enqueue should succeed after reset and partial drain");
    }

    #[tokio::test]
    async fn test_set_rate_limit() {
        let scheduler = DefaultReconstructionScheduler::new(100);

        let state = scheduler.backpressure_state();
        assert_eq!(
            state.throttle_rate, 10,
            "default throttle rate should be 10"
        );

        scheduler.set_rate_limit(50);

        let state = scheduler.backpressure_state();
        assert_eq!(state.throttle_rate, 50);
    }

    #[tokio::test]
    async fn test_backpressure_state_serialization_roundtrip() {
        let state = BackpressureState {
            queue_depth: 5,
            circuit_breaker_threshold: 10,
            circuit_breaker_open: false,
            throttle_rate: 20,
            in_progress: 2,
            completed: 100,
            failed: 3,
            last_state_change: minimal_dual_clock(),
        };

        let json = serde_json::to_string(&state).expect("serialize BackpressureState");
        let decoded: BackpressureState =
            serde_json::from_str(&json).expect("deserialize BackpressureState");
        assert_eq!(state, decoded);
    }

    #[tokio::test]
    async fn test_backpressure_state_reflects_counters() {
        let scheduler = DefaultReconstructionScheduler::new(100);

        // Enqueue and dequeue some jobs.
        let g1 = group();
        let g2 = group();
        scheduler.enqueue(g1, workload_priority(2)).await.unwrap();
        scheduler.enqueue(g2, workload_priority(2)).await.unwrap();

        let _job1 = scheduler.dequeue().await.unwrap();
        scheduler.mark_complete(&g1);

        let _job2 = scheduler.dequeue().await.unwrap();
        scheduler.mark_failed(&g2, "timeout");

        let state = scheduler.backpressure_state();
        assert_eq!(state.queue_depth, 0, "all jobs dequeued");
        assert_eq!(state.in_progress, 0, "all jobs resolved");
        assert_eq!(state.completed, 1);
        assert_eq!(state.failed, 1);
        assert!(!state.circuit_breaker_open);
    }

    #[tokio::test]
    async fn test_reconstruction_priority_ordering() {
        // Verify the derived Ord on ReconstructionPriority.
        use ShardCriticality::*;

        let workload = ReconstructionPriority::new(Workload, 5);
        let data = ReconstructionPriority::new(DataConstraints, 5);
        let policy = ReconstructionPriority::new(Policy, 5);
        let gov = ReconstructionPriority::new(Governance, 5);

        assert!(gov > policy);
        assert!(policy > data);
        assert!(data > workload);

        // Same criticality, different remaining_parity.
        let low_parity = ReconstructionPriority::new(Policy, 1);
        let high_parity = ReconstructionPriority::new(Policy, 10);
        assert!(
            low_parity < high_parity,
            "lower remaining_parity is less in natural Ord"
        );
    }

    #[tokio::test]
    async fn test_reconstruction_request_serialization_roundtrip() {
        let request = ReconstructionRequest {
            shard_id: ShardId(Uuid::new_v4()),
            source_nodes: vec![NodeId(Uuid::new_v4()), NodeId(Uuid::new_v4())],
            target_node: NodeId(Uuid::new_v4()),
            reason: ReconstructionReason::NodeFailure {
                failed_node: NodeId(Uuid::new_v4()),
            },
            priority: ReconstructionPriority::new(ShardCriticality::Governance, 2),
            requested_at: minimal_dual_clock(),
        };

        let json = serde_json::to_string(&request).expect("serialize ReconstructionRequest");
        let decoded: ReconstructionRequest =
            serde_json::from_str(&json).expect("deserialize ReconstructionRequest");
        assert_eq!(request, decoded);
    }

    #[tokio::test]
    async fn test_reconstruction_reason_all_variants() {
        let reasons = vec![
            ReconstructionReason::NodeFailure {
                failed_node: NodeId(Uuid::new_v4()),
            },
            ReconstructionReason::NodeDraining {
                draining_node: NodeId(Uuid::new_v4()),
            },
            ReconstructionReason::Corruption,
            ReconstructionReason::Rebalance,
        ];

        for reason in &reasons {
            let json = serde_json::to_string(reason).expect("serialize reason");
            let _decoded: ReconstructionReason =
                serde_json::from_str(&json).expect("deserialize reason");
        }
    }

    #[tokio::test]
    async fn test_reconstruction_job_serde_roundtrip() {
        let job = ReconstructionJob {
            group: ShardGroupId::new(),
            priority: ReconstructionPriority::new(ShardCriticality::Policy, 3),
            status: ReconstructionStatus::Queued { position: 2 },
        };

        let json = serde_json::to_string(&job).expect("serialize ReconstructionJob");
        let decoded: ReconstructionJob =
            serde_json::from_str(&json).expect("deserialize ReconstructionJob");
        assert_eq!(job, decoded);
    }
}
