//! Placement, conflict, composition, scaling, and solver result types.
//!
//! These are the data types produced by the solver. They mirror the
//! definitions in `specs/architecture/data-models/solver.rs` and
//! `specs/architecture/interfaces/solver.rs`, adapted to use the
//! real types from taba-common, taba-core, and taba-graph.
//!
//! ## Timestamp resolution
//!
//! The spec data-models reference `Timestamp` which does not exist in
//! taba-common. Per M1 convention, creation timestamps use
//! `DualClockEvent` and deadlines use `WallTime`. No types in this
//! module carry a raw timestamp — they use the appropriate clock
//! types from taba-common where needed.

use serde::{Deserialize, Serialize};

use taba_common::{NodeId, Ppm, UnitId};
use taba_core::{Capability, CapabilityMatch, ConflictTuple};

// ===========================================================================
// Placement (simplified result used in SolverResult)
// ===========================================================================

/// Assignment of a unit to a node — the simplified result type.
///
/// This is the lightweight form used in [`SolverResult::placements`].
/// For a full scoring breakdown and placement reason, see
/// [`PlacementDecision`].
///
/// # Determinism
///
/// Placement is deterministic: same graph + same membership = same
/// placement on every node (INV-C3). Ties are broken by
/// lexicographically lowest `NodeId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Placement {
    /// The unit being placed.
    pub unit: UnitId,
    /// The node this unit is assigned to.
    pub node: NodeId,
    /// The total score (in ppm) that earned this placement.
    pub score: Ppm,
}

// ===========================================================================
// Placement decision (detailed, with scoring breakdown)
// ===========================================================================

/// Detailed placement decision with full scoring breakdown and reason.
///
/// Used for audit trails (INV-O1) and debugging. The simplified
/// [`Placement`] is derived from this by extracting `unit_id`,
/// `target_node`, and `score.total`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementDecision {
    /// The unit being placed.
    pub unit_id: UnitId,
    /// The node this unit is assigned to.
    pub target_node: NodeId,
    /// Full scoring breakdown for auditability.
    pub score: PlacementScore,
    /// Why this placement was chosen.
    pub reason: PlacementReason,
}

impl PlacementDecision {
    /// Converts this detailed decision into a simplified [`Placement`].
    #[must_use]
    pub const fn to_placement(&self) -> Placement {
        Placement {
            unit: self.unit_id,
            node: self.target_node,
            score: self.score.total,
        }
    }
}

/// Scoring breakdown for a placement decision.
///
/// All values are in [`Ppm`] (fixed-point, 10^6 scale) for
/// determinism (INV-C3, DL-004). Higher scores are better. The
/// `total` is the weighted sum of all component scores minus the
/// `suspected_penalty`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlacementScore {
    /// Weighted total score. Higher is better.
    pub total: Ppm,
    /// Resource availability score (CPU, memory, storage headroom).
    pub resource_score: Ppm,
    /// Latency/proximity score relative to dependencies.
    pub latency_score: Ppm,
    /// Affinity/anti-affinity score based on tolerance declarations.
    pub affinity_score: Ppm,
    /// Health score of the candidate node.
    pub health_score: Ppm,
    /// Penalty applied if the node is in `Suspected` state (INV-R5).
    pub suspected_penalty: Ppm,
}

impl PlacementScore {
    /// Creates a zero score (all components zero).
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            total: Ppm(0),
            resource_score: Ppm(0),
            latency_score: Ppm(0),
            affinity_score: Ppm(0),
            health_score: Ppm(0),
            suspected_penalty: Ppm(0),
        }
    }
}

/// Why a particular placement was selected.
///
/// Used for audit trails (INV-O1) and debugging.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlacementReason {
    /// Normal placement based on highest score.
    BestScore,
    /// Re-placement due to node failure.
    NodeFailure {
        /// The node that failed, triggering re-placement.
        failed_node: NodeId,
    },
    /// Re-placement due to partition tiebreaker (INV-C3).
    /// The lexicographically lowest `NodeId` wins.
    PartitionTiebreak {
        /// The winning node (lowest `NodeId`).
        winner: NodeId,
        /// The losing node (higher `NodeId`).
        loser: NodeId,
    },
    /// Placement due to scaling event.
    ScaleUp,
}

