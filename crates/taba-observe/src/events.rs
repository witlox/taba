//! Structured event types, emission, and buffering.
//!
//! Every significant system action produces a [`StructuredEvent`]:
//! JSON-formatted, timestamped, and typed. Events are buffered
//! in-memory (M3) and forwarded to configured sinks in future
//! milestones (stdout, file, syslog, log aggregator).
//!
//! ## `EventType` extension
//!
//! The data-model spec defines 13 typed variants. A [`EventType::Custom`]
//! variant is added to bridge the string-based [`EventEmitter::emit`]
//! interface (which takes `event_type: &str`) with the typed
//! [`EventType`] enum. The `#[non_exhaustive]` attribute signals that
//! additional variants may be added — `Custom` is the first.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, LogicalClock, NodeId, WallTime};

use crate::error::ObserveError;

// ===========================================================================
// StructuredEvent
// ===========================================================================

/// Structured event emitted for external consumption.
///
/// Every significant system action produces one of these. Events are
/// JSON-serializable for forwarding to external sinks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredEvent {
    /// When this event occurred (dual-clock: logical + wall).
    pub timestamp: DualClockEvent,
    /// The node that produced this event.
    pub node_id: NodeId,
    /// Structured type classification for this event.
    pub event_type: EventType,
    /// Human-readable or machine-parseable detail.
    pub detail: String,
}

/// High-level event type classification.
///
/// All variants from `specs/architecture/data-models/observe.rs`, plus
/// a [`Custom`](Self::Custom) variant for string-based emission via
/// [`EventEmitter::emit`]. The `#[non_exhaustive]` attribute signals
/// that additional variants may be added in future milestones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventType {
    /// A unit was inserted into the graph.
    UnitInserted {
        /// The unit that was inserted.
        unit_id: taba_common::UnitId,
    },
    /// A unit was terminated.
    UnitTerminated {
        /// The unit that was terminated.
        unit_id: taba_common::UnitId,
    },
    /// The solver placed a unit on a node.
    PlacementDecided {
        /// The unit that was placed.
        unit_id: taba_common::UnitId,
        /// The node the unit was placed on.
        node_id: NodeId,
    },
    /// A conflict was detected during composition.
    ConflictDetected {
        /// Description of the conflict.
        conflict: String,
    },
    /// A promotion was applied to a unit.
    PromotionApplied {
        /// The unit that was promoted.
        unit_id: taba_common::UnitId,
        /// The environment promoted to (e.g., "staging", "prod").
        environment: String,
    },
    /// Drift was detected between desired and actual state.
    DriftDetected {
        /// The unit that drifted.
        unit_id: taba_common::UnitId,
        /// The expected state.
        expected: String,
        /// The actual state observed.
        actual: String,
    },
    /// A node's capability set changed.
    CapabilityChanged {
        /// The node whose capabilities changed.
        node_id: NodeId,
        /// Capabilities that were added.
        added: Vec<String>,
        /// Capabilities that were removed.
        removed: Vec<String>,
    },
    /// A health check result was recorded.
    HealthCheckResult {
        /// The unit that was checked.
        unit_id: taba_common::UnitId,
        /// Whether the unit is healthy.
        healthy: bool,
    },
    /// A task was spawned by a parent unit.
    TaskSpawned {
        /// The parent unit that spawned the task.
        parent: taba_common::UnitId,
        /// The child task unit.
        child: taba_common::UnitId,
    },
    /// A task was terminated.
    TaskTerminated {
        /// The task unit that was terminated.
        unit_id: taba_common::UnitId,
        /// Why the task was terminated.
        reason: String,
    },
    /// A graph compaction was triggered.
    CompactionTriggered {
        /// Number of units compacted.
        units_compacted: u32,
    },
    /// The node entered degraded mode.
    DegradedModeEntered {
        /// Why degraded mode was entered.
        reason: String,
    },
    /// A revocation was merged into the graph.
    RevocationMerged {
        /// The author whose key was revoked.
        author_id: String,
    },
    /// A custom event type identified by name.
    ///
    /// Used by the string-based [`EventEmitter::emit`] interface.
    /// Callers that need typed events should use
    /// [`DefaultEventEmitter::emit_typed`].
    Custom {
        /// The event type name.
        name: String,
    },
}

