//! Shared test utilities, builders, and property test strategies.
//!
//! Provides unit builders (test data factories), an in-memory
//! [`UnitStore`](taba_core::UnitStore) fake, and proptest strategies
//! for core types. No production code depends on this crate — it is
//! used exclusively as a dev-dependency by other taba crates.
//!
//! ## Modules
//!
//! - [`builder`] — fluent builders for [`WorkloadUnit`], [`DataUnit`],
//!   [`PolicyUnit`], [`Capability`], and [`NodeCapabilitySet`].
//! - [`store`] — [`InMemoryUnitStore`] implementing [`UnitStore`](taba_core::UnitStore).
//! - [`arbitrary`] — proptest strategies for property-based testing.
//!
//! [`WorkloadUnit`]: taba_core::WorkloadUnit
//! [`DataUnit`]: taba_core::DataUnit
//! [`PolicyUnit`]: taba_core::PolicyUnit
//! [`Capability`]: taba_core::Capability
//! [`NodeCapabilitySet`]: taba_core::NodeCapabilitySet

pub mod arbitrary;
pub mod builder;
pub mod store;

pub use arbitrary::*;
pub use builder::*;
pub use store::*;
