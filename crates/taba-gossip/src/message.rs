//! Gossip message types: signed envelopes, SWIM probes, membership
//! changes, and witness confirmations.
//!
//! All gossip messages are signed with the sender's Ed25519 identity key
//! (INV-R3). The transport signs and verifies; this module defines the
//! message *shapes* and the payload variants.
//!
//! Timestamps use [`DualClockEvent`] (A002): the legacy `Timestamp` alias
//! was removed from taba-common because it was ambiguous (wall time?
//! logical? both?). `DualClockEvent` is explicit.

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, NodeId};
use taba_security::{KeyRevocation, Signature};

// ---------------------------------------------------------------------------
// NodeState (lifecycle, from data-models/node.rs)
// ---------------------------------------------------------------------------

/// Lifecycle states of a node in the cluster.
///
/// Transition: `Joining -> Attesting -> Active -> Suspected -> Draining
/// -> Left | Failed`.
///
/// This is the **membership lifecycle** state tracked by gossip, distinct
/// from a node's runtime unit state (which lives in taba-node). Suspected
/// nodes remain in the placement pool with health `Unknown` (INV-R5); the
/// solver applies a scoring penalty but does not remove them until SWIM
/// confirms failure via multi-probe consensus (DL-009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NodeState {
    /// Node is bootstrapping — discovering peers via seed nodes.
    Joining,
    /// Node is proving its integrity (TPM attestation when available, A5).
    Attesting,
    /// Node is fully operational — participating in placement and gossip.
    Active,
    /// Node is suspected of failure by the gossip protocol (INV-R5).
    Suspected,
    /// Node is gracefully leaving the cluster — draining workloads.
    Draining,
    /// Node has cleanly left the cluster.
    Left,
    /// Node has been declared failed by multi-probe consensus (INV-R3).
    Failed,
}

// ---------------------------------------------------------------------------
// GossipMessage
// ---------------------------------------------------------------------------

/// A signed gossip message exchanged between nodes.
///
/// All gossip messages are signed with the sender's Ed25519 key (INV-R3).
/// Invalid signatures cause the message to be dropped and the sender
/// flagged for investigation (FM-04). The `sequence` number supports
/// deduplication across retransmission rounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipMessage {
    /// The node that sent this message.
    pub sender: NodeId,
    /// Signature over the message payload (and sender identity).
    pub signature: Signature,
    /// The message payload.
    pub payload: GossipPayload,
    /// When this message was created (dual clock).
    pub sent_at: DualClockEvent,
    /// Monotonic sequence number for deduplication.
    pub sequence: u64,
}

// ---------------------------------------------------------------------------
// GossipPayload
// ---------------------------------------------------------------------------

/// Payload variants for gossip messages.
///
/// `Ping`/`PingReq`/`Ack` carry SWIM probes for failure detection.
/// `MembershipChange` disseminates node lifecycle transitions.
/// `WitnessConfirmation` provides the corroboration required before a
/// node is declared `Failed` (INV-R3). `HealthUpdate` piggybacks
/// peer-observed health on pings. `KeyRevocation` and `SolverVersion`
/// are priority events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GossipPayload {
    /// SWIM ping — direct probe to check if a node is alive.
    Ping(SwimProbe),
    /// SWIM ping-req — ask a third node to probe on our behalf
    /// (indirect probe, reduces false positives, FM-09).
    PingReq(SwimProbe),
    /// SWIM ack — response to a ping or ping-req.
    Ack {
        /// The probe being acknowledged.
        probe_id: u64,
        /// The responding node.
        responder: NodeId,
    },
    /// Membership state change announcement.
    MembershipChange(MembershipChange),
    /// Witness confirmation for a suspected node failure (INV-R3).
    WitnessConfirmation(WitnessConfirmation),
    /// Piggybacked health status update (disseminated with pings).
    HealthUpdate(crate::membership::NodeHealth),
    /// Priority message: key revocation propagation (2× retransmit).
    KeyRevocation(KeyRevocation),
    /// Solver version announcement (for version gating, FM-12).
    SolverVersion {
        /// The node announcing its solver version.
        node_id: NodeId,
        /// The solver version this node runs.
        solver_version: u64,
    },
}

// ---------------------------------------------------------------------------
// SwimProbe
// ---------------------------------------------------------------------------

