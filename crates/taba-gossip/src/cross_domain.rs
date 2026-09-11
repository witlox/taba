//! Cross-domain forwarding protocol (INV-X1 through INV-X6).
//!
//! Bridge nodes relay capability advertisements and forwarding queries
//! across trust-domain boundaries. Bilateral policy is verified before
//! query execution (fail closed, INV-X1).
//!
//! **M4 scope**: this module is a **stub**. The traits are defined per
//! the interface contract, but `forward_query` returns
//! [`GossipError::BridgeUnavailable`], `discover_bridges` returns an
//! empty list, and `validate_bilateral`/`relay_advertisement` return
//! `Ok(())` (simplified). Full implementation is deferred to M5+.

use serde::{Deserialize, Serialize};

use taba_common::{LogicalClock, NodeId, TrustDomainId};

use crate::error::GossipError;

// ---------------------------------------------------------------------------
// ForwardingResult
// ---------------------------------------------------------------------------

/// Cross-domain forwarding result (read-only view, INV-X2).
///
/// Returned by a bridge node after executing a forward query against
/// the target domain's local graph. Signed by the bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardingResult {
    /// Unique request identifier (matches the query).
    pub request_id: u64,
    /// The read-only result payload from the target domain.
    pub result_payload: Vec<u8>,
    /// The bridge node that executed the query.
    pub bridge_node: NodeId,
    /// Logical clock at which the result was produced (freshness, INV-X3).
    pub result_lc: LogicalClock,
}

// ---------------------------------------------------------------------------
// CrossDomainGossip trait
// ---------------------------------------------------------------------------

/// Cross-domain forwarding protocol (INV-X1 through INV-X6).
///
/// Bridge nodes relay capability advertisements and forwarding queries
/// across trust domain boundaries. Bilateral policy is verified before
/// query execution.
pub trait CrossDomainGossip {
    /// Discover bridge nodes for a target domain.
    ///
    /// Returns nodes participating in both the local domain and the
    /// target. If governance restricts bridging (INV-X4), only
    /// designated bridges are returned.
    fn discover_bridges(&self, target_domain: &TrustDomainId) -> Vec<NodeId>;

    /// Execute a cross-domain forwarding query via a bridge.
    ///
    /// 1. Verify bilateral policy in both domains (INV-X1)
    /// 2. Send signed query to bridge
    /// 3. Bridge executes against target domain's local graph
    /// 4. Bridge returns signed result (read-only view, INV-X2)
    /// 5. Cache result locally (fail-open default, INV-X3)
    ///
    /// Returns cached result if bridge is unavailable and cache exists
    /// (fail-open). Returns [`GossipError::BridgeUnavailable`] if no
    /// cache and no bridge.
    fn forward_query(
        &self,
        target_domain: &TrustDomainId,
        query_payload: &[u8],
    ) -> Result<ForwardingResult, GossipError>;

    /// Validate that bilateral policy exists for a cross-domain
    /// interaction.
    ///
    /// Checks both consuming and providing domains. Returns error if
    /// either side is missing authorization (INV-X1, fail closed).
    fn validate_bilateral(
        &self,
        consuming_domain: &TrustDomainId,
        providing_domain: &TrustDomainId,
    ) -> Result<(), GossipError>;

    /// Relay a cross-domain capability advertisement (INV-X5).
    ///
    /// Called by bridge nodes when they receive a `CrossDomainCapability`
    /// governance unit in one domain and relay it to the other.
    fn relay_advertisement(
        &self,
        source_domain: &TrustDomainId,
        capability: &str,
        conditions: &str,
    ) -> Result<(), GossipError>;
}

// ---------------------------------------------------------------------------
// DefaultCrossDomainGossip (M4 stub)
// ---------------------------------------------------------------------------

/// Default implementation of [`CrossDomainGossip`].
///
/// **M4 stub.** Full implementation is deferred to M5+:
/// - `discover_bridges` returns an empty list (no bridges known yet).
/// - `forward_query` returns [`GossipError::BridgeUnavailable`] (no
///   bridge available, and no cache exists in M4).
/// - `validate_bilateral` returns `Ok(())` (simplified — assumes policy
///   exists; a full implementation queries the governance graph).
/// - `relay_advertisement` returns `Ok(())` (no-op for M4).
///
/// The stubs let the trait surface compile and be exercised in tests
/// without a real bridge. When M5+ adds bridge nodes, these methods
/// gain real implementations.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct DefaultCrossDomainGossip {
    _marker: (),
}

impl DefaultCrossDomainGossip {
    /// Creates a new cross-domain gossip stub.
    #[must_use]
    pub const fn new() -> Self {
        Self { _marker: () }
    }
}

impl CrossDomainGossip for DefaultCrossDomainGossip {
    fn discover_bridges(&self, _target_domain: &TrustDomainId) -> Vec<NodeId> {
        // M4 stub: no bridges are known yet.
        Vec::new()
    }

    fn forward_query(
        &self,
        target_domain: &TrustDomainId,
        _query_payload: &[u8],
    ) -> Result<ForwardingResult, GossipError> {
        // M4 stub: no bridge available, and no cache exists.
        Err(GossipError::BridgeUnavailable {
            target: *target_domain,
        })
    }

    fn validate_bilateral(
        &self,
        _consuming_domain: &TrustDomainId,
        _providing_domain: &TrustDomainId,
    ) -> Result<(), GossipError> {
        // M4 stub: simplified — assumes bilateral policy exists.
        // Full implementation (M5+) queries the governance graph and
        // fails closed when policy is missing (INV-X1).
        Ok(())
    }

    fn relay_advertisement(
        &self,
        _source_domain: &TrustDomainId,
        _capability: &str,
        _conditions: &str,
    ) -> Result<(), GossipError> {
        // M4 stub: no-op. Full implementation (M5+) signs and
        // disseminates the relayed advertisement.
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(n: u128) -> TrustDomainId {
        TrustDomainId(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn test_discover_bridges_empty() {
        // M4 stub: returns empty list for any target domain.
        let cd = DefaultCrossDomainGossip::new();
        assert!(cd.discover_bridges(&tid(1)).is_empty());
        assert!(cd.discover_bridges(&tid(2)).is_empty());
    }

    #[test]
    fn test_forward_query_bridge_unavailable() {
        // M4 stub: returns BridgeUnavailable for any target domain.
        let cd = DefaultCrossDomainGossip::new();
        let result = cd.forward_query(&tid(1), b"query");
        assert!(
            matches!(result, Err(GossipError::BridgeUnavailable { target }) if target == tid(1))
        );
    }

    #[test]
    fn test_validate_bilateral_ok() {
        // M4 stub: returns Ok (simplified — assumes policy exists).
        let cd = DefaultCrossDomainGossip::new();
        cd.validate_bilateral(&tid(1), &tid(2))
            .expect("M4 stub should accept any bilateral interaction");
    }
}
