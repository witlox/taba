//! Membership snapshot: the set of nodes available for placement.
//!
//! For M2 (single-node), the [`MembershipSnapshot`] contains just the
//! local node. This type replaces the opaque placeholder defined in
//! `taba-core::store`. The solver reads from this snapshot to
//! determine which nodes can host workloads and what their health
//! status is.
//!
//! ## Determinism
//!
//! The membership snapshot is an input to the solver. Two nodes with
//! identical snapshots produce identical placement decisions (INV-C3).
//! The `generation` counter allows staleness detection.

use serde::{Deserialize, Serialize};

use taba_common::NodeId;
use taba_core::NodeCapabilitySet;

// ===========================================================================
// NodeHealth
// ===========================================================================

/// Health status of a node as observed by the membership protocol.
///
/// Suspected nodes remain in the placement pool but receive a
/// scoring penalty (INV-R5). The solver avoids suspected nodes when
/// alternatives exist but does not remove them until SWIM confirms
/// failure via multi-probe consensus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeHealth {
    /// Node is healthy and actively serving. No scoring penalty.
    Active,
    /// Node is suspected of being unhealthy. Receives a scoring
    /// penalty but is not removed from the placement pool.
    Suspected,
}

// ===========================================================================
// MembershipSnapshot
// ===========================================================================

/// Immutable point-in-time view of cluster membership.
///
/// Contains the set of nodes available for placement, each with its
/// capability set and health status. The `generation` counter is
/// incremented whenever membership changes, allowing staleness
/// detection.
///
/// For M2 (single-node), use [`MembershipSnapshot::single_node`] to
/// create a snapshot containing just the local node.
///
/// # Ordering
///
/// The `nodes` vector is kept in a deterministic order (sorted by
/// `NodeId`) to ensure identical snapshots produce identical solver
/// results across nodes (INV-C3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipSnapshot {
    /// All nodes in the cluster at snapshot time, each with its
    /// capability set and health status. Sorted by `NodeId` for
    /// deterministic iteration.
    pub nodes: Vec<(NodeId, NodeCapabilitySet, NodeHealth)>,
    /// Monotonically increasing generation counter. Incremented on
    /// every membership change (join, leave, health update).
    pub generation: u64,
}

impl MembershipSnapshot {
    /// Creates an empty membership snapshot with the given generation.
    ///
    /// Useful for testing or representing a cluster with no nodes.
    #[must_use]
    pub const fn empty(generation: u64) -> Self {
        Self {
            nodes: Vec::new(),
            generation,
        }
    }

    /// Creates a single-node membership snapshot for M2.
    ///
    /// The local node is added with `Active` health. This is the
    /// typical starting point for single-node composition: the
    /// solver has exactly one node to place workloads on.
    ///
    /// # Example
    ///
    /// ```
    /// use taba_solver::MembershipSnapshot;
    /// use taba_common::NodeId;
    /// use taba_core::NodeCapabilitySet;
    /// use taba_test_harness::NodeCapabilitySetBuilder;
    ///
    /// let node_id = NodeId(uuid::Uuid::new_v4());
    /// let caps = NodeCapabilitySetBuilder::new().build();
    /// let snapshot = MembershipSnapshot::single_node(node_id, caps);
    /// assert_eq!(snapshot.nodes.len(), 1);
    /// assert_eq!(snapshot.generation, 1);
    /// ```
    #[must_use]
    pub fn single_node(node_id: NodeId, caps: NodeCapabilitySet) -> Self {
        Self {
            nodes: vec![(node_id, caps, NodeHealth::Active)],
            generation: 1,
        }
    }

    /// Returns the number of nodes in this snapshot.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if this snapshot contains no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Finds a node by ID, returning its capability set and health.
    #[must_use]
    pub fn get(&self, id: &NodeId) -> Option<&(NodeId, NodeCapabilitySet, NodeHealth)> {
        self.nodes.iter().find(|(nid, _, _)| nid == id)
    }

    /// Returns `true` if the given node is in this snapshot and is
    /// `Active`. Suspected nodes return `false`.
    #[must_use]
    pub fn is_active(&self, id: &NodeId) -> bool {
        self.nodes
            .iter()
            .any(|(nid, _, health)| nid == id && *health == NodeHealth::Active)
    }

