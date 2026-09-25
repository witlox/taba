//! Capability filtering: hard constraints for node eligibility (INV-N2).
//!
//! The [`CapabilityFilter`] trait defines a binary match: a workload's
//! artifact type must match a node's runtime capability. No fallback,
//! no approximation. Additional hard constraints from `artifact.requires`
//! (arch, OS, privilege, custom tags) and environment tags are also
//! checked.
//!
//! All filtering is deterministic: given the same unit, nodes, and
//! promotions, the result is identical on every node (INV-C3).

use taba_common::NodeId;
use taba_core::{
    ArtifactType, NodeCapabilitySet, PrivilegeLevel, PromotionPolicy, RuntimeCapability, Unit,
};

// ===========================================================================
// CapabilityFilter trait
// ===========================================================================

/// Filters nodes by capability — hard constraints (INV-N2).
///
/// A node either can or cannot host a given unit. There is no
/// partial match, no fallback, no approximation. Only nodes where
/// ALL hard constraints are satisfied are returned.
///
/// Hard constraints checked:
/// - `artifact.type` → node runtime capability (binary match)
/// - `artifact.requires` → node capability match (arch, OS, privilege,
///   custom tags)
/// - Environment tag match (via promotion policy or author affinity,
///   INV-E1)
///
/// The result is sorted by `NodeId` for determinism (INV-C3).
pub trait CapabilityFilter {
    /// Filter nodes that can host this unit's artifact.
    ///
    /// Returns only nodes where ALL hard constraints are satisfied,
    /// sorted by `NodeId` (lexicographic, INV-C3).
    ///
    /// Non-workload units (data, policy, governance) are not filtered —
    /// all nodes are returned since they do not require a runtime.
    fn filter(
        &self,
        unit: &Unit,
        nodes: &[(NodeId, NodeCapabilitySet)],
        promotions: &[PromotionPolicy],
    ) -> Vec<NodeId>;
}

// ===========================================================================
// DefaultCapabilityFilter
// ===========================================================================

/// Default, stateless implementation of [`CapabilityFilter`].
///
/// All methods are pure functions: no I/O, no side effects,
/// deterministic. Safe to share across threads (no interior
/// mutability).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultCapabilityFilter;

impl DefaultCapabilityFilter {
    /// Creates a new default capability filter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Checks whether a node's runtime capabilities can execute the
    /// given artifact type (INV-N2).
    ///
    /// `Oci` artifacts can run on nodes with `Oci` or `OciRootless`.
    /// Other artifact types require an exact runtime match.
    fn artifact_runtime_match(artifact_type: ArtifactType, runtimes: &[RuntimeCapability]) -> bool {
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

    /// Checks whether all `artifact.requires` entries are satisfied by
    /// the node's capability set.
    ///
    /// Each requirement string is matched against:
    /// - Node architecture (e.g., `"x86_64"`)
    /// - Node operating system (e.g., `"linux"`)
    /// - `"root"` or `"privileged"` → node must have `PrivilegeLevel::Root`
    ///   or `ports_privileged == true`
    /// - Custom tags (key or value match)
    ///
    /// Requirements that do not match any of the above cause the node
    /// to be excluded.
    fn requires_satisfied(requires: &[String], caps: &NodeCapabilitySet) -> bool {
        requires.iter().all(|req| {
            // Architecture match.
            if *req == caps.arch {
                return true;
            }
            // OS match.
            if *req == caps.os {
                return true;
            }
            // Privilege requirements.
            if *req == "root" || *req == "privileged" {
                return caps.privilege == PrivilegeLevel::Root || caps.ports_privileged;
            }
            // Custom tag key or value match.
            caps.custom_tags.iter().any(|(k, v)| k == req || v == req)
        })
    }

    /// Checks whether the unit is authorized for the node's environment
    /// (INV-E1).
    ///
    /// - `env:dev`: only author affinity is checked (no promotion
    ///   needed). If the node has `author_affinity`, the unit's author
    ///   must match. If no affinity, any unit passes.
    /// - Other environments: a `PromotionPolicy` with `unit_ref ==
    ///   unit.id()` and `target_environment == node.environment` must
    ///   exist. If no such policy, the node is excluded.
    /// - `None` environment: no environment restriction.
    fn environment_matches(
        unit: &Unit,
        caps: &NodeCapabilitySet,
        promotions: &[PromotionPolicy],
    ) -> bool {
        let Some(env) = caps.environment.as_ref() else {
            return true;
        };

        if env == "env:dev" {
            // INV-E1: dev placement requires only author affinity.
            if let Some(affinity) = caps.author_affinity {
                return unit.header().author == affinity;
            }
            // No affinity set — any unit can be placed.
            return true;
        }

        // Non-dev environments require a promotion policy.
        promotions
            .iter()
            .any(|p| p.unit_ref == unit.id() && p.target_environment == *env)
    }
}

impl CapabilityFilter for DefaultCapabilityFilter {
    fn filter(
        &self,
        unit: &Unit,
        nodes: &[(NodeId, NodeCapabilitySet)],
        promotions: &[PromotionPolicy],
    ) -> Vec<NodeId> {
        // Non-workload units do not require a runtime — all nodes
        // are eligible.
        let Some(workload) = (if let Unit::Workload(w) = unit {
            Some(w)
        } else {
            None
        }) else {
            let mut ids: Vec<NodeId> = nodes.iter().map(|(id, _)| *id).collect();
            ids.sort();
            return ids;
        };

        let mut eligible: Vec<NodeId> = Vec::new();
        for (node_id, caps) in nodes {
            // 1. Artifact type → runtime capability (INV-N2).
            if !Self::artifact_runtime_match(workload.artifact.artifact_type, &caps.runtimes) {
                continue;
            }

            // 2. Artifact requires → node capability match.
            if !Self::requires_satisfied(&workload.artifact.requires, caps) {
                continue;
            }

            // 3. Environment tag match (INV-E1).
            if !Self::environment_matches(unit, caps, promotions) {
                continue;
            }

            eligible.push(*node_id);
        }

        // Sort by NodeId for deterministic output (INV-C3).
        eligible.sort();
        eligible
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    #![allow(clippy::redundant_clone)]
    use super::*;
    use taba_common::{AuthorId, UnitId};
    use taba_test_harness::{NodeCapabilitySetBuilder, WorkloadUnitBuilder};
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn test_unit() -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().build())
    }

