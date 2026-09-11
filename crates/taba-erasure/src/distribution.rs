//! Shard distribution across nodes and re-coding when fleet changes.
//!
//! The [`ShardManager`] trait handles the distributed aspects of
//! erasure coding: deciding which node holds which shard, triggering
//! re-coding when nodes join or leave, and fetching shards from
//! remote nodes for reconstruction.
//!
//! Governance shards are actively replicated (full copies on N nodes)
//! in addition to erasure coding (INV-R6).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use taba_common::NodeId;

use crate::coding::{DefaultErasureCoder, ErasureCoder};
use crate::error::{ErasureError, ShardGroupId};
use crate::params::ErasureParams;
use crate::shard::{Shard, ShardAssignment, ShardCriticality};

/// In-memory shard store: group → list of (shard, holding node).
type ShardStore = Arc<Mutex<HashMap<ShardGroupId, Vec<(Shard, NodeId)>>>>;

// ---------------------------------------------------------------------------
// ShardFetcher trait
// ---------------------------------------------------------------------------

/// Fetches shards from remote nodes for reconstruction.
///
/// This is an abstract trait over the transport layer. For M4, only
/// the in-memory [`InMemoryShardFetcher`] exists — real network
/// transfer is provided by taba-gossip in a later phase.
pub trait ShardFetcher: Send + Sync {
    /// Fetch at least `min_shards` shards for the given group.
    ///
    /// Returns the available shards (possibly more than `min_shards`).
    ///
    /// # Errors
    ///
    /// - [`ErasureError::InsufficientShards`] if fewer than `min_shards`
    ///   unique shards are available.
    async fn fetch(
        &self,
        group: &ShardGroupId,
        min_shards: u32,
    ) -> Result<Vec<Shard>, ErasureError>;
}

// ---------------------------------------------------------------------------
// InMemoryShardFetcher
// ---------------------------------------------------------------------------

/// In-memory shard fetcher for M4.
///
/// Retrieves shards from a shared `HashMap`. In production, this
/// would be replaced by a network-based fetcher using taba-gossip
/// for transport.
pub struct InMemoryShardFetcher {
    /// Shared shard store: group → list of (shard, holding node).
    store: ShardStore,
}

impl InMemoryShardFetcher {
    /// Creates a new in-memory fetcher backed by the given store.
    #[must_use]
    pub const fn new(store: ShardStore) -> Self {
        Self { store }
    }
}

impl ShardFetcher for InMemoryShardFetcher {
    async fn fetch(
        &self,
        group: &ShardGroupId,
        min_shards: u32,
    ) -> Result<Vec<Shard>, ErasureError> {
        let store = self
            .store
            .lock()
            .expect("shard store mutex should not be poisoned");

        let entries = match store.get(group) {
            Some(e) if !e.is_empty() => e,
            _ => {
                return Err(ErasureError::InsufficientShards {
                    need: min_shards,
                    have: 0,
                });
            }
        };

        // Deduplicate by shard index, keeping the first copy of each.
        let mut seen_indices: HashMap<u32, Shard> = HashMap::new();
        for (shard, _node) in entries {
            seen_indices
                .entry(shard.index)
                .or_insert_with(|| shard.clone());
        }

        let unique_count = u32::try_from(seen_indices.len()).unwrap_or(u32::MAX);
        if unique_count < min_shards {
            return Err(ErasureError::InsufficientShards {
                need: min_shards,
                have: unique_count,
            });
        }

        Ok(seen_indices.into_values().collect())
    }
}

// ---------------------------------------------------------------------------
// ShardManager trait
// ---------------------------------------------------------------------------

/// Manages shard distribution across nodes and re-coding when fleet
/// size changes.
///
/// Governance unit shards are fully replicated in addition to erasure
/// coding (INV-R6). Non-governance shards are distributed round-robin
/// across available nodes.
///
/// For M4, all state is in-memory. Shard persistence (disk) is
/// deferred to taba-node.
pub trait ShardManager: Send + Sync {
    /// Distribute shards for a newly encoded shard group across
    /// available nodes.
    ///
    /// Selects target nodes to maximize fault isolation. Governance
    /// shards are fully replicated to all available nodes (INV-R6).
    ///
    /// # Errors
    ///
    /// - [`ErasureError::NoAvailableNodes`] if there are no available
    ///   nodes.
    async fn distribute(
        &self,
        group: ShardGroupId,
        shards: Vec<Shard>,
    ) -> Result<Vec<ShardAssignment>, ErasureError>;

