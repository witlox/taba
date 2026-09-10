//! The core solver: deterministic composition and placement (INV-C3).
//!
//! The [`Solver`] trait is the heart of taba's control plane. Given
//! an immutable [`GraphSnapshot`] and a [`MembershipSnapshot`], it
//! produces placement decisions that are identical on every node
//! (INV-C3). The solver is a pure function: no I/O, no side effects,
//! no randomness, no floating-point (DL-004).
//!
//! ## Solver flow
//!
//! For each unit in the graph (iterated in sorted `UnitId` order for
//! determinism):
//! 1. Match capabilities (needs vs provides) across all units
//! 2. Check for security conflicts (fail closed per INV-S2)
//! 3. Check for cyclic recovery dependencies (fail closed per INV-K5)
//! 4. Score candidate nodes using [`PlacementScorer`](crate::PlacementScorer)
//! 5. Place on highest-scoring node
//!
//! Tiebreaker: lexicographically lowest `NodeId` wins (INV-C3).
//! The solver never panics — all error cases are represented in
//! [`SolverResult`](crate::SolverResult).

use std::collections::{BTreeMap, BTreeSet};

use taba_common::UnitId;
use taba_core::{PromotionGateDef, PromotionPolicy, Unit, UnitKind};
use taba_graph::GraphSnapshot;

use crate::conflict::{ConflictDetector, DefaultConflictDetector};
use crate::cycle::{CycleDetector, DefaultCycleDetector};
use crate::filter::{CapabilityFilter, DefaultCapabilityFilter};
use crate::membership::MembershipSnapshot;
use crate::placement::Placement;
use crate::promotion::DefaultPromotionEvaluator;
use crate::resource::DefaultResourceRanker;
use crate::scorer::{DefaultPlacementScorer, PlacementScorer};
use crate::{Conflict, SolverError, SolverResult};

// ===========================================================================
// Solver trait
// ===========================================================================

/// The core solver: computes placements from graph state and membership.
///
/// The solver is deterministic (INV-C3): given identical graph
/// snapshot and identical membership, any node produces identical
/// placement decisions. Composition is order-independent (INV-C6).
///
/// The solver is a pure function. It does not mutate the graph or
/// membership. It reads only from the snapshots provided. No I/O,
/// no randomness, no floating-point.
pub trait Solver {
    /// Compute all placements for the current graph state.
    ///
    /// Evaluates all units in the graph. For each unit:
    /// 1. Match capabilities (needs vs provides) across all units
    /// 2. Check for security conflicts (fail closed per INV-S2)
    /// 3. Check for cyclic recovery dependencies (fail closed per INV-K5)
    /// 4. Score candidate nodes (using [`PlacementScorer`])
    /// 5. Place on highest-scoring node
    ///
    /// Tiebreaker: lexicographically lowest `NodeId` wins (INV-C3).
    ///
    /// Returns the full result including successful placements,
    /// unplaceable units, and detected conflicts. Never panics —
    /// all error cases are represented in [`SolverResult`].
    #[must_use]
    fn solve(&self, graph: &GraphSnapshot, membership: &MembershipSnapshot) -> SolverResult;

    /// Re-evaluate placements affected by a specific set of changed
    /// units.
    ///
    /// Incremental version of [`solve`](Self::solve). Only recomputes
    /// compositions that involve at least one of the changed units.
    /// The result is identical to what `solve` would produce for
    /// those units — this is an optimization, not a different
    /// algorithm.
    ///
    /// Full incremental optimization deferred to M3. For M2, this
    /// calls `solve` and filters the result to only the changed
    /// units.
    #[must_use]
    fn solve_incremental(
        &self,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
        changed: &[UnitId],
    ) -> SolverResult;
}

// ===========================================================================
// DefaultSolver
// ===========================================================================

