//! Observability types: decision trails, structured events, health aggregation.
//!
//! Structural observability falls out of the composition graph (decision trails,
//! promotion audit, drift detection). Integration observability plugs into
//! external systems (OpenTelemetry, Prometheus, alerting webhooks).

use serde::{Deserialize, Serialize};

use crate::common::{
    DualClockEvent, LogicalClock, NodeId, Ppm, UnitId, WallTime,
};

// ---------------------------------------------------------------------------
// Decision trails (INV-O1, INV-O2)
// ---------------------------------------------------------------------------

/// Queryable record of a solver run's inputs and outputs.
/// Every solver run produces one of these (INV-O1).
/// Enables solver replay: deterministic solver means replay produces
/// the exact same result given the same inputs (INV-C3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionTrail {
    /// Unique identifier for this trail.
    pub trail_id: DecisionTrailId,
    /// Graph snapshot used as input.
    pub graph_snapshot_id: String,
    /// Node membership snapshot used as input.
    pub node_membership: Vec<NodeId>,
    /// Resource snapshots used for ranking.
    pub resource_snapshots: Vec<ResourceSnapshotRef>,
    /// Solver version that produced this decision.
    pub solver_version: String,
    /// Placement decisions produced.
    pub placements: Vec<PlacementRecord>,
    /// Conflicts detected.
    pub conflicts: Vec<ConflictRecord>,
    /// When this solver run occurred.
    pub timestamp: DualClockEvent,
}

/// Unique identifier for a decision trail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DecisionTrailId(pub u64);

/// Reference to a resource snapshot used in a solver run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshotRef {
    pub node_id: NodeId,
    pub snapshot_lc: LogicalClock,
}

/// A placement decision in the trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementRecord {
    pub unit_id: UnitId,
    pub placed_on: NodeId,
    /// Why this node was chosen (capability match + resource rank).
    pub rationale: String,
    /// Capability filter results: which nodes passed, which didn't.
    pub capability_filter: Vec<(NodeId, bool)>,
    /// Resource ranking scores for eligible nodes.
    pub resource_scores: Vec<(NodeId, Ppm)>,
}

/// A conflict detected during the solver run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub conflict_type: String,
    pub involved_units: Vec<UnitId>,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Structured events
// ---------------------------------------------------------------------------

/// Structured event emitted for external consumption.
/// Every significant system action produces one of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredEvent {
    pub timestamp: DualClockEvent,
    pub node_id: NodeId,
    pub event_type: EventType,
    pub detail: String,
}

/// High-level event type classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventType {
    UnitInserted { unit_id: UnitId },
    UnitTerminated { unit_id: UnitId },
    PlacementDecided { unit_id: UnitId, node_id: NodeId },
    ConflictDetected { conflict: String },
    PromotionApplied { unit_id: UnitId, environment: String },
    DriftDetected { unit_id: UnitId, expected: String, actual: String },
    CapabilityChanged { node_id: NodeId, added: Vec<String>, removed: Vec<String> },
    HealthCheckResult { unit_id: UnitId, healthy: bool },
    TaskSpawned { parent: UnitId, child: UnitId },
    TaskTerminated { unit_id: UnitId, reason: String },
    CompactionTriggered { units_compacted: u32 },
    DegradedModeEntered { reason: String },
    RevocationMerged { author_id: String },
}

// ---------------------------------------------------------------------------
// Health aggregation
// ---------------------------------------------------------------------------

/// Aggregated health status for a workload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub unit_id: UnitId,
    pub node_id: NodeId,
    pub healthy: bool,
    pub last_check: DualClockEvent,
    pub consecutive_failures: u32,
    pub check_type: String,
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Alerting
// ---------------------------------------------------------------------------

/// Webhook alert payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertPayload {
    pub node_id: NodeId,
    pub event_type: String,
    pub reason: String,
    pub timestamp: DualClockEvent,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Prometheus metrics (type signatures only)
// ---------------------------------------------------------------------------

/// Metric types exposed on the Prometheus endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    pub memory_available_bytes: u64,
    pub cpu_load_ppm: Ppm,
    pub workloads_running: u32,
    pub solver_runs_total: u64,
    pub gossip_messages_total: u64,
    pub artifact_cache_size_bytes: u64,
    pub decision_trails_stored: u64,
    pub compactions_total: u64,
}