// ===========================================================================
// EventEmitter trait
// ===========================================================================

/// Emits structured events for external consumption.
///
/// Every significant system action produces a structured event. Events
/// are formatted as JSON and forwarded to configured sinks (stdout,
/// file, syslog, log aggregator).
///
/// Event emission is non-blocking: events are queued internally and
/// returned immediately. Callers should log-and-continue on error —
/// event emission MUST NOT block system operations.
pub trait EventEmitter {
    /// Emit a structured event.
    ///
    /// Non-blocking: queues the event internally and returns
    /// immediately. Returns `Ok(())` if queued successfully.
    ///
    /// # Errors
    ///
    /// Returns [`ObserveError`] if the internal queue is full
    /// (backpressure). Callers should log-and-continue — event
    /// emission MUST NOT block system operations.
    fn emit(&self, event_type: &str, detail: &str) -> Result<(), ObserveError>;
}

// ===========================================================================
// DefaultEventEmitter
// ===========================================================================

/// Default in-memory implementation of [`EventEmitter`].
///
/// Events are stored in a <code>[Mutex]&lt;[Vec]&lt;[StructuredEvent]&gt;&gt;</code>.
/// The emitter auto-increments a logical clock for each event's
/// timestamp. For M3, all events are in-memory and lost on restart.
///
/// # Example
///
/// ```
/// use taba_observe::{DefaultEventEmitter, EventEmitter};
/// use taba_common::NodeId;
/// use uuid::Uuid;
///
/// let emitter = DefaultEventEmitter::new(NodeId(Uuid::nil()));
/// emitter.emit("unit_inserted", "unit abc123 inserted").expect("emit");
/// assert_eq!(emitter.buffered_count(), 1);
/// ```
pub struct DefaultEventEmitter {
    /// The node ID for events produced by this emitter.
    node_id: NodeId,
    /// Buffered events awaiting consumption.
    events: Mutex<Vec<StructuredEvent>>,
    /// Next logical clock for event timestamps.
    next_lc: AtomicU64,
}

impl DefaultEventEmitter {
    /// Creates a new in-memory event emitter for the given node.
    ///
    /// Logical clocks start at 1. Wall time is set to 0 for M3
    /// (no real wall clock integration).
    #[must_use]
    pub const fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            events: Mutex::new(Vec::new()),
            next_lc: AtomicU64::new(1),
        }
    }

    /// Allocates the next timestamp for an event.
    fn alloc_timestamp(&self) -> DualClockEvent {
        let lc = self.next_lc.fetch_add(1, Ordering::SeqCst);
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        }
    }

    /// Emits a typed structured event.
    ///
    /// Unlike [`EventEmitter::emit`] (which takes a string and creates
    /// a [`EventType::Custom`] variant), this method accepts a fully
    /// typed [`EventType`], preserving structured data.
    ///
    /// # Errors
    ///
    /// Returns [`ObserveError`] if the internal buffer is full.
    pub fn emit_typed(&self, event_type: EventType, detail: &str) -> Result<(), ObserveError> {
        let event = StructuredEvent {
            timestamp: self.alloc_timestamp(),
            node_id: self.node_id,
            event_type,
            detail: detail.to_string(),
        };
        self.events
            .lock()
            .expect("events mutex should not be poisoned")
            .push(event);
        Ok(())
    }

    /// Returns the number of buffered events.
    ///
    /// Useful for testing and monitoring buffer pressure.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
        self.events
            .lock()
            .expect("events mutex should not be poisoned")
            .len()
    }

    /// Drains all buffered events, returning them and clearing the
    /// buffer.
    ///
    /// After calling this method, [`buffered_count`](Self::buffered_count)
    /// will return 0. The returned events are in insertion order.
    #[must_use]
    pub fn drain(&self) -> Vec<StructuredEvent> {
        let mut events = self
            .events
            .lock()
            .expect("events mutex should not be poisoned");
        std::mem::take(&mut *events)
    }
}

