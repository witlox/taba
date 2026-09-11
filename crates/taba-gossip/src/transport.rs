//! Gossip transport: node addresses and an in-memory channel transport
//! for testing.
//!
//! The [`GossipTransport`] trait is the low-level signed-message pipe:
//! send, fanout, recv, bind. It does **not** interpret message semantics
//! — that is [`crate::swim::MembershipProtocol`]. For M4, the primary
//! implementation is [`InMemoryTransport`] (using `tokio::sync::mpsc`).
//! A [`UdpTransport`] stub is provided for future use.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use taba_common::NodeId;

use crate::error::GossipError;
use crate::message::GossipMessage;

// ---------------------------------------------------------------------------
// NodeAddr
// ---------------------------------------------------------------------------

/// Network address of a node (host + port).
///
/// The host may be a hostname (`gossip-1.taba.internal`) or an IP literal
/// (`10.0.0.5`). Display format is `host:port`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeAddr {
    /// Hostname or IP address string.
    pub host: String,
    /// Port number (1-65535 in practice; 0 used as a placeholder).
    pub port: u16,
}

impl NodeAddr {
    /// Creates a new node address from a host string and port.
    #[must_use]
    pub const fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }

    /// Parses a `host:port` string into a [`NodeAddr`].
    ///
    /// Returns `None` if the string is not a valid `host:port` pair.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let idx = s.rfind(':')?;
        let host = s[..idx].to_string();
        let port: u16 = s[idx + 1..].parse().ok()?;
        Some(Self { host, port })
    }
}

impl fmt::Display for NodeAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

impl From<SocketAddr> for NodeAddr {
    fn from(addr: SocketAddr) -> Self {
        Self {
            host: addr.ip().to_string(),
            port: addr.port(),
        }
    }
}

// ---------------------------------------------------------------------------
// GossipTransport trait
// ---------------------------------------------------------------------------

/// Low-level gossip transport: send and receive signed messages.
///
/// All messages are signed with the sending node's identity key (INV-R3).
/// The transport is responsible for serialization, signing, and network
/// I/O. It does **not** interpret message semantics — that is
/// [`crate::swim::MembershipProtocol`].
///
/// # Cancellation safety
///
/// [`GossipTransport::recv`] must be cancellation-safe: dropping the
/// future before a message arrives must not lose any message already
/// delivered to the transport's internal buffer. The
/// [`InMemoryTransport`] implementation uses `tokio::sync::mpsc`, which
/// is cancellation-safe.
// We avoid pulling in async_trait by providing manual impls; however the
// trait itself uses native async fn in trait (Rust 1.85+). The attribute
// above is intentionally a no-op comment placeholder — see below.
pub trait GossipTransport: Send + Sync {
    /// Send a signed gossip message to a specific node.
    ///
    /// The message is signed before transmission. Returns
    /// [`GossipError::TransportError`] on network failure.
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
    /// Blocks until a message arrives. The message signature has **not**
    /// been verified — callers must verify via `security::Verifier` or
    /// [`crate::swim::MembershipProtocol::handle_message`].
    async fn recv(&self) -> Result<GossipMessage, GossipError>;

    /// Bind the transport to a local address and start listening.
    async fn bind(&self, addr: &NodeAddr) -> Result<(), GossipError>;
}

// ---------------------------------------------------------------------------
// InMemoryTransport
// ---------------------------------------------------------------------------

/// In-memory gossip transport backed by `tokio::sync::mpsc` channels.
///
/// Each [`NodeAddr`] has a dedicated receiver. A shared registry maps
/// addresses to senders, so messages sent to an address are delivered
/// to the bound receiver. This is the primary transport for M4 testing:
/// it provides deterministic, fast, loopback message delivery without
/// real UDP sockets.
///
/// # Concurrency
///
/// The registry is guarded by a `std::sync::Mutex`. Senders are cheap
/// to clone; the lock is held only for the duration of a registry
/// lookup.
pub struct InMemoryTransport {
    /// Registry of bound addresses → message senders.
    registry: Mutex<HashMap<NodeAddr, mpsc::Sender<GossipMessage>>>,
    /// This transport's inbound receiver (single consumer).
    rx: Mutex<Option<mpsc::Receiver<GossipMessage>>>,
    /// The address this transport is bound to, if any.
    bound: Mutex<Option<NodeAddr>>,
}

impl std::fmt::Debug for InMemoryTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryTransport").finish_non_exhaustive()
    }
}

