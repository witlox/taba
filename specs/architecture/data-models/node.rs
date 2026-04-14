//! Node types: local state, WAL, reconciliation, health, and operational modes.
//!
//! Every node in taba is a peer -- no distinction between control plane and
//! worker. Each node holds graph shards, runs the solver locally, reconciles
//! local actual state against desired state, and participates in gossip.
//!
//! Node states: Joining -> Attesting -> Active -> Suspected -> Draining -> Left | Failed.
//! Operational modes: Normal | Degraded | Recovery (system-wide).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::common::{NodeId, ShardId, Timestamp, UnitId};
use crate::security::PublicKey;

// ---------------------------------------------------------------------------
// Node state machine
// ---------------------------------------------------------------------------

/// Lifecycle states of a node in the cluster.
/// Transition: Joining -> Attesting -> Active -> Suspected -> Draining -> Left | Failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NodeState {
    /// Node is bootstrapping -- discovering peers via seed nodes.
    Joining,
    /// Node is proving its integrity (TPM attestation when available, A5).
    Attesting,
    /// Node is fully operational -- participating in placement and gossip.
    Active,
    /// Node is suspected of failure by gossip protocol.
    /// Remains in placement pool with health='unknown' (INV-R5).
    /// Solver avoids suspected nodes when alternatives exist.
    Suspected,
    /// Node is gracefully leaving the cluster -- draining workloads.
    Draining,
    /// Node has cleanly left the cluster.
    Left,
    /// Node has been declared failed by multi-probe consensus (INV-R3).
    Failed,
}

// ---------------------------------------------------------------------------
// Operational modes (system-wide)
// ---------------------------------------------------------------------------

/// System-wide operational mode affecting which operations are permitted.
/// See domain-model.md for transitions and triggers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OperationalMode {
    /// All operations allowed.
    Normal,
    /// Entered when erasure threshold exceeded (INV-R4), memory limit exceeded
    /// (INV-R6), or operator-triggered. Authoring, composition, and placement
    /// frozen. Drain and evacuation only. Operator intervention required.
    Degraded {
        /// What triggered degraded mode.
        trigger: DegradedTrigger,
    },
    /// Gradual re-coding underway after degraded trigger resolved.
    /// Placement throttled. Auto-transitions to Normal when recovery completes.
    Recovery,
}

/// What triggered the system to enter Degraded mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DegradedTrigger {
    /// Erasure threshold exceeded -- too many node failures (INV-R4).
    ErasureThresholdExceeded,
    /// Graph memory limit exceeded on this node (INV-R6).
    MemoryLimitExceeded,
    /// Operator explicitly triggered degraded mode.
    OperatorTriggered,
}

// ---------------------------------------------------------------------------
// Write-ahead log (DL-014)
// ---------------------------------------------------------------------------

/// On-disk WAL frame format: length-prefixed protobuf with CRC32C integrity.
///
/// ```text
/// ┌──────────┬──────────┬───────────────────────┬──────────┐
/// │ len: u32 │ crc: u32 │ WalEntry (protobuf)   │ pad 0-7  │
/// └──────────┴──────────┴───────────────────────┴──────────┘
/// ```
///
/// - `len`: payload length in bytes (little-endian u32)
/// - `crc`: CRC32C of the protobuf payload (corruption detection, FM-07)
/// - Payload: prost-encoded WalEntry
/// - Padding: zero bytes to 8-byte alignment
///
/// **Segment naming**: `wal-{sequence_start:016}.log`
/// Lexicographic sort = temporal order. Default segment size: 64 MB.
///
/// **Compaction**: entries are discardable when:
/// - Merged: unit successfully erasure-coded to cluster (durable beyond this node)
/// - Pending: promoted (refs arrived) or expired (1-hour configurable timeout)
/// - Promoted: immediately after corresponding Merged entry is written
///
/// Compaction = new segment with non-discardable entries, atomic rename, delete old.
///
/// **Decision trails** (INV-O1) use the same frame format but a separate file
/// sequence (`trail-{sequence_start:016}.log`) managed by taba-observe.

/// A single entry in the node-local write-ahead log.
/// Every mutation is WAL'd atomically before effects become visible (INV-C4).
/// WAL survives restarts and is the basis for local state recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    /// Monotonically increasing sequence number local to this node.
    pub sequence: u64,
    /// When this entry was written.
    pub written_at: Timestamp,
    /// The type and payload of this WAL entry.
    pub entry_type: WalEntryType,
}

