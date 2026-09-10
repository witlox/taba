//! Foundation types and infrastructure shared by every taba crate.
//!
//! Owns identity newtypes, configuration types, logical/wall clocks,
//! and fixed-point arithmetic. No domain logic — that lives in
//! taba-core and above.
//!
//! ## Modules
//!
//! - [`types`] — identity newtypes ([`UnitId`], [`NodeId`], …), temporal
//!   primitives ([`LogicalClock`], [`WallTime`], [`DualClockEvent`]),
//!   fixed-point ([`Ppm`], [`SignedPpm`]), and value types.
//! - [`config`] — cluster and node configuration ([`ClusterConfig`],
//!   [`NodeConfig`], [`ArchiveBackendConfig`]).
//! - [`error`] — [`CommonError`], the foundation error type.

pub mod config;
pub mod error;
pub mod types;

pub use config::*;
pub use error::*;
pub use types::*;