/// Default implementation of [`Solver`].
///
/// Composes the default implementations of all sub-traits:
/// - [`DefaultConflictDetector`] for conflict detection
/// - [`DefaultCycleDetector`] for cycle detection
/// - [`DefaultCapabilityFilter`] for hard constraint filtering
/// - [`DefaultPlacementScorer`] for node scoring
/// - [`DefaultResourceRanker`] for resource ranking
/// - [`DefaultPromotionEvaluator`] for promotion evaluation
///
/// All sub-components are stateless. The solver is deterministic
/// (INV-C3): same inputs always produce identical outputs. Units are
/// iterated in `BTreeMap` order (sorted by `UnitId`) to ensure
/// reproducibility regardless of insertion order (INV-C6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultSolver {
    conflict_detector: DefaultConflictDetector,
    cycle_detector: DefaultCycleDetector,
    capability_filter: DefaultCapabilityFilter,
    scorer: DefaultPlacementScorer,
    resource_ranker: DefaultResourceRanker,
    promotion_evaluator: DefaultPromotionEvaluator,
}

impl DefaultSolver {
    /// Creates a new default solver with all default sub-components.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            conflict_detector: DefaultConflictDetector::new(),
            cycle_detector: DefaultCycleDetector::new(),
            capability_filter: DefaultCapabilityFilter::new(),
            scorer: DefaultPlacementScorer::new(),
            resource_ranker: DefaultResourceRanker::new(),
            promotion_evaluator: DefaultPromotionEvaluator::new(),
        }
    }

    /// Extracts all [`PromotionGateDef`] objects from governance
    /// units in the graph.
    ///
    /// Governance units of kind `PromotionGate` declare which
    /// environment transitions auto-promote and which require human
    /// approval (INV-E3).
    fn extract_gates(graph: &GraphSnapshot) -> Vec<PromotionGateDef> {
        graph
            .entries
            .values()
            .filter(|e| !e.archived)
            .filter_map(|e| match e.unit() {
                Unit::Governance(taba_core::GovernanceUnit::PromotionGate(gd)) => Some(gd.clone()),
                _ => None,
            })
            .collect()
    }

    /// Collects all [`PromotionPolicy`] objects from the graph.
    ///
    /// For M2, promotion policies are not stored as graph units.
    /// This returns an empty list — the caller (placement filter)
    /// treats empty promotions as "no promotions configured,"
    /// which means only `env:dev` is authorized.
    const fn extract_promotions(_graph: &GraphSnapshot) -> Vec<PromotionPolicy> {
        Vec::new()
    }

    /// Collects `(NodeId, NodeCapabilitySet)` pairs from the
    /// membership snapshot.
    fn collect_node_caps(
        membership: &MembershipSnapshot,
    ) -> Vec<(taba_common::NodeId, taba_core::NodeCapabilitySet)> {
        membership
            .nodes
            .iter()
            .map(|(id, caps, _)| (*id, caps.clone()))
            .collect()
    }

    /// Determines which units are in cycles, for quick lookup.
    fn units_in_cycles(cycles: &[crate::placement::RecoveryCycle]) -> BTreeSet<UnitId> {
        cycles
            .iter()
            .flat_map(|c| c.chain.iter().copied())
            .collect()
    }

    /// Determines which units have unresolved conflicts, for quick lookup.
    fn units_with_conflicts(conflicts: &[Conflict]) -> BTreeSet<UnitId> {
        conflicts
            .iter()
            .filter(|c| c.status == crate::ConflictStatus::Unresolved)
            .flat_map(|c| c.units.iter().copied())
            .collect()
    }
}

