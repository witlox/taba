//! Reed-Solomon erasure coding over GF(2^8) (DL-013), shard
//! distribution, reconstruction with backpressure (INV-R1, FM-13),
//! and re-coding when fleet size changes.
//!
//! Governance units are actively replicated (full copies on N nodes),
//! not just erasure-coded (INV-R6).
//!
//! ## Modules
//!
//! - [`error`] — [`ErasureError`], [`ShardGroupId`]
//! - [`params`] — [`ErasureParams`], [`compute_params`]
//! - [`shard`] — [`Shard`], [`ShardCriticality`], [`ShardAssignment`]
//! - [`coding`] — [`ErasureCoder`] trait, [`DefaultErasureCoder`]
//! - [`distribution`] — [`ShardManager`] trait, [`DefaultShardManager`],
//!   [`ShardFetcher`] trait, [`InMemoryShardFetcher`]
//! - [`reconstruction`] — [`ReconstructionScheduler`] trait,
//!   [`DefaultReconstructionScheduler`], [`ReconstructionRequest`],
//!   [`ReconstructionReason`], [`ReconstructionPriority`],
//!   [`BackpressureState`], [`ReconstructionStatus`],
//!   [`ReconstructionJob`]
//!
//! ## Timestamp resolution
//!
//! Event timestamps use [`DualClockEvent`] (A002: the name "Timestamp"
//! was ambiguous — wall time? logical? both? `DualClockEvent` is
//! explicit). Deadlines use [`WallTime`]. This is consistent with
//! M1+M2.
//!
//! [`ErasureError`]: error::ErasureError
//! [`ShardGroupId`]: error::ShardGroupId
//! [`ErasureParams`]: params::ErasureParams
//! [`compute_params`]: params::compute_params
//! [`Shard`]: shard::Shard
//! [`ShardCriticality`]: shard::ShardCriticality
//! [`ShardAssignment`]: shard::ShardAssignment
//! [`ErasureCoder`]: coding::ErasureCoder
//! [`DefaultErasureCoder`]: coding::DefaultErasureCoder
//! [`ShardManager`]: distribution::ShardManager
//! [`DefaultShardManager`]: distribution::DefaultShardManager
//! [`ShardFetcher`]: distribution::ShardFetcher
//! [`InMemoryShardFetcher`]: distribution::InMemoryShardFetcher
//! [`ReconstructionScheduler`]: reconstruction::ReconstructionScheduler
//! [`DefaultReconstructionScheduler`]: reconstruction::DefaultReconstructionScheduler
//! [`ReconstructionRequest`]: reconstruction::ReconstructionRequest
//! [`ReconstructionReason`]: reconstruction::ReconstructionReason
//! [`ReconstructionPriority`]: reconstruction::ReconstructionPriority
//! [`BackpressureState`]: reconstruction::BackpressureState
//! [`ReconstructionStatus`]: reconstruction::ReconstructionStatus
//! [`ReconstructionJob`]: reconstruction::ReconstructionJob
//! [`DualClockEvent`]: taba_common::DualClockEvent
//! [`WallTime`]: taba_common::WallTime

#![warn(missing_docs)]
// async fn in traits is intentional for M4. The futures' Send-ness is
// determined by the concrete implementation, not the trait.
#![allow(async_fn_in_trait, clippy::unused_async)]
// Many trait implementations are async because the trait will perform
// I/O in production (network, disk). Current implementations are
// synchronous (in-memory) and do not contain `.await` points yet.
#![allow(unknown_lints)]
// Futures returned by async trait methods may not be Send when used
// with in-memory mutexes. Send-ness is determined by the concrete
// implementation, not the trait.
#![allow(clippy::future_not_send)]
// Mutex guards are short-lived and held only for a few operations.
// Explicit `drop()` calls add noise without meaningful benefit.
#![allow(clippy::significant_drop_tightening)]

pub mod coding;
pub mod distribution;
pub mod error;
pub mod params;
pub mod reconstruction;
pub mod shard;

// Error
pub use error::{ErasureError, ShardGroupId};

// Parameters
pub use params::{ErasureParams, compute_params};

// Shard
pub use shard::{Shard, ShardAssignment, ShardCriticality};

// Coding
pub use coding::{DefaultErasureCoder, ErasureCoder};

// Distribution
pub use distribution::{DefaultShardManager, InMemoryShardFetcher, ShardFetcher, ShardManager};

// Reconstruction
pub use reconstruction::{
    BackpressureState, DefaultReconstructionScheduler, ReconstructionJob, ReconstructionPriority,
    ReconstructionReason, ReconstructionRequest, ReconstructionScheduler, ReconstructionStatus,
};
