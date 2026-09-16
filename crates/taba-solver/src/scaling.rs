//! Scaling evaluation: computing scaling decisions from unit-declared
//! parameters (INV-K4).
//!
//! The solver does not invent scaling logic — it evaluates the
//! [`ScalingTrigger`](taba_core::ScalingTrigger) declarations on each
//! workload unit. When a trigger's metric crosses its threshold in the
//! declared direction, the evaluator produces a
//! [`ScalingDecision`](crate::ScalingDecision) surfacing that scaling
//! is needed.
//!
//! All metric values and thresholds are in [`Ppm`](taba_common::Ppm)
//! (fixed-point, 10^6 scale) for determinism (INV-C3, DL-004).

use std::collections::BTreeMap;

use taba_common::{Ppm, UnitId};
use taba_core::{ScaleDirection, Scaling, ScalingTrigger, WorkloadUnit};

use crate::placement::ScalingDecision;

// ===========================================================================
// ScalingEvaluator trait
// ===========================================================================

/// Evaluates scaling triggers declared on workload units (INV-K4).
///
/// The evaluator is a pure function: no I/O, no side effects, no
/// randomness. Given the same unit, current placement count, and
/// metric values, the result is identical on every node (INV-C3).
///
/// The solver surfaces scaling decisions but does not perform the
/// actual scaling — that is the node's job.
pub trait ScalingEvaluator {
    /// Evaluate all scaling triggers for a unit.
    ///
    /// For each trigger in `unit.scaling.triggers`:
    /// - Look up the trigger's metric in `metric_values`.
    /// - If the metric crosses the threshold in the declared direction
    ///   (`Up`: metric > threshold, `Down`: metric < threshold), and
    ///   the current placement count allows scaling in that direction
    ///   (below `max_instances` for `Up`, above `min_instances` for
    ///   `Down`), a [`ScalingDecision`] is produced.
    ///
    /// Returns a (possibly empty) vector of decisions, sorted by
    /// trigger name for determinism.
    #[must_use]
    fn evaluate(
        &self,
        unit: &WorkloadUnit,
        current_instances: u32,
        metric_values: &BTreeMap<String, Ppm>,
    ) -> Vec<ScalingDecision>;
}

// ===========================================================================
// DefaultScalingEvaluator
// ===========================================================================

/// Default, stateless implementation of [`ScalingEvaluator`].
///
/// All methods are pure functions. Safe to share across threads (no
/// interior mutability). The evaluator computes scaling decisions
/// solely from the unit's declared parameters (INV-K4) — it does not
/// invent scaling logic.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultScalingEvaluator;

impl DefaultScalingEvaluator {
    /// Creates a new default scaling evaluator.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Determines whether a trigger fires given the current metric
    /// value (INV-K4).
    ///
    /// - `ScaleDirection::Up`: fires when `metric_value > threshold`.
    /// - `ScaleDirection::Down`: fires when `metric_value < threshold`.
    fn trigger_fires(trigger: &ScalingTrigger, metric_value: Ppm) -> bool {
        match trigger.direction {
            ScaleDirection::Up => metric_value > trigger.threshold,
            ScaleDirection::Down => metric_value < trigger.threshold,
        }
    }

    /// Computes the target instance count for a fired trigger,
    /// or `None` if scaling is not possible in the declared direction.
    ///
    /// - `Up`: target is `max_instances`, but only if
    ///   `current_instances < max_instances`.
    /// - `Down`: target is `min_instances`, but only if
    ///   `current_instances > min_instances`.
    const fn target_for(
        scaling: &Scaling,
        current_instances: u32,
        direction: ScaleDirection,
    ) -> Option<u32> {
        match direction {
            ScaleDirection::Up if current_instances < scaling.max_instances => {
                Some(scaling.max_instances)
            }
            ScaleDirection::Down if current_instances > scaling.min_instances => {
                Some(scaling.min_instances)
            }
            _ => None,
        }
    }
}