    /// Fetch shards from remote nodes for reconstruction.
    ///
    /// Retrieves at least `min_shards` shards for the given group.
    ///
    /// # Errors
    ///
    /// - [`ErasureError::InsufficientShards`] if too few shards are
    ///   available.
    /// - [`ErasureError::TransferError`] on fetch failure (deferred:
    ///   in-memory for M4).
    async fn fetch_shards(
        &self,
        group: &ShardGroupId,
        min_shards: u32,
    ) -> Result<Vec<Shard>, ErasureError>;

    /// Re-code a shard group with new erasure parameters.
    ///
    /// Triggered when fleet size changes significantly. Fetches
    /// existing shards, decodes the original data, re-encodes with
    /// the new parameters, and distributes the new shards.
    ///
    /// # Errors
    ///
    /// - [`ErasureError::RecodingFailed`] if reconstruction or
    ///   re-encoding fails.
    /// - [`ErasureError::InsufficientShards`] if not enough shards
    ///   survive for reconstruction.
    async fn recode(
        &self,
        group: &ShardGroupId,
        new_params: ErasureParams,
    ) -> Result<Vec<ShardAssignment>, ErasureError>;

    /// Get current shard assignments for a group.
    ///
    /// Returns an empty `Vec` if the group has no assignments.
    async fn assignments(&self, group: &ShardGroupId)
    -> Result<Vec<ShardAssignment>, ErasureError>;
}

// ---------------------------------------------------------------------------
// DefaultShardManager
// ---------------------------------------------------------------------------

/// Default in-memory implementation of [`ShardManager`].
///
/// Uses round-robin distribution for non-governance shards and full
/// replication for governance shards (INV-R6). An
/// [`InMemoryShardFetcher`] backs `fetch_shards`.
///
/// For M4, all state is in-memory and lost on restart.
pub struct DefaultShardManager {
    /// Nodes available for shard distribution.
    available_nodes: Vec<NodeId>,
    /// Current shard assignments: group → list of assignments.
    assignments: Mutex<HashMap<ShardGroupId, Vec<ShardAssignment>>>,
    /// In-memory shard store: group → list of (shard, holding node).
    store: ShardStore,
    /// Erasure coder for decode/re-encode during recoding.
    coder: DefaultErasureCoder,
}

impl DefaultShardManager {
    /// Creates a new `DefaultShardManager` with the given available
    /// nodes.
    ///
    /// At least one node must be provided for `distribute` to succeed.
    #[must_use]
    pub fn new(available_nodes: Vec<NodeId>) -> Self {
        Self {
            available_nodes,
            assignments: Mutex::new(HashMap::new()),
            store: Arc::new(Mutex::new(HashMap::new())),
            coder: DefaultErasureCoder::new(),
        }
    }

    /// Returns a reference to the in-memory shard store.
    ///
    /// Useful for sharing with an [`InMemoryShardFetcher`] in tests.
    #[must_use]
    pub fn shard_store(&self) -> ShardStore {
        Arc::clone(&self.store)
    }

    /// Returns the number of available nodes.
    #[must_use]
    pub fn available_node_count(&self) -> usize {
        self.available_nodes.len()
    }
}

impl ShardManager for DefaultShardManager {
    async fn distribute(
        &self,
        group: ShardGroupId,
        shards: Vec<Shard>,
    ) -> Result<Vec<ShardAssignment>, ErasureError> {
        if self.available_nodes.is_empty() {
            return Err(ErasureError::NoAvailableNodes);
        }

        let mut all_assignments = Vec::new();
        let mut store_entries: Vec<(Shard, NodeId)> = Vec::new();
        let node_count = self.available_nodes.len();

        for (i, shard) in shards.into_iter().enumerate() {
            let shard_index = shard.index;
            if shard.criticality == ShardCriticality::Governance {
                // Full replication: governance shards go to ALL nodes (INV-R6).
                for &node in &self.available_nodes {
                    let mut replica = shard.clone();
                    replica.held_by = node;
                    store_entries.push((replica, node));
                    all_assignments.push(ShardAssignment {
                        group,
                        shard_index,
                        node,
                    });
                }
            } else {
                // Round-robin assignment.
                let node = self.available_nodes[i % node_count];
                let mut assigned = shard;
                assigned.held_by = node;
                store_entries.push((assigned, node));
                all_assignments.push(ShardAssignment {
                    group,
                    shard_index,
                    node,
                });
            }
        }

        // Store the shards and assignments.
        {
            let mut store = self
                .store
                .lock()
                .expect("shard store mutex should not be poisoned");
            store.insert(group, store_entries);
        }
        {
            let mut assignments = self
                .assignments
                .lock()
                .expect("assignments mutex should not be poisoned");
            assignments.insert(group, all_assignments.clone());
        }

        Ok(all_assignments)
    }