impl EventEmitter for DefaultEventEmitter {
    fn emit(&self, event_type: &str, detail: &str) -> Result<(), ObserveError> {
        self.emit_typed(
            EventType::Custom {
                name: event_type.to_string(),
            },
            detail,
        )
    }
}

impl std::fmt::Debug for DefaultEventEmitter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultEventEmitter")
            .field("node_id", &self.node_id)
            .field("buffered_count", &self.buffered_count())
            .finish_non_exhaustive()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::UnitId;
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn test_dual_clock(lc: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        }
    }

    // -- Required tests -----------------------------------------------------

    #[test]
    fn test_emit_creates_event() {
        let emitter = DefaultEventEmitter::new(test_node_id());

        assert_eq!(emitter.buffered_count(), 0);

        emitter
            .emit("unit_inserted", "unit abc123 was inserted")
            .expect("emit should succeed");

        assert_eq!(emitter.buffered_count(), 1);

        let events = emitter.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].detail, "unit abc123 was inserted");
        assert_eq!(
            events[0].event_type,
            EventType::Custom {
                name: "unit_inserted".to_string()
            }
        );
    }

    #[test]
    fn test_emit_many_events() {
        let emitter = DefaultEventEmitter::new(test_node_id());

        for i in 0..100 {
            emitter
                .emit(&format!("event_{i}"), &format!("detail {i}"))
                .expect("emit should succeed");
        }

        assert_eq!(emitter.buffered_count(), 100);

        let events = emitter.drain();
        assert_eq!(events.len(), 100);

        // Verify events are in insertion order with incrementing LCs.
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.timestamp.logical_clock, LogicalClock((i + 1) as u64));
        }
    }

    #[test]
    fn test_drain_returns_and_clears() {
        let emitter = DefaultEventEmitter::new(test_node_id());

        emitter.emit("a", "detail a").expect("emit");
        emitter.emit("b", "detail b").expect("emit");
        emitter.emit("c", "detail c").expect("emit");

        assert_eq!(emitter.buffered_count(), 3);

        let events = emitter.drain();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].detail, "detail a");
        assert_eq!(events[1].detail, "detail b");
        assert_eq!(events[2].detail, "detail c");

        // Buffer should be empty after drain.
        assert_eq!(emitter.buffered_count(), 0);

        // Subsequent emit should start fresh.
        emitter.emit("d", "detail d").expect("emit");
        assert_eq!(emitter.buffered_count(), 1);
    }

    #[test]
    fn test_event_type_all_variants() {
        let uid = test_unit_id();
        let nid = test_node_id();

        let variants = vec![
            EventType::UnitInserted { unit_id: uid },
            EventType::UnitTerminated { unit_id: uid },
            EventType::PlacementDecided {
                unit_id: uid,
                node_id: nid,
            },
            EventType::ConflictDetected {
                conflict: "storage incompatibility".to_string(),
            },
            EventType::PromotionApplied {
                unit_id: uid,
                environment: "staging".to_string(),
            },
            EventType::DriftDetected {
                unit_id: uid,
                expected: "running".to_string(),
                actual: "stopped".to_string(),
            },
            EventType::CapabilityChanged {
                node_id: nid,
                added: vec!["oci".to_string()],
                removed: vec!["wasm".to_string()],
            },
            EventType::HealthCheckResult {
                unit_id: uid,
                healthy: true,
            },
            EventType::TaskSpawned {
                parent: uid,
                child: test_unit_id(),
            },
            EventType::TaskTerminated {
                unit_id: uid,
                reason: "completed".to_string(),
            },
            EventType::CompactionTriggered {
                units_compacted: 42,
            },
            EventType::DegradedModeEntered {
                reason: "memory limit".to_string(),
            },
            EventType::RevocationMerged {
                author_id: "author-abc".to_string(),
            },
            EventType::Custom {
                name: "custom_event".to_string(),
            },
        ];

        assert_eq!(variants.len(), 14, "should test all EventType variants");

        for variant in &variants {
            let json = serde_json::to_string(variant).expect("serialize EventType");
            let decoded: EventType = serde_json::from_str(&json).expect("deserialize EventType");
            assert_eq!(*variant, decoded, "roundtrip failed for: {variant:?}");
        }
    }

    #[test]
    fn test_emit_typed_preserves_structured_data() {
        let emitter = DefaultEventEmitter::new(test_node_id());
        let unit_id = test_unit_id();
        let node_id = test_node_id();

        emitter
            .emit_typed(
                EventType::PlacementDecided { unit_id, node_id },
                "placed on highest-scoring node",
            )
            .expect("emit_typed should succeed");

        let events = emitter.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].event_type,
            EventType::PlacementDecided { unit_id, node_id }
        );
    }

    #[test]
    fn test_structured_event_serialization_roundtrip() {
        let event = StructuredEvent {
            timestamp: test_dual_clock(42),
            node_id: test_node_id(),
            event_type: EventType::UnitInserted {
                unit_id: test_unit_id(),
            },
            detail: "unit xyz was inserted at lc=42".to_string(),
        };

        let json = serde_json::to_string(&event).expect("serialize StructuredEvent");
        let decoded: StructuredEvent =
            serde_json::from_str(&json).expect("deserialize StructuredEvent");
        assert_eq!(event, decoded);
    }

    // -- Property tests -----------------------------------------------------

    use proptest::prelude::*;

    fn arb_event_type(variant: u8) -> EventType {
        let uid = UnitId(Uuid::nil());
        let nid = NodeId(Uuid::nil());
        match variant % 14 {
            0 => EventType::UnitInserted { unit_id: uid },
            1 => EventType::UnitTerminated { unit_id: uid },
            2 => EventType::PlacementDecided {
                unit_id: uid,
                node_id: nid,
            },
            3 => EventType::ConflictDetected {
                conflict: "test".to_string(),
            },
            4 => EventType::PromotionApplied {
                unit_id: uid,
                environment: "dev".to_string(),
            },
            5 => EventType::DriftDetected {
                unit_id: uid,
                expected: "a".to_string(),
                actual: "b".to_string(),
            },
            6 => EventType::CapabilityChanged {
                node_id: nid,
                added: vec![],
                removed: vec![],
            },
            7 => EventType::HealthCheckResult {
                unit_id: uid,
                healthy: true,
            },
            8 => EventType::TaskSpawned {
                parent: uid,
                child: uid,
            },
            9 => EventType::TaskTerminated {
                unit_id: uid,
                reason: "done".to_string(),
            },
            10 => EventType::CompactionTriggered { units_compacted: 0 },
            11 => EventType::DegradedModeEntered {
                reason: "test".to_string(),
            },
            12 => EventType::RevocationMerged {
                author_id: "a".to_string(),
            },
            _ => EventType::Custom {
                name: "custom".to_string(),
            },
        }
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_event_serialization_roundtrip(variant in 0u8..14) {
            let event_type = arb_event_type(variant);
            let event = StructuredEvent {
                timestamp: test_dual_clock(u64::from(variant)),
                node_id: NodeId(Uuid::nil()),
                event_type,
                detail: "proptest detail".to_string(),
            };
            let json = serde_json::to_string(&event).expect("serialize");
            let decoded: StructuredEvent = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(event, decoded);
        }
    }
}