impl Solver for DefaultSolver {
    fn solve(&self, graph: &GraphSnapshot, membership: &MembershipSnapshot) -> SolverResult {
        // Detect conflicts (INV-S2, INV-K2).
        let conflicts = self.conflict_detector.detect_conflicts(graph);
        let conflicted_units = Self::units_with_conflicts(&conflicts);

        // Detect cycles (INV-K5).
        let cycles = self.cycle_detector.detect_cycles(graph);
        let cyclic_units = Self::units_in_cycles(&cycles);

        // Extract promotions and gates for capability filtering.
        let _gates = Self::extract_gates(graph);
        let promotions = Self::extract_promotions(graph);
        let node_caps = Self::collect_node_caps(membership);

        // Collect active workload units in sorted order (BTreeMap).
        let active_workloads: BTreeMap<UnitId, &Unit> = graph
            .entries
            .values()
            .filter(|e| !e.archived)
            .filter(|e| e.unit().kind() == UnitKind::Workload)
            .map(|e| (e.unit_id(), e.unit()))
            .collect();

        let mut placements = Vec::new();
        let mut unplaceable = Vec::new();

        for (&unit_id, unit) in &active_workloads {
            // Step 2: Check for unresolved conflicts (fail closed, INV-S2).
            if conflicted_units.contains(&unit_id) {
                let conflict = conflicts
                    .iter()
                    .find(|c| c.units.contains(&unit_id))
                    .cloned()
                    .unwrap_or_else(|| Conflict {
                        units: vec![unit_id],
                        capability: taba_core::Capability::new("unknown", "unknown"),
                        status: crate::ConflictStatus::Unresolved,
                    });
                unplaceable.push((unit_id, SolverError::UnresolvedConflict { conflict }));
                continue;
            }

            // Step 3: Check for cyclic recovery dependencies (INV-K5).
            if cyclic_units.contains(&unit_id) {
                let cycle = cycles
                    .iter()
                    .find(|c| c.chain.contains(&unit_id))
                    .cloned()
                    .unwrap_or_else(|| crate::placement::RecoveryCycle {
                        chain: vec![unit_id],
                    });
                unplaceable.push((unit_id, SolverError::CyclicDependency { cycle }));
                continue;
            }

            // Step 1 + 4: Filter nodes by capability (hard constraints, INV-N2).
            let eligible = self.capability_filter.filter(unit, &node_caps, &promotions);

            if eligible.is_empty() {
                unplaceable.push((
                    unit_id,
                    SolverError::NoCapableNode {
                        unit: unit_id,
                        unmet: unit.needs().to_vec(),
                    },
                ));
                continue;
            }

            // Step 4: Score candidate nodes.
            let ranked = self.scorer.rank_nodes(unit, graph, membership);

            if ranked.is_empty() {
                unplaceable.push((
                    unit_id,
                    SolverError::NoCapableNode {
                        unit: unit_id,
                        unmet: unit.needs().to_vec(),
                    },
                ));
                continue;
            }

            // Step 5: Place on highest-scoring node.
            // ranked is sorted by score descending, ties by NodeId ascending.
            let (best_node, best_score) = &ranked[0];
            placements.push(Placement {
                unit: unit_id,
                node: *best_node,
                score: *best_score,
            });
        }

        let mut result = SolverResult {
            placements,
            unplaceable,
            conflicts,
        };

        // Sort all result vectors for deterministic output (INV-C3).
        result.sort();

        result
    }

    fn solve_incremental(
        &self,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
        changed: &[UnitId],
    ) -> SolverResult {
        // Full incremental optimization deferred to M3. For M2, we
        // call solve and filter the result to only the changed units.
        let full = self.solve(graph, membership);

        let changed_set: BTreeSet<UnitId> = changed.iter().copied().collect();

        let placements = full
            .placements
            .into_iter()
            .filter(|p| changed_set.contains(&p.unit))
            .collect();

        let unplaceable = full
            .unplaceable
            .into_iter()
            .filter(|(u, _)| changed_set.contains(u))
            .collect();

        // Conflicts are not filtered — they may involve units outside
        // the changed set. For M2, we include all conflicts from the
        // full solve. M3 will optimize this.
        let conflicts = full.conflicts;

        SolverResult {
            placements,
            unplaceable,
            conflicts,
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::helpers::*;
    use taba_common::{NodeId, UnitId};
    use taba_core::{Capability, NodeCapabilitySet, RuntimeCapability};
    use taba_test_harness::{NodeCapabilitySetBuilder, WorkloadUnitBuilder};
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn empty_graph() -> GraphSnapshot {
        GraphSnapshot::new(0, BTreeMap::new(), BTreeMap::new())
    }

    fn empty_membership() -> MembershipSnapshot {
        MembershipSnapshot::empty(0)
    }

    fn single_node_membership(caps: NodeCapabilitySet) -> (NodeId, MembershipSnapshot) {
        let node_id = test_node_id();
        let membership = MembershipSnapshot::single_node(node_id, caps);
        (node_id, membership)
    }

    fn workload_with(id: UnitId) -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build())
    }

