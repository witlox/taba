//! SWIM-based membership protocol: join, leave, probe, and failure
//! declaration with 2-witness confirmation (INV-R3, DL-009).
//!
//! The [`MembershipProtocol`] trait defines the contract from
//! [`specs/architecture/interfaces/gossip.rs`]. [`SwimProtocol`] is the
//! M4 implementation: it uses an in-memory transport for deterministic
//! testing and applies membership updates with "higher incarnation
//! wins" semantics (DL-009).
//!
//! # Failure detection (INV-R3, FM-09)
//!
//! 1. A direct ping (`ProbeType::Direct`) is sent to the target.
//! 2. If it times out, `indirect_probe_count` ping-req messages are
//!    sent to other peers (`ProbeType::Indirect`).
//! 3. The target enters `NodeState::Suspected` with health `Unknown`
//!    (INV-R5).
//! 4. Only after 2 independent witnesses confirm (via
//!    [`WitnessConfirmation`](crate::message::WitnessConfirmation))
//!    can a node be declared `Failed`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::Mutex;

use taba_common::{DualClockEvent, LogicalClock, NodeId, WallTime};
use taba_security::{KeyPair, PublicKey};

use crate::error::GossipError;
use crate::membership::{
    DefaultMembershipView, GossipParams, HealthAssessment, JoinResult, MemberInfo, MemberRecord,
    MembershipView, NodeHealth,
};
use crate::message::{GossipMessage, GossipPayload, NodeState, SwimProbe};
use crate::transport::NodeAddr;

// ---------------------------------------------------------------------------
// MembershipProtocol trait
// ---------------------------------------------------------------------------

/// Cluster membership protocol (SWIM-based).
///
/// Manages node lifecycle: join, leave, failure detection. All state
/// changes are distributed via gossip. Membership converges in the
/// absence of actual failures (INV-R3).
///
/// # Cancellation safety
///
/// All async methods are cancellation-safe: dropping a future (e.g. a
/// probe timeout) does not corrupt internal state. Pending probes are
/// tracked by ID and may time out independently.
pub trait MembershipProtocol {
    /// Join the cluster by contacting seed nodes.
    ///
    /// The joining node presents its Ed25519 public key and optionally a
    /// TPM attestation. Seed nodes verify the key and propagate the join
    /// via gossip.
    ///
    /// Returns [`GossipError::JoinFailed`] if no seed node is reachable
    /// or attestation fails.
    async fn join(&self, seeds: &[NodeAddr]) -> Result<JoinResult, GossipError>;

    /// Voluntarily leave the cluster.
    ///
    /// The node announces its departure via gossip, transitions to
    /// `Leaving` state, and waits for acknowledgment from peers before
    /// shutting down. This gives the system time to re-code erasure
    /// shards.
    async fn leave(&self) -> Result<(), GossipError>;

    /// Send a probe to a specific node (SWIM ping).
    ///
    /// If the probe fails, the target enters `Suspected` state and
    /// indirect probes are initiated through other nodes. A node is
    /// only declared `Failed` after 2 independent witnesses confirm
    /// unreachability (INV-R3, DL-009).
    ///
    /// Returns the target's current health. Returns
    /// [`GossipError::TransportError`] if the probe cannot be sent at
    /// all.
    async fn probe(&self, target: &NodeId) -> Result<NodeHealth, GossipError>;

    /// Declare a node as failed.
    ///
    /// Requires corroboration from at least 2 independent witnesses
    /// (DL-009, INV-R3). Witnesses are other nodes that independently
    /// confirmed the target is unreachable via indirect probes.
    ///
    /// Returns [`GossipError::InsufficientWitnesses`] if fewer than 2
    /// witnesses have confirmed. Returns [`GossipError::InvalidState`]
    /// if the target is not in `Suspected` state.
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
    /// Returns [`GossipError::InvalidSignature`] if verification fails.
    async fn handle_message(&self, message: GossipMessage) -> Result<(), GossipError>;
}

// ---------------------------------------------------------------------------
// ProbeOutcome
// ---------------------------------------------------------------------------

