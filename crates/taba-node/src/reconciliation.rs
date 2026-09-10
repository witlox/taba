//! Reconciliation — converging actual state toward desired state.
//!
//! Each node reconciles itself independently — there is no central
//! reconciliation loop. Drift is detected locally and corrected
//! locally. The [`Reconciler`] trait defines the interface; the
//! [`DefaultReconciler`] provides a generic implementation that uses
//! a [`RuntimeExecutor`] to start, stop, and drain units.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use taba_common::UnitId;
use taba_core::Unit;

use crate::error::NodeError;
use crate::mode::ModeManager;
use crate::runtime::{RuntimeExecutor, RuntimeState};

// ---------------------------------------------------------------------------
// LocalPlacement
// ---------------------------------------------------------------------------

/// A placement assigned to this node by the solver.
///
/// The `desired_state` is what the solver says should be running
/// on this node. The reconciler compares this against `actual_state`
/// and takes corrective action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPlacement {
    /// The unit that should be on this node.
    pub unit: UnitId,
    /// The desired runtime state for this unit.
    pub desired_state: RuntimeState,
}

// ---------------------------------------------------------------------------
// Drift
// ---------------------------------------------------------------------------

/// A divergence between desired and actual state for a single unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drift {
    /// The unit that has drifted.
    pub unit: UnitId,
    /// What was desired by the solver.
    pub desired: RuntimeState,
    /// What was actually observed.
    pub actual: RuntimeState,
}

// ---------------------------------------------------------------------------
// Reconciler trait
// ---------------------------------------------------------------------------

/// Reconciles desired state (from solver placements) with actual state
/// (what is running on this node).
///
/// Each node reconciles itself independently — there is no central
/// reconciliation loop. Drift is detected locally and corrected locally.
///
/// Reconciliation is NOT permitted in Degraded mode — only drain and
/// evacuation are allowed. Calling `reconcile` in Degraded mode
/// returns [`NodeError::DegradedModeRestriction`].
pub trait Reconciler {
    /// Reconcile all placements assigned to this node.
    ///
    /// Compares desired state (solver placements) against actual state
    /// (running processes). For each drift:
    /// - Desired=Running, Actual=Stopped → start the unit
    /// - Desired=Stopped, Actual=Running → drain then stop the unit
    /// - Desired=Running, Actual=Failed → restart (respecting failure semantics)
    ///
    /// Returns the list of drifts that were detected (and corrected).
    ///
    /// # Errors
    ///
    /// - [`NodeError::DegradedModeRestriction`] if called in Degraded mode.
    /// - [`NodeError::ReconciliationFailed`] if a runtime operation fails.
    async fn reconcile(&self, placements: &[LocalPlacement]) -> Result<Vec<Drift>, NodeError>;

    /// Detect drift between desired and actual state without correcting it.
    ///
    /// Read-only inspection. Returns all units where desired != actual.
    async fn detect_drift(&self, placements: &[LocalPlacement]) -> Vec<Drift>;

    /// Drain a specific unit (graceful shutdown).
    ///
    /// Transitions the unit to `Draining` state, waits for the
    /// declared drain timeout, then stops it.
    ///
    /// # Errors
    ///
    /// - [`NodeError::UnitNotFound`] if the unit is not on this node.
    /// - [`NodeError::ReconciliationFailed`] if the drain fails.
    async fn drain(&self, unit: &UnitId) -> Result<(), NodeError>;

    /// Evacuate all units from this node (pre-leave or pre-degraded).
    ///
    /// Drains all running units. Used before `gossip::MembershipProtocol::leave`
    /// to give the system time to re-place units elsewhere.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if any drain fails.
    async fn evacuate(&self) -> Result<(), NodeError>;
}

// ---------------------------------------------------------------------------
// DefaultReconciler
// ---------------------------------------------------------------------------