/// A SWIM probe (ping or ping-req) for failure detection.
///
/// Indirect probes (`PingReq`) reduce false positives from transient
/// network issues (FM-09): when a direct ping fails, the initiator asks
/// `indirect_probe_count` peers to probe the target on its behalf.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwimProbe {
    /// Unique probe identifier for matching acks.
    pub probe_id: u64,
    /// The node being probed.
    pub target: NodeId,
    /// Who initiated the probe.
    pub initiator: NodeId,
    /// Whether this is a direct or indirect probe.
    pub probe_type: ProbeType,
    /// When the probe was sent (for timeout calculation, dual clock).
    pub sent_at: DualClockEvent,
}

/// Type of SWIM probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProbeType {
    /// Direct ping to target.
    Direct,
    /// Indirect: asking a third node to probe on our behalf.
    Indirect,
}

// ---------------------------------------------------------------------------
// MembershipChange
// ---------------------------------------------------------------------------

/// A change in cluster membership disseminated via gossip.
///
/// Higher incarnation numbers supersede lower ones: a node that restarts
/// increments its incarnation to override stale `Suspected`/`Failed`
/// state from a previous lifecycle (DL-009).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipChange {
    /// The node whose membership changed.
    pub node_id: NodeId,
    /// The previous state (if known).
    pub from_state: Option<NodeState>,
    /// The new state.
    pub to_state: NodeState,
    /// When this change was observed (dual clock).
    pub observed_at: DualClockEvent,
    /// The node that first observed this change.
    pub observed_by: NodeId,
    /// Incarnation number associated with this change.
    pub incarnation: u64,
}

// ---------------------------------------------------------------------------
// WitnessConfirmation
// ---------------------------------------------------------------------------