// ===========================================================================
// Solver result
// ===========================================================================

/// The full result of a solver run.
///
/// Contains successful placements, units that could not be placed
/// (with their errors), and detected conflicts. The solver never
/// panics — all failure modes are represented here.
///
/// # Determinism
///
/// For identical inputs, `SolverResult` is identical on every node
/// (INV-C3). The `placements` vector is sorted by `UnitId`. The
/// `unplaceable` and `conflicts` vectors are also sorted for
/// reproducibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolverResult {
    /// Units successfully placed, sorted by `UnitId`.
    pub placements: Vec<Placement>,
    /// Units that could not be placed (with their errors),
    /// sorted by `UnitId`.
    pub unplaceable: Vec<(UnitId, crate::SolverError)>,
    /// Detected conflicts requiring policy resolution,
    /// sorted deterministically.
    pub conflicts: Vec<Conflict>,
}

impl SolverResult {
    /// Creates an empty result (no placements, no conflicts).
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            placements: Vec::new(),
            unplaceable: Vec::new(),
            conflicts: Vec::new(),
        }
    }

    /// Returns `true` if this result has no placements, no
    /// unplaceable units, and no conflicts.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.placements.is_empty() && self.unplaceable.is_empty() && self.conflicts.is_empty()
    }

    /// Sorts all internal vectors for deterministic output.
    ///
    /// Placements and unplaceable are sorted by `UnitId`. Conflicts
    /// are sorted by their first unit ID then capability.
    pub fn sort(&mut self) {
        self.placements.sort_by_key(|p| p.unit);
        self.unplaceable.sort_by_key(|(u, _)| *u);
        self.conflicts.sort_by(|a, b| {
            a.units
                .first()
                .cmp(&b.units.first())
                .then_with(|| a.capability.cmp(&b.capability))
        });
    }
}

// ===========================================================================
// Conflict (used in SolverResult)
// ===========================================================================

/// A detected conflict between units.
///
/// Conflicts occur when capability declarations are incompatible and
/// no policy resolves them (INV-S2). The solver fails closed —
/// composition is refused until explicit policy resolves the conflict.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Conflict {
    /// The units involved in the conflict.
    pub units: Vec<UnitId>,
    /// The capability that triggered the conflict.
    pub capability: Capability,
    /// Whether a policy exists but is insufficient, or no policy
    /// exists at all.
    pub status: ConflictStatus,
}

/// Status of a detected conflict.
///
/// - `Unresolved`: no policy exists for this conflict.
/// - `Revoked`: a policy existed but has been revoked without
///   replacement.
/// - `Ambiguous`: multiple non-revoked policies claim to resolve
///   this conflict (INV-C7 violation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConflictStatus {
    /// No policy resolves this conflict.
    Unresolved,
    /// A policy existed but has been revoked without replacement.
    Revoked,
    /// Multiple non-revoked policies claim to resolve this conflict
    /// (INV-C7 violation).
    Ambiguous,
}

// ===========================================================================
// Recovery cycle
// ===========================================================================

/// A cycle in recovery dependency declarations.
///
/// Recovery relationships declare dependency ordering on failure.
/// Cycles in these relationships are unresolvable without explicit
/// policy (INV-K5). The chain lists unit IDs in dependency order,
/// forming a loop: the last unit depends on the first.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecoveryCycle {
    /// The unit IDs forming the cycle, in dependency order.
    /// The last unit depends on the first, closing the loop.
    pub chain: Vec<UnitId>,
}

// ===========================================================================
// Supersession chain
// ===========================================================================