    /// Returns an iterator over all node IDs in this snapshot.
    ///
    /// The order is the same as the internal `nodes` vector.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.nodes.iter().map(|(id, _, _)| *id)
    }

    /// Adds a node to this snapshot, keeping the nodes sorted by ID.
    ///
    /// If a node with the same ID already exists, it is replaced.
    /// The generation is not incremented — the caller is responsible
    /// for generation management.
    pub fn add_node(&mut self, node_id: NodeId, caps: NodeCapabilitySet, health: NodeHealth) {
        // Remove existing entry for this node ID if present.
        self.nodes.retain(|(nid, _, _)| nid != &node_id);
        self.nodes.push((node_id, caps, health));
        self.nodes.sort_by_key(|a| a.0);
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_test_harness::NodeCapabilitySetBuilder;

    fn test_caps() -> NodeCapabilitySet {
        NodeCapabilitySetBuilder::new().build()
    }

    #[test]
    fn test_node_health_variants() {
        assert_ne!(NodeHealth::Active, NodeHealth::Suspected);

        let json = serde_json::to_string(&NodeHealth::Active).expect("serialize");
        let decoded: NodeHealth = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(NodeHealth::Active, decoded);
    }

    #[test]
    fn test_membership_snapshot_empty() {
        let snapshot = MembershipSnapshot::empty(0);
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.len(), 0);
        assert_eq!(snapshot.generation, 0);
    }

    #[test]
    fn test_membership_snapshot_single_node() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let caps = test_caps();
        let snapshot = MembershipSnapshot::single_node(node_id, caps.clone());

        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.generation, 1);
        assert!(!snapshot.is_empty());

        let (id, node_caps, health) = &snapshot.nodes[0];
        assert_eq!(*id, node_id);
        assert_eq!(*node_caps, caps);
        assert_eq!(*health, NodeHealth::Active);
    }

    #[test]
    fn test_membership_snapshot_get() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let snapshot = MembershipSnapshot::single_node(node_id, test_caps());

        assert!(snapshot.get(&node_id).is_some());
        let other = NodeId(uuid::Uuid::new_v4());
        assert!(snapshot.get(&other).is_none());
    }

    #[test]
    fn test_membership_snapshot_is_active() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let snapshot = MembershipSnapshot::single_node(node_id, test_caps());
        assert!(snapshot.is_active(&node_id));

        let mut snapshot = snapshot;
        snapshot.add_node(node_id, test_caps(), NodeHealth::Suspected);
        assert!(!snapshot.is_active(&node_id));
    }

    #[test]
    fn test_membership_snapshot_node_ids() {
        let id1 = NodeId(uuid::Uuid::from_u128(1));
        let id2 = NodeId(uuid::Uuid::from_u128(2));
        let id3 = NodeId(uuid::Uuid::from_u128(3));

        let mut snapshot = MembershipSnapshot::empty(1);
        snapshot.add_node(id3, test_caps(), NodeHealth::Active);
        snapshot.add_node(id1, test_caps(), NodeHealth::Active);
        snapshot.add_node(id2, test_caps(), NodeHealth::Active);

        let ids: Vec<_> = snapshot.node_ids().collect();
        // Should be sorted by NodeId (add_node sorts).
        assert_eq!(ids, vec![id1, id2, id3]);
    }

    #[test]
    fn test_membership_snapshot_add_node_replaces() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let mut snapshot = MembershipSnapshot::single_node(node_id, test_caps());
        assert_eq!(snapshot.len(), 1);
        assert!(snapshot.is_active(&node_id));

        // Replace with Suspected health.
        snapshot.add_node(node_id, test_caps(), NodeHealth::Suspected);
        assert_eq!(snapshot.len(), 1, "replacing should not add a duplicate");
        assert!(!snapshot.is_active(&node_id));
    }

    #[test]
    fn test_membership_snapshot_serialization_roundtrip() {
        let snapshot = MembershipSnapshot::single_node(NodeId(uuid::Uuid::new_v4()), test_caps());
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let decoded: MembershipSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn test_membership_snapshot_multiple_nodes() {
        let id1 = NodeId(uuid::Uuid::from_u128(100));
        let id2 = NodeId(uuid::Uuid::from_u128(200));

        let mut snapshot = MembershipSnapshot::empty(1);
        snapshot.add_node(id1, test_caps(), NodeHealth::Active);
        snapshot.add_node(id2, test_caps(), NodeHealth::Suspected);

        assert_eq!(snapshot.len(), 2);
        assert!(snapshot.is_active(&id1));
        assert!(!snapshot.is_active(&id2));
    }
}