    async fn fetch_shards(
        &self,
        group: &ShardGroupId,
        min_shards: u32,
    ) -> Result<Vec<Shard>, ErasureError> {
        let fetcher = InMemoryShardFetcher::new(Arc::clone(&self.store));
        fetcher.fetch(group, min_shards).await
    }

    async fn recode(
        &self,
        group: &ShardGroupId,
        new_params: ErasureParams,
    ) -> Result<Vec<ShardAssignment>, ErasureError> {
        // Fetch existing shards. We need at least k (old data_shards)
        // for reconstruction.
        let existing_shards = {
            let store = self
                .store
                .lock()
                .expect("shard store mutex should not be poisoned");
            match store.get(group) {
                Some(entries) if !entries.is_empty() => {
                    // Deduplicate by shard index.
                    let mut seen: HashMap<u32, Shard> = HashMap::new();
                    for (shard, _node) in entries {
                        seen.entry(shard.index).or_insert_with(|| shard.clone());
                    }
                    seen.into_values().collect::<Vec<_>>()
                }
                _ => {
                    return Err(ErasureError::RecodingFailed {
                        reason: format!("no existing shards for group {group}"),
                    });
                }
            }
        };

        if existing_shards.is_empty() {
            return Err(ErasureError::RecodingFailed {
                reason: format!("no existing shards for group {group}"),
            });
        }

        // Get old params from the first existing shard.
        let old_params = existing_shards[0].params;

        // Check we have enough shards for reconstruction.
        let present_count = u32::try_from(existing_shards.len()).unwrap_or(u32::MAX);
        if present_count < old_params.data_shards {
            return Err(ErasureError::InsufficientShards {
                need: old_params.data_shards,
                have: present_count,
            });
        }

        // Decode the original data.
        let original_data = self
            .coder
            .decode(&existing_shards, old_params)
            .map_err(|e| ErasureError::RecodingFailed {
                reason: format!("decode failed during recode: {e}"),
            })?;

        // Preserve metadata from old shards.
        let covers_units = existing_shards
            .iter()
            .flat_map(|s| s.covers_units.iter().copied())
            .collect::<std::collections::BTreeSet<_>>();
        let max_criticality = existing_shards
            .iter()
            .map(|s| s.criticality)
            .max()
            .unwrap_or(ShardCriticality::Workload);

        // Encode with new parameters.
        let mut new_shards = self.coder.encode(&original_data, new_params).map_err(|e| {
            ErasureError::RecodingFailed {
                reason: format!("encode failed during recode: {e}"),
            }
        })?;

        // Apply preserved metadata.
        for shard in &mut new_shards {
            shard.covers_units.clone_from(&covers_units);
            shard.criticality = max_criticality;
        }

        // Distribute the new shards (overwrites old store and assignments).
        self.distribute(*group, new_shards).await
    }

