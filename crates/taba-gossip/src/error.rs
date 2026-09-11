//! Gossip protocol error type.
//!
//! All errors that can arise from the SWIM membership protocol, signed
//! gossip transport, capability advertisement, cross-domain forwarding,
//! and fleet command propagation. Variant names and field shapes follow
//! [`specs/architecture/interfaces/gossip.rs`] and the error taxonomy in
//! [`specs/architecture/error-taxonomy.md`].
//!
//! Every variant is categorized as **Retryable**, **Permanent**, or
//! **Security** so that callers can decide whether to re-attempt an
//! operation or surface it to the operator.

use taba_common::{NodeId, TrustDomainId};

/// Errors produced by the gossip protocol and transport.
///
/// Gossip messages are signed (INV-R3); signature failures are fatal for
/// the offending message. Membership state changes require 2 independent
/// witnesses (DL-009, INV-R3). Cross-domain operations fail closed when
/// bilateral policy is missing (INV-X1) or no bridge is available
/// (INV-X6). Fleet commands are rate-limited (F-A314).
#[derive(Debug, thiserror::Error)]
pub enum GossipError {
    /// Gossip message signature verification failed (INV-R3, FM-04).
    ///
    /// The message is dropped and the sender flagged for investigation.
    /// **Security** — fatal for this message.
    #[error("invalid signature from node {from}: {reason}")]
    InvalidSignature {
        /// The node that sent the unverifiable message.
        from: NodeId,
        /// Why verification failed.
        reason: String,
    },

    /// The requested node is not a known member of the cluster.
    ///
    /// **Permanent** for the current view — the node may join later.
    #[error("node {node} is not a member of the cluster")]
    NotAMember {
        /// The unknown node.
        node: NodeId,
    },

    /// Join failed — no seed node reachable, attestation failed, etc.
    ///
    /// **Retryable** after checking network connectivity and seed
    /// configuration (see error-taxonomy `BootstrapFailed`).
    #[error("join failed: {reason}")]
    JoinFailed {
        /// Why the join attempt failed.
        reason: String,
    },

    /// Transport-level error: network unreachable, timeout, etc.
    ///
    /// **Retryable** — SWIM continues probing other peers.
    #[error("transport error: {reason}")]
    TransportError {
        /// Underlying transport failure description.
        reason: String,
    },

    /// Witness requirement not met for a membership state change
    /// (INV-R3, DL-009, FM-09).
    ///
    /// At least 2 independent witnesses must confirm a node failure
    /// before it is declared. **Retryable** — more witnesses needed.
    #[error("insufficient witnesses for node {node}: have {have}, need {need}")]
    InsufficientWitnesses {
        /// The node whose failure is being declared.
        node: NodeId,
        /// Number of witnesses collected so far.
        have: u8,
        /// Number of witnesses required (INV-R3: 2).
        need: u8,
    },

    /// The node is in a state that does not permit the requested
    /// operation (e.g. declaring a node failed that is not Suspected).
    ///
    /// **Permanent** until the node transitions.
    #[error("node {node} is in state {state}: {reason}")]
    InvalidState {
        /// The node in an invalid state.
        node: NodeId,
        /// The node's current health state.
        state: crate::membership::HealthAssessment,
        /// Why the operation is not permitted in this state.
        reason: String,
    },

    /// Cross-domain: no bilateral policy exists for the interaction
    /// (INV-X1). Operations fail closed.
    ///
    /// **Fatal** — operator must establish bilateral policy.
    #[error(
        "no bilateral policy between consuming domain {consuming} and providing domain {providing}"
    )]
    BilateralPolicyMissing {
        /// The domain consuming the cross-domain resource.
        consuming: TrustDomainId,
        /// The domain providing the cross-domain resource.
        providing: TrustDomainId,
    },

    /// Cross-domain: no bridge node available to the target domain
    /// (INV-X6, FM-25).
    ///
    /// **Retryable** — a bridge may join later.
    #[error("no bridge available to target domain {target}")]
    BridgeUnavailable {
        /// The domain that cannot be reached.
        target: TrustDomainId,
    },

    /// Cross-domain: governance requires freshness but the bridge is
    /// down and no fresh cached result exists (INV-X3).
    ///
    /// **Retryable** — retry after the bridge recovers.
    #[error("stale result rejected for target domain {target}")]
    StaleResultRejected {
        /// The domain whose cached result was rejected.
        target: TrustDomainId,
    },

    /// Fleet command rate limit exceeded (F-A314).
    ///
    /// Only one command of each type is allowed per configured logical
    /// clock delta. **Retryable** — retry after the rate window elapses.
    #[error("fleet command rate limited for command type '{command_type}'")]
    FleetCommandRateLimited {
        /// The command type that was rate-limited.
        command_type: String,
    },
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn nid() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn tid() -> TrustDomainId {
        TrustDomainId(Uuid::new_v4())
    }

    #[test]
    fn test_all_error_variants_display() {
        // Every variant must produce a non-empty, human-readable Display
        // string. This guards against accidentally introducing a variant
        // without a #[error("...")] message.
        let cases: Vec<GossipError> = vec![
            GossipError::InvalidSignature {
                from: nid(),
                reason: "bad sig".to_string(),
            },
            GossipError::NotAMember { node: nid() },
            GossipError::JoinFailed {
                reason: "no seeds".to_string(),
            },
            GossipError::TransportError {
                reason: "timeout".to_string(),
            },
            GossipError::InsufficientWitnesses {
                node: nid(),
                have: 1,
                need: 2,
            },
            GossipError::InvalidState {
                node: nid(),
                state: crate::membership::HealthAssessment::Healthy,
                reason: "not suspected".to_string(),
            },
            GossipError::BilateralPolicyMissing {
                consuming: tid(),
                providing: tid(),
            },
            GossipError::BridgeUnavailable { target: tid() },
            GossipError::StaleResultRejected { target: tid() },
            GossipError::FleetCommandRateLimited {
                command_type: "refresh-capabilities".to_string(),
            },
        ];

        for err in &cases {
            let msg = err.to_string();
            assert!(
                !msg.is_empty(),
                "GossipError variant must have a non-empty Display message: {err:?}"
            );
        }
    }
}
