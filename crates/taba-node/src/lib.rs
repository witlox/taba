//! Per-node daemon: WAL persistence, reconciliation, health, and
//! operational mode management.
//!
//! This crate owns what happens on a single node: converging actual
//! state toward desired state (reconciliation), persisting mutations
//! to the WAL, reporting health, and managing operational mode
//! transitions (Normal, Degraded, Recovery).
//!
//! ## Modules
//!
//! - [`error`] — [`NodeError`], [`WalPosition`]
//! - [`mode`] — [`OperationalMode`], [`ModeManager`] trait,
//!   [`DefaultModeManager`]
//! - [`proto`] — protobuf message types for WAL serialization (DL-014)
//! - [`wal`] — [`WalManager`] trait, [`DiskWalManager`],
//!   [`WalEntry`], [`WalConfig`]
//! - [`runtime`] — [`RuntimeExecutor`] trait,
//!   [`SimulatedRuntime`], [`DockerRuntime`]
//! - [`reconciliation`] — [`Reconciler`] trait,
//!   [`DefaultReconciler`], [`Drift`], [`LocalPlacement`]
//! - [`health`] — [`HealthReporter`] trait,
//!   [`DefaultHealthReporter`], [`HealthStatus`]
//! - [`discovery`] — [`CapabilityDiscoverer`] trait,
//!   [`DefaultCapabilityDiscoverer`]
//! - [`artifact`] — [`ArtifactFetcher`] trait,
//!   [`DefaultArtifactFetcher`], [`ArtifactRef`]
//! - [`spawner`] — [`TaskSpawner`] trait,
//!   [`DefaultTaskSpawner`]
//! - [`health_check`] — [`HealthCheckOrchestrator`] trait,
//!   [`DefaultHealthCheckOrchestrator`], [`HealthCheckResult`]
//!
//! [`NodeError`]: error::NodeError
//! [`WalPosition`]: error::WalPosition
//! [`OperationalMode`]: mode::OperationalMode
//! [`ModeManager`]: mode::ModeManager
//! [`WalManager`]: wal::WalManager
//! [`DiskWalManager`]: wal::DiskWalManager
//! [`WalEntry`]: wal::WalEntry
//! [`WalConfig`]: wal::WalConfig
//! [`RuntimeExecutor`]: runtime::RuntimeExecutor
//! [`SimulatedRuntime`]: runtime::SimulatedRuntime
//! [`DockerRuntime`]: runtime::DockerRuntime
//! [`Reconciler`]: reconciliation::Reconciler
//! [`DefaultReconciler`]: reconciliation::DefaultReconciler
//! [`Drift`]: reconciliation::Drift
//! [`LocalPlacement`]: reconciliation::LocalPlacement
//! [`HealthReporter`]: health::HealthReporter
//! [`DefaultHealthReporter`]: health::DefaultHealthReporter
//! [`HealthStatus`]: health::HealthStatus
//! [`CapabilityDiscoverer`]: discovery::CapabilityDiscoverer
//! [`DefaultCapabilityDiscoverer`]: discovery::DefaultCapabilityDiscoverer
//! [`ArtifactFetcher`]: artifact::ArtifactFetcher
//! [`DefaultArtifactFetcher`]: artifact::DefaultArtifactFetcher
//! [`ArtifactRef`]: artifact::ArtifactRef
//! [`TaskSpawner`]: spawner::TaskSpawner
//! [`HealthCheckOrchestrator`]: health_check::HealthCheckOrchestrator
//! [`DefaultHealthCheckOrchestrator`]: health_check::DefaultHealthCheckOrchestrator
//! [`HealthCheckResult`]: health_check::HealthCheckResult

#![warn(missing_docs)]
// async fn in traits is intentional for M3. The futures' Send-ness is
// determined by the concrete implementation, not the trait.
#![allow(async_fn_in_trait)]
// Many trait implementations are async because the trait will perform
// I/O in production (Docker, network, disk). Current implementations
// may be synchronous (simulated, in-memory) and not contain `.await`
// points yet.
#![allow(unknown_lints)]
// Mutex guards are short-lived and held only for a few operations.
// Explicit `drop()` calls add noise without meaningful benefit.
#![allow(clippy::significant_drop_tightening)]
// Futures returned by async trait methods may capture non-`Send`
// references (e.g., `&mut dyn FnMut` in `replay`). Send-ness is
// determined by the concrete implementation, not the trait.
#![allow(clippy::future_not_send)]

pub mod artifact;
pub mod discovery;
pub mod error;
pub mod eviction;
pub mod health;
pub mod health_check;
pub mod mode;
pub mod proto;
pub mod reconciliation;
pub mod runtime;
pub mod spawner;
pub mod wal;

// Re-export key public types at the crate root for convenience.
pub use artifact::{ArtifactFetcher, ArtifactRef, DefaultArtifactFetcher};
pub use discovery::{CapabilityDiscoverer, DefaultCapabilityDiscoverer};
pub use error::{NodeError, WalPosition};
pub use eviction::{EvictionPolicy, ShardLocation};
pub use health::{DefaultHealthReporter, HealthReporter, HealthStatus};
pub use health_check::{
    DefaultHealthCheckOrchestrator, HealthCheckOrchestrator, HealthCheckResult,
};
pub use mode::{DefaultModeManager, DegradedReason, ModeManager, OperationalMode};
pub use reconciliation::{DefaultReconciler, Drift, LocalPlacement, Reconciler};
pub use runtime::{DockerRuntime, RuntimeExecutor, RuntimeState, SimulatedRuntime};
pub use spawner::{DefaultTaskSpawner, TaskSpawner};
pub use wal::{DiskWalManager, WalConfig, WalEntry, WalEntryType, WalManager};
