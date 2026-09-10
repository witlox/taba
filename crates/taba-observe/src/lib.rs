//! Decision trails, structured events, health aggregation, and
//! integration export for taba.
//!
//! Cross-cutting observability. Structural observability (decision
//! trails, promotion audit) is recorded in the graph. Integration
//! observability (Prometheus, OpenTelemetry, webhooks) exports to
//! external systems.
//!
//! ## Architecture
//!
//! - [`trail`] — decision trail recording, storage, query (INV-O1,
//!   INV-O2)
//! - [`replay`] — solver replay from historical trail (deterministic,
//!   INV-C3)
//! - [`events`] — structured event types, emission, buffering
//! - [`health`] — health check result aggregation, status computation
//!   (INV-O3)
//! - [`prometheus`] — Prometheus exposition format endpoint
//! - [`alert`] — webhook dispatch, best-effort delivery
//! - [`error`] — [`ObserveError`] error type
//!
//! ## Key invariants
//!
//! - **INV-O1**: Every solver run produces a decision trail.
//! - **INV-O2**: Decision trail retention defaults to
//!   since-last-compaction.
//! - **INV-O3**: Progressive health checks — aggregation side.
//! - **INV-C3**: Solver replay produces the exact same result
//!   (deterministic solver).
//!
//! ## M3 status
//!
//! All state is in-memory and lost on restart. Future milestones
//! will persist trails via the WAL (M3+), add real HTTP dispatch
//! (M5), and OpenTelemetry integration (M5+).
//!
//! ## Timestamp resolution
//!
//! The spec data-models reference `Timestamp` which does not exist in
//! taba-common (renamed per design decision A002). This crate uses
//! [`DualClockEvent`](taba_common::DualClockEvent) for event
//! timestamps and [`WallTime`](taba_common::WallTime) for deadlines,
//! consistent with M1+M2.

#![warn(missing_docs)]

pub mod alert;
pub mod error;
pub mod events;
pub mod health;
pub mod prometheus;
pub mod replay;
pub mod trail;

// Re-export key public types at the crate root for convenience.

// Error
pub use error::ObserveError;

// Trail
pub use trail::{
    ConflictRecord, DecisionTrail, DecisionTrailId, DecisionTrailQuery, DecisionTrailRecorder,
    DefaultDecisionTrailRecorder, PlacementRecord, ResourceSnapshotRef,
};

// Replay
pub use replay::{DefaultSolverReplayer, SolverReplay};

// Events
pub use events::{DefaultEventEmitter, EventEmitter, EventType, StructuredEvent};

// Health
pub use health::{DefaultHealthAggregator, HealthAggregator, HealthStatus};

// Prometheus
pub use prometheus::{DefaultPrometheusExporter, NodeMetrics, PrometheusExporter};

// Alert
pub use alert::{AlertDispatcher, AlertPayload, DefaultAlertDispatcher};
