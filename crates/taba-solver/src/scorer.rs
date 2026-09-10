//! Placement scoring: deterministic node scoring for composition (INV-C3).
//!
//! The [`PlacementScorer`] trait scores candidate nodes for unit
//! placement. All arithmetic is fixed-point [`Ppm`] — no
//! floating-point anywhere (DL-004). Scores are comparable and
//! deterministic: identical inputs produce identical scores on any
//! node.
//!
//! ## Scoring factors
//!
//! - **Capability match quality** (exact > compatible): an exact
//!   runtime match (e.g., `Oci` artifact on `Oci` runtime) scores
//!   higher than a compatible match (e.g., `Oci` artifact on
//!   `OciRootless` runtime).
//! - **Node health** (INV-R5): `Active` nodes score higher than
//!   `Suspected` nodes. Suspected nodes receive a penalty but are
//!   not excluded.
//! - **Resource availability**: constant for M2 (actual resource
//!   ranking is delegated to [`crate::ResourceRanker`]).
//! - **Existing placements**: spread factor — for M2, this is
//!   constant (no per-node placement state in the graph snapshot).

use taba_common::{NodeId, Ppm};
use taba_core::{ArtifactType, RuntimeCapability, Unit};
use taba_graph::GraphSnapshot;

use crate::SolverError;
use crate::membership::{MembershipSnapshot, NodeHealth};
use crate::placement::PlacementScore;

// ===========================================================================
// PlacementScorer trait
// ===========================================================================

