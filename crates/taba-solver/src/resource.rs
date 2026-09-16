//! Resource ranking: soft constraints for best-fit placement (INV-N3).
//!
//! The [`ResourceRanker`] trait ranks eligible nodes by dynamic
//! resource availability. Unlike capabilities (hard constraints),
//! resources change constantly and are used for ranking, not
//! filtering. All arithmetic is fixed-point [`Ppm`] for
//! determinism (INV-C3, F-A306).
//!
//! Resource ranking runs AFTER capability filtering — only nodes
//! that passed hard constraints are ranked.

use taba_common::{NodeId, Ppm};
use taba_core::{ResourceSnapshot, Unit};

// ===========================================================================
// ResourceRanker trait
// ===========================================================================

/// Ranks nodes by dynamic resource availability (soft constraints,
/// INV-N3).
///
/// Uses versioned resource snapshots for determinism (F-A306). Same
/// snapshot version → same ranking on all nodes. Higher [`Ppm`] =
/// better fit. Ties broken by lexicographic [`NodeId`].
///
/// The ranker is a pure function: no I/O, no side effects,
/// deterministic. It operates only on the `eligible_nodes` and
/// `resources` slices provided — it does not access the graph or
/// membership directly.
pub trait ResourceRanker {
    /// Rank eligible nodes by resource fit for a unit.
    ///
    /// Returns a `Vec<(NodeId, Ppm)>` sorted by score descending.
    /// Ties broken by lexicographic `NodeId` (INV-C3). Nodes without
    /// a resource snapshot receive a zero score (ranked last).
    ///
    /// # Parameters
    ///
    /// - `unit`: The unit being placed (used for resource tolerance
    ///   declarations in future milestones; currently unused).
    /// - `eligible_nodes`: Node IDs that passed capability filtering.
    /// - `resources`: Resource snapshots, one per node. Only entries
    ///   whose `node_id` is in `eligible_nodes` are ranked.
    #[must_use]
    fn rank(
        &self,
        unit: &Unit,
        eligible_nodes: &[NodeId],
        resources: &[(NodeId, ResourceSnapshot)],
    ) -> Vec<(NodeId, Ppm)>;
}

// ===========================================================================
// DefaultResourceRanker
// ===========================================================================

/// Default, stateless implementation of [`ResourceRanker`].
///
/// Scoring model (all values in ppm, scale 10^6):
///
/// | Factor | Formula |
/// |--------|---------|
/// | Memory | `available / total * 1_000_000` |
/// | CPU | `1_000_000 - cpu_load_ppm` |
/// | GPU | `gpu_available * 100_000` |
///
/// The total is the sum of all three factors. Higher total = better
/// fit. Ties are broken by lexicographic `NodeId` (INV-C3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultResourceRanker;

impl DefaultResourceRanker {
    /// Creates a new default resource ranker.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Computes a memory availability score based on absolute
    /// available memory (INV-N3: best-fit ranking).
    ///
    /// A node with more available memory ranks higher, regardless of
    /// total memory. The score is `available_gib * 50_000` ppm,
    /// capped at `Ppm::ONE` (1,000,000). A node with 20 GiB or more
    /// available scores the maximum.
    ///
    /// Returns `Ppm(0)` if `available` is zero (no memory available).
    fn memory_score(rs: &ResourceSnapshot) -> Ppm {
        if rs.memory_available_bytes == 0 {
            return Ppm(0);
        }
        let gib: u128 = 1024 * 1024 * 1024;
        let avail = u128::from(rs.memory_available_bytes);
        let score = (avail / gib) * 50_000;
        Ppm(u64::try_from(score.min(1_000_000)).unwrap_or(u64::MAX))
    }

    /// Computes a CPU availability score — inverse of CPU load.
    ///
    /// A fully idle CPU (load = 0) scores `Ppm::ONE` (`1_000_000`).
    /// A fully loaded CPU (load = `1_000_000`) scores `Ppm(0)`.
    fn cpu_score(rs: &ResourceSnapshot) -> Ppm {
        Ppm::ONE - rs.cpu_load_ppm
    }

    /// Computes a GPU availability score.
    ///
    /// Each available GPU adds `Ppm(100_000)` to the score.
    /// The result saturates at `u64::MAX`.
    fn gpu_score(rs: &ResourceSnapshot) -> Ppm {
        Ppm(u64::from(rs.gpu_available).saturating_mul(100_000))
    }

    /// Computes the total resource score for a single snapshot.
    fn total_score(rs: &ResourceSnapshot) -> Ppm {
        Self::memory_score(rs) + Self::cpu_score(rs) + Self::gpu_score(rs)
    }
}