/// Witness confirmation that a suspected node has truly failed.
///
/// At least 2 independent witnesses are required before declaring a
/// node `Failed` (INV-R3, DL-009). This prevents false positives from
/// transient network issues (FM-09). Each witness contributes one
/// [`WitnessConfirmation`] containing the probe that went unanswered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessConfirmation {
    /// The node being declared failed.
    pub suspect: NodeId,
    /// The witness confirming the failure.
    pub witness: NodeId,
    /// Evidence: the probe that went unanswered.
    pub failed_probe: SwimProbe,
    /// When this confirmation was made (dual clock).
    pub confirmed_at: DualClockEvent,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{LogicalClock, WallTime};

    fn now() -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1_000 },
            timezone: "UTC".to_string(),
        }
    }

    fn test_key_pair() -> taba_security::KeyPair {
        taba_security::KeyPair::generate()
    }

    fn test_node_id() -> NodeId {
        NodeId(uuid::Uuid::new_v4())
    }

    #[test]
    fn test_gossip_message_serialization_roundtrip() {
        let key_pair = test_key_pair();
        let payload = GossipPayload::Ack {
            probe_id: 42,
            responder: test_node_id(),
        };
        let signature = key_pair
            .signing_key()
            .sign_raw(b"test payload")
            .expect("signing should succeed");
        let original = GossipMessage {
            sender: test_node_id(),
            signature,
            payload,
            sent_at: now(),
            sequence: 7,
        };

        let json = serde_json::to_string(&original).expect("serialize GossipMessage");
        let decoded: GossipMessage =
            serde_json::from_str(&json).expect("deserialize GossipMessage");

        // Compare by re-serializing: GossipMessage does not derive
        // PartialEq (Signature is not PartialEq-friendly across all
        // types, but it IS PartialEq — compare fields directly).
        assert_eq!(decoded.sender, original.sender);
        assert_eq!(decoded.signature, original.signature);
        assert_eq!(decoded.sequence, original.sequence);
        assert_eq!(decoded.sent_at, original.sent_at);
        match (&original.payload, &decoded.payload) {
            (
                GossipPayload::Ack {
                    probe_id: a,
                    responder: ra,
                },
                GossipPayload::Ack {
                    probe_id: b,
                    responder: rb,
                },
            ) => {
                assert_eq!(a, b);
                assert_eq!(ra, rb);
            }
            _ => panic!("payload variant mismatch after round-trip"),
        }
    }

    #[test]
    fn test_gossip_payload_all_variants() {
        let probe = SwimProbe {
            probe_id: 1,
            target: test_node_id(),
            initiator: test_node_id(),
            probe_type: ProbeType::Direct,
            sent_at: now(),
        };
        let change = MembershipChange {
            node_id: test_node_id(),
            from_state: Some(NodeState::Active),
            to_state: NodeState::Suspected,
            observed_at: now(),
            observed_by: test_node_id(),
            incarnation: 3,
        };
        let witness = WitnessConfirmation {
            suspect: test_node_id(),
            witness: test_node_id(),
            failed_probe: probe.clone(),
            confirmed_at: now(),
        };
        let health = crate::membership::NodeHealth {
            status: crate::membership::HealthAssessment::Healthy,
            observed_at: now(),
            observer: test_node_id(),
        };
        let revocation = taba_security::KeyRevocation {
            author_id: taba_common::AuthorId(uuid::Uuid::new_v4()),
            revoked_key: *test_key_pair().public_key(),
            revoked_at: now(),
            reason: taba_security::RevocationReason::Compromised,
            authorized_by: taba_common::AuthorId(uuid::Uuid::new_v4()),
            version: taba_common::Version(1),
        };

        let payloads: Vec<GossipPayload> = vec![
            GossipPayload::Ping(probe.clone()),
            GossipPayload::PingReq(probe),
            GossipPayload::Ack {
                probe_id: 9,
                responder: test_node_id(),
            },
            GossipPayload::MembershipChange(change),
            GossipPayload::WitnessConfirmation(witness),
            GossipPayload::HealthUpdate(health),
            GossipPayload::KeyRevocation(revocation),
            GossipPayload::SolverVersion {
                node_id: test_node_id(),
                solver_version: 5,
            },
        ];

        for payload in &payloads {
            let json = serde_json::to_string(payload)
                .unwrap_or_else(|e| panic!("serialize {payload:?}: {e}"));
            let decoded: GossipPayload =
                serde_json::from_str(&json).unwrap_or_else(|e| panic!("deserialize: {e}"));
            let json2 = serde_json::to_string(&decoded).expect("re-serialize");
            assert_eq!(json, json2, "payload must round-trip identically");
        }
    }

    #[test]
    fn test_swim_probe_serialization_roundtrip() {
        let probe = SwimProbe {
            probe_id: 99,
            target: test_node_id(),
            initiator: test_node_id(),
            probe_type: ProbeType::Indirect,
            sent_at: now(),
        };
        let json = serde_json::to_string(&probe).expect("serialize SwimProbe");
        let decoded: SwimProbe = serde_json::from_str(&json).expect("deserialize SwimProbe");
        assert_eq!(decoded.probe_id, probe.probe_id);
        assert_eq!(decoded.target, probe.target);
        assert_eq!(decoded.initiator, probe.initiator);
        assert_eq!(decoded.probe_type, probe.probe_type);
        assert_eq!(decoded.sent_at, probe.sent_at);
    }

    #[test]
    fn test_membership_change_serialization_roundtrip() {
        let change = MembershipChange {
            node_id: test_node_id(),
            from_state: Some(NodeState::Active),
            to_state: NodeState::Failed,
            observed_at: now(),
            observed_by: test_node_id(),
            incarnation: 7,
        };
        let json = serde_json::to_string(&change).expect("serialize MembershipChange");
        let decoded: MembershipChange =
            serde_json::from_str(&json).expect("deserialize MembershipChange");
        assert_eq!(decoded.node_id, change.node_id);
        assert_eq!(decoded.from_state, change.from_state);
        assert_eq!(decoded.to_state, change.to_state);
        assert_eq!(decoded.observed_at, change.observed_at);
        assert_eq!(decoded.incarnation, change.incarnation);
    }

    #[test]
    fn test_witness_confirmation_serialization_roundtrip() {
        let probe = SwimProbe {
            probe_id: 5,
            target: test_node_id(),
            initiator: test_node_id(),
            probe_type: ProbeType::Direct,
            sent_at: now(),
        };
        let witness = WitnessConfirmation {
            suspect: test_node_id(),
            witness: test_node_id(),
            failed_probe: probe.clone(),
            confirmed_at: now(),
        };
        let json = serde_json::to_string(&witness).expect("serialize WitnessConfirmation");
        let decoded: WitnessConfirmation =
            serde_json::from_str(&json).expect("deserialize WitnessConfirmation");
        assert_eq!(decoded.suspect, witness.suspect);
        assert_eq!(decoded.witness, witness.witness);
        assert_eq!(decoded.failed_probe.probe_id, probe.probe_id);
        assert_eq!(decoded.confirmed_at, witness.confirmed_at);
    }
}
