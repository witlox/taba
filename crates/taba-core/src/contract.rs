//! Behavioral contracts: tolerances, trust, scaling, failure, and recovery.
//!
//! These types define the non-functional properties a workload unit
//! declares — what it can tolerate, what it trusts, how it scales, what
//! happens when it fails, and how it recovers. The solver and
//! reconciliation engine use these declarations to make placement and
//! lifecycle decisions.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use taba_common::{AuthorId, Ppm, UnitId};

// ---------------------------------------------------------------------------
// Tolerances
// ---------------------------------------------------------------------------

/// Latency and failure budgets a workload can tolerate.
///
/// The solver respects these declarations when matching workloads to
/// nodes (INV-K3). An empty tolerance (all fields `None`/empty) is
/// rejected by validation — a workload must declare at least one
/// tolerance dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tolerances {
    /// Maximum acceptable latency to dependent services.
    pub max_latency: Option<Duration>,
    /// Tolerated failure modes for this workload (e.g., `"timeout"`, `"503"`).
    pub failure_modes: Vec<String>,
    /// Consistency requirements (e.g., `"strong"`, `"eventual"`).
    pub consistency: Option<String>,
}

impl Tolerances {
    /// Returns `true` if this declaration has at least one meaningful
    /// tolerance dimension (`max_latency`, `failure_modes`, or consistency).
    ///
    /// A workload with all-`None`/empty tolerances is malformed.
    #[must_use]
    pub fn is_meaningful(&self) -> bool {
        self.max_latency.is_some() || !self.failure_modes.is_empty() || self.consistency.is_some()
    }
}

// ---------------------------------------------------------------------------
// Trust
// ---------------------------------------------------------------------------

/// Identity-based trust declaration. Not network-topology-based.
///
/// A workload declares which authors or units it trusts for a given
/// access type. The solver uses these to resolve composition security
/// (INV-S2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustDeclaration {
    /// The author or unit this workload trusts.
    pub trusted_entity: AuthorId,
    /// What kind of access is trusted (e.g., `"read"`, `"write"`).
    pub access_type: String,
}

// ---------------------------------------------------------------------------
// Scaling
// ---------------------------------------------------------------------------

/// Scaling parameters for a workload unit.
///
/// The solver computes scaling decisions from these declared parameters
/// (INV-K4). `min_instances` must not exceed `max_instances`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scaling {
    /// Minimum number of instances.
    pub min_instances: u32,
    /// Maximum number of instances.
    pub max_instances: u32,
    /// Named triggers that cause scale-up or scale-down.
    pub triggers: Vec<ScalingTrigger>,
}

/// A named trigger for scaling decisions.
///
/// Each trigger references a metric and threshold. When the metric
/// crosses the threshold in the given direction, the solver initiates
/// a scaling action (INV-K4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScalingTrigger {
    /// Human-readable name for this trigger.
    pub name: String,
    /// Metric to evaluate (e.g., `"cpu_ppm"`, `"queue_depth"`).
    pub metric: String,
    /// Threshold value in parts-per-million (fixed-point, INV-C3).
    pub threshold: Ppm,
    /// Direction: scale up or scale down.
    pub direction: ScaleDirection,
}

/// Direction of a scaling action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScaleDirection {
    /// Scale up (add instances).
    Up,
    /// Scale down (remove instances).
    Down,
}

// ---------------------------------------------------------------------------
// Failure semantics
// ---------------------------------------------------------------------------

/// What happens when a workload fails.
///
/// Declares behavior for out-of-memory (OOM), unexpected crash, and
/// graceful shutdown. The reconciliation engine uses these to determine
/// recovery actions (INV-K5, INV-W2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureSemantics {
    /// Behavior on OOM.
    pub on_oom: OomBehavior,
    /// Behavior on unexpected crash.
    pub on_crash: CrashBehavior,
    /// Behavior on graceful shutdown request.
    pub on_shutdown: ShutdownBehavior,
}

/// OOM behavior declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OomBehavior {
    /// Back off inputs and retry.
    BackoffInputs,
    /// Restart immediately.
    Restart,
    /// Fail permanently, require manual intervention.
    FailPermanent,
}

/// Crash behavior declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CrashBehavior {
    /// This crash means something is wrong — do not auto-restart.
    Unexpected,
    /// Transient crash, safe to restart with backoff.
    RestartWithBackoff {
        /// Maximum number of restart attempts.
        max_retries: u32,
    },
}

/// Graceful shutdown behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ShutdownBehavior {
    /// Drain connections and exit.
    DrainAndExit {
        /// Maximum time to spend draining.
        timeout: Duration,
    },
    /// Immediate exit, no draining.
    Immediate,
}

// ---------------------------------------------------------------------------
// Recovery
// ---------------------------------------------------------------------------

