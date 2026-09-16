//! Promotion evaluation: environment authorization and gate governance
//! (INV-E1, INV-E2, INV-E3, INV-S8a).
//!
//! A workload unit can only be placed on nodes whose environment tag
//! matches a promotion policy authorizing that unit version for that
//! environment (INV-E1). The exception is `env:dev`, which requires
//! only author affinity (no promotion needed).
//!
//! Promotion gates are governance units that declare which
//! environment transitions auto-promote and which require human
//! approval (INV-E3). If no gate exists, all transitions default to
//! auto-promote.

use serde::{Deserialize, Serialize};

use taba_common::UnitId;
use taba_core::{PromotionGateDef, PromotionMode, PromotionPolicy, Unit};

// ===========================================================================
// PromotionResult
// ===========================================================================

/// Result of promotion evaluation for a unit.
///
/// Lists which environments the unit is authorized for and which
/// are blocked (with reasons). Environments not listed in either
/// set are implicitly unauthorized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionResult {
    /// Environments this unit is authorized for.
    ///
    /// Always includes `"env:dev"` (INV-E1: author affinity only,
    /// no promotion needed).
    pub authorized_envs: Vec<String>,
    /// Environments blocked (no promotion or gate requires human
    /// approval). Each entry is `(environment, reason)`.
    pub blocked_envs: Vec<(String, String)>,
}

// ===========================================================================
// PromotionCollision
// ===========================================================================

/// A collision between two promotion policies (INV-S8a).
///
/// Occurs when two non-revoked promotion policies for the same unit
/// and target environment make different decisions (e.g., authorize
/// different versions). Same-decision collisions are transparently
/// deduped by lowest `PolicyId` and do not produce a
/// `PromotionCollision`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PromotionCollision {
    /// The first conflicting policy's unit ID.
    pub policy_a: UnitId,
    /// The second conflicting policy's unit ID.
    pub policy_b: UnitId,
    /// The environment where the collision was detected.
    pub environment: String,
    /// Human-readable detail about the collision.
    pub detail: String,
}

// ===========================================================================
// PromotionEvaluator trait
// ===========================================================================

/// Evaluates promotion policies and promotion gates
/// (INV-E1, INV-E2, INV-E3).
///
/// All methods are pure functions: no I/O, no side effects,
/// deterministic. Given the same unit, promotions, and gates, the
/// result is identical on every node (INV-C3).
pub trait PromotionEvaluator {
    /// Determine which environments a unit version is authorized for.
    ///
    /// For `env:dev`: always authorized (checks author affinity at
    /// placement time, no promotion needed — INV-E1).
    ///
    /// For other environments: a `PromotionPolicy` with
    /// `unit_ref == unit.id()` and `target_environment == env` must
    /// exist. If a `PromotionGate` has a transition to that
    /// environment with `HumanApproval` mode, the environment is
    /// blocked. If no gate exists, all transitions default to
    /// auto-promote (INV-E3).
    #[must_use]
    fn evaluate(
        &self,
        unit: &Unit,
        promotions: &[PromotionPolicy],
        gates: &[PromotionGateDef],
    ) -> PromotionResult;

    /// Detect promotion policy collisions (INV-S8a).
    ///
    /// - Same decision (same `unit_ref`, same `version`, same
    ///   `target_environment`) → dedup by lowest `PolicyId`
    ///   (the policy's `header.id`). Returns `None`.
    /// - Different decisions (same `unit_ref` and
    ///   `target_environment`, different `version`) → fail closed.
    ///   Returns `Some(PromotionCollision)`.
    #[must_use]
    fn detect_promotion_collision(
        &self,
        promotions: &[PromotionPolicy],
    ) -> Option<PromotionCollision>;
}

// ===========================================================================
// DefaultPromotionEvaluator
// ===========================================================================

/// Default, stateless implementation of [`PromotionEvaluator`].
///
/// All methods are pure functions. Safe to share across threads (no
/// interior mutability).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultPromotionEvaluator;

impl DefaultPromotionEvaluator {
    /// Creates a new default promotion evaluator.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Finds the gate mode for a target environment, if any.
    ///
    /// Searches all gate definitions for a transition with
    /// `to_env == target_env`. Returns the first match's mode. If no
    /// gate exists, returns `None` (meaning auto-promote per INV-E3).
    fn gate_mode(target_env: &str, gates: &[PromotionGateDef]) -> Option<PromotionMode> {
        gates.iter().flat_map(|g| &g.transitions).find_map(|t| {
            if t.to_env == target_env {
                Some(t.mode)
            } else {
                None
            }
        })
    }
}