/// Default implementation of [`Reconciler`].
///
/// Uses a [`RuntimeExecutor`] to start, stop, and drain units. Tracks
/// actual state in a [`BTreeMap`] behind a [`std::sync::Mutex`]. All
/// async operations call the runtime executor and update the actual
/// state map.
///
/// The reconciler is generic over `R: RuntimeExecutor` so that tests
/// can use [`SimulatedRuntime`](crate::runtime::SimulatedRuntime) and
/// production can use [`DockerRuntime`](crate::runtime::DockerRuntime).
///
/// Thread-safe via interior mutability. The mutex is never held across
/// an `await` point.
pub struct DefaultReconciler<R: RuntimeExecutor> {
    /// The runtime executor for starting/stopping units.
    runtime: R,
    /// Actual state of units on this node, keyed by unit ID.
    actual_state: std::sync::Mutex<BTreeMap<UnitId, RuntimeState>>,
    /// Units known to this node (for drain/evacuate lookups).
    units: std::sync::Mutex<BTreeMap<UnitId, Unit>>,
    /// Optional mode manager (for Degraded mode checks).
    mode_manager: Option<std::sync::Arc<dyn ModeManager + Send + Sync>>,
}

impl<R: RuntimeExecutor> DefaultReconciler<R> {
    /// Creates a new `DefaultReconciler` with the given runtime executor.
    #[must_use]
    pub fn new(runtime: R) -> Self {
        Self {
            runtime,
            actual_state: std::sync::Mutex::new(BTreeMap::new()),
            units: std::sync::Mutex::new(BTreeMap::new()),
            mode_manager: None,
        }
    }

    /// Sets the mode manager for Degraded mode checks.
    ///
    /// When set, `reconcile` will check the current mode and return
    /// [`NodeError::DegradedModeRestriction`] if the node is in
    /// Degraded mode.
    #[must_use]
    pub fn with_mode_manager(
        mut self,
        mode_manager: std::sync::Arc<dyn ModeManager + Send + Sync>,
    ) -> Self {
        self.mode_manager = Some(mode_manager);
        self
    }

    /// Gets the current actual state of a unit.
    ///
    /// Falls back to checking the runtime executor if the unit is not
    /// yet tracked in `actual_state`. This ensures reconciliation sees
    /// the real runtime state on the first pass.
    fn get_actual(&self, unit_id: &UnitId) -> RuntimeState {
        let state = {
            let guard = self
                .actual_state
                .lock()
                .expect("actual_state mutex should not be poisoned");
            guard.get(unit_id).copied()
        };

        if let Some(state) = state {
            return state;
        }

        let unit = self
            .units
            .lock()
            .expect("units mutex should not be poisoned")
            .get(unit_id)
            .cloned();

        unit.map_or(RuntimeState::Unknown, |u| self.runtime.check_state(&u))
    }

    /// Sets the actual state of a unit.
    fn set_actual(&self, unit_id: UnitId, state: RuntimeState) {
        self.actual_state
            .lock()
            .expect("actual_state mutex should not be poisoned")
            .insert(unit_id, state);
    }

    /// Checks whether reconciliation is permitted (not in Degraded mode).
    fn check_mode(&self) -> Result<(), NodeError> {
        if let Some(mgr) = &self.mode_manager {
            let mode = mgr.current_mode();
            if mode.is_degraded() {
                return Err(NodeError::DegradedModeRestriction {
                    operation: "reconcile".to_string(),
                });
            }
        }
        Ok(())
    }
}