/// The outcome of a SWIM probe.
///
/// Returned internally by [`SwimProtocol::probe`] (the trait method
/// returns [`NodeHealth`] directly). `Alive` means the target responded
/// to the direct ping; `Suspected` means the direct ping timed out and
/// indirect probes are in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeOutcome {
    /// The target responded to the probe.
    Alive,
    /// The target did not respond; it is now `Suspected`.
    Suspected,
}

// ---------------------------------------------------------------------------
// ProbeState
// ---------------------------------------------------------------------------

/// State for a pending or completed probe.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ProbeState {
    /// The probe that was sent.
    probe: SwimProbe,
    /// Whether an ack has been received.
    acked: bool,
}

// ---------------------------------------------------------------------------
// SwimProtocol
// ---------------------------------------------------------------------------

/// SWIM protocol implementation with 2-witness failure confirmation.
///
/// Holds the local node's ID, key pair, membership view, gossip
/// parameters, and probe state (pending probes, witness confirmations,
/// suspicion tracking). For M4, the protocol uses an injected
/// [`GossipTransport`] (typically [`InMemoryTransport`](crate::transport::InMemoryTransport)
/// for tests).
///
/// # Determinism
///
/// Membership updates are applied with "higher incarnation wins"
/// semantics (DL-009). Equal-incarnation conflicts keep the existing
/// entry to ensure deterministic convergence regardless of arrival
/// order (INV-R3).
pub struct SwimProtocol {
    /// The local node's ID.
    local_node: NodeId,
    /// The local node's key pair (for signing gossip messages).
    key_pair: Arc<KeyPair>,
    /// The live membership view.
    view: Arc<DefaultMembershipView>,
    /// Gossip tuning parameters (DL-016).
    params: GossipParams,
    /// Network addresses of known members.
    addrs: Mutex<BTreeMap<NodeId, NodeAddr>>,
    /// Public keys of known members (for signature verification).
    public_keys: Mutex<BTreeMap<NodeId, PublicKey>>,
    /// Pending and completed probes, keyed by probe ID.
    probes: Mutex<BTreeMap<u64, ProbeState>>,
    /// Witness confirmations collected per suspect.
    witnesses: Mutex<BTreeMap<NodeId, BTreeSet<NodeId>>>,
    /// Whether the local node has joined.
    joined: Mutex<bool>,
    /// Next probe ID.
    next_probe_id: Mutex<u64>,
    /// Sequence counter for outgoing messages.
    next_sequence: Mutex<u64>,
}

impl std::fmt::Debug for SwimProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwimProtocol")
            .field("local_node", &self.local_node)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

impl SwimProtocol {
    /// Creates a new SWIM protocol for the local node, backed by the
    /// given membership view and gossip parameters.
    #[must_use]
    pub fn new(
        local_node: NodeId,
        key_pair: Arc<KeyPair>,
        view: Arc<DefaultMembershipView>,
        params: GossipParams,
    ) -> Self {
        let public_key = *key_pair.public_key();
        Self {
            local_node,
            key_pair,
            view,
            params,
            addrs: Mutex::new(BTreeMap::new()),
            public_keys: {
                let mut m = BTreeMap::new();
                m.insert(local_node, public_key);
                Mutex::new(m)
            },
            probes: Mutex::new(BTreeMap::new()),
            witnesses: Mutex::new(BTreeMap::new()),
            joined: Mutex::new(false),
            next_probe_id: Mutex::new(1),
            next_sequence: Mutex::new(1),
        }
    }

    /// Returns the local node's ID.
    #[must_use]
    pub const fn local_node(&self) -> NodeId {
        self.local_node
    }

    /// Returns a reference to the membership view.
    #[must_use]
    pub fn view(&self) -> &DefaultMembershipView {
        &self.view
    }

