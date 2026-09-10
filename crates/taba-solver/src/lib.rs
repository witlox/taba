//! Deterministic composition solver, placement scoring, conflict
//! detection, cycle detection, resource ranking, and promotion
//! evaluation.
//!
//! The solver is a pure function: graph snapshot + membership →
//! placements. All arithmetic is fixed-point [`Ppm`](taba_common::Ppm)
//! (10^6, u64). No floating-point anywhere (INV-C3, DL-004).
//! Composition result is order-independent (INV-C6).
//!
//! ## Architecture
//!
//! - [`solver`] — [`Solver`] trait and [`DefaultSolver`]
//! - [`conflict`] — [`ConflictDetector`] trait and
//!   [`DefaultConflictDetector`]
//! - [`scorer`] — [`PlacementScorer`] trait and
//!   [`DefaultPlacementScorer`]
//! - [`cycle`] — [`CycleDetector`] trait and [`DefaultCycleDetector`]
//! - [`filter`] — [`CapabilityFilter`] trait and
//!   [`DefaultCapabilityFilter`]
//! - [`resource`] — [`ResourceRanker`] trait and
//!   [`DefaultResourceRanker`]
//! - [`promotion`] — [`PromotionEvaluator`] trait and
//!   [`DefaultPromotionEvaluator`]
//! - [`membership`] — [`MembershipSnapshot`], [`NodeHealth`]
//! - [`placement`] — result types: [`SolverResult`], [`Placement`],
//!   [`PlacementDecision`], [`Conflict`], [`RecoveryCycle`], etc.
//! - [`error`] — [`SolverError`]
//!
//! ## Key invariants
//!
//! - **INV-C3**: The solver is deterministic. Same input = same
//!   output on any node. All arithmetic is fixed-point ppm.
//! - **INV-C6**: Composition is independent of insertion order.
//! - **INV-S2**: Security conflicts fail closed.
//! - **INV-K5**: Cyclic recovery dependencies fail closed.
//! - **INV-R5**: Suspected nodes receive a scoring penalty but are
//!   not removed from the placement pool.
//!
//! [`Ppm`]: taba_common::Ppm

#![warn(missing_docs)]
// Test variable names are short by convention (a, b, c for graph nodes).
#![allow(clippy::similar_names)]
// INV-C3: the solver must never use floating-point arithmetic.
// All scoring is fixed-point Ppm (10^6, u64).
#![deny(clippy::float_arithmetic)]

pub mod conflict;
pub mod cycle;
pub mod error;
pub mod filter;
pub mod membership;
pub mod placement;
pub mod promotion;
pub mod resource;
pub mod scorer;
pub mod solver;

// Re-export key public types at the crate root for convenience.

// Error
pub use error::SolverError;

// Membership
pub use membership::{MembershipSnapshot, NodeHealth};

// Placement types
pub use placement::{
    CompositionResult, Conflict, ConflictReport, ConflictStatus, ConflictType, Placement,
    PlacementDecision, PlacementReason, PlacementScore, RecoveryCycle, ScalingDecision,
    SupersessionChain, UnmatchedNeed,
};

// Solver result
pub use placement::SolverResult;

// Traits and default implementations
pub use conflict::{ConflictDetector, DefaultConflictDetector};
pub use cycle::{CycleDetector, DefaultCycleDetector};
pub use filter::{CapabilityFilter, DefaultCapabilityFilter};
pub use promotion::{
    DefaultPromotionEvaluator, PromotionCollision, PromotionEvaluator, PromotionResult,
};
pub use resource::{DefaultResourceRanker, ResourceRanker};
pub use scorer::{DefaultPlacementScorer, PlacementScorer};
pub use solver::{DefaultSolver, Solver};