impl<R: RuntimeExecutor + Send + Sync> Reconciler for DefaultReconciler<R> {
    async fn reconcile(&self, placements: &[LocalPlacement]) -> Result<Vec<Drift>, NodeError> {
        // Check mode before reconciling.
        self.check_mode()?;

        let mut drifts = Vec::new();

        for placement in placements {
            let actual = self.get_actual(&placement.unit);
            let desired = placement.desired_state;

            if actual == desired {
                continue;
            }

            // Record the drift.
            drifts.push(Drift {
                unit: placement.unit,
                desired,
                actual,
            });

            // Take corrective action.
            match (desired, actual) {
                (
                    RuntimeState::Running,
                    RuntimeState::Stopped | RuntimeState::Unknown | RuntimeState::Pending,
                ) => {
                    // Start the unit.
                    let unit = self
                        .units
                        .lock()
                        .expect("units mutex should not be poisoned")
                        .get(&placement.unit)
                        .cloned();
                    if let Some(unit) = unit {
                        let result = self.runtime.start(&unit)?;
                        self.set_actual(placement.unit, result);
                    }
                }
                (RuntimeState::Running, RuntimeState::Failed) => {
                    // Restart on failure (respecting failure semantics).
                    let unit = self
                        .units
                        .lock()
                        .expect("units mutex should not be poisoned")
                        .get(&placement.unit)
                        .cloned();
                    if let Some(unit) = unit {
                        let result = self.runtime.start(&unit)?;
                        self.set_actual(placement.unit, result);
                    }
                }
                (RuntimeState::Stopped, RuntimeState::Running) => {
                    // Drain then stop.
                    let unit = self
                        .units
                        .lock()
                        .expect("units mutex should not be poisoned")
                        .get(&placement.unit)
                        .cloned();
                    if let Some(unit) = unit {
                        let result = self.runtime.drain(&unit, Duration::from_secs(30))?;
                        self.set_actual(placement.unit, result);
                    }
                }
                _ => {
                    // No action needed for other state combinations.
                }
            }
        }

        Ok(drifts)
    }

    async fn detect_drift(&self, placements: &[LocalPlacement]) -> Vec<Drift> {
        let mut drifts = Vec::new();

        for placement in placements {
            let actual = self.get_actual(&placement.unit);
            if actual != placement.desired_state {
                drifts.push(Drift {
                    unit: placement.unit,
                    desired: placement.desired_state,
                    actual,
                });
            }
        }

        drifts
    }

    async fn drain(&self, unit: &UnitId) -> Result<(), NodeError> {
        let unit_opt = self
            .units
            .lock()
            .expect("units mutex should not be poisoned")
            .get(unit)
            .cloned();

        let unit = unit_opt.ok_or(NodeError::UnitNotFound { id: *unit })?;

        let result = self.runtime.drain(&unit, Duration::from_secs(30))?;
        self.set_actual(unit.id(), result);
        Ok(())
    }

    async fn evacuate(&self) -> Result<(), NodeError> {
        let unit_ids: Vec<(UnitId, Unit)> = {
            let units = self
                .units
                .lock()
                .expect("units mutex should not be poisoned");
            units.iter().map(|(id, u)| (*id, u.clone())).collect()
        };

        let running_units: Vec<(UnitId, Unit)> = unit_ids
            .into_iter()
            .filter(|(id, _)| self.get_actual(id) == RuntimeState::Running)
            .collect();

        for (id, unit) in running_units {
            let result = self.runtime.drain(&unit, Duration::from_secs(30))?;
            self.set_actual(id, result);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode::OperationalMode;
    use crate::runtime::SimulatedRuntime;
    use taba_test_harness::WorkloadUnitBuilder;

    fn test_unit(id: UnitId) -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build())
    }

    fn placement(id: UnitId, state: RuntimeState) -> LocalPlacement {
        LocalPlacement {
            unit: id,
            desired_state: state,
        }
    }

    fn reconciler_with_unit(
        id: UnitId,
        state: RuntimeState,
    ) -> DefaultReconciler<SimulatedRuntime> {
        let runtime = SimulatedRuntime::new();
        runtime.set_state(id, state);
        let reconciler = DefaultReconciler::new(runtime);
        reconciler
            .units
            .lock()
            .expect("units mutex")
            .insert(id, test_unit(id));
        reconciler
    }

