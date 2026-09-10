//! Webhook alert dispatch, retry, and best-effort delivery.
//!
//! The [`AlertDispatcher`] fires webhooks on significant events:
//! degraded mode entry, policy conflicts, promotion failures. Alert
//! delivery is best-effort — failure does not block system operations.
//!
//! ## M3 limitation
//!
//! M3: in-memory logging only. HTTP dispatch deferred to M5 (Usable).
//! The [`DefaultAlertDispatcher`] logs alerts via `tracing::warn!` and
//! returns `Ok(())`. This allows the alert dispatch interface to be
//! wired into the node lifecycle without a real HTTP client.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, NodeId};

use crate::error::ObserveError;

// ===========================================================================
// AlertPayload
// ===========================================================================

/// Webhook alert payload.
///
/// Sent to configured webhook URLs on significant events. The
/// `event_type` and `reason` fields enable structured filtering on
/// the receiving end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertPayload {
    /// The node that triggered this alert.
    pub node_id: NodeId,
    /// Type of event that triggered the alert (e.g., `degraded_mode`,
    /// `policy_conflict`).
    pub event_type: String,
    /// Human-readable reason for the alert.
    pub reason: String,
    /// When the alert was generated.
    pub timestamp: DualClockEvent,
    /// Additional detail about the alert.
    pub detail: String,
}

// ===========================================================================
// AlertDispatcher trait
// ===========================================================================

/// Fires alerting webhooks on significant events.
///
/// Alert delivery is best-effort: failure to deliver does not block
/// the system operation. The dispatcher retries with exponential
/// backoff (up to 3 attempts) in future milestones.
///
/// # Cancellation safety
///
/// The `dispatch` method is cancellation-safe. If the caller cancels
/// the future, the alert may not be delivered, but no state is
/// corrupted.
pub trait AlertDispatcher {
    /// Send an alert webhook (best-effort).
    ///
    /// # Arguments
    ///
    /// * `url` — the webhook endpoint URL
    /// * `payload` — the alert payload to send
    ///
    /// # Errors
    ///
    /// Returns [`ObserveError::ExportFailed`] if the alert could not
    /// be delivered. Callers should log-and-continue — alert failure
    /// MUST NOT block system operations.
    fn dispatch(
        &self,
        url: &str,
        payload: &AlertPayload,
    ) -> impl std::future::Future<Output = Result<(), ObserveError>> + Send;
}

// ===========================================================================
// DefaultAlertDispatcher
// ===========================================================================

/// Default implementation of [`AlertDispatcher`] for M3.
///
/// M3: in-memory logging only. HTTP dispatch deferred to M5 (Usable).
///
/// Logs alerts via `tracing::warn!` and returns `Ok(())`. The
/// dispatched count is tracked via an atomic counter for testing
/// and monitoring.
#[derive(Debug)]
pub struct DefaultAlertDispatcher {
    /// Number of alerts dispatched since creation.
    dispatched_count: AtomicU64,
}

impl Default for DefaultAlertDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultAlertDispatcher {
    /// Creates a new alert dispatcher with zero dispatched count.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dispatched_count: AtomicU64::new(0),
        }
    }

    /// Returns the number of alerts dispatched since creation.
    ///
    /// Useful for testing and monitoring alert volume.
    #[must_use]
    pub fn dispatched_count(&self) -> u64 {
        self.dispatched_count.load(Ordering::SeqCst)
    }
}

impl AlertDispatcher for DefaultAlertDispatcher {
    fn dispatch(
        &self,
        url: &str,
        payload: &AlertPayload,
    ) -> impl std::future::Future<Output = Result<(), ObserveError>> + Send {
        tracing::warn!(
            node_id = ?payload.node_id,
            event_type = %payload.event_type,
            reason = %payload.reason,
            url = %url,
            detail = %payload.detail,
            "alert dispatched (M3: log-only, HTTP deferred to M5)"
        );

        self.dispatched_count.fetch_add(1, Ordering::SeqCst);

        std::future::ready(Ok(()))
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{LogicalClock, WallTime};
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn test_dual_clock(lc: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        }
    }

    fn sample_payload() -> AlertPayload {
        AlertPayload {
            node_id: test_node_id(),
            event_type: "degraded_mode".to_string(),
            reason: "memory limit exceeded".to_string(),
            timestamp: test_dual_clock(42),
            detail: "graph memory usage exceeded configured limit".to_string(),
        }
    }

    // -- Required tests -----------------------------------------------------

    #[tokio::test]
    async fn test_dispatch_succeeds() {
        let dispatcher = DefaultAlertDispatcher::new();
        let payload = sample_payload();

        let result = dispatcher
            .dispatch("https://hooks.example.com/alert", &payload)
            .await;

        assert!(result.is_ok(), "M3 dispatch should always succeed");
        assert_eq!(dispatcher.dispatched_count(), 1);
    }

    #[test]
    fn test_alert_payload_serialization_roundtrip() {
        let payload = sample_payload();

        let json = serde_json::to_string(&payload).expect("serialize AlertPayload");
        let decoded: AlertPayload = serde_json::from_str(&json).expect("deserialize AlertPayload");
        assert_eq!(payload, decoded);
    }

    #[tokio::test]
    async fn test_dispatched_count() {
        let dispatcher = DefaultAlertDispatcher::new();
        let payload = sample_payload();

        assert_eq!(dispatcher.dispatched_count(), 0);

        dispatcher
            .dispatch("https://example.com/hook1", &payload)
            .await
            .unwrap();
        assert_eq!(dispatcher.dispatched_count(), 1);

        dispatcher
            .dispatch("https://example.com/hook2", &payload)
            .await
            .unwrap();
        assert_eq!(dispatcher.dispatched_count(), 2);

        dispatcher
            .dispatch("https://example.com/hook3", &payload)
            .await
            .unwrap();
        assert_eq!(dispatcher.dispatched_count(), 3);
    }

    #[tokio::test]
    async fn test_dispatch_with_different_payloads() {
        let dispatcher = DefaultAlertDispatcher::new();

        let payload1 = AlertPayload {
            node_id: test_node_id(),
            event_type: "policy_conflict".to_string(),
            reason: "two policies for same conflict".to_string(),
            timestamp: test_dual_clock(10),
            detail: "involved units: [a, b]".to_string(),
        };

        let payload2 = AlertPayload {
            node_id: test_node_id(),
            event_type: "promotion_failed".to_string(),
            reason: "human approval required".to_string(),
            timestamp: test_dual_clock(20),
            detail: "unit xyz requires promotion gate approval".to_string(),
        };

        dispatcher
            .dispatch("https://hooks.example.com", &payload1)
            .await
            .unwrap();
        dispatcher
            .dispatch("https://hooks.example.com", &payload2)
            .await
            .unwrap();

        assert_eq!(dispatcher.dispatched_count(), 2);
    }
}