/// The supersession state of a conflict tuple's policy chain.
///
/// Only one non-revoked policy may resolve any given conflict tuple
/// (INV-C7). The supersession chain is an immutable, versioned
/// lineage — the solver uses the latest non-revoked version.
///
/// If all policies in the chain are revoked, the conflict returns
/// to `Unresolved` status.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SupersessionChain {
    /// The units whose conflict this chain resolves.
    pub conflict_units: Vec<UnitId>,
    /// The capability where the conflict was detected.
    pub capability: Capability,
    /// Policy IDs ordered from oldest to newest. The last entry is
    /// the active policy (if not revoked).
    pub policies: Vec<UnitId>,
}

// ===========================================================================
// Composition result
// ===========================================================================

/// Result of resolving capability matches between units.
///
/// Composition is independent of unit insertion order (INV-C6).
/// The solver evaluates all capability needs against all provides to
/// build composition results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompositionResult {
    /// The set of units participating in this composition.
    pub participants: Vec<UnitId>,
    /// The capability matches that bind these units together.
    pub matches: Vec<CapabilityMatch>,
    /// Whether all needs are satisfied.
    pub fully_satisfied: bool,
    /// Capabilities still unmatched (empty if `fully_satisfied`).
    pub unmatched_needs: Vec<UnmatchedNeed>,
}

/// A capability need that could not be matched to any provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnmatchedNeed {
    /// The unit that declared the need.
    pub unit_id: UnitId,
    /// The capability that could not be matched.
    pub capability: Capability,
}

// ===========================================================================
// Conflict report (detailed, with type and policy state)
// ===========================================================================

/// A detailed conflict report with conflict type classification.
///
/// Used in the full solver output (for audit trails, INV-O1) and
/// by the conflict detector for reporting. Carries more context
/// than the simplified [`Conflict`] used in [`SolverResult`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConflictReport {
    /// The conflict tuple identifying the conflicting units and capability.
    pub conflict: ConflictTuple,
    /// Human-readable description of the conflict.
    pub description: String,
    /// The type of conflict detected.
    pub conflict_type: ConflictType,
    /// Whether a policy already exists for this conflict.
    pub has_policy: bool,
}

/// Types of conflict the solver can detect.
///
/// This enum is `#[non_exhaustive]` — new conflict types may be
/// added in future milestones as the solver evolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConflictType {
    /// Incompatible security requirements between units.
    SecurityIncompatible,
    /// Ambiguous capability match (multiple providers, no disambiguation).
    AmbiguousMatch,
    /// Purpose mismatch on capability (INV-K2).
    PurposeMismatch,
    /// Classification conflict between data units (INV-S7).
    ClassificationConflict,
    /// Cyclic recovery dependencies (INV-K5).
    CyclicRecoveryDependency,
    /// Conflicting retention requirements (FM-010).
    RetentionConflict,
}

// ===========================================================================
// Scaling decision
// ===========================================================================