impl ResourceRanker for DefaultResourceRanker {
    fn rank(
        &self,
        _unit: &Unit,
        eligible_nodes: &[NodeId],
        resources: &[(NodeId, ResourceSnapshot)],
    ) -> Vec<(NodeId, Ppm)> {
        let eligible_set: std::collections::BTreeSet<NodeId> =
            eligible_nodes.iter().copied().collect();

        let mut scored: Vec<(NodeId, Ppm)> = eligible_set
            .iter()
            .map(|&node_id| {
                let score = resources
                    .iter()
                    .find(|(rid, _)| rid == &node_id)
                    .map_or(Ppm(0), |(_, rs)| Self::total_score(rs));
                (node_id, score)
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
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn resource_snapshot(
        node_id: NodeId,
        memory_total: u64,
        memory_available: u64,
        cpu_load: Ppm,
        gpu_available: u32,
    ) -> ResourceSnapshot {
        ResourceSnapshot {
            node_id,
            logical_clock: taba_common::LogicalClock(1),
            memory_total_bytes: memory_total,
            memory_available_bytes: memory_available,
            cpu_cores: 4,
            cpu_load_ppm: cpu_load,
            disk_available_bytes: 100 * 1024 * 1024 * 1024,
            gpu_available,
        }
    }

    fn test_unit() -> taba_core::Unit {
        Unit::Workload(taba_test_harness::WorkloadUnitBuilder::new().build())
    }

    #[test]
    fn test_rank_higher_resources_better() {
        let node_rich = test_node_id();
        let node_poor = test_node_id();

        let rs_rich = resource_snapshot(
            node_rich,
            16 * 1024 * 1024 * 1024,
            12 * 1024 * 1024 * 1024,
            Ppm(100_000),
            2,
        );
        let rs_poor = resource_snapshot(
            node_poor,
            8 * 1024 * 1024 * 1024,
            2 * 1024 * 1024 * 1024,
            Ppm(800_000),
            0,
        );

        let eligible = vec![node_rich, node_poor];
        let resources = vec![(node_rich, rs_rich), (node_poor, rs_poor)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(
            ranked[0].0, node_rich,
            "node with more resources should rank first"
        );
        assert!(ranked[0].1 > ranked[1].1);
    }

    #[test]
    fn test_rank_tiebreak() {
        // Two nodes with identical resources.
        let id_low = NodeId(Uuid::from_u128(1));
        let id_high = NodeId(Uuid::from_u128(2));

        let rs_low = resource_snapshot(
            id_low,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(500_000),
            1,
        );
        let rs_high = resource_snapshot(
            id_high,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(500_000),
            1,
        );

        let eligible = vec![id_high, id_low];
        let resources = vec![(id_low, rs_low), (id_high, rs_high)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(ranked[0].1, ranked[1].1, "scores should be equal");
        assert_eq!(
            ranked[0].0, id_low,
            "lower NodeId should come first (INV-C3)"
        );
        assert_eq!(ranked[1].0, id_high);
    }

    #[test]
    fn test_rank_empty() {
        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &[], &[]);
        assert!(ranked.is_empty());
    }

    #[test]
    fn test_rank_node_without_snapshot() {
        let node_with = test_node_id();
        let node_without = test_node_id();

        let rs = resource_snapshot(
            node_with,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(500_000),
            1,
        );

        let eligible = vec![node_with, node_without];
        let resources = vec![(node_with, rs)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked[0].0, node_with,
            "node with snapshot should rank first"
        );
        assert!(
            ranked[0].1 > ranked[1].1,
            "node without snapshot should have lower score"
        );
        assert_eq!(ranked[1].1, Ppm(0), "missing snapshot should score zero");
    }

    #[test]
    fn test_rank_cpu_load_inverse() {
        let node_idle = test_node_id();
        let node_busy = test_node_id();

        let rs_idle = resource_snapshot(
            node_idle,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(0),
            0,
        );
        let rs_busy = resource_snapshot(
            node_busy,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(900_000),
            0,
        );

        let eligible = vec![node_idle, node_busy];
        let resources = vec![(node_idle, rs_idle), (node_busy, rs_busy)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(ranked[0].0, node_idle, "idle CPU should rank higher");
    }

    #[test]
    fn test_rank_gpu_available() {
        let node_with_gpu = test_node_id();
        let node_no_gpu = test_node_id();

        let rs_gpu = resource_snapshot(
            node_with_gpu,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(500_000),
            4,
        );
        let rs_no_gpu = resource_snapshot(
            node_no_gpu,
            8 * 1024 * 1024 * 1024,
            4 * 1024 * 1024 * 1024,
            Ppm(500_000),
            0,
        );

        let eligible = vec![node_with_gpu, node_no_gpu];
        let resources = vec![(node_with_gpu, rs_gpu), (node_no_gpu, rs_no_gpu)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(
            ranked[0].0, node_with_gpu,
            "node with GPUs should rank higher"
        );
    }

    #[test]
    fn test_rank_sorted_descending() {
        let nodes: Vec<_> = (0u32..5)
            .map(|i| NodeId(Uuid::from_u128(u128::from(i))))
            .collect();
        let resources: Vec<_> = nodes
            .iter()
            .enumerate()
            .map(|(i, &node_id)| {
                // Higher index = more resources = higher score.
                let avail = u64::from(u32::try_from(i + 1).unwrap_or(1)) * 1024 * 1024 * 1024;
                (
                    node_id,
                    resource_snapshot(node_id, 8 * 1024 * 1024 * 1024, avail, Ppm(500_000), 0),
                )
            })
            .collect();

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &nodes, &resources);

        // Should be sorted by score descending.
        for i in 0..ranked.len() - 1 {
            assert!(
                ranked[i].1 >= ranked[i + 1].1,
                "scores should be non-increasing at index {i}: {} >= {}",
                ranked[i].1.0,
                ranked[i + 1].1.0
            );
        }
    }

    #[test]
    fn test_memory_score_zero_total() {
        let rs = ResourceSnapshot {
            node_id: test_node_id(),
            logical_clock: taba_common::LogicalClock(1),
            memory_total_bytes: 0,
            memory_available_bytes: 0,
            cpu_cores: 4,
            cpu_load_ppm: Ppm(500_000),
            disk_available_bytes: 0,
            gpu_available: 0,
        };
        let score = DefaultResourceRanker::memory_score(&rs);
        assert_eq!(score, Ppm(0), "zero total memory should score zero");
    }

    // -- INV-N3: absolute available memory ranking -------------------------

    #[test]
    fn scenario_16gb_vs_8gb_available_ranks_higher() {
        // INV-N3: A node with more available resources should rank
        // higher (best-fit). Both nodes have the same CPU load and
        // GPU count; the only difference is available memory.
        let node_16gb = test_node_id();
        let node_8gb = test_node_id();

        let rs_16gb = resource_snapshot(
            node_16gb,
            32 * 1024 * 1024 * 1024, // 32 GiB total
            16 * 1024 * 1024 * 1024, // 16 GiB available
            Ppm(500_000),            // 50% CPU load
            0,                       // no GPUs
        );
        let rs_8gb = resource_snapshot(
            node_8gb,
            32 * 1024 * 1024 * 1024, // 32 GiB total
            8 * 1024 * 1024 * 1024,  // 8 GiB available
            Ppm(500_000),            // 50% CPU load (same)
            0,                       // no GPUs (same)
        );

        let eligible = vec![node_16gb, node_8gb];
        let resources = vec![(node_16gb, rs_16gb), (node_8gb, rs_8gb)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(
            ranked[0].0, node_16gb,
            "node with 16 GiB available should rank higher than 8 GiB (INV-N3)"
        );
        assert!(
            ranked[0].1 > ranked[1].1,
            "16 GiB score ({:?}) should be higher than 8 GiB score ({:?})",
            ranked[0].1,
            ranked[1].1
        );
    }

    #[test]
    fn scenario_no_resources_ranks_lowest() {
        // INV-N3: A node with no available resources should rank
        // lowest among all candidates.
        let node_rich = test_node_id();
        let node_empty = test_node_id();

        let rs_rich = resource_snapshot(
            node_rich,
            16 * 1024 * 1024 * 1024, // 16 GiB total
            12 * 1024 * 1024 * 1024, // 12 GiB available
            Ppm(100_000),            // 10% CPU load
            2,                       // 2 GPUs
        );
        let rs_empty = resource_snapshot(
            node_empty,
            0,              // no total memory
            0,              // no available memory
            Ppm(1_000_000), // 100% CPU load (fully busy)
            0,              // no GPUs
        );

        let eligible = vec![node_rich, node_empty];
        let resources = vec![(node_rich, rs_rich), (node_empty, rs_empty)];

        let ranker = DefaultResourceRanker::new();
        let ranked = ranker.rank(&test_unit(), &eligible, &resources);

        assert_eq!(
            ranked[0].0, node_rich,
            "node with resources should rank first"
        );
        assert_eq!(
            ranked[1].0, node_empty,
            "node with no resources should rank last (INV-N3)"
        );
        assert_eq!(
            ranked[1].1,
            Ppm(0),
            "node with no resources should have score zero (INV-N3)"
        );
    }
}