/// Dependency ordering on failure recovery.
///
/// Cycles in recovery relationships fail closed (INV-K5). The solver
/// detects cycles and surfaces them as unresolvable conflicts requiring
/// explicit policy declaring restart priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryRelationship {
    /// The unit that must be drained or restarted first.
    pub depends_on: UnitId,
    /// What must happen to the dependency before this unit recovers.
    pub action: RecoveryAction,
}

/// Action required on a recovery dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RecoveryAction {
    /// Dependency must be drained before this unit restarts.
    DrainFirst,
    /// Dependency must be healthy before this unit restarts.
    WaitForHealthy,
    /// Dependency must be restarted before this unit restarts.
    RestartFirst,
}

/// How a workload recovers state after restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StateRecovery {
    /// No state to recover.
    Stateless,
    /// Replay from a specific offset in an event stream.
    ReplayFromOffset {
        /// The event stream to replay from.
        stream: String,
        /// The offset to start replaying from.
        offset: u64,
    },
    /// Requires quorum of peers before serving.
    RequireQuorum {
        /// Minimum number of healthy peers required.
        min_peers: u32,
    },
}

/// What happens when the hosting node fails (INV-N5).
///
/// Default is environment-derived: `env:dev` defaults to `LeaveDead`,
/// all other environments default to `Replace`. A per-unit
/// `placement_on_failure` declaration overrides the environment default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlacementOnFailure {
    /// Solver re-places the workload to another eligible node.
    Replace,
    /// Workload is left dead (not re-placed). Dev default.
    LeaveDead,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tolerances_is_meaningful_with_max_latency() {
        let t = Tolerances {
            max_latency: Some(Duration::from_millis(100)),
            failure_modes: Vec::new(),
            consistency: None,
        };
        assert!(t.is_meaningful());
    }

    #[test]
    fn test_tolerances_is_meaningful_with_failure_modes() {
        let t = Tolerances {
            max_latency: None,
            failure_modes: vec!["timeout".to_string()],
            consistency: None,
        };
        assert!(t.is_meaningful());
    }

    #[test]
    fn test_tolerances_is_meaningful_with_consistency() {
        let t = Tolerances {
            max_latency: None,
            failure_modes: Vec::new(),
            consistency: Some("strong".to_string()),
        };
        assert!(t.is_meaningful());
    }

    #[test]
    fn test_tolerances_not_meaningful_when_empty() {
        let t = Tolerances {
            max_latency: None,
            failure_modes: Vec::new(),
            consistency: None,
        };
        assert!(!t.is_meaningful());
    }

    #[test]
    fn test_scaling_serialization_roundtrip() {
        let scaling = Scaling {
            min_instances: 1,
            max_instances: 10,
            triggers: vec![ScalingTrigger {
                name: "high-cpu".to_string(),
                metric: "cpu_ppm".to_string(),
                threshold: Ppm(800_000),
                direction: ScaleDirection::Up,
            }],
        };

        let json = serde_json::to_string(&scaling).expect("serialize Scaling");
        let decoded: Scaling = serde_json::from_str(&json).expect("deserialize Scaling");
        assert_eq!(scaling, decoded);
    }

    #[test]
    fn test_failure_semantics_serialization_roundtrip() {
        let fs = FailureSemantics {
            on_oom: OomBehavior::BackoffInputs,
            on_crash: CrashBehavior::RestartWithBackoff { max_retries: 3 },
            on_shutdown: ShutdownBehavior::DrainAndExit {
                timeout: Duration::from_secs(30),
            },
        };

        let json = serde_json::to_string(&fs).expect("serialize FailureSemantics");
        let decoded: FailureSemantics =
            serde_json::from_str(&json).expect("deserialize FailureSemantics");
        assert_eq!(fs, decoded);
    }

    #[test]
    fn test_recovery_relationship_serialization_roundtrip() {
        let rr = RecoveryRelationship {
            depends_on: UnitId(uuid::Uuid::new_v4()),
            action: RecoveryAction::DrainFirst,
        };

        let json = serde_json::to_string(&rr).expect("serialize RecoveryRelationship");
        let decoded: RecoveryRelationship =
            serde_json::from_str(&json).expect("deserialize RecoveryRelationship");
        assert_eq!(rr, decoded);
    }

    #[test]
    fn test_state_recovery_serialization_roundtrip() {
        let sr = StateRecovery::ReplayFromOffset {
            stream: "events".to_string(),
            offset: 42,
        };

        let json = serde_json::to_string(&sr).expect("serialize StateRecovery");
        let decoded: StateRecovery =
            serde_json::from_str(&json).expect("deserialize StateRecovery");
        assert_eq!(sr, decoded);
    }

    #[test]
    fn test_placement_on_failure_serialization_roundtrip() {
        for pof in [PlacementOnFailure::Replace, PlacementOnFailure::LeaveDead] {
            let json = serde_json::to_string(&pof).expect("serialize PlacementOnFailure");
            let decoded: PlacementOnFailure =
                serde_json::from_str(&json).expect("deserialize PlacementOnFailure");
            assert_eq!(pof, decoded);
        }
    }
}
