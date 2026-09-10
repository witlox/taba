//! Health check result aggregation and status computation.
//!
//! The [`HealthAggregator`] collects health check results from all
//! workloads on a node (INV-O3). taba-observe aggregates results;
//! taba-node orchestrates the actual health check execution.
//!
//! ## Timestamp resolution
//!
//! `last_check` uses [`DualClockEvent`], consistent with M1+M2 (A002).

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, NodeId, UnitId};

// ===========================================================================
// HealthStatus
// ===========================================================================

/// Aggregated health status for a workload.
///
/// Produced by a health check (executed by taba-node) and aggregated
/// by taba-observe. The `consecutive_failures` counter enables
/// configurable thresholds before declaring a workload unhealthy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthStatus {
    /// The unit whose health was checked.
    pub unit_id: UnitId,
    /// The node hosting the unit at check time.
    pub node_id: NodeId,
    /// Whether the unit is currently healthy.
    pub healthy: bool,
    /// When the last health check occurred.
    pub last_check: DualClockEvent,
    /// Number of consecutive failed checks (0 if healthy).
    pub consecutive_failures: u32,
    /// Type of health check performed (e.g., "http", "tcp", "exec").
    pub check_type: String,
    /// Additional detail about the health check result.
    pub detail: Option<String>,
}

// ===========================================================================
// HealthAggregator trait
// ===========================================================================

/// Aggregates health check results from all workloads on a node.
///
/// The aggregator is the single source of "is this workload healthy?"
/// for the node. It collects results from health check executors
/// (taba-node) and provides query interfaces for placement decisions,
/// alerting, and dashboards.
pub trait HealthAggregator {
    /// Report a health check result for a workload.
    ///
    /// Stores or updates the health status for the given unit. If a
    /// status already exists for this unit, it is replaced.
    ///
    /// This method is infallible — reporting a health result never
    /// blocks system operations.
    fn report(&self, unit_id: &UnitId, status: &HealthStatus);

    /// Query the current health status of a workload.
    ///
    /// Returns `None` if no health check has been reported for this
    /// unit.
    fn query(&self, unit_id: &UnitId) -> Option<HealthStatus>;

    /// Query all unhealthy workloads on this node.
    ///
    /// Returns all statuses where `healthy == false`, in unspecified
    /// order. Useful for alerting and dashboard display.
    fn unhealthy(&self) -> Vec<HealthStatus>;
}

// ===========================================================================
// DefaultHealthAggregator
// ===========================================================================

/// Default in-memory implementation of [`HealthAggregator`].
///
/// Health statuses are stored in a <code>[Mutex]&lt;[HashMap]&lt;[UnitId],
/// [HealthStatus]&gt;&gt;</code>. For M3, all state is in-memory and
/// lost on restart.
#[derive(Debug, Default)]
pub struct DefaultHealthAggregator {
    statuses: Mutex<HashMap<UnitId, HealthStatus>>,
}

impl DefaultHealthAggregator {
    /// Creates a new, empty in-memory health aggregator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            statuses: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the number of health statuses currently stored.
    ///
    /// Useful for testing and monitoring.
    #[must_use]
    pub fn count(&self) -> usize {
        self.statuses
            .lock()
            .expect("statuses mutex should not be poisoned")
            .len()
    }
}

impl HealthAggregator for DefaultHealthAggregator {
    fn report(&self, unit_id: &UnitId, status: &HealthStatus) {
        let mut statuses = self
            .statuses
            .lock()
            .expect("statuses mutex should not be poisoned");
        statuses.insert(*unit_id, status.clone());
    }

    fn query(&self, unit_id: &UnitId) -> Option<HealthStatus> {
        let statuses = self
            .statuses
            .lock()
            .expect("statuses mutex should not be poisoned");
        statuses.get(unit_id).cloned()
    }

    fn unhealthy(&self) -> Vec<HealthStatus> {
        let statuses = self
            .statuses
            .lock()
            .expect("statuses mutex should not be poisoned");
        statuses.values().filter(|s| !s.healthy).cloned().collect()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::LogicalClock;
    use uuid::Uuid;

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn test_dual_clock(lc: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: taba_common::WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        }
    }

    fn healthy_status(unit_id: UnitId, node_id: NodeId) -> HealthStatus {
        HealthStatus {
            unit_id,
            node_id,
            healthy: true,
            last_check: test_dual_clock(1),
            consecutive_failures: 0,
            check_type: "http".to_string(),
            detail: Some("200 OK".to_string()),
        }
    }