/// A scaling decision derived from unit-declared parameters (INV-K4).
///
/// The solver does not invent scaling logic — it evaluates declared
/// triggers. When a trigger's metric crosses its threshold in the
/// declared direction, the solver produces a `ScalingDecision`.
///
/// All metric values and thresholds are in [`Ppm`] (fixed-point,
/// 10^6 scale) for determinism (INV-C3, DL-004).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScalingDecision {
    /// The unit to scale.
    pub unit_id: UnitId,
    /// Current instance count.
    pub current_instances: u32,
    /// Target instance count after scaling.
    pub target_instances: u32,
    /// Which trigger fired.
    pub trigger_name: String,
    /// The metric value that triggered the scaling (in ppm).
    pub metric_value: Ppm,
    /// The threshold that was crossed (in ppm).
    pub threshold: Ppm,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    // -- Placement -----------------------------------------------------------

    #[test]
    fn test_placement_construction() {
        let unit = test_unit_id();
        let node = test_node_id();
        let placement = Placement {
            unit,
            node,
            score: Ppm(750_000),
        };
        assert_eq!(placement.unit, unit);
        assert_eq!(placement.node, node);
        assert_eq!(placement.score, Ppm(750_000));
    }

    #[test]
    fn test_placement_serialization_roundtrip() {
        let placement = Placement {
            unit: test_unit_id(),
            node: test_node_id(),
            score: Ppm(500_000),
        };
        let json = serde_json::to_string(&placement).expect("serialize");
        let decoded: Placement = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(placement, decoded);
    }

    // -- PlacementDecision ---------------------------------------------------

    #[test]
    fn test_placement_decision_to_placement() {
        let unit = test_unit_id();
        let node = test_node_id();
        let decision = PlacementDecision {
            unit_id: unit,
            target_node: node,
            score: PlacementScore {
                total: Ppm(800_000),
                resource_score: Ppm(300_000),
                latency_score: Ppm(200_000),
                affinity_score: Ppm(100_000),
                health_score: Ppm(200_000),
                suspected_penalty: Ppm(0),
            },
            reason: PlacementReason::BestScore,
        };
        let placement = decision.to_placement();
        assert_eq!(placement.unit, unit);
        assert_eq!(placement.node, node);
        assert_eq!(placement.score, Ppm(800_000));
    }

    #[test]
    fn test_placement_score_zero() {
        let score = PlacementScore::zero();
        assert_eq!(score.total, Ppm(0));
        assert_eq!(score.resource_score, Ppm(0));
        assert_eq!(score.latency_score, Ppm(0));
        assert_eq!(score.affinity_score, Ppm(0));
        assert_eq!(score.health_score, Ppm(0));
        assert_eq!(score.suspected_penalty, Ppm(0));
    }

    #[test]
    fn test_placement_reason_variants() {
        let node = test_node_id();
        let reasons = vec![
            PlacementReason::BestScore,
            PlacementReason::NodeFailure { failed_node: node },
            PlacementReason::PartitionTiebreak {
                winner: node,
                loser: NodeId(Uuid::new_v4()),
            },
            PlacementReason::ScaleUp,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).expect("serialize PlacementReason");
            let decoded: PlacementReason =
                serde_json::from_str(&json).expect("deserialize PlacementReason");
            assert_eq!(*reason, decoded);
        }
    }

    // -- SolverResult --------------------------------------------------------

    #[test]
    fn test_solver_result_empty() {
        let result = SolverResult::empty();
        assert!(result.is_clean());
        assert!(result.placements.is_empty());
        assert!(result.unplaceable.is_empty());
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_solver_result_not_clean_with_placements() {
        let result = SolverResult {
            placements: vec![Placement {
                unit: test_unit_id(),
                node: test_node_id(),
                score: Ppm(500_000),
            }],
            ..SolverResult::empty()
        };
        assert!(!result.is_clean());
    }

    #[test]
    fn test_solver_result_sort() {
        let id_low = UnitId(Uuid::from_u128(1));
        let id_high = UnitId(Uuid::from_u128(2));

        let mut result = SolverResult {
            placements: vec![
                Placement {
                    unit: id_high,
                    node: test_node_id(),
                    score: Ppm(500_000),
                },
                Placement {
                    unit: id_low,
                    node: test_node_id(),
                    score: Ppm(500_000),
                },
            ],
            unplaceable: vec![
                (
                    id_high,
                    crate::SolverError::InternalError {
                        reason: "test".to_string(),
                    },
                ),
                (
                    id_low,
                    crate::SolverError::InternalError {
                        reason: "test".to_string(),
                    },
                ),
            ],
            conflicts: Vec::new(),
        };

        result.sort();
        assert_eq!(result.placements[0].unit, id_low);
        assert_eq!(result.placements[1].unit, id_high);
        assert_eq!(result.unplaceable[0].0, id_low);
    }

    // -- Conflict ------------------------------------------------------------

    #[test]
    fn test_conflict_construction() {
        let units = vec![test_unit_id(), test_unit_id()];
        let cap = Capability::new("storage", "redis");
        let conflict = Conflict {
            units: units.clone(),
            capability: cap.clone(),
            status: ConflictStatus::Unresolved,
        };
        assert_eq!(conflict.units, units);
        assert_eq!(conflict.capability, cap);
        assert_eq!(conflict.status, ConflictStatus::Unresolved);
    }

    #[test]
    fn test_conflict_status_variants() {
        let statuses = [
            ConflictStatus::Unresolved,
            ConflictStatus::Revoked,
            ConflictStatus::Ambiguous,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).expect("serialize");
            let decoded: ConflictStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*status, decoded);
        }
    }

    // -- RecoveryCycle -------------------------------------------------------

    #[test]
    fn test_recovery_cycle_construction() {
        let chain = vec![test_unit_id(), test_unit_id(), test_unit_id()];
        let cycle = RecoveryCycle {
            chain: chain.clone(),
        };
        assert_eq!(cycle.chain, chain);
    }

    // -- SupersessionChain ---------------------------------------------------

    #[test]
    fn test_supersession_chain_construction() {
        let chain = SupersessionChain {
            conflict_units: vec![test_unit_id()],
            capability: Capability::new("storage", "redis"),
            policies: vec![test_unit_id(), test_unit_id()],
        };
        assert_eq!(chain.policies.len(), 2);
    }

    // -- CompositionResult ---------------------------------------------------

    #[test]
    fn test_composition_result_satisfied() {
        let result = CompositionResult {
            participants: vec![test_unit_id(), test_unit_id()],
            matches: vec![CapabilityMatch {
                needer: test_unit_id(),
                need: Capability::new("storage", "redis"),
                provider: test_unit_id(),
                provided: Capability::new("storage", "redis"),
            }],
            fully_satisfied: true,
            unmatched_needs: Vec::new(),
        };
        assert!(result.fully_satisfied);
        assert!(result.unmatched_needs.is_empty());
    }

    #[test]
    fn test_composition_result_unsatisfied() {
        let need = Capability::new("storage", "redis");
        let result = CompositionResult {
            participants: vec![test_unit_id()],
            matches: Vec::new(),
            fully_satisfied: false,
            unmatched_needs: vec![UnmatchedNeed {
                unit_id: test_unit_id(),
                capability: need,
            }],
        };
        assert!(!result.fully_satisfied);
        assert_eq!(result.unmatched_needs.len(), 1);
    }

    // -- ConflictReport ------------------------------------------------------

    #[test]
    fn test_conflict_report_construction() {
        let report = ConflictReport {
            conflict: ConflictTuple {
                unit_ids: std::collections::BTreeSet::from([test_unit_id()]),
                capability_name: "storage".to_string(),
            },
            description: "missing provider".to_string(),
            conflict_type: ConflictType::AmbiguousMatch,
            has_policy: false,
        };
        assert_eq!(report.conflict_type, ConflictType::AmbiguousMatch);
        assert!(!report.has_policy);
    }

    #[test]
    fn test_conflict_type_variants() {
        let types = [
            ConflictType::SecurityIncompatible,
            ConflictType::AmbiguousMatch,
            ConflictType::PurposeMismatch,
            ConflictType::ClassificationConflict,
            ConflictType::CyclicRecoveryDependency,
            ConflictType::RetentionConflict,
        ];
        for ct in &types {
            let json = serde_json::to_string(ct).expect("serialize");
            let decoded: ConflictType = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*ct, decoded);
        }
    }

    // -- ScalingDecision -----------------------------------------------------

    #[test]
    fn test_scaling_decision_construction() {
        let decision = ScalingDecision {
            unit_id: test_unit_id(),
            current_instances: 2,
            target_instances: 5,
            trigger_name: "high-cpu".to_string(),
            metric_value: Ppm(850_000),
            threshold: Ppm(800_000),
        };
        assert_eq!(decision.current_instances, 2);
        assert_eq!(decision.target_instances, 5);
        assert_eq!(decision.trigger_name, "high-cpu");
        assert_eq!(decision.metric_value, Ppm(850_000));
    }

    #[test]
    fn test_scaling_decision_serialization_roundtrip() {
        let decision = ScalingDecision {
            unit_id: test_unit_id(),
            current_instances: 1,
            target_instances: 3,
            trigger_name: "queue-depth".to_string(),
            metric_value: Ppm(900_000),
            threshold: Ppm(750_000),
        };
        let json = serde_json::to_string(&decision).expect("serialize");
        let decoded: ScalingDecision = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decision, decoded);
    }
}