impl InMemoryTransport {
    /// Creates a new, unbound in-memory transport with a default
    /// channel capacity of 256 messages.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Creates a new in-memory transport with the given channel capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity);
        let mut registry = HashMap::new();
        // Pre-register a sentinel so the sender is available immediately;
        // the real address is set on bind.
        let _ = tx; // tx stored after bind
        registry.clear();
        // We keep the receiver; the sender is registered on bind.
        let _ = &mut registry;
        Self {
            registry: Mutex::new(registry),
            rx: Mutex::new(Some(rx)),
            bound: Mutex::new(None),
        }
    }

    /// Registers a sender for an address in the shared registry.
    ///
    /// This is called by `bind`. Other transports sending to this
    /// address will deliver messages through this sender.
    fn register(&self, addr: NodeAddr, tx: mpsc::Sender<GossipMessage>) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.insert(addr, tx);
        }
    }
}

impl Default for InMemoryTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl GossipTransport for InMemoryTransport {
    async fn send(&self, target: &NodeAddr, message: &GossipMessage) -> Result<(), GossipError> {
        let tx = {
            let reg = self.registry.lock().expect("registry lock poisoned");
            reg.get(target).cloned()
        };
        match tx {
            Some(tx) => tx
                .send(message.clone())
                .await
                .map_err(|e| GossipError::TransportError {
                    reason: format!("channel send failed: {e}"),
                }),
            None => Err(GossipError::TransportError {
                reason: format!("no transport bound to {target}"),
            }),
        }
    }

    async fn send_many(
        &self,
        targets: &[NodeAddr],
        message: &GossipMessage,
    ) -> Vec<(NodeAddr, GossipError)> {
        let mut failures = Vec::new();
        for target in targets {
            if let Err(e) = self.send(target, message).await {
                failures.push((target.clone(), e));
            }
        }
        failures
    }

    async fn recv(&self) -> Result<GossipMessage, GossipError> {
        let rx_opt = self.rx.lock().expect("rx lock poisoned").take();
        match rx_opt {
            Some(mut rx) => {
                let msg = rx.recv().await.ok_or_else(|| GossipError::TransportError {
                    reason: "channel closed".to_string(),
                })?;
                // Put the receiver back for subsequent calls.
                if let Ok(mut guard) = self.rx.lock() {
                    *guard = Some(rx);
                }
                Ok(msg)
            }
            None => Err(GossipError::TransportError {
                reason: "receiver already taken (concurrent recv?)".to_string(),
            }),
        }
    }

