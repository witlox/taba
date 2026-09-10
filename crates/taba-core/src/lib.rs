//! Core domain types: units, capabilities, contracts, and data governance.
//!
//! taba-core owns the unit type system — the definition of what a unit
//! is, what capabilities are, how contracts are structured, and
//! validation of well-formedness. This is the domain model in code.
//!
//! ## Modules
//!
//! - [`unit`] — [`Unit`], [`WorkloadUnit`], [`DataUnit`], [`PolicyUnit`],
//!   [`GovernanceUnit`], [`UnitState`], [`UnitKind`], and all subtypes.
//! - [`capability`] — [`Capability`], [`CapabilityMatch`], [`CapabilityMatcher`],
//!   [`DefaultCapabilityMatcher`].
//! - [`contract`] — [`Tolerances`], [`TrustDeclaration`], [`Scaling`],
//!   [`FailureSemantics`], [`RecoveryRelationship`], [`StateRecovery`],
//!   [`PlacementOnFailure`], and related types.
//! - [`data`] — [`Classification`], [`Provenance`], [`RetentionPolicy`],
//!   [`ConsentScope`], [`DataHierarchy`], [`TaintResult`], and related types.
//! - [`artifact`] — [`Artifact`], [`ArtifactType`].
//! - [`health`] — [`HealthCheck`], [`HealthCheckType`].
//! - [`delegation`] — [`DelegationToken`], [`SpawnContext`].
//! - [`tombstone`] — [`Tombstone`], [`TombstoneUnitType`], [`TerminationReason`].
//! - [`node_capability`] — [`NodeCapabilitySet`], [`PrivilegeLevel`],
//!   [`RuntimeCapability`], [`ResourceSnapshot`].
//! - [`validation`] — [`CoreError`], [`UnitValidator`], [`DefaultValidator`].
//! - [`store`] — [`UnitStore`], [`GraphSnapshot`], [`MembershipSnapshot`].
//!
//! [`Unit`]: unit::Unit
//! [`WorkloadUnit`]: unit::WorkloadUnit
//! [`DataUnit`]: unit::DataUnit
//! [`PolicyUnit`]: unit::PolicyUnit
//! [`GovernanceUnit`]: unit::GovernanceUnit
//! [`UnitState`]: unit::UnitState
//! [`UnitKind`]: unit::UnitKind
//! [`Capability`]: capability::Capability
//! [`CapabilityMatch`]: capability::CapabilityMatch
//! [`CapabilityMatcher`]: capability::CapabilityMatcher
//! [`DefaultCapabilityMatcher`]: capability::DefaultCapabilityMatcher
//! [`Tolerances`]: contract::Tolerances
//! [`TrustDeclaration`]: contract::TrustDeclaration
//! [`Scaling`]: contract::Scaling
//! [`FailureSemantics`]: contract::FailureSemantics
//! [`RecoveryRelationship`]: contract::RecoveryRelationship
//! [`StateRecovery`]: contract::StateRecovery
//! [`PlacementOnFailure`]: contract::PlacementOnFailure
//! [`Classification`]: data::Classification
//! [`Provenance`]: data::Provenance
//! [`RetentionPolicy`]: data::RetentionPolicy
//! [`ConsentScope`]: data::ConsentScope
//! [`DataHierarchy`]: data::DataHierarchy
//! [`TaintResult`]: data::TaintResult
//! [`Artifact`]: artifact::Artifact
//! [`ArtifactType`]: artifact::ArtifactType
//! [`HealthCheck`]: health::HealthCheck
//! [`HealthCheckType`]: health::HealthCheckType
//! [`DelegationToken`]: delegation::DelegationToken
//! [`SpawnContext`]: delegation::SpawnContext
//! [`Tombstone`]: tombstone::Tombstone
//! [`TombstoneUnitType`]: tombstone::TombstoneUnitType
//! [`TerminationReason`]: tombstone::TerminationReason
//! [`NodeCapabilitySet`]: node_capability::NodeCapabilitySet
//! [`PrivilegeLevel`]: node_capability::PrivilegeLevel
//! [`RuntimeCapability`]: node_capability::RuntimeCapability
//! [`ResourceSnapshot`]: node_capability::ResourceSnapshot
//! [`CoreError`]: validation::CoreError
//! [`UnitValidator`]: validation::UnitValidator
//! [`DefaultValidator`]: validation::DefaultValidator
//! [`UnitStore`]: store::UnitStore
//! [`GraphSnapshot`]: store::GraphSnapshot
//! [`MembershipSnapshot`]: store::MembershipSnapshot

pub mod artifact;
pub mod capability;
pub mod contract;
pub mod data;
pub mod delegation;
pub mod health;
pub mod node_capability;
pub mod store;
pub mod tombstone;
pub mod unit;
pub mod validation;

// Re-export all public types at the crate root for convenience.
// Each module owns its types; the re-exports make them accessible
// without path qualification (e.g., `taba_core::Unit` instead of
// `taba_core::unit::Unit`).
pub use artifact::{Artifact, ArtifactType};
pub use capability::{Capability, CapabilityMatch, CapabilityMatcher, DefaultCapabilityMatcher};
pub use contract::{
    CrashBehavior, FailureSemantics, OomBehavior, PlacementOnFailure, RecoveryAction,
    RecoveryRelationship, ScaleDirection, Scaling, ScalingTrigger, ShutdownBehavior, StateRecovery,
    Tolerances, TrustDeclaration,
};
pub use data::{
    Classification, ConsentScope, ConsentType, ConstraintRelation, DataHierarchy, DataSchema,
    Purpose, RetentionMode, RetentionPolicy, StorageRequirements, TaintResult,
};
pub use delegation::{DelegationToken, SpawnContext};
pub use health::{HealthCheck, HealthCheckType};
pub use node_capability::{NodeCapabilitySet, PrivilegeLevel, ResourceSnapshot, RuntimeCapability};
pub use store::{GraphSnapshot, MembershipSnapshot, UnitStore};
pub use tombstone::{TerminationReason, Tombstone, TombstoneUnitType};
pub use unit::{
    ConflictTuple, CrossDomainCapabilityDef, GovernanceUnit, KeyRevocationDef, OperationalCommand,
    OperationalCommandType, PolicyResolution, PolicyUnit, PromotionGateDef, PromotionMode,
    PromotionPolicy, PromotionTransition, RoleAssignment, TrustDomainDef, Unit, UnitHeader,
    UnitKind, UnitState, UnitTypeScope, WorkloadKind, WorkloadUnit,
};
pub use validation::{CoreError, DefaultValidator, UnitValidator};

// Re-export Provenance from data (not in the main unit module).
pub use data::Provenance;