    #[tokio::test]
    async fn test_reconcile_no_drift() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Running);

        let drifts = reconciler
            .reconcile(&[placement(id, RuntimeState::Running)])
            .await
            .expect("reconcile should succeed");

        assert!(drifts.is_empty(), "no drift when desired == actual");
    }

    #[tokio::test]
    async fn test_reconcile_start_needed() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Stopped);

        let drifts = reconciler
            .reconcile(&[placement(id, RuntimeState::Running)])
            .await
            .expect("reconcile should succeed");

        assert_eq!(drifts.len(), 1, "should detect drift");
        assert_eq!(drifts[0].unit, id);
        assert_eq!(drifts[0].desired, RuntimeState::Running);
        assert_eq!(drifts[0].actual, RuntimeState::Stopped);

        // Unit should now be running.
        assert_eq!(reconciler.get_actual(&id), RuntimeState::Running);
    }

    #[tokio::test]
    async fn test_reconcile_stop_needed() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Running);

        let drifts = reconciler
            .reconcile(&[placement(id, RuntimeState::Stopped)])
            .await
            .expect("reconcile should succeed");

        assert_eq!(drifts.len(), 1, "should detect drift");
        assert_eq!(drifts[0].desired, RuntimeState::Stopped);
        assert_eq!(drifts[0].actual, RuntimeState::Running);

        // Unit should now be stopped.
        assert_eq!(reconciler.get_actual(&id), RuntimeState::Stopped);
    }

    #[tokio::test]
    async fn test_reconcile_restart_on_failure() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Failed);

        let drifts = reconciler
            .reconcile(&[placement(id, RuntimeState::Running)])
            .await
            .expect("reconcile should succeed");

        assert_eq!(drifts.len(), 1, "should detect drift");
        assert_eq!(drifts[0].desired, RuntimeState::Running);
        assert_eq!(drifts[0].actual, RuntimeState::Failed);

        // Unit should be restarted (now Running).
        assert_eq!(reconciler.get_actual(&id), RuntimeState::Running);
    }

    #[tokio::test]
    async fn test_detect_drift_readonly() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Stopped);

        // detect_drift should NOT modify actual state.
        let drifts = reconciler
            .detect_drift(&[placement(id, RuntimeState::Running)])
            .await;

        assert_eq!(drifts.len(), 1, "should detect drift");
        assert_eq!(drifts[0].actual, RuntimeState::Stopped);

        // State should be unchanged.
        assert_eq!(
            reconciler.get_actual(&id),
            RuntimeState::Stopped,
            "detect_drift should not modify actual state"
        );
    }

    #[tokio::test]
    async fn test_drain_unit() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Running);

        reconciler.drain(&id).await.expect("drain should succeed");

        assert_eq!(reconciler.get_actual(&id), RuntimeState::Stopped);
    }

    #[tokio::test]
    async fn test_drain_unknown_unit() {
        let runtime = SimulatedRuntime::new();
        let reconciler = DefaultReconciler::new(runtime);

        let unknown_id = UnitId(uuid::Uuid::new_v4());
        let result = reconciler.drain(&unknown_id).await;

        assert!(
            matches!(result, Err(NodeError::UnitNotFound { id }) if id == unknown_id),
            "draining unknown unit should return UnitNotFound"
        );
    }

    #[tokio::test]
    async fn test_evacuate_all() {
        let id1 = UnitId(uuid::Uuid::new_v4());
        let id2 = UnitId(uuid::Uuid::new_v4());

        let runtime = SimulatedRuntime::new();
        runtime.set_state(id1, RuntimeState::Running);
        runtime.set_state(id2, RuntimeState::Running);

        let reconciler = DefaultReconciler::new(runtime);
        {
            let mut units = reconciler.units.lock().expect("units mutex");
            units.insert(id1, test_unit(id1));
            units.insert(id2, test_unit(id2));
        }

        reconciler
            .evacuate()
            .await
            .expect("evacuate should succeed");

        assert_eq!(reconciler.get_actual(&id1), RuntimeState::Stopped);
        assert_eq!(reconciler.get_actual(&id2), RuntimeState::Stopped);
    }

    #[tokio::test]
    async fn test_reconcile_in_degraded_mode() {
        let id = UnitId(uuid::Uuid::new_v4());
        let reconciler = reconciler_with_unit(id, RuntimeState::Stopped);

        let mode_mgr = std::sync::Arc::new(crate::mode::DefaultModeManager::with_mode(
            OperationalMode::Degraded {
                reason: crate::mode::DegradedReason::MemoryLimitExceeded,
            },
        ));

        let reconciler = reconciler.with_mode_manager(mode_mgr);

        let result = reconciler
            .reconcile(&[placement(id, RuntimeState::Running)])
            .await;

        assert!(
            matches!(result, Err(NodeError::DegradedModeRestriction { .. })),
            "reconcile in Degraded mode should return DegradedModeRestriction, got: {result:?}"
        );
    }
}