    #[test]
    fn test_filter_exact_match() {
        // Node with Oci runtime, workload needs Oci artifact.
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let unit = test_unit(); // default artifact is Oci

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps)], &[]);

        assert_eq!(result, vec![node_id]);
    }

    #[test]
    fn test_filter_no_match() {
        // Node without Oci runtime, workload needs Oci artifact.
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Wasm])
            .build();
        let unit = test_unit();

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps)], &[]);

        assert!(result.is_empty(), "node without Oci should be excluded");
    }

    #[test]
    fn test_filter_multiple_nodes() {
        // Two nodes: one with Oci, one with Wasm.
        let oci_node = test_node_id();
        let wasm_node = test_node_id();
        let oci_caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let wasm_caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Wasm])
            .build();

        // Sort nodes by ID to ensure deterministic filter output.
        let mut nodes = vec![(oci_node, oci_caps), (wasm_node, wasm_caps)];
        nodes.sort_by_key(|(id, _)| *id);

        let unit = test_unit();
        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &nodes, &[]);

        assert_eq!(result, vec![oci_node]);
    }

    #[test]
    fn test_filter_environment_tag() {
        // Node with env:prod, no promotion policy → excluded.
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_environment(Some("env:prod".to_string()))
            .build();
        let unit = test_unit();

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps.clone())], &[]);

        assert!(
            result.is_empty(),
            "prod node without promotion policy should be excluded"
        );

        // With a matching promotion policy → included.
        let promotion = PromotionPolicy {
            header: taba_core::UnitHeader {
                id: UnitId(Uuid::new_v4()),
                author: AuthorId(Uuid::new_v4()),
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
            unit_ref: unit.id(),
            version: "v1".to_string(),
            target_environment: "env:prod".to_string(),
            rationale: "approved for prod".to_string(),
        };

        let result = filter.filter(&unit, &[(node_id, caps)], &[promotion]);
        assert_eq!(result, vec![node_id]);
    }

    #[test]
    fn test_filter_env_dev_author_affinity() {
        let author = AuthorId(Uuid::new_v4());
        let node_id = test_node_id();

        // Node with env:dev and author affinity.
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_environment(Some("env:dev".to_string()))
            .with_author_affinity(author)
            .build();

        // Unit by the matching author → included.
        let unit_match = Unit::Workload(WorkloadUnitBuilder::new().with_author(author).build());
        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit_match, &[(node_id, caps.clone())], &[]);
        assert_eq!(result, vec![node_id]);

        // Unit by a different author → excluded.
        let other_author = AuthorId(Uuid::new_v4());
        let unit_other =
            Unit::Workload(WorkloadUnitBuilder::new().with_author(other_author).build());
        let result = filter.filter(&unit_other, &[(node_id, caps)], &[]);
        assert!(
            result.is_empty(),
            "dev node with affinity should exclude other authors"
        );
    }

    #[test]
    fn test_filter_env_dev_no_affinity() {
        // Node with env:dev but no author affinity (default) → any unit included.
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_environment(Some("env:dev".to_string()))
            .build();

        let unit = test_unit();
        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps)], &[]);
        assert_eq!(result, vec![node_id]);
    }

    #[test]
    fn test_filter_no_environment() {
        // Node with no environment tag → no environment restriction.
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_environment(None)
            .build();

        let unit = test_unit();
        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps)], &[]);
        assert_eq!(result, vec![node_id]);
    }

    #[test]
    fn test_filter_oci_rootless_satisfies_oci() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::OciRootless])
            .build();
        let unit = test_unit(); // Oci artifact

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps)], &[]);
        assert_eq!(
            result,
            vec![node_id],
            "OciRootless should satisfy Oci artifact"
        );
    }

    #[test]
    fn test_filter_requires_arch() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_arch("aarch64")
            .build();

        // Build a unit with requires=["aarch64"]
        let mut workload = WorkloadUnitBuilder::new().build();
        workload.artifact.requires = vec!["aarch64".to_string()];
        let unit = Unit::Workload(workload);

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps)], &[]);
        assert_eq!(result, vec![node_id]);

        // Now require "x86_64" which the node doesn't have.
        let mut workload2 = WorkloadUnitBuilder::new().build();
        workload2.artifact.requires = vec!["x86_64".to_string()];
        let unit2 = Unit::Workload(workload2);

        let caps_x86 = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_arch("aarch64")
            .build();
        let result2 = filter.filter(&unit2, &[(node_id, caps_x86)], &[]);
        assert!(result2.is_empty(), "arch mismatch should exclude node");
    }

    #[test]
    fn test_filter_requires_privileged() {
        let node_id = test_node_id();

        // Non-root node, privileged requirement → excluded.
        let caps_user = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_privilege(PrivilegeLevel::User)
            .build();

        let mut workload = WorkloadUnitBuilder::new().build();
        workload.artifact.requires = vec!["root".to_string()];
        let unit = Unit::Workload(workload);

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(node_id, caps_user)], &[]);
        assert!(
            result.is_empty(),
            "non-root node should be excluded for root requirement"
        );

        // Root node → included.
        let caps_root = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .with_privilege(PrivilegeLevel::Root)
            .build();
        let result = filter.filter(&unit, &[(node_id, caps_root)], &[]);
        assert_eq!(result, vec![node_id]);
    }

    #[test]
    fn test_filter_non_workload_all_nodes() {
        let node1 = test_node_id();
        let node2 = test_node_id();
        let caps1 = NodeCapabilitySetBuilder::new().build();
        let caps2 = NodeCapabilitySetBuilder::new().build();

        let mut nodes = vec![(node1, caps1), (node2, caps2)];
        nodes.sort_by_key(|(id, _)| *id);

        // Policy unit — no runtime requirement.
        let policy = taba_core::PolicyUnit {
            header: taba_core::UnitHeader {
                id: UnitId(Uuid::new_v4()),
                author: AuthorId(Uuid::new_v4()),
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
                unit_ids: std::collections::BTreeSet::from([UnitId(Uuid::new_v4())]),
                capability_name: "storage".to_string(),
            },
            resolution: taba_core::PolicyResolution::Allow,
            scope: taba_common::TrustDomainId(Uuid::new_v4()),
            rationale: "test".to_string(),
            supersedes: None,
            version: taba_common::Version(1),
            revoked: false,
        };

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&Unit::Policy(policy), &nodes, &[]);
        assert_eq!(
            result.len(),
            2,
            "non-workload should be eligible on all nodes"
        );
    }

    #[test]
    fn test_filter_sorted_output() {
        // Create nodes in reverse order; output should be sorted.
        let id_high = NodeId(Uuid::from_u128(999));
        let id_low = NodeId(Uuid::from_u128(1));

        let caps_high = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let caps_low = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();

        let unit = test_unit();
        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&unit, &[(id_high, caps_high), (id_low, caps_low)], &[]);

        assert_eq!(
            result,
            vec![id_low, id_high],
            "output should be sorted by NodeId"
        );
    }
    #[test]
    fn test_filter_microvm_match() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::MicroVm])
            .build();
        let mut unit = WorkloadUnitBuilder::new().build();
        unit.artifact.artifact_type = ArtifactType::MicroVm;
        unit.artifact.artifact_ref = "vmlinux-5.10".to_string();
        unit.artifact.kernel_ref = Some("/opt/vmlinux".to_string());
        unit.artifact.rootfs_ref = Some("/opt/rootfs.ext4".to_string());

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&Unit::Workload(unit), &[(node_id, caps)], &[]);

        assert!(
            result.contains(&node_id),
            "node with MicroVm runtime should be eligible for MicroVm workload"
        );
    }

    #[test]
    fn test_filter_microvm_no_match() {
        let node_id = test_node_id();
        let caps = NodeCapabilitySetBuilder::new()
            .with_runtimes(vec![RuntimeCapability::Oci])
            .build();
        let mut unit = WorkloadUnitBuilder::new().build();
        unit.artifact.artifact_type = ArtifactType::MicroVm;
        unit.artifact.artifact_ref = "vmlinux-5.10".to_string();

        let filter = DefaultCapabilityFilter::new();
        let result = filter.filter(&Unit::Workload(unit), &[(node_id, caps)], &[]);

        assert!(
            !result.contains(&node_id),
            "node without MicroVm runtime should not be eligible for MicroVm workload"
        );
    }
}