/// The three WAL entry types (INV-C4).
/// Mutations form a partial (causal) order, not a total order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WalEntryType {
    /// A unit has been verified and merged into the local graph state.
    Merged {
        unit_id: UnitId,
        /// Serialized signed unit (opaque bytes for WAL storage).
        payload: Vec<u8>,
    },
    /// A unit has been verified but references are not yet satisfied.
    /// Held until referenced units arrive (causal buffering).
    Pending {
        unit_id: UnitId,
        /// Serialized signed unit.
        payload: Vec<u8>,
        /// The references that are not yet present.
        missing_refs: Vec<UnitId>,
    },
    /// A previously pending unit has been activated after its references arrived.
    Promoted {
        unit_id: UnitId,
    },
}

/// WAL segment configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalConfig {
    /// Maximum segment size in bytes before rotation (default: 64 MB).
    pub max_segment_bytes: u64,
    /// Timeout for pending entries before they are eligible for discard (default: 1 hour).
    pub pending_expiry: std::time::Duration,
    /// Directory for WAL segment files.
    pub wal_dir: String,
    /// Directory for decision trail segment files (taba-observe).
    pub trail_dir: String,
}

// ---------------------------------------------------------------------------
// Health and reconciliation
// ---------------------------------------------------------------------------

/// Health status of a node, observed by peers (not self-reported).
/// Byzantine resistance: health is determined by peer observation (FM-04).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// The node being observed.
    pub node_id: NodeId,
    /// Current node lifecycle state.
    pub state: NodeState,
    /// Current operational mode.
    pub operational_mode: OperationalMode,
    /// Resource utilization metrics (all in Ppm for determinism).
    pub resources: ResourceUtilization,
    /// When this health status was last updated.
    pub observed_at: Timestamp,
    /// The nodes that observed and corroborated this status.
    pub observers: Vec<NodeId>,
}

/// Resource utilization metrics for a node.
/// All values in Ppm (0 = idle, 1_000_000 = fully utilized).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// CPU utilization in Ppm.
    pub cpu_ppm: crate::common::Ppm,
    /// Memory utilization in Ppm.
    pub memory_ppm: crate::common::Ppm,
    /// Storage utilization in Ppm.
    pub storage_ppm: crate::common::Ppm,
    /// Graph memory usage relative to limit in Ppm (for INV-R6).
    pub graph_memory_ppm: crate::common::Ppm,
}

/// State of the local reconciliation loop.
/// Each node independently converges actual state to desired state.
/// No central reconciliation loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationState {
    /// The node performing reconciliation.
    pub node_id: NodeId,
    /// Units that should be running on this node (desired).
    pub desired_placements: BTreeMap<UnitId, DesiredPlacement>,
    /// Units actually running on this node (actual).
    pub actual_state: BTreeMap<UnitId, ActualState>,
    /// Detected drifts between desired and actual.
    pub drifts: Vec<Drift>,
    /// When reconciliation last ran.
    pub last_reconciled_at: Timestamp,
}

/// A desired placement for this node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesiredPlacement {
    pub unit_id: UnitId,
    /// Expected state of this unit.
    pub expected_state: crate::core::UnitState,
}

/// Actual state of a unit on this node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActualState {
    pub unit_id: UnitId,
    /// The runtime state (running, crashed, etc.).
    pub runtime_state: RuntimeState,
    /// When this state was last observed.
    pub observed_at: Timestamp,
}

/// Runtime state of a unit on a specific node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RuntimeState {
    Starting,
    Running,
    Crashed,
    Stopped,
    Unknown,
}

/// A divergence between desired and actual state on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drift {
    pub unit_id: UnitId,
    /// What was expected.
    pub expected: crate::core::UnitState,
    /// What was actually observed.
    pub actual: RuntimeState,
    /// When the drift was detected.
    pub detected_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Shard assignment
// ---------------------------------------------------------------------------

/// Assignment of erasure-coded graph shards to this node.
/// Shard redistribution happens on membership changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardAssignment {
    /// The node holding these shards.
    pub node_id: NodeId,
    /// Shard IDs assigned to this node.
    pub shards: Vec<ShardId>,
    /// When this assignment was last updated.
    pub assigned_at: Timestamp,
}
