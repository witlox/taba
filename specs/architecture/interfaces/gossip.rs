// taba-gossip: SWIM-based membership protocol and gossip transport.
//
// This crate owns cluster membership, failure detection, and the signed
// gossip message transport. It does NOT carry graph data — gossip is for
// membership only. Graph synchronization uses graph deltas over the transport.
//
// All gossip messages are signed with the sending node's identity key
// (INV-R3). Membership state changes require 2 independent witnesses (DL-009).

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct NodeId(/* opaque */);

/// Ed25519 public key identifying a node.
pub struct NodeKey(/* opaque */);

/// Signed gossip message envelope.
pub struct GossipMessage(/* opaque */);

/// Network address of a node (IP + port).
pub struct NodeAddr(/* opaque */);

/// Node health as observed by the membership protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeHealth {
    /// Node is responding to probes.
    Alive,
    /// Node missed probes; indirect probes in progress (INV-R5).
    Suspected,
    /// Node confirmed failed by 2+ witnesses (INV-R3).
    Failed,
    /// Node is voluntarily leaving the cluster.
    Leaving,
}

/// A node's membership record.
pub struct MemberRecord {
    pub id: NodeId,
    pub key: NodeKey,
    pub addr: NodeAddr,
    pub health: NodeHealth,
    /// Lamport-like incarnation number to distinguish restarts.
    pub incarnation: u64,
    /// Solver version announced by this node (for upgrade gating, FM-12).
    pub solver_version: u64,
}

/// The result of a join attempt.
pub struct JoinResult {
    pub node_id: NodeId,
    pub seed_members: Vec<MemberRecord>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum GossipError {
    /// Gossip message signature verification failed.
    InvalidSignature { from: NodeId, reason: String },
    /// Node is not a member of the cluster.
    NotAMember { node: NodeId },
    /// Join failed (seed unreachable, attestation failed, etc.).
    JoinFailed { reason: String },
    /// Transport-level error (network unreachable, timeout, etc.).
    TransportError { reason: String },
    /// Witness requirement not met for membership state change.
    InsufficientWitnesses { node: NodeId, have: u8, need: u8 },
    /// Node is in a state that does not permit this operation.
    InvalidState { node: NodeId, state: NodeHealth, reason: String },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Cluster membership protocol (SWIM-based).
///
/// Manages node lifecycle: join, leave, failure detection. All state changes
/// are distributed via gossip. Membership converges in the absence of actual
/// failures (INV-R3).
pub trait MembershipProtocol {
    /// Join the cluster by contacting seed nodes.
    ///
    /// The joining node presents its Ed25519 public key and optionally a
    /// TPM attestation. Seed nodes verify the key and propagate the join
    /// via gossip.
    ///
    /// Returns `GossipError::JoinFailed` if no seed node is reachable or
    /// attestation fails.
    async fn join(&self, seeds: &[NodeAddr]) -> Result<JoinResult, GossipError>;

    /// Voluntarily leave the cluster.
    ///
    /// The node announces its departure via gossip, transitions to Leaving
    /// state, and waits for acknowledgment from peers before shutting down.
    /// This gives the system time to re-code erasure shards.
    async fn leave(&self) -> Result<(), GossipError>;

    /// Send a probe to a specific node (SWIM ping).
    ///
    /// If the probe fails, the target enters Suspected state and indirect
    /// probes are initiated through other nodes. A node is only declared
    /// Failed after 2 independent witnesses confirm unreachability (INV-R3,
    /// DL-009).
    ///
    /// Returns the target's current health. Returns
    /// `GossipError::TransportError` if the probe cannot be sent at all.
    async fn probe(&self, target: &NodeId) -> Result<NodeHealth, GossipError>;

    /// Declare a node as failed.
    ///
    /// Requires corroboration from at least 2 independent witnesses
    /// (DL-009, INV-R3). Witnesses are other nodes that independently
    /// confirmed the target is unreachable via indirect probes.
    ///
    /// Returns `GossipError::InsufficientWitnesses` if fewer than 2
    /// witnesses have confirmed. Returns `GossipError::InvalidState` if
    /// the target is not in Suspected state.
    async fn declare_failed(
        &self,
        target: &NodeId,
        witnesses: &[NodeId],
    ) -> Result<(), GossipError>;

    /// Handle an incoming gossip message.
    ///
    /// Verifies the message signature (INV-R3) and applies membership
    /// updates. Stale updates (lower incarnation number) are discarded.
    ///
    /// Returns `GossipError::InvalidSignature` if verification fails.
    async fn handle_message(&self, message: GossipMessage) -> Result<(), GossipError>;
}

/// Low-level gossip transport: send and receive signed messages.
///
/// All messages are signed with the sending node's identity key (INV-R3).
/// The transport is responsible for serialization, signing, and network I/O.
/// It does NOT interpret message semantics — that is `MembershipProtocol`.
pub trait GossipTransport {
    /// Send a signed gossip message to a specific node.
    ///
    /// The message is signed before transmission. Returns
    /// `GossipError::TransportError` on network failure.
    async fn send(&self, target: &NodeAddr, message: &GossipMessage) -> Result<(), GossipError>;

    /// Send a signed gossip message to multiple nodes (fanout).
    ///
    /// Best-effort: some sends may fail. Returns the list of nodes that
    /// failed to receive the message.
    async fn send_many(
        &self,
        targets: &[NodeAddr],
        message: &GossipMessage,
    ) -> Vec<(NodeAddr, GossipError)>;

    /// Receive the next incoming gossip message.
    ///
    /// Blocks until a message arrives. The message signature has NOT been
    /// verified — callers must verify via `security::Verifier` or
    /// `MembershipProtocol::handle_message`.
    async fn recv(&self) -> Result<GossipMessage, GossipError>;

    /// Bind the transport to a local address and start listening.
    async fn bind(&self, addr: &NodeAddr) -> Result<(), GossipError>;
}

/// Read-only view of the current cluster membership.
///
/// Used by the solver (via `MembershipSnapshot`) and by other crates that
/// need to query membership without mutating it.
pub trait MembershipView {
    /// Get all currently known members and their health.
    fn members(&self) -> Vec<MemberRecord>;

    /// Get a specific node's membership record.
    ///
    /// Returns `GossipError::NotAMember` if the node is unknown.
    fn get(&self, id: &NodeId) -> Result<MemberRecord, GossipError>;

    /// Get all nodes with a specific health status.
    fn by_health(&self, health: NodeHealth) -> Vec<MemberRecord>;

    /// Get the count of active (Alive) nodes.
    fn active_count(&self) -> usize;

    /// Check whether all active nodes report the same solver version.
    ///
    /// Used during solver upgrade ceremony (FM-12). Returns the version
    /// if uniform, or None if there is version skew.
    fn uniform_solver_version(&self) -> Option<u64>;

    /// Take an immutable snapshot for the solver.
    ///
    /// The snapshot is decoupled from membership changes that occur after
    /// it is taken. Used as input to `solver::Solver::solve`.
    fn snapshot(&self) -> MembershipSnapshot;
}

/// Opaque snapshot of membership state at a point in time.
pub struct MembershipSnapshot(/* opaque */);