    fn graph_with_units(units: Vec<Unit>) -> GraphSnapshot {
        let mut entries = BTreeMap::new();
        for unit in units {
            let entry = entry_from_unit(unit);
            entries.insert(entry.unit_id(), entry);
        }
        GraphSnapshot::new(1, entries, BTreeMap::new())
    }

    // -- Solver tests --------------------------------------------------------

    #[test]
    fn test_solve_empty_graph() {
        let solver = DefaultSolver::new();
        let result = solver.solve(&empty_graph(), &empty_membership());
        assert!(result.is_clean(), "empty graph should produce clean result");
    }

    #[test]
    fn test_solve_single_unit_single_node() {
        let unit_id = test_unit_id();
        let unit = workload_with(unit_id);

        let (node_id, membership) = single_node_membership(NodeCapabilitySetBuilder::new().build());

        let graph = graph_with_units(vec![unit]);
        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &membership);

        assert_eq!(result.placements.len(), 1);
        assert_eq!(result.placements[0].unit, unit_id);
        assert_eq!(result.placements[0].node, node_id);
        assert!(result.unplaceable.is_empty());
        assert!(result.conflicts.is_empty());
    }

    #[test]
    fn test_solve_no_capable_node() {
        let unit_id = test_unit_id();
        // Unit needs Oci, but node only has Wasm.
        let unit = workload_with(unit_id);

        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Wasm])
            .build();
        let (node_id, membership) = single_node_membership(caps);

        let graph = graph_with_units(vec![unit]);
        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &membership);

        assert!(
            result.placements.is_empty(),
            "no capable node should produce no placements"
        );
        assert_eq!(result.unplaceable.len(), 1);
        assert_eq!(result.unplaceable[0].0, unit_id);
        assert!(
            matches!(result.unplaceable[0].1, SolverError::NoCapableNode { .. }),
            "should be NoCapableNode, got: {:?}",
            result.unplaceable[0].1
        );
        let _ = node_id;
    }

    #[test]
    fn test_solve_deterministic_tiebreak() {
        // Two nodes with identical capabilities — lowest NodeId wins.
        let id_low = NodeId(Uuid::from_u128(1));
        let id_high = NodeId(Uuid::from_u128(2));

        let caps_low = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let caps_high = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();

        let membership = MembershipSnapshot {
            nodes: vec![
                (id_high, caps_high, crate::NodeHealth::Active),
                (id_low, caps_low, crate::NodeHealth::Active),
            ],
            generation: 1,
        };

        let unit = workload_with(test_unit_id());
        let graph = graph_with_units(vec![unit]);

        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &membership);

        assert_eq!(result.placements.len(), 1);
        assert_eq!(
            result.placements[0].node, id_low,
            "lowest NodeId should win (INV-C3)"
        );
    }

    #[test]
    fn test_solve_order_independent() {
        // Same units in different insertion orders → same result.
        let id_a = UnitId(Uuid::from_u128(1));
        let id_b = UnitId(Uuid::from_u128(2));

        let unit_a = workload_with(id_a);
        let unit_b = workload_with(id_b);

        // Insert A first, then B.
        let graph_ab = graph_with_units(vec![unit_a.clone(), unit_b.clone()]);

        // Insert B first, then A.
        let graph_ba = graph_with_units(vec![unit_b, unit_a]);

        let caps = NodeCapabilitySetBuilder::new().build();
        let membership = MembershipSnapshot::single_node(test_node_id(), caps);

        let solver = DefaultSolver::new();
        let result_ab = solver.solve(&graph_ab, &membership);
        let result_ba = solver.solve(&graph_ba, &membership);

        assert_eq!(
            result_ab, result_ba,
            "same units in different order should produce identical results (INV-C6)"
        );
    }

    #[test]
    fn test_solve_incremental_matches_full() {
        let unit_id = test_unit_id();
        let unit = workload_with(unit_id);

        let (node_id, membership) = single_node_membership(NodeCapabilitySetBuilder::new().build());
        let graph = graph_with_units(vec![unit]);

        let solver = DefaultSolver::new();
        let full = solver.solve(&graph, &membership);
        let incremental = solver.solve_incremental(&graph, &membership, &[unit_id]);

        // Incremental result for the changed unit should match full.
        assert_eq!(incremental.placements.len(), 1);
        assert_eq!(incremental.placements[0], full.placements[0]);
        let _ = node_id;
    }

    #[test]
    fn test_solve_incremental_filters() {
        let id_a = test_unit_id();
        let id_b = test_unit_id();
        let unit_a = workload_with(id_a);
        let unit_b = workload_with(id_b);

        let (node_id, membership) = single_node_membership(NodeCapabilitySetBuilder::new().build());
        let graph = graph_with_units(vec![unit_a, unit_b]);

        let solver = DefaultSolver::new();
        let incremental = solver.solve_incremental(&graph, &membership, &[id_a]);

        // Only unit_a should be in the incremental result.
        assert_eq!(incremental.placements.len(), 1);
        assert_eq!(incremental.placements[0].unit, id_a);
        let _ = node_id;
    }

    #[test]
    fn test_solve_multiple_units() {
        let id_a = test_unit_id();
        let id_b = test_unit_id();
        let unit_a = workload_with(id_a);
        let unit_b = workload_with(id_b);

        let (node_id, membership) = single_node_membership(NodeCapabilitySetBuilder::new().build());
        let graph = graph_with_units(vec![unit_a, unit_b]);

        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &membership);

        assert_eq!(result.placements.len(), 2, "both units should be placed");
        // Placements sorted by UnitId.
        let mut sorted_ids = [id_a, id_b];
        sorted_ids.sort();
        assert_eq!(result.placements[0].unit, sorted_ids[0]);
        assert_eq!(result.placements[1].unit, sorted_ids[1]);
        let _ = node_id;
    }

    #[test]
    fn test_solve_never_panics_on_empty_membership() {
        let unit = workload_with(test_unit_id());
        let graph = graph_with_units(vec![unit]);

        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &empty_membership());

        // No nodes → all units unplaceable.
        assert!(result.placements.is_empty());
        assert_eq!(result.unplaceable.len(), 1);
    }

    #[test]
    fn test_solve_cyclic_dependency() {
        use taba_core::{RecoveryAction, RecoveryRelationship};

        let id_a = UnitId(Uuid::from_u128(1));
        let id_b = UnitId(Uuid::from_u128(2));

        let mut w_a = WorkloadUnitBuilder::new().with_id(id_a).build();
        w_a.recovery_relationships = vec![RecoveryRelationship {
            depends_on: id_b,
            action: RecoveryAction::DrainFirst,
        }];
        let mut w_b = WorkloadUnitBuilder::new().with_id(id_b).build();
        w_b.recovery_relationships = vec![RecoveryRelationship {
            depends_on: id_a,
            action: RecoveryAction::DrainFirst,
        }];

        let (node_id, membership) = single_node_membership(NodeCapabilitySetBuilder::new().build());
        let graph = graph_with_units(vec![Unit::Workload(w_a), Unit::Workload(w_b)]);

        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &membership);

        // Both units should be unplaceable due to cycle.
        assert!(
            result.unplaceable.len() >= 2,
            "cyclic units should be unplaceable, got {} unplaceable",
            result.unplaceable.len()
        );
        for (_, err) in &result.unplaceable {
            assert!(
                matches!(err, SolverError::CyclicDependency { .. }),
                "should be CyclicDependency, got: {err:?}"
            );
        }
        let _ = node_id;
    }

    #[test]
    fn test_solve_unresolved_conflict() {
        // Unit needs "storage:redis", no unit provides it.
        let id_a = test_unit_id();
        let unit_a = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_a)
                .with_needs(vec![Capability::new("storage", "redis")])
                .build(),
        );

        let (node_id, membership) = single_node_membership(NodeCapabilitySetBuilder::new().build());
        let graph = graph_with_units(vec![unit_a]);

        let solver = DefaultSolver::new();
        let result = solver.solve(&graph, &membership);

        // Unit with unresolved conflict should be unplaceable.
        assert!(
            result.placements.is_empty(),
            "conflicted unit should not be placed"
        );
        assert!(!result.conflicts.is_empty(), "conflicts should be reported");
        let _ = node_id;
    }

    // -- Property tests ------------------------------------------------------

    use proptest::prelude::*;

    /// Strategy for generating a small set of workload units with unique IDs.
    fn arb_units() -> impl Strategy<Value = Vec<Unit>> {
        prop::collection::vec(1u128..1_000_000, 0..5).prop_map(|ids| {
            ids.into_iter()
                .map(|i| workload_with(UnitId(uuid::Uuid::from_u128(i))))
                .collect()
        })
    }

    /// Strategy for generating a membership snapshot with 1-3 nodes.
    fn arb_membership() -> impl Strategy<Value = MembershipSnapshot> {
        prop::collection::vec(1u128..1_000_000, 1..4).prop_map(|ids| {
            let nodes: Vec<_> = ids
                .into_iter()
                .map(|i| {
                    (
                        NodeId(uuid::Uuid::from_u128(i)),
                        NodeCapabilitySetBuilder::new().build(),
                        crate::NodeHealth::Active,
                    )
                })
                .collect();
            MembershipSnapshot {
                nodes,
                generation: 1,
            }
        })
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_solve_deterministic(
            units in arb_units(),
            membership in arb_membership(),
        ) {
            let graph = graph_with_units(units);

            let solver = DefaultSolver::new();
            let result1 = solver.solve(&graph, &membership);
            let result2 = solver.solve(&graph, &membership);

            prop_assert_eq!(result1, result2, "solve(g, m) == solve(g, m) must hold (INV-C3)");
        }

        #[test]
        fn proptest_solve_order_independent(
            unit_ids in prop::collection::vec(1u128..1_000_000, 0..5),
            membership in arb_membership(),
        ) {
            // Build units from IDs.
            let units_a: Vec<Unit> = unit_ids.iter()
                .map(|&i| workload_with(UnitId(uuid::Uuid::from_u128(i))))
                .collect();
            let mut units_b = units_a.clone();
            units_b.reverse();

            let graph_a = graph_with_units(units_a);
            let graph_b = graph_with_units(units_b);

            let solver = DefaultSolver::new();
            let result_a = solver.solve(&graph_a, &membership);
            let result_b = solver.solve(&graph_b, &membership);

            prop_assert_eq!(
                result_a, result_b,
                "same units in different order must produce same result (INV-C6)"
            );
        }

        #[test]
        fn proptest_rank_nodes_deterministic(
            unit_id in 1u128..1_000_000,
            node_ids in prop::collection::vec(1u128..1_000_000, 1..5),
        ) {
            let unit = workload_with(UnitId(uuid::Uuid::from_u128(unit_id)));

            let nodes: Vec<_> = node_ids.into_iter()
                .map(|i| (
                    NodeId(uuid::Uuid::from_u128(i)),
                    NodeCapabilitySetBuilder::new().build(),
                    crate::NodeHealth::Active,
                ))
                .collect();
            let membership = MembershipSnapshot { nodes, generation: 1 };

            let graph = empty_graph();
            let scorer = DefaultPlacementScorer::new();

            let ranked1 = scorer.rank_nodes(&unit, &graph, &membership);
            let ranked2 = scorer.rank_nodes(&unit, &graph, &membership);

            prop_assert_eq!(ranked1, ranked2, "ranking must be deterministic");
        }
    }
}