impl PromotionEvaluator for DefaultPromotionEvaluator {
    fn evaluate(
        &self,
        unit: &Unit,
        promotions: &[PromotionPolicy],
        gates: &[PromotionGateDef],
    ) -> PromotionResult {
        let mut authorized_envs: Vec<String> = vec!["env:dev".to_string()];
        let mut blocked_envs: Vec<(String, String)> = Vec::new();

        // Find all promotion policies for this unit.
        for promo in promotions {
            if promo.unit_ref != unit.id() {
                continue;
            }

            let env = &promo.target_environment;
            if env == "env:dev" {
                // Already authorized — no need to check gate.
                continue;
            }

            // Check the gate for this environment (INV-E3).
            match Self::gate_mode(env, gates) {
                Some(PromotionMode::HumanApproval) => {
                    blocked_envs.push((env.clone(), format!("human approval required for {env}")));
                }
                Some(PromotionMode::Auto) | None => {
                    // Auto-promote or no gate (defaults to auto, INV-E3).
                    if !authorized_envs.contains(env) {
                        authorized_envs.push(env.clone());
                    }
                }
            }
        }

        // Sort for determinism.
        authorized_envs.sort();
        blocked_envs.sort();

        PromotionResult {
            authorized_envs,
            blocked_envs,
        }
    }

    fn detect_promotion_collision(
        &self,
        promotions: &[PromotionPolicy],
    ) -> Option<PromotionCollision> {
        // Group promotions by (unit_ref, target_environment).
        for i in 0..promotions.len() {
            for j in (i + 1)..promotions.len() {
                let a = &promotions[i];
                let b = &promotions[j];

                // Same unit and environment?
                if a.unit_ref != b.unit_ref || a.target_environment != b.target_environment {
                    continue;
                }

                // Same version → same decision → dedup (no collision).
                if a.version == b.version {
                    continue;
                }

                // Different version → different decision → collision (INV-S8a).
                return Some(PromotionCollision {
                    policy_a: a.header.id,
                    policy_b: b.header.id,
                    environment: a.target_environment.clone(),
                    detail: format!(
                        "conflicting promotion for unit {:?} in {}: version '{}' vs '{}'",
                        a.unit_ref, a.target_environment, a.version, b.version
                    ),
                });
            }
        }

        None
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{AuthorId, TrustDomainId};
    use taba_core::{PromotionGateDef, PromotionMode, PromotionTransition, UnitHeader, UnitState};
    use taba_test_harness::WorkloadUnitBuilder;
    use uuid::Uuid;

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn test_header(id: UnitId) -> UnitHeader {
        UnitHeader {
            id,
            author: AuthorId(Uuid::new_v4()),
            trust_domain: TrustDomainId(Uuid::new_v4()),
            created_at: taba_common::DualClockEvent {
                logical_clock: taba_common::LogicalClock(1),
                wall_time: taba_common::WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: UnitState::Declared,
            version: None,
        }
    }

    fn promotion_policy(
        policy_id: UnitId,
        unit_ref: UnitId,
        version: &str,
        target_env: &str,
    ) -> PromotionPolicy {
        PromotionPolicy {
            header: test_header(policy_id),
            unit_ref,
            version: version.to_string(),
            target_environment: target_env.to_string(),
            rationale: "test promotion".to_string(),
        }
    }

    fn gate_def(transitions: Vec<PromotionTransition>) -> PromotionGateDef {
        PromotionGateDef {
            header: test_header(test_unit_id()),
            transitions,
        }
    }

    fn workload() -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().build())
    }

    #[test]
    fn test_evaluate_dev_no_promotion() {
        // env:dev is always authorized (INV-E1).
        let unit = workload();
        let evaluator = DefaultPromotionEvaluator::new();

        let result = evaluator.evaluate(&unit, &[], &[]);
        assert!(
            result.authorized_envs.contains(&"env:dev".to_string()),
            "env:dev should always be authorized"
        );
        assert!(result.blocked_envs.is_empty(), "nothing should be blocked");
    }