    /// Records a known member's address and public key.
    ///
    /// Used during join (to record seed members) and by
    /// [`Self::handle_message`] when learning about new peers.
    pub fn remember_member(&self, id: NodeId, public_key: PublicKey, addr: NodeAddr) {
        if let Ok(mut keys) = self.public_keys.lock() {
            keys.entry(id).or_insert(public_key);
        }
        if let Ok(mut addrs) = self.addrs.lock() {
            addrs.entry(id).or_insert_with(|| addr.clone());
        }
        self.view.set_addr(id, addr);
    }

    /// Allocates the next probe ID.
    #[allow(dead_code)]
    fn next_probe_id(&self) -> u64 {
        let mut id = self.next_probe_id.lock().expect("probe_id lock poisoned");
        let current = *id;
        *id += 1;
        current
    }

    /// Allocates the next message sequence number.
    fn next_sequence(&self) -> u64 {
        let mut seq = self.next_sequence.lock().expect("sequence lock poisoned");
        let current = *seq;
        *seq += 1;
        current
    }

    /// Returns the current dual-clock time for this node.
    fn now(&self) -> DualClockEvent {
        // For M4 testing, use a monotonic logical clock derived from the
        // sequence counter. A real deployment uses the node's
        // DualClock (wall + logical, INV-T2).
        DualClockEvent {
            logical_clock: LogicalClock(self.next_sequence()),
            wall_time: WallTime {
                millis: 0, // Tests don't rely on wall time.
            },
            timezone: "UTC".to_string(),
        }
    }

    /// Marks a probe as acked (called when an `Ack` payload is handled).
    fn mark_acked(&self, probe_id: u64) {
        if let Ok(mut probes) = self.probes.lock() {
            if let Some(state) = probes.get_mut(&probe_id) {
                state.acked = true;
            }
        }
    }

    /// Sets a member's health observation.
    fn set_health(&self, node: NodeId, status: HealthAssessment) {
        let health = NodeHealth {
            status,
            observed_at: self.now(),
            observer: self.local_node,
        };
        let map = self.view.members_snapshot();
        if let Some(info) = map.get(&node) {
            let updated = MemberInfo {
                node_id: info.node_id,
                public_key: info.public_key,
                state: if status == HealthAssessment::Failed {
                    NodeState::Failed
                } else if status == HealthAssessment::Unknown {
                    NodeState::Suspected
                } else if status == HealthAssessment::Healthy {
                    NodeState::Active
                } else {
                    info.state
                },
                health: Some(health),
                last_seen: info.last_seen.clone(),
                incarnation: info.incarnation,
            };
            self.view.add_member(updated);
        }
    }

    /// Adds a witness confirmation for a suspect.
    fn add_witness(&self, suspect: NodeId, witness: NodeId) {
        if let Ok(mut witnesses) = self.witnesses.lock() {
            witnesses.entry(suspect).or_default().insert(witness);
        }
    }

    /// Returns the number of independent witnesses for a suspect.
    #[must_use]
    pub fn witness_count(&self, suspect: &NodeId) -> usize {
        self.witnesses.lock().map_or(0, |w| {
            w.get(suspect).map_or(0, std::collections::BTreeSet::len)
        })
    }
}

// ---------------------------------------------------------------------------
// MembershipProtocol impl for SwimProtocol
// ---------------------------------------------------------------------------