    async fn assignments(
        &self,
        group: &ShardGroupId,
    ) -> Result<Vec<ShardAssignment>, ErasureError> {
        let assignments = self
            .assignments
            .lock()
            .expect("assignments mutex should not be poisoned");
        Ok(assignments.get(group).cloned().unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding::ErasureCoder;
    use uuid::Uuid;

    fn test_params() -> ErasureParams {
        ErasureParams {
            total_shards: 3,
            data_shards: 2,
            parity_shards: 1,
            resilience_pct: 33,
        }
    }

    fn test_nodes(count: usize) -> Vec<NodeId> {
        (0..count).map(|_| NodeId(Uuid::new_v4())).collect()
    }

    fn encode_shards(
        coder: &DefaultErasureCoder,
        data: &[u8],
        params: ErasureParams,
    ) -> Vec<Shard> {
        coder.encode(data, params).expect("encode should succeed")
    }

    #[tokio::test]
    async fn test_distribute_shards() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes.clone());
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let shards = encode_shards(&coder, b"hello world", params);
        let group = ShardGroupId::new();

        let assignments = manager.distribute(group, shards).await.expect("distribute");

        assert_eq!(assignments.len(), 3, "3 shards → 3 assignments");
        // Each node should appear at least once (round-robin with 3 shards, 3 nodes).
        let assigned_nodes: std::collections::HashSet<NodeId> =
            assignments.iter().map(|a| a.node).collect();
        assert_eq!(
            assigned_nodes.len(),
            3,
            "each node should get at least 1 shard"
        );
    }

    #[tokio::test]
    async fn test_distribute_more_shards_than_nodes() {
        let nodes = test_nodes(2);
        let manager = DefaultShardManager::new(nodes);
        let coder = DefaultErasureCoder::new();
        let params = ErasureParams {
            total_shards: 5,
            data_shards: 3,
            parity_shards: 2,
            resilience_pct: 40,
        };
        let shards = encode_shards(&coder, b"more shards than nodes", params);
        let group = ShardGroupId::new();

        let assignments = manager.distribute(group, shards).await.expect("distribute");

        assert_eq!(assignments.len(), 5, "5 shards → 5 assignments");
        // With round-robin, some nodes get >1 shard.
        let mut node_counts: HashMap<NodeId, usize> = HashMap::new();
        for a in &assignments {
            *node_counts.entry(a.node).or_default() += 1;
        }
        assert!(
            node_counts.values().any(|&c| c > 1),
            "with 5 shards and 2 nodes, at least one node should have >1 shard"
        );
    }

    #[tokio::test]
    async fn test_distribute_governance_replicated() {
        let nodes = test_nodes(4);
        let manager = DefaultShardManager::new(nodes.clone());
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let mut shards = encode_shards(&coder, b"governance data", params);

        // Mark all shards as governance criticality.
        for shard in &mut shards {
            shard.criticality = ShardCriticality::Governance;
        }

        let group = ShardGroupId::new();
        let assignments = manager.distribute(group, shards).await.expect("distribute");

        // 3 shards × 4 nodes = 12 assignments (full replication).
        assert_eq!(
            assignments.len(),
            12,
            "governance shards should be fully replicated to all nodes"
        );

        // Each shard index should appear once per node.
        for &node in &nodes {
            let count = assignments.iter().filter(|a| a.node == node).count();
            assert_eq!(count, 3, "each node should have all 3 governance shards");
        }
    }

    #[tokio::test]
    async fn test_distribute_no_available_nodes() {
        let manager = DefaultShardManager::new(vec![]);
        let coder = DefaultErasureCoder::new();
        let shards = encode_shards(&coder, b"data", test_params());
        let group = ShardGroupId::new();

        let result = manager.distribute(group, shards).await;
        assert!(matches!(result, Err(ErasureError::NoAvailableNodes)));
    }