impl ScalingEvaluator for DefaultScalingEvaluator {
    fn evaluate(
        &self,
        unit: &WorkloadUnit,
        current_instances: u32,
        metric_values: &BTreeMap<String, Ppm>,
    ) -> Vec<ScalingDecision> {
        let unit_id: UnitId = unit.header.id;
        let mut decisions = Vec::new();

        for trigger in &unit.scaling.triggers {
            // Look up the current metric value. If the metric is not
            // provided, the trigger cannot fire (INV-K4: the solver
            // evaluates declared parameters, not invented ones).
            let Some(&metric_value) = metric_values.get(&trigger.metric) else {
                continue;
            };

            if !Self::trigger_fires(trigger, metric_value) {
                continue;
            }

            // Determine the target instance count. If scaling is not
            // possible in the declared direction (already at min/max),
            // no decision is produced.
            let Some(target_instances) =
                Self::target_for(&unit.scaling, current_instances, trigger.direction)
            else {
                continue;
            };

            decisions.push(ScalingDecision {
                unit_id,
                current_instances,
                target_instances,
                trigger_name: trigger.name.clone(),
                metric_value,
                threshold: trigger.threshold,
            });
        }

        // Sort by trigger name for deterministic output (INV-C3).
        decisions.sort_by(|a, b| a.trigger_name.cmp(&b.trigger_name));
        decisions
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_core::{ScaleDirection, Scaling, ScalingTrigger};
    use taba_test_harness::WorkloadUnitBuilder;
    use uuid::Uuid;

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    /// Builds a workload with the given scaling parameters and triggers.
    fn workload_with_scaling(scaling: Scaling) -> WorkloadUnit {
        let mut w = WorkloadUnitBuilder::new().build();
        w.scaling = scaling;
        w
    }

    #[test]
    fn scenario_scale_up_when_below_min_and_trigger_fires() {
        // INV-K4: A workload with min_instances=2 and a trigger
        // queue>100 returns a scaling recommendation when the
        // current placement count is below the minimum.
        let unit_id = test_unit_id();
        let unit = WorkloadUnitBuilder::new()
            .with_id(unit_id)
            .with_scaling(2, 5)
            .build();
        // Override the trigger list with a queue-depth trigger.
        let mut unit = unit;
        unit.scaling.triggers = vec![ScalingTrigger {
            name: "queue-depth".to_string(),
            metric: "queue".to_string(),
            threshold: Ppm(100),
            direction: ScaleDirection::Up,
        }];

        // Current placement count is 1 (below min_instances=2).
        // Metric value for "queue" is 150 (above threshold 100).
        let mut metrics = BTreeMap::new();
        metrics.insert("queue".to_string(), Ppm(150));

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 1, &metrics);

        assert_eq!(decisions.len(), 1, "should produce one scaling decision");
        let d = &decisions[0];
        assert_eq!(d.unit_id, unit_id);
        assert_eq!(d.current_instances, 1);
        assert_eq!(d.target_instances, 5, "target should be max_instances");
        assert_eq!(d.trigger_name, "queue-depth");
        assert_eq!(d.metric_value, Ppm(150));
        assert_eq!(d.threshold, Ppm(100));
    }

    #[test]
    fn scenario_no_decision_when_trigger_does_not_fire() {
        let unit = workload_with_scaling(Scaling {
            min_instances: 2,
            max_instances: 5,
            triggers: vec![ScalingTrigger {
                name: "queue-depth".to_string(),
                metric: "queue".to_string(),
                threshold: Ppm(100),
                direction: ScaleDirection::Up,
            }],
        });

        // Metric is below threshold — trigger does not fire.
        let mut metrics = BTreeMap::new();
        metrics.insert("queue".to_string(), Ppm(50));

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 1, &metrics);