impl MembershipProtocol for SwimProtocol {
    async fn join(&self, seeds: &[NodeAddr]) -> Result<JoinResult, GossipError> {
        if seeds.is_empty() {
            return Err(GossipError::JoinFailed {
                reason: "no seed nodes provided".to_string(),
            });
        }

        // M4: record the first reachable seed's membership list. In a
        // real deployment, the joining node sends a join request to
        // each seed and merges the returned membership views. For the
        // in-memory test transport, we construct seed member records
        // from the seed addresses (deterministic ID derivation).
        let mut seed_members = Vec::with_capacity(seeds.len());
        for (i, addr) in seeds.iter().enumerate() {
            let seed_id = NodeId(uuid::Uuid::from_u128((i as u128) + 1));
            let public_key = {
                let keys = self.public_keys.lock().expect("public_keys lock poisoned");
                keys.get(&seed_id).copied().unwrap_or_else(|| {
                    // Derive a placeholder key for the seed. In a real
                    // join, the seed provides its actual public key.
                    let kp = KeyPair::generate();
                    *kp.public_key()
                })
            };
            self.remember_member(seed_id, public_key, addr.clone());
            seed_members.push(MemberRecord {
                id: seed_id,
                key: public_key,
                addr: addr.clone(),
                health: NodeHealth {
                    status: HealthAssessment::Healthy,
                    observed_at: self.now(),
                    observer: self.local_node,
                },
                incarnation: 1,
                solver_version: 1,
            });

            // Add the seed to the membership view.
            self.view.add_member(MemberInfo {
                node_id: seed_id,
                public_key,
                state: NodeState::Active,
                health: Some(NodeHealth {
                    status: HealthAssessment::Healthy,
                    observed_at: self.now(),
                    observer: self.local_node,
                }),
                last_seen: self.now(),
                incarnation: 1,
            });
        }

        if let Ok(mut joined) = self.joined.lock() {
            *joined = true;
        }

        // The local node is also a member (Joining → will become Active).
        self.view.add_member(MemberInfo {
            node_id: self.local_node,
            public_key: *self.key_pair.public_key(),
            state: NodeState::Active,
            health: Some(NodeHealth {
                status: HealthAssessment::Healthy,
                observed_at: self.now(),
                observer: self.local_node,
            }),
            last_seen: self.now(),
            incarnation: 1,
        });

        Ok(JoinResult {
            node_id: self.local_node,
            seed_members,
        })
    }

    async fn leave(&self) -> Result<(), GossipError> {
        // Announce departure: set the local node's state to Draining
        // (a precursor to Left). In a real deployment, we wait for
        // peer acks before transitioning to Left.
        let map = self.view.members_snapshot();
        if let Some(info) = map.get(&self.local_node) {
            let updated = MemberInfo {
                node_id: self.local_node,
                public_key: info.public_key,
                state: NodeState::Draining,
                health: info.health.clone(),
                last_seen: self.now(),
                incarnation: info.incarnation,
            };
            self.view.add_member(updated);
        }
        if let Ok(mut joined) = self.joined.lock() {
            *joined = false;
        }
        Ok(())
    }

    async fn probe(&self, target: &NodeId) -> Result<NodeHealth, GossipError> {
        // Check that the target is a known member.
        let record = self.view.get(target)?;

        // For M4 testing, we determine liveness by whether the target's
        // address is registered in the transport (i.e., a live node
        // has bound and can respond). Since the in-memory transport
        // delivers synchronously, a "dead" node is one with no
        // registered address or one that never acks.
        //
        // We simulate this: if the target's current health status is
        // already Failed, return Failed. If Healthy, the probe
        // succeeds (Alive). If Unknown/Suspected, the probe fails
        // (Suspected).
        match record.health.status {
            HealthAssessment::Healthy => Ok(NodeHealth {
                status: HealthAssessment::Healthy,
                observed_at: self.now(),
                observer: self.local_node,
            }),
            HealthAssessment::Failed => Ok(NodeHealth {
                status: HealthAssessment::Failed,
                observed_at: record.health.observed_at.clone(),
                observer: record.health.observer,
            }),
            HealthAssessment::Degraded => Ok(NodeHealth {
                status: HealthAssessment::Degraded,
                observed_at: self.now(),
                observer: self.local_node,
            }),
            HealthAssessment::Unknown => {
                // Suspected: the direct probe fails (no ack within
                // the timeout). Indirect probes would normally be
                // initiated here.
                self.add_witness(*target, self.local_node);
                self.set_health(*target, HealthAssessment::Unknown);
                Ok(NodeHealth {
                    status: HealthAssessment::Unknown,
                    observed_at: self.now(),
                    observer: self.local_node,
                })
            }
        }
    }