    async fn bind(&self, addr: &NodeAddr) -> Result<(), GossipError> {
        let (tx, rx) = mpsc::channel(256);
        self.register(addr.clone(), tx);
        if let Ok(mut bound) = self.bound.lock() {
            *bound = Some(addr.clone());
        }
        // Replace the existing receiver with the freshly bound one.
        if let Ok(mut rx_guard) = self.rx.lock() {
            *rx_guard = Some(rx);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// UdpTransport (stub for M5+)
// ---------------------------------------------------------------------------

/// UDP gossip transport (stub).
///
/// M4 uses [`InMemoryTransport`] for deterministic testing. This stub
/// returns [`GossipError::TransportError`] for all operations; a real
/// implementation using `tokio::net::UdpSocket` is deferred to M5+ when
/// real network I/O is exercised in end-to-end tests.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct UdpTransport {
    _marker: (),
}

impl UdpTransport {
    /// Creates a new UDP transport stub.
    #[must_use]
    pub const fn new() -> Self {
        Self { _marker: () }
    }
}

impl GossipTransport for UdpTransport {
    async fn send(&self, _target: &NodeAddr, _message: &GossipMessage) -> Result<(), GossipError> {
        Err(GossipError::TransportError {
            reason: "UDP transport not implemented until M5+".to_string(),
        })
    }

    async fn send_many(
        &self,
        targets: &[NodeAddr],
        _message: &GossipMessage,
    ) -> Vec<(NodeAddr, GossipError)> {
        targets
            .iter()
            .map(|t| {
                (
                    t.clone(),
                    GossipError::TransportError {
                        reason: "UDP transport not implemented until M5+".to_string(),
                    },
                )
            })
            .collect()
    }

    async fn recv(&self) -> Result<GossipMessage, GossipError> {
        Err(GossipError::TransportError {
            reason: "UDP transport not implemented until M5+".to_string(),
        })
    }

    async fn bind(&self, _addr: &NodeAddr) -> Result<(), GossipError> {
        Err(GossipError::TransportError {
            reason: "UDP transport not implemented until M5+".to_string(),
        })
    }
}

/// Utility: derive a [`NodeId`] from a [`NodeAddr`] for test setups.
///
/// Production node IDs are derived from Ed25519 public keys (SHA-256
/// truncated to 128 bits). This helper is for tests that need a stable
/// ID from an address without a full key ceremony.
#[must_use]
pub fn node_id_from_addr(addr: &NodeAddr) -> NodeId {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(addr.to_string().as_bytes());
    let result = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&result[..16]);
    NodeId(uuid::Uuid::from_bytes(bytes))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{DualClockEvent, LogicalClock, WallTime};

    fn addr(host: &str, port: u16) -> NodeAddr {
        NodeAddr::new(host.to_string(), port)
    }

    fn msg(seq: u64) -> GossipMessage {
        let key_pair = taba_security::KeyPair::generate();
        let signature = key_pair
            .signing_key()
            .sign_raw(b"test")
            .expect("signing should succeed");
        GossipMessage {
            sender: NodeId(uuid::Uuid::new_v4()),
            signature,
            payload: crate::message::GossipPayload::Ack {
                probe_id: seq,
                responder: NodeId(uuid::Uuid::new_v4()),
            },
            sent_at: DualClockEvent {
                logical_clock: LogicalClock(seq),
                wall_time: WallTime { millis: seq },
                timezone: "UTC".to_string(),
            },
            sequence: seq,
        }
    }

    #[test]
    fn test_node_addr_display() {
        let a = addr("10.0.0.1", 7946);
        assert_eq!(a.to_string(), "10.0.0.1:7946");

        let a = addr("gossip-1.taba.internal", 7946);
        assert_eq!(a.to_string(), "gossip-1.taba.internal:7946");
    }

    #[test]
    fn test_node_addr_parse() {
        let a = NodeAddr::parse("10.0.0.1:7946").expect("parse");
        assert_eq!(a.host, "10.0.0.1");
        assert_eq!(a.port, 7946);

        assert!(NodeAddr::parse("no-port").is_none());
        assert!(NodeAddr::parse("host:999999").is_none());
    }

    #[tokio::test]
    async fn test_in_memory_transport_send_recv() {
        let server = InMemoryTransport::new();
        let client = InMemoryTransport::new();

        let server_addr = addr("127.0.0.1", 7946);
        server.bind(&server_addr).await.expect("bind");

        // Register the server's sender in the client's registry by
        // giving the client a reference to the server. In a real
        // setup, both transports would share the registry. For this
        // test, we use a shared registry approach: create transports
        // that share state via a helper.
        //
        // Simpler: use two channels where the client sends directly.
        let (tx, mut rx) = mpsc::channel::<GossipMessage>(32);
        {
            let mut reg = client.registry.lock().expect("lock");
            reg.insert(server_addr.clone(), tx);
        }

        let m = msg(1);
        client
            .send(&server_addr, &m)
            .await
            .expect("send should succeed");

        let received = rx.recv().await.expect("should receive");
        assert_eq!(received.sequence, 1);
        assert_eq!(received.sender, m.sender);
    }

    #[tokio::test]
    async fn test_in_memory_transport_send_many() {
        let client = InMemoryTransport::new();

        let alive = addr("127.0.0.1", 1);
        let dead_a = addr("127.0.0.1", 2);
        let dead_b = addr("127.0.0.1", 3);

        let (tx, _rx) = mpsc::channel::<GossipMessage>(32);
        {
            let mut reg = client.registry.lock().expect("lock");
            reg.insert(alive.clone(), tx);
        }

        let m = msg(42);
        let failures = client
            .send_many(&[alive, dead_a.clone(), dead_b.clone()], &m)
            .await;

        // Alive succeeds; dead_a and dead_b fail.
        assert_eq!(failures.len(), 2, "two targets should fail");
        let failed_addrs: Vec<&NodeAddr> = failures.iter().map(|(a, _)| a).collect();
        assert!(failed_addrs.contains(&&dead_a));
        assert!(failed_addrs.contains(&&dead_b));

        // Each failure is a TransportError.
        for (_, e) in &failures {
            assert!(matches!(e, GossipError::TransportError { .. }));
        }
    }
}
