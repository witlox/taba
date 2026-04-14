//! Gossip types: SWIM-based membership, failure detection, and witness confirmation.
//!
//! taba uses a SWIM-like gossip protocol with Lifeguard-style auto-tuning
//! (DL-016) for membership and failure detection (INV-R3). All gossip messages
//! are signed with the sending node's identity key. Membership state changes
//! (node declared failed) require corroboration from at least 2 independent
//! witnesses (INV-R3). Gossip is O(n) in dissemination, targeting hundreds to
//! low thousands of nodes (A4).
//!
//! **Default parameters** (DL-016):
//! - gossip_interval: 500ms (one random peer probed per interval)
//! - suspicion_timeout: 5s (base, auto-scaled with cluster size)
//! - witness_count: 2 (INV-R3)
//! - indirect_probe_count: 3 (peers asked for ping-req on direct failure)
//! - retransmit_multiplier: 4 (piggyback rounds = mult × log2(N))
//! - max_piggyback_entries: 8 (bounds message size)
//! - suspicion_multiplier: 4 (effective timeout = mult × ceil(log2(N)) × interval)
//!
//! Key revocation priority: retransmitted for 2× normal rounds for rapid convergence.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::common::{NodeId, Timestamp};
use crate::node::{HealthStatus, NodeState};
use crate::security::{PublicKey, Signature};

// ---------------------------------------------------------------------------
// Gossip messages
// ---------------------------------------------------------------------------

/// A signed gossip message exchanged between nodes.
/// All gossip messages are signed with the sender's Ed25519 key (INV-R3).
/// Invalid signatures cause the message to be dropped and the sender
/// flagged for investigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipMessage {
    /// The node that sent this message.
    pub sender: NodeId,
    /// Signature over the message payload.
    pub signature: Signature,
    /// The message payload.
    pub payload: GossipPayload,
    /// When this message was created.
    pub sent_at: Timestamp,
    /// Monotonic sequence number for deduplication.
    pub sequence: u64,
}

/// Payload variants for gossip messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GossipPayload {
    /// SWIM ping -- direct probe to check if a node is alive.
    Ping(SwimProbe),
    /// SWIM ping-req -- ask a third node to probe on our behalf (indirect probe).
    PingReq(SwimProbe),
    /// SWIM ack -- response to a ping or ping-req.
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
    HealthUpdate(HealthStatus),
    /// Priority message: key revocation propagation.
    KeyRevocation(crate::security::KeyRevocation),
    /// Solver version announcement (for version gating, FM-12).
    SolverVersion {
        node_id: NodeId,
        solver_version: u64,
    },
}

// ---------------------------------------------------------------------------
// SWIM protocol types
// ---------------------------------------------------------------------------

/// A SWIM probe (ping or ping-req) for failure detection.
/// Indirect probes (ping-req) reduce false positives from transient
/// network issues (FM-09).
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
    /// When the probe was sent (for timeout calculation).
    pub sent_at: Timestamp,
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
// Membership
// ---------------------------------------------------------------------------

/// Current view of cluster membership from a single node's perspective.
/// Eventually consistent -- all nodes converge in the absence of actual
/// failures (INV-R3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipView {
    /// Known nodes and their current states.
    pub members: BTreeMap<NodeId, MemberInfo>,
    /// The local node's ID.
    pub local_node: NodeId,
    /// When this view was last updated.
    pub updated_at: Timestamp,
    /// Incarnation number for this node (incremented on rejoin to override
    /// stale suspicion from previous incarnation).
    pub incarnation: u64,
}

/// Information about a cluster member.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    /// The node's ID.
    pub node_id: NodeId,
    /// The node's public key (for gossip signature verification).
    pub public_key: PublicKey,
    /// Current state as known by this node.
    pub state: NodeState,
    /// Latest health status (peer-observed).
    pub health: Option<NodeHealth>,
    /// When this member was last heard from.
    pub last_seen: Timestamp,
    /// Incarnation number (for superseding stale state).
    pub incarnation: u64,
}

/// Health observation of a node as seen by peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealth {
    /// Overall health assessment.
    pub status: HealthAssessment,
    /// When this health observation was made.
    pub observed_at: Timestamp,
    /// Which node made this observation.
    pub observer: NodeId,
}

/// Overall health assessment of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HealthAssessment {
    /// Node is responsive and healthy.
    Healthy,
    /// Node is responsive but reporting degraded performance.
    Degraded,
    /// Node health is unknown (suspected state, INV-R5).
    Unknown,
    /// Node has been confirmed failed by witness consensus.
    Failed,
}

// ---------------------------------------------------------------------------
// Membership changes
// ---------------------------------------------------------------------------

/// A change in cluster membership disseminated via gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipChange {
    /// The node whose membership changed.
    pub node_id: NodeId,
    /// The previous state (if known).
    pub from_state: Option<NodeState>,
    /// The new state.
    pub to_state: NodeState,
    /// When this change was observed.
    pub observed_at: Timestamp,
    /// The node that first observed this change.
    pub observed_by: NodeId,
    /// Incarnation number associated with this change.
    pub incarnation: u64,
}

/// Witness confirmation that a suspected node has truly failed.
/// At least 2 independent witnesses are required before declaring
/// a node failed (INV-R3). Prevents false positives (FM-09).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessConfirmation {
    /// The node being declared failed.
    pub suspect: NodeId,
    /// The witness confirming the failure.
    pub witness: NodeId,
    /// Evidence: the probe that went unanswered.
    pub failed_probe: SwimProbe,
    /// When this confirmation was made.
    pub confirmed_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Gossip tuning parameters (DL-016)
// ---------------------------------------------------------------------------

/// SWIM gossip protocol tuning parameters with Lifeguard-style auto-scaling.
///
/// These are stored in ClusterConfig and propagated via gossip. The effective
/// suspicion timeout auto-scales with cluster size:
/// `effective = max(base_timeout, suspicion_mult × ceil(log2(N)) × interval)`
///
/// Bandwidth estimate at 10k nodes: ~1 KB/s per node, ~10 MB/s cluster-wide.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipParams {
    /// Base probe interval — one random peer probed per interval.
    /// Default: 500ms.
    pub gossip_interval: std::time::Duration,
    /// Base suspicion timeout — time a node stays Suspected before witnesses
    /// can confirm failure. Must be > gossip_interval × indirect_probe_count.
    /// Default: 5s.
    pub suspicion_timeout: std::time::Duration,
    /// Number of independent witnesses required before declaring a node failed.
    /// Default: 2 (INV-R3).
    pub witness_count: u8,
    /// Number of peers asked to do indirect probes (ping-req) when a direct
    /// ping fails. Default: 3 (standard SWIM).
    pub indirect_probe_count: u8,
    /// Membership changes piggybacked for `retransmit_multiplier × log2(N)`
    /// rounds. Default: 4.
    pub retransmit_multiplier: u8,
    /// Maximum membership changes piggybacked per message. Bounds message size.
    /// Default: 8.
    pub max_piggyback_entries: u8,
    /// Suspicion timeout scales as `suspicion_multiplier × ceil(log2(N)) × interval`.
    /// Larger clusters get proportionally more time, reducing false positives.
    /// Default: 4.
    pub suspicion_multiplier: u8,
    /// Key revocation messages use `2 × retransmit_multiplier × log2(N)` rounds
    /// (double normal) for rapid convergence. This is hardcoded behavior, not
    /// a configurable parameter.
}