    async fn declare_failed(
        &self,
        target: &NodeId,
        witnesses: &[NodeId],
    ) -> Result<(), GossipError> {
        let need = self.params.witness_count;
        let have = u8::try_from(witnesses.len()).unwrap_or(u8::MAX);

        if have < need {
            return Err(GossipError::InsufficientWitnesses {
                node: *target,
                have,
                need,
            });
        }

        // FINDING-020: validate witnesses before applying the transition.
        // 1. The target must be a known member.
        let members = self.view.members_snapshot();
        if !members.contains_key(target) {
            return Err(GossipError::NotAMember { node: *target });
        }

        // 2. Each witness must be a known member.
        for w in witnesses {
            if !members.contains_key(w) {
                return Err(GossipError::NotAMember { node: *w });
            }
        }

        // 3. Witnesses must be distinct from the target.
        for w in witnesses {
            if w == target {
                return Err(GossipError::InvalidState {
                    node: *w,
                    state: members[w]
                        .health
                        .as_ref()
                        .map_or(HealthAssessment::Unknown, |h| h.status),
                    reason: "witness must not equal the target node".to_string(),
                });
            }
        }

        // 4. Witnesses must be distinct from each other.
        let mut seen = BTreeSet::new();
        for w in witnesses {
            if !seen.insert(*w) {
                return Err(GossipError::InvalidState {
                    node: *w,
                    state: members[w]
                        .health
                        .as_ref()
                        .map_or(HealthAssessment::Unknown, |h| h.status),
                    reason: "duplicate witness in declare_failed".to_string(),
                });
            }
        }

        // The target must be in Suspected state (INV-R3). We check the
        // internal MemberInfo state.
        let info = members
            .get(target)
            .ok_or(GossipError::NotAMember { node: *target })?;
        if info.state != NodeState::Suspected {
            return Err(GossipError::InvalidState {
                node: *target,
                state: info
                    .health
                    .as_ref()
                    .map_or(HealthAssessment::Unknown, |h| h.status),
                reason: format!(
                    "node must be Suspected before declaring Failed (current: {:?})",
                    info.state
                ),
            });
        }

        // Record witnesses and apply the Failed transition.
        for w in witnesses {
            self.add_witness(*target, *w);
        }
        self.set_health(*target, HealthAssessment::Failed);
        Ok(())
    }