/// Scores candidate nodes for unit placement.
///
/// All arithmetic is fixed-point [`Ppm`]. No floating-point. Division
/// rounds toward zero. Scores are comparable and deterministic
/// (INV-C3).
///
/// Returns [`Err(SolverError)`] if the node cannot host the unit at
/// all (e.g., not in membership, no matching runtime). Returns
/// [`Ok(Ppm)`] with the score otherwise. Higher scores are better.
pub trait PlacementScorer {
    /// Score a single candidate node for a specific unit.
    ///
    /// Considers capability match quality, node health, resource
    /// availability, scaling parameters, and existing placements.
    ///
    /// # Errors
    ///
    /// Returns [`SolverError::NoCapableNode`] if the node cannot host
    /// the unit (no matching runtime). Returns
    /// [`SolverError::InternalError`] if the node is not in the
    /// membership snapshot.
    fn score(
        &self,
        unit: &Unit,
        node: &NodeId,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> Result<Ppm, SolverError>;

    /// Score all candidate nodes for a unit, returning them sorted by
    /// score (descending). Ties broken by lexicographically lowest
    /// `NodeId` (INV-C3).
    ///
    /// Nodes that return `Err` from [`score`](Self::score) are
    /// excluded from the result.
    #[must_use]
    fn rank_nodes(
        &self,
        unit: &Unit,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> Vec<(NodeId, Ppm)>;
}

// ===========================================================================
// DefaultPlacementScorer
// ===========================================================================

/// Default, stateless implementation of [`PlacementScorer`].
///
/// Scoring model (all values in ppm, scale 10^6):
///
/// | Factor | Active | Suspected |
/// |--------|--------|-----------|
/// | Base | 500,000 | 500,000 |
/// | Exact runtime match | +200,000 | +200,000 |
/// | Compatible runtime match | +100,000 | +100,000 |
/// | Health | +200,000 | +50,000 |
/// | Resource | +100,000 | +100,000 |
/// | Latency | +100,000 | +100,000 |
/// | Affinity | +100,000 | +100,000 |
/// | Suspected penalty | 0 | -150,000 |
///
/// The model is intentionally simple for M2. Future milestones will
/// incorporate actual resource snapshots, tolerance matching, and
/// spread/pack strategies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultPlacementScorer;

impl DefaultPlacementScorer {
    /// Creates a new default placement scorer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Determines whether an artifact type exactly matches a runtime
    /// capability.
    ///
    /// `Oci` ↔ `RuntimeCapability::Oci`, `Native` ↔ `Native`, etc.
    /// `OciRootless` is a compatible match for `Oci`, not an exact
    /// one.
    fn is_exact_runtime_match(artifact_type: ArtifactType, runtimes: &[RuntimeCapability]) -> bool {
        match artifact_type {
            ArtifactType::Oci => runtimes.contains(&RuntimeCapability::Oci),
            ArtifactType::Native => runtimes.contains(&RuntimeCapability::Native),
            ArtifactType::Wasm => runtimes.contains(&RuntimeCapability::Wasm),
            ArtifactType::K8sManifest => runtimes.contains(&RuntimeCapability::K8s),
            _ => false,
        }
    }

    /// Determines whether an artifact type is satisfied by any
    /// runtime capability (exact or compatible).
    fn can_host(artifact_type: ArtifactType, runtimes: &[RuntimeCapability]) -> bool {
        match artifact_type {
            ArtifactType::Oci => runtimes
                .iter()
                .any(|r| matches!(r, RuntimeCapability::Oci | RuntimeCapability::OciRootless)),
            ArtifactType::Native => runtimes
                .iter()
                .any(|r| matches!(r, RuntimeCapability::Native)),
            ArtifactType::Wasm => runtimes
                .iter()
                .any(|r| matches!(r, RuntimeCapability::Wasm)),
            ArtifactType::K8sManifest => {
                runtimes.iter().any(|r| matches!(r, RuntimeCapability::K8s))
            }
            _ => false,
        }
    }

    /// Computes the full [`PlacementScore`] breakdown for a node.
    ///
    /// Returns `None` if the node cannot host the unit (no matching
    /// runtime for workload units).
    fn compute_score(
        unit: &Unit,
        caps_runtimes: &[RuntimeCapability],
        health: NodeHealth,
    ) -> Option<PlacementScore> {
        // Determine capability match quality.
        let (cap_bonus, has_match) = match unit {
            Unit::Workload(w) => {
                let at = w.artifact.artifact_type;
                if !Self::can_host(at, caps_runtimes) {
                    return None;
                }
                let bonus = if Self::is_exact_runtime_match(at, caps_runtimes) {
                    Ppm(200_000)
                } else {
                    Ppm(100_000)
                };
                (bonus, true)
            }
            // Non-workload units have no runtime requirement.
            _ => (Ppm(200_000), true),
        };

        if !has_match {
            return None;
        }

        // Health-based scoring (INV-R5).
        let (health_score, suspected_penalty) = match health {
            NodeHealth::Active => (Ppm(200_000), Ppm(0)),
            NodeHealth::Suspected => (Ppm(50_000), Ppm(150_000)),
        };

        // Constant factors for M2.
        let base = Ppm(500_000);
        let resource_score = Ppm(100_000);
        let latency_score = Ppm(100_000);
        let affinity_score = Ppm(100_000);

        // Total = base + cap + health + resource + latency + affinity - penalty.
        // All Ppm operations saturate, so this is safe.
        let total =
            base + cap_bonus + health_score + resource_score + latency_score + affinity_score
                - suspected_penalty;

        Some(PlacementScore {
            total,
            resource_score,
            latency_score,
            affinity_score,
            health_score,
            suspected_penalty,
        })
    }
}

impl PlacementScorer for DefaultPlacementScorer {
    fn score(
        &self,
        unit: &Unit,
        node: &NodeId,
        _graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> Result<Ppm, SolverError> {
        // Find the node in the membership snapshot.
        let (_, caps, health) = membership
            .get(node)
            .ok_or_else(|| SolverError::InternalError {
                reason: format!("node {node:?} not in membership snapshot"),
            })?;

        // Compute the score. Returns None if the node cannot host the unit.
        let score = Self::compute_score(unit, &caps.runtimes, *health).ok_or_else(|| {
            // Determine unmet capabilities for the error.
            let unmet: Vec<_> = match unit {
                Unit::Workload(w) => w.needs.clone(),
                _ => Vec::new(),
            };
            SolverError::NoCapableNode {
                unit: unit.id(),
                unmet,
            }
        })?;

        Ok(score.total)
    }

    fn rank_nodes(
        &self,
        unit: &Unit,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> Vec<(NodeId, Ppm)> {
        let mut scored: Vec<(NodeId, Ppm)> = membership
            .nodes
            .iter()
            .filter_map(|(node_id, _, _)| {
                self.score(unit, node_id, graph, membership)
                    .ok()
                    .map(|s| (*node_id, s))
            })
            .collect();

        // Sort by score descending, ties by NodeId ascending (INV-C3).
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use taba_common::NodeId;
    use taba_core::NodeCapabilitySet;
    use taba_test_harness::{NodeCapabilitySetBuilder, WorkloadUnitBuilder};
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn empty_graph() -> GraphSnapshot {
        GraphSnapshot::new(0, BTreeMap::new(), BTreeMap::new())
    }

    fn membership_with(
        node_id: NodeId,
        caps: NodeCapabilitySet,
        health: NodeHealth,
    ) -> MembershipSnapshot {
        MembershipSnapshot {
            nodes: vec![(node_id, caps, health)],
            generation: 1,
        }
    }

    fn oci_workload() -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().build())
    }

    #[test]
    fn test_score_exact_capability_match() {
        let node = test_node_id();
        let caps_exact = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let caps_compatible = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::OciRootless])
            .build();

        let mem_exact = membership_with(node, caps_exact, NodeHealth::Active);
        let mem_compat = membership_with(node, caps_compatible, NodeHealth::Active);

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let score_exact = scorer
            .score(&unit, &node, &graph, &mem_exact)
            .expect("exact match should score");
        let score_compat = scorer
            .score(&unit, &node, &graph, &mem_compat)
            .expect("compatible match should score");

        assert!(
            score_exact > score_compat,
            "exact match ({score_exact:?}) should score higher than compatible ({score_compat:?})"
        );
    }

    #[test]
    fn test_score_suspected_penalty() {
        let node = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();

        let mem_active = membership_with(node, caps.clone(), NodeHealth::Active);
        let mem_suspected = membership_with(node, caps, NodeHealth::Suspected);

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let score_active = scorer
            .score(&unit, &node, &graph, &mem_active)
            .expect("active should score");
        let score_suspected = scorer
            .score(&unit, &node, &graph, &mem_suspected)
            .expect("suspected should score");

        assert!(
            score_active > score_suspected,
            "active ({score_active:?}) should score higher than suspected ({score_suspected:?}) per INV-R5"
        );
    }

    #[test]
    fn test_score_no_capability_match() {
        let node = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Wasm])
            .build();
        let membership = membership_with(node, caps, NodeHealth::Active);

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let result = scorer.score(&unit, &node, &graph, &membership);
        assert!(
            result.is_err(),
            "node without Oci runtime should return Err"
        );
        assert!(
            matches!(result, Err(SolverError::NoCapableNode { .. })),
            "should be NoCapableNode, got: {result:?}"
        );
    }

    #[test]
    fn test_score_node_not_in_membership() {
        let node = test_node_id();
        let other = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let membership = membership_with(other, caps, NodeHealth::Active);

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let result = scorer.score(&unit, &node, &graph, &membership);
        assert!(
            matches!(result, Err(SolverError::InternalError { .. })),
            "node not in membership should return InternalError, got: {result:?}"
        );
    }

    #[test]
    fn test_rank_nodes_sorted_descending() {
        let oci_node = test_node_id();
        let wasm_node = test_node_id();

        let oci_caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        // Wasm-only node can't host Oci, so use OciRootless for the
        // compatible match.
        let oci_rootless_caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::OciRootless])
            .build();

        let membership = MembershipSnapshot {
            nodes: vec![
                (oci_node, oci_caps, NodeHealth::Active),
                (wasm_node, oci_rootless_caps, NodeHealth::Active),
            ],
            generation: 1,
        };

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let ranked = scorer.rank_nodes(&unit, &graph, &membership);
        assert_eq!(ranked.len(), 2);
        // Exact match (Oci) should score higher than compatible (OciRootless).
        assert_eq!(ranked[0].0, oci_node, "exact match should be first");
        assert_eq!(ranked[1].0, wasm_node, "compatible match should be second");
        assert!(ranked[0].1 > ranked[1].1);
    }

    #[test]
    fn test_rank_nodes_tiebreak() {
        // Two nodes with identical capabilities and health.
        // The one with the lower NodeId should come first.
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
                (id_high, caps_high, NodeHealth::Active),
                (id_low, caps_low, NodeHealth::Active),
            ],
            generation: 1,
        };

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let ranked = scorer.rank_nodes(&unit, &graph, &membership);
        assert_eq!(ranked.len(), 2);
        // Same score, tie broken by lower NodeId.
        assert_eq!(ranked[0].1, ranked[1].1, "scores should be equal");
        assert_eq!(
            ranked[0].0, id_low,
            "lower NodeId should come first (INV-C3)"
        );
        assert_eq!(ranked[1].0, id_high);
    }

    #[test]
    fn test_rank_nodes_excludes_unable() {
        let oci_node = test_node_id();
        let wasm_node = test_node_id();

        let oci_caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let wasm_caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Wasm])
            .build();

        let membership = MembershipSnapshot {
            nodes: vec![
                (oci_node, oci_caps, NodeHealth::Active),
                (wasm_node, wasm_caps, NodeHealth::Active),
            ],
            generation: 1,
        };

        let scorer = DefaultPlacementScorer::new();
        let unit = oci_workload();
        let graph = empty_graph();

        let ranked = scorer.rank_nodes(&unit, &graph, &membership);
        assert_eq!(ranked.len(), 1, "only Oci node should be ranked");
        assert_eq!(ranked[0].0, oci_node);
    }

    #[test]
    fn test_score_non_workload_unit() {
        let node = test_node_id();
        let caps = NodeCapabilitySetBuilder::new().build();
        let membership = membership_with(node, caps, NodeHealth::Active);

        let scorer = DefaultPlacementScorer::new();
        let policy = taba_core::PolicyUnit {
            header: taba_core::UnitHeader {
                id: taba_common::UnitId(Uuid::new_v4()),
                author: taba_common::AuthorId(Uuid::new_v4()),
                trust_domain: taba_common::TrustDomainId(Uuid::new_v4()),
                created_at: taba_common::DualClockEvent {
                    logical_clock: taba_common::LogicalClock(1),
                    wall_time: taba_common::WallTime { millis: 1000 },
                    timezone: "UTC".to_string(),
                },
                validity: None,
                state: taba_core::UnitState::Declared,
                version: None,
            },
            conflict: taba_core::ConflictTuple {
                unit_ids: std::collections::BTreeSet::from([taba_common::UnitId(Uuid::new_v4())]),
                capability_name: "storage".to_string(),
            },
            resolution: taba_core::PolicyResolution::Allow,
            scope: taba_common::TrustDomainId(Uuid::new_v4()),
            rationale: "test".to_string(),
            supersedes: None,
            version: taba_common::Version(1),
            revoked: false,
        };

        let graph = empty_graph();
        let result = scorer.score(&Unit::Policy(policy), &node, &graph, &membership);
        assert!(result.is_ok(), "non-workload should score on any node");
    }
}
