//! Delegation tokens and spawn context for bounded task creation.
//!
//! When a service is placed on a node, the author pre-signs a
//! [`DelegationToken`] scoping the node's spawn authority (INV-W4).
//! The node uses this token to sign spawned tasks — it never holds the
//! author's private key. Spawned tasks inherit operational authority
//! from the token but NOT governance authority (INV-W4a).

use serde::{Deserialize, Serialize};

use taba_common::{DelegationTokenId, LogicalClock, NodeId, TrustDomainId, UnitId};

// ---------------------------------------------------------------------------
// Delegation token
// ---------------------------------------------------------------------------

/// Delegation token for spawned task signing (INV-W4).
///
/// Pre-signed by the author at service placement time. The node uses
/// this token to sign spawned tasks — it never holds the author's
/// private key. Tokens are scoped to one service, one node, one
/// logical-clock range, and one spawn count limit.
///
/// Token validation (signature, scope, LC range, spawn count) is
/// performed by taba-security. This type carries the raw bytes only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationToken {
    /// Unique identifier for this token.
    pub id: DelegationTokenId,
    /// The service this token authorizes spawning for.
    pub service_id: UnitId,
    /// The node authorized to use this token.
    pub node_id: NodeId,
    /// Trust domain scope.
    pub trust_domain: TrustDomainId,
    /// Logical clock range during which this token is valid.
    pub valid_lc_range: (LogicalClock, LogicalClock),
    /// Maximum number of tasks this token can spawn.
    pub max_spawns: u32,
    /// Current spawn count (tracked by the node, verified at merge).
    pub current_spawns: u32,
    /// Author's signature over this token (raw bytes — verification is
    /// taba-security's responsibility).
    pub author_signature: Vec<u8>,
    /// Whether this token has been revoked.
    pub revoked: bool,
}

// ---------------------------------------------------------------------------
// Spawn context
// ---------------------------------------------------------------------------

/// Context for a spawned bounded task (INV-W4).
///
/// A bounded task that was spawned by a running service carries this
/// context, linking it to the parent service and the delegation token
/// that authorized the spawn. Maximum spawn depth: 4 (INV-W3),
/// enforced at graph merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnContext {
    /// The parent service that spawned this task.
    pub spawned_by: UnitId,
    /// The delegation token authorizing this spawn.
    pub delegation_token_id: DelegationTokenId,
    /// Depth in the spawn chain (1 = direct spawn from service, max 4 per INV-W3).
    pub spawn_depth: u8,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegation_token_serialization_roundtrip() {
        let token = DelegationToken {
            id: DelegationTokenId(uuid::Uuid::new_v4()),
            service_id: UnitId(uuid::Uuid::new_v4()),
            node_id: NodeId(uuid::Uuid::new_v4()),
            trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
            valid_lc_range: (LogicalClock(10), LogicalClock(100)),
            max_spawns: 50,
            current_spawns: 3,
            author_signature: vec![0u8; 64],
            revoked: false,
        };

        let json = serde_json::to_string(&token).expect("serialize DelegationToken");
        let decoded: DelegationToken =
            serde_json::from_str(&json).expect("deserialize DelegationToken");

        // DelegationToken contains Vec<u8> which has PartialEq, but we
        // compare JSON strings because DualClockEvent isn't involved here.
        // Actually all fields have PartialEq, so let's compare directly.
        let json2 = serde_json::to_string(&decoded).expect("re-serialize DelegationToken");
        assert_eq!(json, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_delegation_token_revoked() {
        let token = DelegationToken {
            id: DelegationTokenId(uuid::Uuid::new_v4()),
            service_id: UnitId(uuid::Uuid::new_v4()),
            node_id: NodeId(uuid::Uuid::new_v4()),
            trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
            valid_lc_range: (LogicalClock(10), LogicalClock(100)),
            max_spawns: 50,
            current_spawns: 3,
            author_signature: vec![1u8, 2u8, 3u8],
            revoked: true,
        };

        let json = serde_json::to_string(&token).expect("serialize revoked token");
        assert!(json.contains("true"), "revoked should be true in JSON");
    }

    #[test]
    fn test_spawn_context_serialization_roundtrip() {
        let ctx = SpawnContext {
            spawned_by: UnitId(uuid::Uuid::new_v4()),
            delegation_token_id: DelegationTokenId(uuid::Uuid::new_v4()),
            spawn_depth: 1,
        };

        let json = serde_json::to_string(&ctx).expect("serialize SpawnContext");
        let decoded: SpawnContext = serde_json::from_str(&json).expect("deserialize SpawnContext");
        assert_eq!(ctx, decoded);
    }

    #[test]
    fn test_spawn_context_max_depth() {
        // INV-W3: max spawn depth is 4.
        for depth in 1u8..=4 {
            let ctx = SpawnContext {
                spawned_by: UnitId(uuid::Uuid::new_v4()),
                delegation_token_id: DelegationTokenId(uuid::Uuid::new_v4()),
                spawn_depth: depth,
            };
            assert_eq!(ctx.spawn_depth, depth);
        }
    }
}
