//! Capability and resource advertisement via gossip (INV-N1, INV-N3).
//!
//! Nodes advertise capabilities (rarely — on change/refresh) and
//! resources (periodically). Both are stored in the membership view so
//! the solver can use them for hard filtering (capabilities) and soft
//! ranking (resources).
//!
//! For M4, advertisement stores the local node's capabilities in the
//! membership view and records the resource snapshot for later
//! piggyback. Full piggyback dissemination is exercised once gossip
//! runs in a multi-node test.

use std::sync::Mutex;

use taba_common::NodeId;
use taba_core::{NodeCapabilitySet, ResourceSnapshot};

use crate::error::GossipError;
use crate::membership::DefaultMembershipView;

// ---------------------------------------------------------------------------
// CapabilityAdvertiser trait
// ---------------------------------------------------------------------------

/// Capability and resource advertisement via gossip (INV-N1, INV-N3).
///
/// Nodes advertise capabilities (rarely, on change/refresh) and
/// resources (periodically). Both are included in the [`MembershipView`]
/// for solver use.
///
/// [`MembershipView`]: crate::membership::DefaultMembershipView
pub trait CapabilityAdvertiser {
    /// Advertise this node's capability set via gossip.
    ///
    /// Called on startup, on `taba refresh`, and on fleet refresh
    /// command. The capability set is stored in the membership view and
    /// piggybacked on subsequent gossip messages.
    async fn advertise_capabilities(
        &self,
        capabilities: &NodeCapabilitySet,
    ) -> Result<(), GossipError>;

    /// Advertise this node's resource snapshot via gossip.
    ///
    /// Called periodically (configurable interval). Includes logical
    /// clock for versioning (solver determinism per F-A306).
    async fn advertise_resources(&self, resources: &ResourceSnapshot) -> Result<(), GossipError>;
}

// ---------------------------------------------------------------------------
// DefaultCapabilityAdvertiser
// ---------------------------------------------------------------------------

/// Default implementation of [`CapabilityAdvertiser`].
///
/// Stores advertised capabilities and resources in the membership view
/// (via [`DefaultMembershipView::set_capabilities`]) and keeps the most
/// recent resource snapshot for piggyback on gossip messages.
///
/// # Concurrency
///
/// The latest resource snapshot is guarded by a `std::sync::Mutex`. The
/// lock is held only briefly during snapshot updates.
pub struct DefaultCapabilityAdvertiser {
    /// The membership view to update with advertised capabilities.
    view: std::sync::Arc<DefaultMembershipView>,
    /// The local node's ID.
    local_node: NodeId,
    /// Most recent advertised resource snapshot.
    latest_resource: Mutex<Option<ResourceSnapshot>>,
}

impl std::fmt::Debug for DefaultCapabilityAdvertiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultCapabilityAdvertiser")
            .field("local_node", &self.local_node)
            .finish_non_exhaustive()
    }
}

impl DefaultCapabilityAdvertiser {
    /// Creates a new advertiser for the local node, backed by the given
    /// membership view.
    #[must_use]
    pub const fn new(view: std::sync::Arc<DefaultMembershipView>, local_node: NodeId) -> Self {
        Self {
            view,
            local_node,
            latest_resource: Mutex::new(None),
        }
    }

    /// Returns the most recently advertised resource snapshot, if any.
    ///
    /// Used by the gossip loop to piggyback resource updates.
    #[must_use]
    pub fn latest_resource(&self) -> Option<ResourceSnapshot> {
        self.latest_resource
            .lock()
            .expect("resource lock poisoned")
            .clone()
    }
}

impl CapabilityAdvertiser for DefaultCapabilityAdvertiser {
    async fn advertise_capabilities(
        &self,
        capabilities: &NodeCapabilitySet,
    ) -> Result<(), GossipError> {
        self.view
            .set_capabilities(self.local_node, capabilities.clone());
        tracing::debug!(
            node = %self.local_node.0,
            "advertised capabilities via gossip"
        );
        Ok(())
    }

    async fn advertise_resources(&self, resources: &ResourceSnapshot) -> Result<(), GossipError> {
        if let Ok(mut guard) = self.latest_resource.lock() {
            *guard = Some(resources.clone());
        }
        tracing::debug!(
            node = %self.local_node.0,
            logical_clock = resources.logical_clock.0,
            "advertised resources via gossip"
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::LogicalClock;
    use taba_test_harness::NodeCapabilitySetBuilder;

    fn nid(n: u128) -> NodeId {
        NodeId(uuid::Uuid::from_u128(n))
    }

    #[tokio::test]
    async fn test_advertise_capabilities() {
        let view = std::sync::Arc::new(DefaultMembershipView::new(nid(1)));
        let advertiser = DefaultCapabilityAdvertiser::new(view.clone(), nid(1));

        let caps = NodeCapabilitySetBuilder::new()
            .with_arch("aarch64")
            .with_os("linux")
            .build();

        advertiser
            .advertise_capabilities(&caps)
            .await
            .expect("advertise should succeed");

        // The capability set is stored in the membership view.
        let stored = view
            .capabilities(&nid(1))
            .expect("capabilities should be stored");
        assert_eq!(stored, caps, "stored capabilities must match advertised");
    }

    #[tokio::test]
    async fn test_advertise_resources() {
        let view = std::sync::Arc::new(DefaultMembershipView::new(nid(1)));
        let advertiser = DefaultCapabilityAdvertiser::new(view.clone(), nid(1));

        let resource = ResourceSnapshot {
            node_id: nid(1),
            logical_clock: LogicalClock(7),
            memory_total_bytes: 16 * 1024 * 1024 * 1024,
            memory_available_bytes: 8 * 1024 * 1024 * 1024,
            cpu_cores: 8,
            cpu_load_ppm: taba_common::Ppm(250_000),
            disk_available_bytes: 100 * 1024 * 1024 * 1024,
            gpu_available: 2,
        };

        advertiser
            .advertise_resources(&resource)
            .await
            .expect("advertise should succeed");

        // The resource snapshot is stored in the advertiser for piggyback.
        let latest = advertiser
            .latest_resource()
            .expect("resource should be stored");
        assert_eq!(latest, resource, "stored resource must match advertised");
    }
}
