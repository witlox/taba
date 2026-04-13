// taba-observe: Decision trail recording, solver replay, structured events,
// health aggregation, and integration export.
//
// Cross-cutting observability. Structural observability (decision trails,
// promotion audit) is recorded in the graph. Integration observability
// (Prometheus, OpenTelemetry, webhooks) exports to external systems.

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct UnitId(/* opaque */);
pub struct NodeId(/* opaque */);
pub struct LogicalClock(/* opaque */);
pub struct GraphSnapshot(/* opaque */);
pub struct MembershipSnapshot(/* opaque */);
pub struct SolverResult(/* opaque */);
pub struct DualClockEvent(/* opaque */);

/// Unique ID for a decision trail entry.
pub struct DecisionTrailId(pub u64);

/// A recorded decision trail (inputs + outputs of a solver run).
pub struct DecisionTrail {
    pub trail_id: DecisionTrailId,
    pub graph_snapshot_id: String,
    pub node_membership: Vec<NodeId>,
    pub solver_version: String,
    pub placements: Vec<PlacementRecord>,
    pub conflicts: Vec<ConflictRecord>,
    pub timestamp: DualClockEvent,
}

pub struct PlacementRecord {
    pub unit_id: UnitId,
    pub placed_on: NodeId,
    pub rationale: String,
}

pub struct ConflictRecord {
    pub conflict_type: String,
    pub involved_units: Vec<UnitId>,
    pub detail: String,
}

pub struct HealthStatus {
    pub unit_id: UnitId,
    pub healthy: bool,
    pub detail: Option<String>,
}

pub struct AlertPayload {
    pub event_type: String,
    pub reason: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum ObserveError {
    /// Decision trail not found for the given ID or time range.
    TrailNotFound { trail_id: DecisionTrailId },
    /// Solver replay failed (snapshot unavailable, compacted, etc.).
    ReplayFailed { reason: String },
    /// Export to external system failed (Prometheus, OTEL, webhook).
    ExportFailed { target: String, reason: String },
    /// Retention policy prevents access to compacted trail.
    TrailCompacted { trail_id: DecisionTrailId },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Records decision trails for every solver run (INV-O1).
///
/// Every solver execution produces a decision trail: the inputs (graph
/// snapshot, membership), outputs (placements, conflicts), and solver
/// version. Trails are stored in the graph and queryable.
pub trait DecisionTrailRecorder {
    /// Record a solver run's inputs and outputs as a decision trail.
    ///
    /// Called by the node after each solver invocation. The trail is
    /// persisted to the graph (via WAL) and becomes queryable.
    fn record(
        &self,
        graph_snapshot_id: &str,
        membership: &MembershipSnapshot,
        result: &SolverResult,
        solver_version: &str,
    ) -> Result<DecisionTrailId, ObserveError>;
}

/// Queries decision trails (INV-O1, INV-O2).
pub trait DecisionTrailQuery {
    /// Query trails for a specific unit placement.
    ///
    /// "Why was unit X placed on node Y?" — returns the trail(s) that
    /// contain placement decisions for the given unit.
    fn query_by_unit(
        &self,
        unit_id: &UnitId,
    ) -> Result<Vec<DecisionTrail>, ObserveError>;

    /// Query trails within a logical clock range.
    ///
    /// Retention: since-last-compaction by default (INV-O2).
    /// Returns trails in chronological order.
    fn query_by_range(
        &self,
        from_lc: &LogicalClock,
        to_lc: &LogicalClock,
    ) -> Result<Vec<DecisionTrail>, ObserveError>;
}

/// Replays a solver run from a historical decision trail (INV-C3).
///
/// Deterministic solver means replay produces the exact same result.
/// Used for debugging: "show me the solver's view at time T."
pub trait SolverReplay {
    /// Replay the solver using inputs from a recorded trail.
    ///
    /// Reconstructs the graph snapshot and membership from the trail,
    /// invokes the solver, and verifies the output matches the recorded
    /// placements. Divergence indicates a determinism regression (FM-11).
    fn replay(
        &self,
        trail_id: &DecisionTrailId,
    ) -> Result<SolverResult, ObserveError>;
}

/// Emits structured events for external consumption.
///
/// Every significant system action produces a structured event. Events
/// are formatted as JSON and forwarded to configured sinks (stdout, file,
/// syslog, log aggregator).
pub trait EventEmitter {
    /// Emit a structured event.
    ///
    /// Best-effort delivery. Emission failure does not block the system
    /// operation that triggered the event.
    fn emit(&self, event_type: &str, detail: &str);
}

/// Aggregates health check results from all workloads on a node.
pub trait HealthAggregator {
    /// Report a health check result for a workload.
    fn report(&self, unit_id: &UnitId, status: &HealthStatus);

    /// Query the current health status of a workload.
    fn query(&self, unit_id: &UnitId) -> Option<HealthStatus>;

    /// Query all unhealthy workloads on this node.
    fn unhealthy(&self) -> Vec<HealthStatus>;
}

/// Exposes Prometheus-format metrics endpoint.
pub trait PrometheusExporter {
    /// Render all metrics in Prometheus exposition format.
    ///
    /// Metrics include: memory, CPU, workloads running, solver runs,
    /// gossip messages, artifact cache size, decision trails stored,
    /// compactions total.
    fn render_metrics(&self) -> String;
}

/// Fires alerting webhooks on significant events.
pub trait AlertDispatcher {
    /// Send an alert webhook (best-effort).
    ///
    /// Failure to deliver does not block the system operation.
    /// Retries with exponential backoff (up to 3 attempts).
    async fn dispatch(&self, url: &str, payload: &AlertPayload) -> Result<(), ObserveError>;
}