    #[tokio::test]
    async fn test_fetch_shards_sufficient() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes);
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let shards = encode_shards(&coder, b"fetch me", params);
        let group = ShardGroupId::new();

        manager.distribute(group, shards).await.expect("distribute");

        let fetched = manager
            .fetch_shards(&group, 2)
            .await
            .expect("fetch with min_shards=2 should succeed");

        assert!(fetched.len() >= 2, "should fetch at least 2 shards");
    }

    #[tokio::test]
    async fn test_fetch_shards_insufficient() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes);
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let shards = encode_shards(&coder, b"not enough", params);
        let group = ShardGroupId::new();

        manager.distribute(group, shards).await.expect("distribute");

        // Request more shards than available (3 available, request 4).
        let result = manager.fetch_shards(&group, 4).await;
        assert!(matches!(
            result,
            Err(ErasureError::InsufficientShards { need: 4, have: 3 })
        ));
    }

    #[tokio::test]
    async fn test_fetch_shards_nonexistent_group() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes);
        let group = ShardGroupId::new();

        let result = manager.fetch_shards(&group, 1).await;
        assert!(matches!(
            result,
            Err(ErasureError::InsufficientShards { need: 1, have: 0 })
        ));
    }

    #[tokio::test]
    async fn test_recode() {
        let nodes = test_nodes(5);
        let manager = DefaultShardManager::new(nodes);
        let coder = DefaultErasureCoder::new();
        let old_params = test_params(); // k=2, m=1, n=3
        let data = b"recodable data for testing";
        let shards = encode_shards(&coder, data, old_params);
        let group = ShardGroupId::new();

        manager.distribute(group, shards).await.expect("distribute");

        // Recode with new params: k=3, m=2, n=5
        let new_params = ErasureParams {
            total_shards: 5,
            data_shards: 3,
            parity_shards: 2,
            resilience_pct: 40,
        };

        let new_assignments = manager
            .recode(&group, new_params)
            .await
            .expect("recode should succeed");

        assert_eq!(new_assignments.len(), 5, "should produce 5 new assignments");

        // Verify we can decode from the new shards.
        let fetched = manager
            .fetch_shards(&group, 3)
            .await
            .expect("fetch after recode");

        let decoded = coder
            .decode(&fetched, new_params)
            .expect("decode after recode should succeed");

        assert_eq!(
            decoded, data,
            "recoded shards should reconstruct original data"
        );
    }

    #[tokio::test]
    async fn test_recode_nonexistent_group() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes);
        let group = ShardGroupId::new();
        let new_params = test_params();

        let result = manager.recode(&group, new_params).await;
        assert!(matches!(result, Err(ErasureError::RecodingFailed { .. })));
    }

    #[tokio::test]
    async fn test_assignments() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes);
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let shards = encode_shards(&coder, b"assignment test", params);
        let group = ShardGroupId::new();

        manager.distribute(group, shards).await.expect("distribute");

        let result = manager.assignments(&group).await.expect("assignments");
        assert_eq!(result.len(), 3, "should return 3 assignments");

        // Verify assignments have the correct group.
        for a in &result {
            assert_eq!(a.group, group);
        }

        // Verify shard indices are 0, 1, 2.
        let mut indices: Vec<u32> = result.iter().map(|a| a.shard_index).collect();
        indices.sort_unstable();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn test_assignments_nonexistent_group() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes);
        let group = ShardGroupId::new();

        let result = manager.assignments(&group).await.expect("assignments");
        assert!(
            result.is_empty(),
            "nonexistent group should return empty assignments"
        );
    }

    #[tokio::test]
    async fn test_distribute_preserves_held_by() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes.clone());
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let shards = encode_shards(&coder, b"held by test", params);
        let group = ShardGroupId::new();

        let assignments = manager.distribute(group, shards).await.expect("distribute");

        // After distribution, each assignment's node should match a
        // shard's held_by in the store.
        let store = manager
            .store
            .lock()
            .expect("store mutex should not be poisoned");
        let entries = store.get(&group).expect("group should be in store");
        for a in &assignments {
            let found = entries
                .iter()
                .any(|(s, n)| s.index == a.shard_index && *n == a.node);
            assert!(found, "assignment {a:?} should have matching store entry");
        }
    }

    #[tokio::test]
    async fn test_governance_shard_full_replication_in_store() {
        let nodes = test_nodes(3);
        let manager = DefaultShardManager::new(nodes.clone());
        let coder = DefaultErasureCoder::new();
        let params = test_params();
        let mut shards = encode_shards(&coder, b"gov data", params);
        for shard in &mut shards {
            shard.criticality = ShardCriticality::Governance;
        }
        let group = ShardGroupId::new();

        manager.distribute(group, shards).await.expect("distribute");

        let store = manager.store.lock().expect("store mutex");
        let entries = store.get(&group).expect("group in store");
        // 3 shards × 3 nodes = 9 entries (full replication).
        assert_eq!(
            entries.len(),
            9,
            "governance shards should have 3×3=9 store entries"
        );

        // Verify each node has a copy of each shard index.
        for &node in &nodes {
            for shard_index in 0..3u32 {
                let found = entries
                    .iter()
                    .any(|(s, n)| s.index == shard_index && *n == node);
                assert!(found, "node {node:?} should have shard index {shard_index}");
            }
        }
    }
}