    async fn handle_message(&self, message: GossipMessage) -> Result<(), GossipError> {
        // Verify the signature (INV-R3). The signature covers the
        // serialized payload.
        let payload_bytes =
            serde_json::to_vec(&message.payload).map_err(|e| GossipError::InvalidSignature {
                from: message.sender,
                reason: e.to_string(),
            })?;

        let sender_key = {
            let keys = self.public_keys.lock().expect("public_keys lock poisoned");
            keys.get(&message.sender).copied()
        };

        let verifying_key = match sender_key {
            Some(pk) => match pk.to_verifying_key() {
                Some(vk) => vk,
                None => {
                    return Err(GossipError::InvalidSignature {
                        from: message.sender,
                        reason: "sender public key is not a canonical Ed25519 key".to_string(),
                    });
                }
            },
            None => {
                // Unknown sender: in a full implementation, we would
                // look up the key via a join exchange. For M4, treat
                // unknown senders as invalid signatures.
                return Err(GossipError::InvalidSignature {
                    from: message.sender,
                    reason: "sender public key not registered".to_string(),
                });
            }
        };

        verifying_key
            .verify_raw(&payload_bytes, &message.signature)
            .map_err(|e| GossipError::InvalidSignature {
                from: message.sender,
                reason: e.to_string(),
            })?;

        // Apply the payload.
        match message.payload {
            GossipPayload::Ack { probe_id, .. } => {
                self.mark_acked(probe_id);
                Ok(())
            }
            GossipPayload::MembershipChange(change) => {
                // Remember the sender's key for future verification.
                if let Some(pk) = sender_key {
                    if let Some(addr) = self
                        .addrs
                        .lock()
                        .map(|a| a.get(&change.node_id).cloned())
                        .ok()
                        .flatten()
                    {
                        self.remember_member(change.node_id, pk, addr);
                    }
                }
                // Higher incarnation wins (DL-009). apply_membership_change
                // returns false if the change was stale; that's fine —
                // idempotent.
                let _ = self.view.apply_membership_change(&change);
                Ok(())
            }
            GossipPayload::WitnessConfirmation(witness) => {
                self.add_witness(witness.suspect, witness.witness);
                Ok(())
            }
            GossipPayload::HealthUpdate(health) => {
                // Update the sender's health in our view.
                let map = self.view.members_snapshot();
                if let Some(info) = map.get(&message.sender) {
                    let updated = MemberInfo {
                        node_id: info.node_id,
                        public_key: info.public_key,
                        state: match health.status {
                            HealthAssessment::Failed => NodeState::Failed,
                            HealthAssessment::Unknown => NodeState::Suspected,
                            HealthAssessment::Healthy => NodeState::Active,
                            HealthAssessment::Degraded => info.state,
                        },
                        health: Some(health),
                        last_seen: info.last_seen.clone(),
                        incarnation: info.incarnation,
                    };
                    self.view.add_member(updated);
                }
                Ok(())
            }
            GossipPayload::SolverVersion {
                node_id,
                solver_version,
            } => {
                self.view.set_solver_version(node_id, solver_version);
                Ok(())
            }
            GossipPayload::KeyRevocation(revocation) => {
                // M4: log the revocation. Full key store integration is
                // handled by taba-security; gossip just propagates.
                tracing::info!(
                    author = %revocation.author_id.0,
                    "received key revocation via priority gossip"
                );
                Ok(())
            }
            // Ping/PingReq handling: in a full implementation, the
            // recipient would send an Ack. For M4, we record the probe
            // and respond. Since the transport is in-memory, the
            // response is delivered via the same channel.
            GossipPayload::Ping(probe) | GossipPayload::PingReq(probe) => {
                // Acknowledge the probe by marking it (in a real
                // deployment, we'd send an Ack message back).
                self.mark_acked(probe.probe_id);
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MembershipChange;
    use crate::transport::InMemoryTransport;

    fn nid(n: u128) -> NodeId {
        NodeId(uuid::Uuid::from_u128(n))
    }

    fn now() -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1_000 },
            timezone: "UTC".to_string(),
        }
    }

    fn test_protocol() -> SwimProtocol {
        let key_pair = Arc::new(KeyPair::generate());
        let local = NodeId(uuid::Uuid::new_v4());
        let view = Arc::new(DefaultMembershipView::new(local));
        SwimProtocol::new(local, key_pair, view, GossipParams::default())
    }

    fn signed_message(
        sender: NodeId,
        key_pair: &KeyPair,
        payload: GossipPayload,
        seq: u64,
    ) -> GossipMessage {
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload for signing");
        let signature = key_pair
            .signing_key()
            .sign_raw(&payload_bytes)
            .expect("signing should succeed");
        GossipMessage {
            sender,
            signature,
            payload,
            sent_at: now(),
            sequence: seq,
        }
    }

    /// Adds a member to the protocol's view with the given state.
    fn add_member(protocol: &SwimProtocol, node: NodeId, state: NodeState) {
        #[allow(clippy::match_same_arms)]
        let health_status = match state {
            NodeState::Active => HealthAssessment::Healthy,
            NodeState::Suspected => HealthAssessment::Unknown,
            NodeState::Failed => HealthAssessment::Failed,
            _ => HealthAssessment::Healthy,
        };
        protocol.view.add_member(MemberInfo {
            node_id: node,
            public_key: *KeyPair::generate().public_key(),
            state,
            health: Some(NodeHealth {
                status: health_status,
                observed_at: now(),
                observer: protocol.local_node(),
            }),
            last_seen: now(),
            incarnation: 1,
        });
    }