    fn unhealthy_status(unit_id: UnitId, node_id: NodeId) -> HealthStatus {
        HealthStatus {
            unit_id,
            node_id,
            healthy: false,
            last_check: test_dual_clock(2),
            consecutive_failures: 3,
            check_type: "tcp".to_string(),
            detail: Some("connection refused".to_string()),
        }
    }

    // -- Required tests -----------------------------------------------------

    #[test]
    fn test_report_and_query() {
        let aggregator = DefaultHealthAggregator::new();
        let unit_id = test_unit_id();
        let node_id = test_node_id();
        let status = healthy_status(unit_id, node_id);

        aggregator.report(&unit_id, &status);

        let queried = aggregator
            .query(&unit_id)
            .expect("status should exist after report");
        assert_eq!(queried, status);
        assert!(queried.healthy);
        assert_eq!(queried.unit_id, unit_id);
        assert_eq!(queried.node_id, node_id);
        assert_eq!(queried.check_type, "http");
    }

    #[test]
    fn test_query_missing() {
        let aggregator = DefaultHealthAggregator::new();
        let unit_id = test_unit_id();

        assert!(
            aggregator.query(&unit_id).is_none(),
            "no status should exist for unreported unit"
        );
    }

    #[test]
    fn test_unhealthy_filters() {
        let aggregator = DefaultHealthAggregator::new();
        let node_id = test_node_id();

        let healthy_unit = test_unit_id();
        let unhealthy_unit = test_unit_id();
        let another_unhealthy = test_unit_id();

        aggregator.report(&healthy_unit, &healthy_status(healthy_unit, node_id));
        aggregator.report(&unhealthy_unit, &unhealthy_status(unhealthy_unit, node_id));
        aggregator.report(
            &another_unhealthy,
            &unhealthy_status(another_unhealthy, node_id),
        );

        let unhealthy = aggregator.unhealthy();
        assert_eq!(unhealthy.len(), 2, "should return only unhealthy units");

        // All returned statuses should have healthy == false.
        for status in &unhealthy {
            assert!(
                !status.healthy,
                "unhealthy() should only return unhealthy statuses"
            );
        }

        // The healthy unit should not be in the result.
        assert!(
            !unhealthy.iter().any(|s| s.unit_id == healthy_unit),
            "healthy unit should not appear in unhealthy()"
        );
    }

    #[test]
    fn test_report_overwrites() {
        let aggregator = DefaultHealthAggregator::new();
        let unit_id = test_unit_id();
        let node_id = test_node_id();

        // First report: healthy.
        aggregator.report(&unit_id, &healthy_status(unit_id, node_id));
        assert!(aggregator.query(&unit_id).expect("should exist").healthy);

        // Second report: unhealthy. Replaces the first.
        aggregator.report(&unit_id, &unhealthy_status(unit_id, node_id));
        let queried = aggregator.query(&unit_id).expect("should exist");
        assert!(!queried.healthy, "second report should overwrite the first");
        assert_eq!(queried.consecutive_failures, 3);

        // Count should still be 1 (not 2).
        assert_eq!(
            aggregator.count(),
            1,
            "overwrite should not add a new entry"
        );
    }

    #[test]
    fn test_health_status_serialization_roundtrip() {
        let status = HealthStatus {
            unit_id: test_unit_id(),
            node_id: test_node_id(),
            healthy: false,
            last_check: test_dual_clock(42),
            consecutive_failures: 5,
            check_type: "exec".to_string(),
            detail: Some("exit code 1".to_string()),
        };

        let json = serde_json::to_string(&status).expect("serialize HealthStatus");
        let decoded: HealthStatus = serde_json::from_str(&json).expect("deserialize HealthStatus");
        assert_eq!(status, decoded);
    }

    #[test]
    fn test_health_status_serialization_roundtrip_no_detail() {
        let status = HealthStatus {
            unit_id: test_unit_id(),
            node_id: test_node_id(),
            healthy: true,
            last_check: test_dual_clock(10),
            consecutive_failures: 0,
            check_type: "http".to_string(),
            detail: None,
        };

        let json = serde_json::to_string(&status).expect("serialize HealthStatus");
        let decoded: HealthStatus = serde_json::from_str(&json).expect("deserialize HealthStatus");
        assert_eq!(status, decoded);
        assert!(decoded.detail.is_none());
    }
}