    #[test]
    fn test_evaluate_prod_with_promotion() {
        let unit = workload();
        let unit_id = unit.id();

        let promotion = promotion_policy(test_unit_id(), unit_id, "v1", "env:prod");
        let evaluator = DefaultPromotionEvaluator::new();

        let result = evaluator.evaluate(&unit, &[promotion], &[]);
        assert!(
            result.authorized_envs.contains(&"env:prod".to_string()),
            "prod with promotion should be authorized"
        );
        assert!(
            result.authorized_envs.contains(&"env:dev".to_string()),
            "env:dev should still be authorized"
        );
        assert!(result.blocked_envs.is_empty(), "nothing should be blocked");
    }

    #[test]
    fn test_evaluate_prod_without_promotion() {
        let unit = workload();

        let evaluator = DefaultPromotionEvaluator::new();
        let result = evaluator.evaluate(&unit, &[], &[]);

        // env:dev is always authorized, but env:prod is not (no promotion).
        assert!(
            result.authorized_envs.contains(&"env:dev".to_string()),
            "env:dev should be authorized"
        );
        assert!(
            !result.authorized_envs.contains(&"env:prod".to_string()),
            "env:prod should not be authorized without promotion"
        );
    }

    #[test]
    fn test_evaluate_gate_human_approval() {
        let unit = workload();
        let unit_id = unit.id();

        let promotion = promotion_policy(test_unit_id(), unit_id, "v1", "env:prod");
        let gate = gate_def(vec![PromotionTransition {
            from_env: "env:test".to_string(),
            to_env: "env:prod".to_string(),
            mode: PromotionMode::HumanApproval,
        }]);

        let evaluator = DefaultPromotionEvaluator::new();
        let result = evaluator.evaluate(&unit, &[promotion], &[gate]);

        assert!(
            !result.authorized_envs.contains(&"env:prod".to_string()),
            "env:prod should not be auto-authorized with HumanApproval gate"
        );
        assert!(
            result.blocked_envs.iter().any(|(env, _)| env == "env:prod"),
            "env:prod should be blocked"
        );
        let (_, reason) = result
            .blocked_envs
            .iter()
            .find(|(env, _)| env == "env:prod")
            .expect("env:prod should be in blocked_envs");
        assert!(
            reason.contains("human approval"),
            "reason should mention human approval: {reason}"
        );
    }

    #[test]
    fn test_evaluate_gate_auto() {
        let unit = workload();
        let unit_id = unit.id();

        let promotion = promotion_policy(test_unit_id(), unit_id, "v1", "env:prod");
        let gate = gate_def(vec![PromotionTransition {
            from_env: "env:test".to_string(),
            to_env: "env:prod".to_string(),
            mode: PromotionMode::Auto,
        }]);

        let evaluator = DefaultPromotionEvaluator::new();
        let result = evaluator.evaluate(&unit, &[promotion], &[gate]);

        assert!(
            result.authorized_envs.contains(&"env:prod".to_string()),
            "env:prod should be authorized with Auto gate"
        );
        assert!(result.blocked_envs.is_empty(), "nothing should be blocked");
    }

    #[test]
    fn test_evaluate_multiple_promotions() {
        let unit = workload();
        let unit_id = unit.id();

        let promotions = vec![
            promotion_policy(test_unit_id(), unit_id, "v1", "env:test"),
            promotion_policy(test_unit_id(), unit_id, "v1", "env:staging"),
        ];

        let evaluator = DefaultPromotionEvaluator::new();
        let result = evaluator.evaluate(&unit, &promotions, &[]);

        assert!(result.authorized_envs.contains(&"env:dev".to_string()));
        assert!(result.authorized_envs.contains(&"env:test".to_string()));
        assert!(result.authorized_envs.contains(&"env:staging".to_string()));
        assert!(result.blocked_envs.is_empty());
    }

    #[test]
    fn scenario_promote_to_prod_does_not_remove_from_test() {
        // INV-E2: Promotion policies are cumulative and non-exclusive.
        // Promoting to env:prod does NOT remove the unit from env:test.
        // Environments are independent placement targets, not a
        // pipeline with mutual exclusion.
        let unit = workload();
        let unit_id = unit.id();

        // Promote to env:test first, then env:prod.
        let promotions = vec![
            promotion_policy(test_unit_id(), unit_id, "v1", "env:test"),
            promotion_policy(test_unit_id(), unit_id, "v1", "env:prod"),
        ];

        let evaluator = DefaultPromotionEvaluator::new();
        let result = evaluator.evaluate(&unit, &promotions, &[]);

        // Both env:test AND env:prod should be authorized (cumulative).
        assert!(
            result.authorized_envs.contains(&"env:test".to_string()),
            "env:test should still be authorized after promotion to env:prod (INV-E2)"
        );
        assert!(
            result.authorized_envs.contains(&"env:prod".to_string()),
            "env:prod should be authorized after promotion"
        );
        assert!(
            result.authorized_envs.contains(&"env:dev".to_string()),
            "env:dev should always be authorized"
        );
        assert!(result.blocked_envs.is_empty(), "nothing should be blocked");
    }