        assert!(
            decisions.is_empty(),
            "no decision when trigger does not fire"
        );
    }

    #[test]
    fn scenario_no_decision_when_metric_missing() {
        let unit = workload_with_scaling(Scaling {
            min_instances: 2,
            max_instances: 5,
            triggers: vec![ScalingTrigger {
                name: "queue-depth".to_string(),
                metric: "queue".to_string(),
                threshold: Ppm(100),
                direction: ScaleDirection::Up,
            }],
        });

        // No metric values provided — trigger cannot fire.
        let metrics = BTreeMap::new();

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 1, &metrics);

        assert!(
            decisions.is_empty(),
            "no decision when metric value is not provided (INV-K4)"
        );
    }

    #[test]
    fn scenario_no_decision_when_already_at_max() {
        let unit = workload_with_scaling(Scaling {
            min_instances: 2,
            max_instances: 5,
            triggers: vec![ScalingTrigger {
                name: "queue-depth".to_string(),
                metric: "queue".to_string(),
                threshold: Ppm(100),
                direction: ScaleDirection::Up,
            }],
        });

        // Current placement count is already at max_instances=5.
        let mut metrics = BTreeMap::new();
        metrics.insert("queue".to_string(), Ppm(150));

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 5, &metrics);

        assert!(
            decisions.is_empty(),
            "no scale-up decision when already at max_instances"
        );
    }

    #[test]
    fn scenario_scale_down_when_above_min_and_trigger_fires() {
        let unit_id = test_unit_id();
        let unit = WorkloadUnitBuilder::new()
            .with_id(unit_id)
            .with_scaling(2, 5)
            .build();
        let mut unit = unit;
        unit.scaling.triggers = vec![ScalingTrigger {
            name: "low-cpu".to_string(),
            metric: "cpu_ppm".to_string(),
            threshold: Ppm(200_000),
            direction: ScaleDirection::Down,
        }];

        // Current placement count is 4 (above min_instances=2).
        // Metric value for "cpu_ppm" is 100_000 (below threshold 200_000).
        let mut metrics = BTreeMap::new();
        metrics.insert("cpu_ppm".to_string(), Ppm(100_000));

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 4, &metrics);

        assert_eq!(decisions.len(), 1, "should produce one scale-down decision");
        let d = &decisions[0];
        assert_eq!(d.unit_id, unit_id);
        assert_eq!(d.current_instances, 4);
        assert_eq!(d.target_instances, 2, "target should be min_instances");
        assert_eq!(d.trigger_name, "low-cpu");
    }

    #[test]
    fn scenario_no_scale_down_when_already_at_min() {
        let unit = workload_with_scaling(Scaling {
            min_instances: 2,
            max_instances: 5,
            triggers: vec![ScalingTrigger {
                name: "low-cpu".to_string(),
                metric: "cpu_ppm".to_string(),
                threshold: Ppm(200_000),
                direction: ScaleDirection::Down,
            }],
        });

        // Current placement count is already at min_instances=2.
        let mut metrics = BTreeMap::new();
        metrics.insert("cpu_ppm".to_string(), Ppm(100_000));

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 2, &metrics);

        assert!(
            decisions.is_empty(),
            "no scale-down decision when already at min_instances"
        );
    }

    #[test]
    fn scenario_multiple_triggers_sorted_by_name() {
        let unit_id = test_unit_id();
        let unit = WorkloadUnitBuilder::new()
            .with_id(unit_id)
            .with_scaling(1, 10)
            .build();
        let mut unit = unit;
        // Deliberately unsorted trigger names.
        unit.scaling.triggers = vec![
            ScalingTrigger {
                name: "zeta-trigger".to_string(),
                metric: "metric_z".to_string(),
                threshold: Ppm(100),
                direction: ScaleDirection::Up,
            },
            ScalingTrigger {
                name: "alpha-trigger".to_string(),
                metric: "metric_a".to_string(),
                threshold: Ppm(100),
                direction: ScaleDirection::Up,
            },
        ];

        let mut metrics = BTreeMap::new();
        metrics.insert("metric_z".to_string(), Ppm(200));
        metrics.insert("metric_a".to_string(), Ppm(200));

        let evaluator = DefaultScalingEvaluator::new();
        let decisions = evaluator.evaluate(&unit, 1, &metrics);

        assert_eq!(decisions.len(), 2, "both triggers should fire");
        // Decisions must be sorted by trigger name (INV-C3).
        assert_eq!(decisions[0].trigger_name, "alpha-trigger");
        assert_eq!(decisions[1].trigger_name, "zeta-trigger");
    }

    #[test]
    fn scenario_evaluator_is_deterministic() {
        let unit = workload_with_scaling(Scaling {
            min_instances: 2,
            max_instances: 5,
            triggers: vec![ScalingTrigger {
                name: "queue-depth".to_string(),
                metric: "queue".to_string(),
                threshold: Ppm(100),
                direction: ScaleDirection::Up,
            }],
        });

        let mut metrics = BTreeMap::new();
        metrics.insert("queue".to_string(), Ppm(150));

        let evaluator = DefaultScalingEvaluator::new();
        let d1 = evaluator.evaluate(&unit, 1, &metrics);
        let d2 = evaluator.evaluate(&unit, 1, &metrics);

        assert_eq!(
            d1, d2,
            "same inputs must produce identical results (INV-C3)"
        );
    }
}