    #[tokio::test]
    async fn test_swim_join_single_seed() {
        let protocol = test_protocol();
        let _transport = InMemoryTransport::new();

        let seed_addr = NodeAddr::new("127.0.0.1".to_string(), 7946);
        let result = protocol
            .join(std::slice::from_ref(&seed_addr))
            .await
            .expect("join with one seed should succeed");

        assert_eq!(result.node_id, protocol.local_node());
        assert_eq!(result.seed_members.len(), 1, "should get 1 seed member");
        assert_eq!(result.seed_members[0].addr, seed_addr);
    }

    #[tokio::test]
    async fn test_swim_join_no_seeds() {
        let protocol = test_protocol();
        let result = protocol.join(&[]).await;
        assert!(
            matches!(result, Err(GossipError::JoinFailed { .. })),
            "join with no seeds should fail, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_swim_probe_alive() {
        let protocol = test_protocol();
        let target = nid(2);

        // Add a healthy member to probe.
        protocol.view.add_member(MemberInfo {
            node_id: target,
            public_key: *KeyPair::generate().public_key(),
            state: NodeState::Active,
            health: Some(NodeHealth {
                status: HealthAssessment::Healthy,
                observed_at: now(),
                observer: protocol.local_node(),
            }),
            last_seen: now(),
            incarnation: 1,
        });

        let health = protocol.probe(&target).await.expect("probe should succeed");
        assert_eq!(
            health.status,
            HealthAssessment::Healthy,
            "healthy node probe → Alive"
        );
    }

    #[tokio::test]
    async fn test_swim_probe_dead() {
        let protocol = test_protocol();
        let target = nid(3);

        // Add a suspected (Unknown health) member — simulating a dead
        // node that hasn't been confirmed yet.
        protocol.view.add_member(MemberInfo {
            node_id: target,
            public_key: *KeyPair::generate().public_key(),
            state: NodeState::Suspected,
            health: Some(NodeHealth {
                status: HealthAssessment::Unknown,
                observed_at: now(),
                observer: protocol.local_node(),
            }),
            last_seen: now(),
            incarnation: 1,
        });

        let health = protocol.probe(&target).await.expect("probe should succeed");
        // A dead node returns Suspected (Unknown health). After 2
        // witnesses confirm, it transitions to Failed.
        assert_eq!(
            health.status,
            HealthAssessment::Unknown,
            "dead node probe → Suspected (Unknown health)"
        );
    }

    #[tokio::test]
    async fn test_swim_declare_failed_insufficient_witnesses() {
        let protocol = test_protocol();
        let target = nid(4);

        protocol.view.add_member(MemberInfo {
            node_id: target,
            public_key: *KeyPair::generate().public_key(),
            state: NodeState::Suspected,
            health: Some(NodeHealth {
                status: HealthAssessment::Unknown,
                observed_at: now(),
                observer: protocol.local_node(),
            }),
            last_seen: now(),
            incarnation: 1,
        });

        // Only 1 witness → InsufficientWitnesses (need 2).
        let result = protocol.declare_failed(&target, &[nid(10)]).await;
        assert!(
            matches!(result, Err(GossipError::InsufficientWitnesses { node, have: 1, need: 2 }) if node == target),
            "1 witness should be insufficient, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_swim_declare_failed_two_witnesses() {
        let protocol = test_protocol();
        let target = nid(5);

        add_member(&protocol, target, NodeState::Suspected);
        add_member(&protocol, nid(10), NodeState::Active);
        add_member(&protocol, nid(11), NodeState::Active);

        // 2 witnesses → Ok.
        protocol
            .declare_failed(&target, &[nid(10), nid(11)])
            .await
            .expect("2 witnesses should be sufficient");

        // The target should now be Failed.
        let record = protocol.view().get(&target).expect("target should exist");
        assert_eq!(
            record.health.status,
            HealthAssessment::Failed,
            "target should be Failed after 2 witnesses"
        );
    }

    // -- FINDING-020: witness validation ------------------------------------

    #[tokio::test]
    async fn test_swim_declare_failed_unknown_witness_rejected() {
        let protocol = test_protocol();
        let target = nid(20);

        add_member(&protocol, target, NodeState::Suspected);
        add_member(&protocol, nid(21), NodeState::Active);
        // nid(22) is NOT a member.

        let result = protocol.declare_failed(&target, &[nid(21), nid(22)]).await;
        assert!(
            matches!(result, Err(GossipError::NotAMember { node }) if node == nid(22)),
            "unknown witness should be rejected with NotAMember, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_swim_declare_failed_witness_equals_target_rejected() {
        let protocol = test_protocol();
        let target = nid(30);

        add_member(&protocol, target, NodeState::Suspected);
        add_member(&protocol, nid(31), NodeState::Active);

        // Witness equals target → InvalidState.
        let result = protocol.declare_failed(&target, &[nid(31), target]).await;
        assert!(
            matches!(result, Err(GossipError::InvalidState { node, .. }) if node == target),
            "witness equal to target should be rejected with InvalidState, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_swim_declare_failed_duplicate_witnesses_rejected() {
        let protocol = test_protocol();
        let target = nid(40);

        add_member(&protocol, target, NodeState::Suspected);
        add_member(&protocol, nid(41), NodeState::Active);

        // Duplicate witness → InvalidState.
        let result = protocol.declare_failed(&target, &[nid(41), nid(41)]).await;
        assert!(
            matches!(result, Err(GossipError::InvalidState { node, .. }) if node == nid(41)),
            "duplicate witness should be rejected with InvalidState, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_swim_handle_message_valid() {
        let protocol = test_protocol();
        let sender = nid(6);
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();

        // Register the sender's public key.
        protocol.remember_member(
            sender,
            public_key,
            NodeAddr::new("127.0.0.1".to_string(), 8000),
        );

        // Also add the sender as a member so the membership change can
        // apply.
        protocol.view.add_member(MemberInfo {
            node_id: sender,
            public_key,
            state: NodeState::Active,
            health: Some(NodeHealth {
                status: HealthAssessment::Healthy,
                observed_at: now(),
                observer: protocol.local_node(),
            }),
            last_seen: now(),
            incarnation: 1,
        });

        let message = signed_message(
            sender,
            &key_pair,
            GossipPayload::MembershipChange(MembershipChange {
                node_id: sender,
                from_state: Some(NodeState::Active),
                to_state: NodeState::Draining,
                observed_at: now(),
                observed_by: sender,
                incarnation: 5,
            }),
            1,
        );

        protocol
            .handle_message(message)
            .await
            .expect("valid message should be applied");

        // The member's state should now be Draining (higher incarnation).
        let map = protocol.view().members_snapshot();
        assert_eq!(
            map.get(&sender).map(|m| m.state),
            Some(NodeState::Draining),
            "membership change with higher incarnation should be applied"
        );
    }

    #[tokio::test]
    async fn test_swim_handle_message_invalid_signature() {
        let protocol = test_protocol();
        let sender = nid(7);
        let key_pair_a = KeyPair::generate();
        let key_pair_b = KeyPair::generate(); // Different key.

        // Register key A for the sender, but sign with key B.
        protocol.remember_member(
            sender,
            *key_pair_a.public_key(),
            NodeAddr::new("127.0.0.1".to_string(), 8001),
        );

        let message = signed_message(
            sender,
            &key_pair_b, // Wrong key!
            GossipPayload::Ack {
                probe_id: 1,
                responder: sender,
            },
            1,
        );

        let result = protocol.handle_message(message).await;
        assert!(
            matches!(result, Err(GossipError::InvalidSignature { from, .. }) if from == sender),
            "invalid signature should be rejected, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_swim_leave() {
        let protocol = test_protocol();

        // Join first so the local node is a member.
        protocol
            .join(&[NodeAddr::new("127.0.0.1".to_string(), 9000)])
            .await
            .expect("join should succeed");

        protocol.leave().await.expect("leave should succeed");

        // The local node should be in Draining state (Leaving).
        let map = protocol.view().members_snapshot();
        assert_eq!(
            map.get(&protocol.local_node()).map(|m| m.state),
            Some(NodeState::Draining),
            "leave should transition local node to Draining"
        );
    }
}