    #[test]
    fn test_evaluate_promotion_for_different_unit() {
        let unit = workload();
        let other_id = test_unit_id();

        // Promotion for a different unit — should not authorize.
        let promotion = promotion_policy(test_unit_id(), other_id, "v1", "env:prod");
        let evaluator = DefaultPromotionEvaluator::new();

        let result = evaluator.evaluate(&unit, &[promotion], &[]);
        assert!(
            !result.authorized_envs.contains(&"env:prod".to_string()),
            "promotion for different unit should not authorize"
        );
    }

    // -- detect_promotion_collision ------------------------------------------

    #[test]
    fn test_detect_promotion_collision_same() {
        // Same unit, same version, same target_env → same decision → dedup.
        let unit_ref = test_unit_id();
        let policies = vec![
            promotion_policy(test_unit_id(), unit_ref, "v1", "env:prod"),
            promotion_policy(test_unit_id(), unit_ref, "v1", "env:prod"),
        ];

        let evaluator = DefaultPromotionEvaluator::new();
        let collision = evaluator.detect_promotion_collision(&policies);

        assert!(
            collision.is_none(),
            "same decision should dedup (no collision)"
        );
    }

    #[test]
    fn test_detect_promotion_collision_different() {
        // Same unit, same target_env, different version → collision.
        let unit_ref = test_unit_id();
        let env = "env:prod".to_string();
        let policies = vec![
            promotion_policy(test_unit_id(), unit_ref, "v1", &env),
            promotion_policy(test_unit_id(), unit_ref, "v2", &env),
        ];

        let evaluator = DefaultPromotionEvaluator::new();
        let collision = evaluator.detect_promotion_collision(&policies);

        assert!(
            collision.is_some(),
            "different decisions should produce a collision"
        );
        let c = collision.unwrap();
        assert_eq!(c.environment, env);
        assert!(!c.detail.is_empty());
    }

    #[test]
    fn test_detect_promotion_collision_different_env() {
        // Same unit, different target_env → no collision.
        let unit_ref = test_unit_id();
        let policies = vec![
            promotion_policy(test_unit_id(), unit_ref, "v1", "env:test"),
            promotion_policy(test_unit_id(), unit_ref, "v1", "env:prod"),
        ];

        let evaluator = DefaultPromotionEvaluator::new();
        let collision = evaluator.detect_promotion_collision(&policies);

        assert!(
            collision.is_none(),
            "different environments should not collide"
        );
    }

    #[test]
    fn test_detect_promotion_collision_different_unit() {
        // Different units, same env → no collision.
        let unit_a = test_unit_id();
        let unit_b = test_unit_id();
        let policies = vec![
            promotion_policy(test_unit_id(), unit_a, "v1", "env:prod"),
            promotion_policy(test_unit_id(), unit_b, "v1", "env:prod"),
        ];

        let evaluator = DefaultPromotionEvaluator::new();
        let collision = evaluator.detect_promotion_collision(&policies);

        assert!(collision.is_none(), "different units should not collide");
    }

    #[test]
    fn test_detect_promotion_collision_empty() {
        let evaluator = DefaultPromotionEvaluator::new();
        assert!(evaluator.detect_promotion_collision(&[]).is_none());
    }

    #[test]
    fn test_promotion_result_serialization_roundtrip() {
        let result = PromotionResult {
            authorized_envs: vec!["env:dev".to_string(), "env:prod".to_string()],
            blocked_envs: vec![(
                "env:staging".to_string(),
                "human approval required".to_string(),
            )],
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: PromotionResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(result, decoded);
    }

    #[test]
    fn test_promotion_collision_serialization_roundtrip() {
        let collision = PromotionCollision {
            policy_a: test_unit_id(),
            policy_b: test_unit_id(),
            environment: "env:prod".to_string(),
            detail: "version conflict".to_string(),
        };
        let json = serde_json::to_string(&collision).expect("serialize");
        let decoded: PromotionCollision = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(collision, decoded);
    }
}
