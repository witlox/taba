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
use taba_core::{ArtifactType, NodeCapabilitySet, RuntimeCapability, Tolerances, Unit};
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
            ArtifactType::MicroVm => runtimes.contains(&RuntimeCapability::MicroVm),
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
            ArtifactType::MicroVm => runtimes
                .iter()
                .any(|r| matches!(r, RuntimeCapability::MicroVm)),
            _ => false,
        }
    }

    /// Computes the full [`PlacementScore`] breakdown for a node.
    ///
    /// Returns `None` if the node cannot host the unit (no matching
    /// runtime for workload units).
    ///
    /// Tolerance scoring (INV-K3):
    /// - **Latency**: if the workload declares `max_latency` and the
    ///   node advertises a `latency` custom tag, the node's latency
    ///   must be ≤ the tolerance. Meeting the tolerance scores higher;
    ///   exceeding it scores lower.
    /// - **Failure modes**: if the workload declares `failure_modes`
    ///   and the node advertises `supports_failure` custom tags, the
    ///   node must support at least one declared failure mode.
    ///   Supporting at least one scores higher; supporting none
    ///   scores lower.
    fn compute_score(
        unit: &Unit,
        caps: &NodeCapabilitySet,
        health: NodeHealth,
    ) -> Option<PlacementScore> {
        // Determine capability match quality.
        let (cap_bonus, has_match) = match unit {
            Unit::Workload(w) => {
                let at = w.artifact.artifact_type;
                if !Self::can_host(at, &caps.runtimes) {
                    return None;
                }
                let bonus = if Self::is_exact_runtime_match(at, &caps.runtimes) {
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

        // Tolerance scoring (INV-K3). Extract the workload's
        // tolerances (non-workload units have no tolerance
        // declarations — use neutral defaults).
        let neutral_tolerances = Tolerances {
            max_latency: None,
            failure_modes: Vec::new(),
            consistency: None,
        };
        let tolerances = match unit {
            Unit::Workload(w) => &w.tolerates,
            _ => &neutral_tolerances,
        };

        let resource_score = Ppm(100_000);
        let latency_score = Self::latency_tolerance_score(tolerances, caps);
        let affinity_score = Self::failure_mode_tolerance_score(tolerances, caps);

        // Total = base + cap + health + resource + latency + affinity - penalty.
        // All Ppm operations saturate, so this is safe.
        let total = Ppm(500_000)
            + cap_bonus
            + health_score
            + resource_score
            + latency_score
            + affinity_score
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

    /// Computes the latency tolerance score (INV-K3).
    ///
    /// If the workload declares `max_latency` and the node advertises
    /// a `latency` custom tag (in milliseconds, e.g., `"50ms"`):
    /// - Node latency ≤ tolerance → `Ppm(150_000)` (meets tolerance)
    /// - Node latency > tolerance → `Ppm(50_000)` (exceeds tolerance)
    ///
    /// If either the workload or the node has no latency declaration,
    /// returns the neutral default `Ppm(100_000)`.
    fn latency_tolerance_score(tolerances: &Tolerances, caps: &NodeCapabilitySet) -> Ppm {
        let Some(max_latency) = tolerances.max_latency else {
            return Ppm(100_000);
        };
        let Some(node_latency_ms) = Self::node_latency_ms(caps) else {
            return Ppm(100_000);
        };

        let max_latency_ms = max_latency.as_millis();
        if node_latency_ms <= max_latency_ms {
            Ppm(150_000)
        } else {
            Ppm(50_000)
        }
    }

    /// Computes the failure-mode tolerance score (INV-K3).
    ///
    /// If the workload declares `failure_modes` and the node
    /// advertises `supports_failure` custom tags:
    /// - Node supports at least one declared mode → `Ppm(150_000)`
    /// - Node supports none → `Ppm(50_000)`
    ///
    /// If either the workload or the node has no failure-mode
    /// declarations, returns the neutral default `Ppm(100_000)`.
    fn failure_mode_tolerance_score(tolerances: &Tolerances, caps: &NodeCapabilitySet) -> Ppm {
        if tolerances.failure_modes.is_empty() {
            return Ppm(100_000);
        }

        let supported: Vec<&str> = caps
            .custom_tags
            .iter()
            .filter(|(k, _)| k == "supports_failure")
            .map(|(_, v)| v.as_str())
            .collect();

        if supported.is_empty() {
            return Ppm(100_000);
        }

        let node_supports_any = tolerances
            .failure_modes
            .iter()
            .any(|fm| supported.contains(&fm.as_str()));

        if node_supports_any {
            Ppm(150_000)
        } else {
            Ppm(50_000)
        }
    }

    /// Parses a latency string from a node's custom tags into
    /// milliseconds.
    ///
    /// Supports `"50ms"`, `"100ms"`, `"1s"` (converted to 1000ms),
    /// or a bare number (interpreted as milliseconds).
    fn parse_latency_ms(s: &str) -> Option<u128> {
        if let Some(ms_str) = s.strip_suffix("ms") {
            return ms_str.parse::<u128>().ok();
        }
        if let Some(s_str) = s.strip_suffix('s') {
            return s_str.parse::<u128>().ok().map(|v| v.saturating_mul(1000));
        }
        s.parse::<u128>().ok()
    }

    /// Extracts the node's advertised latency (in milliseconds) from
    /// its custom tags.
    ///
    /// Looks for a `("latency", "Xms")` entry where X is a number.
    /// Returns `None` if no latency tag is present or it cannot be
    /// parsed.
    fn node_latency_ms(caps: &NodeCapabilitySet) -> Option<u128> {
        caps.custom_tags
            .iter()
            .find(|(k, _)| k == "latency")
            .and_then(|(_, v)| Self::parse_latency_ms(v))
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
        let score = Self::compute_score(unit, caps, *health).ok_or_else(|| {
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
    #![allow(clippy::redundant_clone)]
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
        let mem_suspected = membership_with(node, caps.clone(), NodeHealth::Suspected);

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
        let membership = membership_with(node, caps.clone(), NodeHealth::Active);

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
        let membership = membership_with(other, caps.clone(), NodeHealth::Active);

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
        let membership = membership_with(node, caps.clone(), NodeHealth::Active);

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

    // -- INV-K3: Tolerance matching ---------------------------------------

    #[test]
    fn scenario_latency_meets_tolerance_scores_higher() {
        // INV-K3: If the workload tolerates max_latency:50ms and the
        // node's latency is 30ms (≤ tolerance), the score should be
        // higher than a node whose latency is 100ms (> tolerance).
        let node_good = test_node_id();
        let node_bad = test_node_id();

        let caps_good = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_custom_tags(vec![("latency".to_string(), "30ms".to_string())])
            .build();
        let caps_bad = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_custom_tags(vec![("latency".to_string(), "100ms".to_string())])
            .build();

        let mut workload = WorkloadUnitBuilder::new().build();
        workload.tolerates = taba_core::Tolerances {
            max_latency: Some(std::time::Duration::from_millis(50)),
            failure_modes: Vec::new(),
            consistency: None,
        };
        let unit = Unit::Workload(workload);

        let membership = MembershipSnapshot {
            nodes: vec![
                (node_good, caps_good, NodeHealth::Active),
                (node_bad, caps_bad, NodeHealth::Active),
            ],
            generation: 1,
        };

        let scorer = DefaultPlacementScorer::new();
        let graph = empty_graph();

        let score_good = scorer
            .score(&unit, &node_good, &graph, &membership)
            .expect("good node should score");
        let score_bad = scorer
            .score(&unit, &node_bad, &graph, &membership)
            .expect("bad node should score");

        assert!(
            score_good > score_bad,
            "node with latency 30ms (≤ tolerance 50ms) should score higher \
             ({score_good:?}) than node with latency 100ms (> tolerance) ({score_bad:?}) (INV-K3)"
        );
    }

    #[test]
    fn scenario_failure_mode_supported_scores_higher() {
        // INV-K3: If the workload tolerates failure:restart and the
        // node supports restart, the score should be higher than a
        // node that does not support restart.
        let node_good = test_node_id();
        let node_bad = test_node_id();

        let caps_good = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_custom_tags(vec![(
                "supports_failure".to_string(),
                "restart".to_string(),
            )])
            .build();
        let caps_bad = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_custom_tags(vec![("supports_failure".to_string(), "crash".to_string())])
            .build();

        let mut workload = WorkloadUnitBuilder::new().build();
        workload.tolerates = taba_core::Tolerances {
            max_latency: None,
            failure_modes: vec!["restart".to_string()],
            consistency: None,
        };
        let unit = Unit::Workload(workload);

        let membership = MembershipSnapshot {
            nodes: vec![
                (node_good, caps_good, NodeHealth::Active),
                (node_bad, caps_bad, NodeHealth::Active),
            ],
            generation: 1,
        };

        let scorer = DefaultPlacementScorer::new();
        let graph = empty_graph();

        let score_good = scorer
            .score(&unit, &node_good, &graph, &membership)
            .expect("good node should score");
        let score_bad = scorer
            .score(&unit, &node_bad, &graph, &membership)
            .expect("bad node should score");

        assert!(
            score_good > score_bad,
            "node supporting failure mode 'restart' should score higher \
             ({score_good:?}) than node supporting 'crash' only ({score_bad:?}) (INV-K3)"
        );
    }

    #[test]
    fn scenario_no_tolerance_info_uses_neutral_score() {
        // INV-K3: When neither the workload nor the node declares
        // tolerance information, the score should be the same as the
        // neutral default (no penalty, no bonus).
        let node = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build(); // no custom tags

        let mut workload = WorkloadUnitBuilder::new().build();
        workload.tolerates = taba_core::Tolerances {
            max_latency: None,
            failure_modes: Vec::new(),
            consistency: None,
        };
        let unit = Unit::Workload(workload);

        let membership = membership_with(node, caps.clone(), NodeHealth::Active);
        let scorer = DefaultPlacementScorer::new();
        let graph = empty_graph();

        let score = scorer
            .score(&unit, &node, &graph, &membership)
            .expect("should score");

        // Neutral latency + affinity = 100,000 + 100,000 = 200,000
        // (base 500,000 + cap 200,000 + health 200,000 + resource 100,000
        //  + latency 100,000 + affinity 100,000 = 1,200,000)
        assert_eq!(
            score,
            Ppm(1_200_000),
            "neutral tolerance (no info) should produce score 1,200,000, got {score:?}"
        );
    }
    #[test]
    fn test_score_microvm_exact_match() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::MicroVm])
            .build();
        let mut unit = WorkloadUnitBuilder::new().build();
        unit.artifact.artifact_type = ArtifactType::MicroVm;

        let scorer = DefaultPlacementScorer;
        let score = scorer.score(
            &Unit::Workload(unit),
            &node_id,
            &empty_graph(),
            &membership_with(node_id, caps.clone(), NodeHealth::Active),
        );

        assert!(
            score.expect("score should succeed").as_raw() > 0,
            "exact MicroVm match should produce non-zero score"
        );
    }

    #[test]
    fn test_score_wasm_exact_match() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Wasm])
            .build();
        let mut unit = WorkloadUnitBuilder::new().build();
        unit.artifact.artifact_type = ArtifactType::Wasm;

        let scorer = DefaultPlacementScorer;
        let score = scorer.score(
            &Unit::Workload(unit),
            &node_id,
            &empty_graph(),
            &membership_with(node_id, caps.clone(), NodeHealth::Active),
        );

        assert!(
            score.expect("score should succeed").as_raw() > 0,
            "exact Wasm match should produce non-zero score"
        );
    }

    #[test]
    fn test_score_native_exact_match() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Native])
            .build();
        let mut unit = WorkloadUnitBuilder::new().build();
        unit.artifact.artifact_type = ArtifactType::Native;

        let scorer = DefaultPlacementScorer;
        let score = scorer.score(
            &Unit::Workload(unit),
            &node_id,
            &empty_graph(),
            &membership_with(node_id, caps.clone(), NodeHealth::Active),
        );

        assert!(
            score.expect("score should succeed").as_raw() > 0,
            "exact Native match should produce non-zero score"
        );
    }
}
