//! End-to-end UDP transport tests.
//!
//! These tests bind real `tokio::net::UdpSocket`s on the loopback
//! interface (`127.0.0.1:0` → ephemeral ports) and verify that signed
//! [`GossipMessage`] envelopes survive serialization, a UDP round-trip,
//! and deserialization. They are marked `#[ignore = "slow:requires-network"]`
//! because they exercise real socket I/O that is not deterministic on a
//! loaded CI runner.
//!
//! Run with:
//! ```text
//! cargo test -p taba-gossip --test udp -- --ignored
//! ```

use std::time::Duration;

use taba_common::{DualClockEvent, LogicalClock, NodeId, WallTime};
use taba_gossip::{GossipMessage, GossipPayload, GossipTransport, NodeAddr, UdpTransport};
use taba_security::Signature;

/// Builds a minimal signed gossip message for transport testing.
///
/// The signature is a fixed zero array — the transport does not verify
/// signatures (that is [`taba_gossip::MembershipProtocol`]'s job), it
/// only round-trips the bytes.
fn test_message(seq: u64) -> GossipMessage {
    GossipMessage {
        sender: NodeId(uuid::Uuid::new_v4()),
        signature: Signature::from_bytes([0u8; 64]),
        payload: GossipPayload::Ack {
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

/// Binds a [`UdpTransport`] to `127.0.0.1:0` and returns the transport
/// plus the ephemeral address the OS assigned.
async fn bind_ephemeral() -> (UdpTransport, NodeAddr) {
    let transport = UdpTransport::new();
    transport
        .bind(&NodeAddr::new("127.0.0.1".to_string(), 0))
        .await
        .expect("bind should succeed on 127.0.0.1:0");

    let addr = transport
        .local_addr()
        .expect("local_addr should be available after bind");
    (transport, NodeAddr::from(addr))
}

/// Two transports on ephemeral ports: A sends to B, B receives and the
/// round-tripped message matches the original (same sequence + sender).
#[tokio::test]
#[ignore = "slow:requires-network"]
async fn scenario_udp_transport_round_trip() {
    let (transport_a, _) = bind_ephemeral().await;
    let (transport_b, addr_b) = bind_ephemeral().await;

    let message = test_message(7);
    transport_a
        .send(&addr_b, &message)
        .await
        .expect("send from A to B should succeed");

    let received = tokio::time::timeout(Duration::from_secs(5), transport_b.recv())
        .await
        .expect("recv should not time out")
        .expect("recv should succeed");

    assert_eq!(received.sequence, 7, "sequence should round-trip");
    assert_eq!(received.sender, message.sender, "sender should round-trip");
    assert_eq!(
        received.signature, message.signature,
        "signature should round-trip"
    );
    assert_eq!(
        received.sent_at.logical_clock, message.sent_at.logical_clock,
        "sent_at should round-trip"
    );

    match &received.payload {
        GossipPayload::Ack { probe_id, .. } => {
            assert_eq!(*probe_id, 7u64, "probe_id should round-trip");
        }
        other => panic!("expected Ack payload, got {other:?}"),
    }
}

/// `send_many` from A to [B, C]: both B and C should independently
/// receive the same message.
#[tokio::test]
#[ignore = "slow:requires-network"]
async fn scenario_udp_transport_send_many_fanout() {
    let (transport_a, _) = bind_ephemeral().await;
    let (transport_b, addr_b) = bind_ephemeral().await;
    let (transport_c, addr_c) = bind_ephemeral().await;

    let message = test_message(42);
    let failures = transport_a
        .send_many(&[addr_b.clone(), addr_c.clone()], &message)
        .await;
    assert!(
        failures.is_empty(),
        "send_many should have no failures: {failures:?}"
    );

    let recv_b = tokio::time::timeout(Duration::from_secs(5), transport_b.recv())
        .await
        .expect("B recv should not time out")
        .expect("B recv should succeed");
    assert_eq!(recv_b.sequence, 42, "B should receive sequence 42");

    let recv_c = tokio::time::timeout(Duration::from_secs(5), transport_c.recv())
        .await
        .expect("C recv should not time out")
        .expect("C recv should succeed");
    assert_eq!(recv_c.sequence, 42, "C should receive sequence 42");

    // Both receivers see the same sender — the message was fanned out
    // identically to both peers.
    assert_eq!(
        recv_b.sender, recv_c.sender,
        "both receivers see same sender"
    );
}
