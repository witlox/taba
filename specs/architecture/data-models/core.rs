//! Core domain types: units, capabilities, contracts, and data governance.
//!
//! This module defines the fundamental unit model -- the self-describing,
//! signed, typed entities that are the building blocks of every taba
//! deployment. Units compose through capability matching resolved by the
//! solver. The five unit states (Declared -> Composed -> Placed -> Running ->
//! Draining -> Terminated) represent the lifecycle from authoring to teardown.

use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::common::{
    AuthorId, NodeId, Ppm, Timestamp, TrustDomainId, UnitId, ValidityWindow, Version,
};

// ---------------------------------------------------------------------------
// Unit -- the fundamental primitive
// ---------------------------------------------------------------------------

/// The fundamental taba primitive. A self-describing, signed, typed entity
/// carrying capability declarations, behavioral contracts, and security
/// requirements. Every unit in the graph is signed by an author with valid
/// scope (INV-S3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Unit {
    /// A compute process -- runtime-agnostic (container, microVM, Wasm, native).
    Workload(WorkloadUnit),
    /// A dataset carrying schema, classification, provenance, and constraints.
    Data(DataUnit),
    /// Resolves a capability conflict between other units.
    Policy(PolicyUnit),
    /// Trust domain definitions, role assignments, or certifications.
    Governance(GovernanceUnit),
}

/// Metadata common to all unit types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitHeader {
    /// Globally unique, immutable identifier.
    pub id: UnitId,
    /// Author who signed this unit.
    pub author: AuthorId,
    /// Trust domain this unit belongs to.
    pub trust_domain: TrustDomainId,
    /// When this unit was created.
    pub created_at: Timestamp,
    /// Validity window for the unit and its signature.
    pub validity: ValidityWindow,
    /// Current lifecycle state.
    pub state: UnitState,
}

/// Lifecycle states of a unit from authoring to termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UnitState {
    /// Unit has been authored and signed but not yet composed.
    Declared,
    /// Solver has resolved this unit's capabilities into a composition.
    Composed,
    /// Solver has assigned this unit to a node.
    Placed,
    /// Unit is actively running on its assigned node.
    Running,
    /// Unit is gracefully shutting down.
    Draining,
    /// Unit has completed its lifecycle.
    Terminated,
}

// ---------------------------------------------------------------------------
// Workload unit
// ---------------------------------------------------------------------------

/// A compute process with full behavioral contracts.
/// Runtime-agnostic: the isolation mechanism (container, microVM, Wasm,
/// native process) is itself a declared capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// Capabilities this workload requires from the environment.
    pub needs: Vec<Capability>,
    /// Capabilities this workload exposes to other units.
    pub provides: Vec<Capability>,
    /// Latency and failure budgets this workload can tolerate.
    pub tolerates: Tolerations,
    /// Identity-based trust declarations.
    pub trusts: Vec<TrustDeclaration>,
    /// Scaling parameters (min/max instances, triggers).
    pub scaling: Scaling,
    /// What happens when this workload fails.
    pub failure_semantics: FailureSemantics,
    /// Dependency ordering on failure recovery (cycles fail closed per INV-K5).
    pub recovery_relationships: Vec<RecoveryRelationship>,
    /// How this workload recovers state after restart.
    pub state_recovery: StateRecovery,
}

// ---------------------------------------------------------------------------
// Data unit
// ---------------------------------------------------------------------------

/// A dataset carrying constraints. Hierarchical: parent contains children.
/// Granularity is demand-driven -- children exist only where constraints
/// diverge from parent. Max hierarchy depth: 16 levels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// Typed schema declaration for this dataset.
    pub schema: DataSchema,
    /// Data classification in the lattice (INV-S4, INV-S7).
    pub classification: Classification,
    /// How this data was produced (provenance chain, INV-D1).
    pub provenance: Option<Provenance>,
    /// How long this data must be retained and on what legal basis (INV-D2).
    pub retention: RetentionPolicy,
    /// What this data may be used for (purpose-qualified capabilities).
    pub consent_scope: Vec<ConsentScope>,
    /// Storage requirements (encryption, jurisdiction).
    pub storage_requirements: StorageRequirements,
    /// Parent data unit, if this is a child in a hierarchy (INV-D3).
    pub parent: Option<UnitId>,
    /// Capabilities this data unit provides (e.g., "provides dataset X").
    pub provides: Vec<Capability>,
}

// ---------------------------------------------------------------------------
// Policy unit
// ---------------------------------------------------------------------------

/// Resolves a capability conflict between other units.
/// Required whenever the solver detects incompatible declarations (INV-S2).
/// Only one non-revoked policy per conflict tuple (INV-C7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The specific conflict this policy resolves (set of unit IDs + capability name).
    pub conflict: ConflictTuple,
    /// The resolution: allow, deny, or conditional.
    pub resolution: PolicyResolution,
    /// Trust domain scope of this policy.
    pub scope: TrustDomainId,
    /// Human-readable justification for this policy decision.
    pub rationale: String,
    /// If this policy supersedes an earlier one, the ID of the replaced policy.
    /// Creates a versioned lineage chain (INV-C7).
    pub supersedes: Option<UnitId>,
    /// Version in the supersession chain. First policy is version 1.
    pub version: Version,
    /// Whether this policy has been revoked.
    pub revoked: bool,
}

/// The conflict tuple that a policy resolves: a set of unit IDs plus
/// the capability name where the conflict was detected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConflictTuple {
    /// The units involved in the conflict (sorted for determinism).
    pub unit_ids: BTreeSet<UnitId>,
    /// The capability name where the conflict was detected.
    pub capability_name: String,
}

/// How a policy resolves a conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PolicyResolution {
    /// Allow the composition unconditionally.
    Allow,
    /// Deny the composition.
    Deny,
    /// Allow under specific conditions.
    Conditional {
        /// Conditions that must hold for this resolution to apply.
        conditions: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Governance unit
// ---------------------------------------------------------------------------

/// Trust domain definitions, role scope assignments, and certification
/// attestations. Created through multi-party agreement (INV-S6, INV-S10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceUnit {
    /// Defines a trust domain boundary.
    TrustDomainDef(TrustDomainDef),
    /// Assigns an author to a scoped role.
    RoleAssignment(RoleAssignment),
    /// Attests that a composition meets a standard.
    Certification(Certification),
}

/// Trust domain boundary definition.
/// Creation requires multi-party signing (minimum 2 distinct authors, INV-S10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustDomainDef {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The trust domain this unit defines.
    pub domain_id: TrustDomainId,
    /// Human-readable name for the trust domain.
    pub name: String,
    /// Description of this domain's purpose.
    pub description: String,
    /// Authors who co-signed the domain creation.
    pub signers: Vec<AuthorId>,
    /// When this trust domain expires (if ever).
    pub expires_at: Option<Timestamp>,
}

/// Assigns an author to a scoped role within a trust domain.
/// No two distinct authors may have identical scope tuples (INV-S8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleAssignment {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The author receiving the role.
    pub assignee: AuthorId,
    /// Which unit types this author can create.
    pub unit_type_scope: Vec<UnitTypeScope>,
    /// Which trust domains this author can operate in.
    pub trust_domain_scope: Vec<TrustDomainId>,
}

/// Which unit types an author is authorized to create.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum UnitTypeScope {
    Workload,
    Data,
    Policy,
    Governance,
}

/// Attestation that a composition meets a particular standard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Certification {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// The composition being certified.
    pub composition_id: UnitId,
    /// Name of the standard or regulation.
    pub standard: String,
    /// Details of the certification.
    pub details: String,
    /// When this certification expires.
    pub expires_at: Option<Timestamp>,
}

// ---------------------------------------------------------------------------
// Capability system
// ---------------------------------------------------------------------------

/// A typed resource or service a unit needs or provides.
/// Capabilities are tuples: (type, name, purpose?).
/// Sorted lexicographically by (type, name, purpose) before matching
/// to ensure determinism regardless of declaration order (INV-K2).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Capability {
    /// The type of capability (e.g., "storage", "compute", "network").
    pub cap_type: String,
    /// The name of the capability (e.g., "postgres-compatible", "http").
    pub name: String,
    /// Optional purpose qualifier. If declared, must match during composition.
    /// Purpose mismatch triggers a conflict requiring policy (INV-K2).
    pub purpose: Option<String>,
}

/// Result of matching a `needs` capability against a `provides` capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMatch {
    /// The unit that declared the need.
    pub needer: UnitId,
    /// The specific capability needed.
    pub need: Capability,
    /// The unit that provides the capability.
    pub provider: UnitId,
    /// The specific capability provided.
    pub provided: Capability,
}

// ---------------------------------------------------------------------------
// Behavioral contracts
// ---------------------------------------------------------------------------

/// Latency and failure budgets a workload can tolerate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tolerations {
    /// Maximum acceptable latency to dependent services.
    pub max_latency: Option<Duration>,
    /// Tolerated failure modes for this workload.
    pub failure_modes: Vec<String>,
    /// Consistency requirements (e.g., "strong", "eventual").
    pub consistency: Option<String>,
}

/// Identity-based trust declaration. Not network-topology-based.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustDeclaration {
    /// The author or unit this workload trusts.
    pub trusted_entity: AuthorId,
    /// What kind of access is trusted.
    pub access_type: String,
}

/// Scaling parameters for a workload unit.
/// The solver uses these to compute scaling decisions (INV-K4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scaling {
    /// Minimum number of instances.
    pub min_instances: u32,
    /// Maximum number of instances.
    pub max_instances: u32,
    /// Named triggers that cause scale-up/down.
    pub triggers: Vec<ScalingTrigger>,
}

/// A named trigger for scaling decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingTrigger {
    /// Human-readable name for this trigger.
    pub name: String,
    /// Metric to evaluate (e.g., "cpu_ppm", "queue_depth").
    pub metric: String,
    /// Threshold value in Ppm.
    pub threshold: Ppm,
    /// Direction: scale up or scale down.
    pub direction: ScaleDirection,
}

/// Direction of a scaling action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScaleDirection {
    Up,
    Down,
}

/// What happens when a workload fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureSemantics {
    /// Behavior on OOM.
    pub on_oom: OomBehavior,
    /// Behavior on unexpected crash.
    pub on_crash: CrashBehavior,
    /// Behavior on graceful shutdown request.
    pub on_shutdown: ShutdownBehavior,
}

/// OOM behavior declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OomBehavior {
    /// Back off inputs and retry.
    BackoffInputs,
    /// Restart immediately.
    Restart,
    /// Fail permanently, require manual intervention.
    FailPermanent,
}

/// Crash behavior declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CrashBehavior {
    /// This crash means something is wrong -- do not auto-restart.
    Unexpected,
    /// Transient crash, safe to restart with backoff.
    RestartWithBackoff { max_retries: u32 },
}

/// Graceful shutdown behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ShutdownBehavior {
    /// Drain connections and exit.
    DrainAndExit { timeout: Duration },
    /// Immediate exit, no draining.
    Immediate,
}

/// Dependency ordering on failure recovery.
/// Cycles in recovery relationships fail closed (INV-K5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRelationship {
    /// The unit that must be drained/restarted first.
    pub depends_on: UnitId,
    /// What must happen to the dependency before this unit recovers.
    pub action: RecoveryAction,
}

/// Action required on a recovery dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RecoveryAction {
    /// Dependency must be drained before this unit restarts.
    DrainFirst,
    /// Dependency must be healthy before this unit restarts.
    WaitForHealthy,
    /// Dependency must be restarted before this unit restarts.
    RestartFirst,
}

/// How a workload recovers state after restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum StateRecovery {
    /// No state to recover.
    Stateless,
    /// Replay from a specific offset in an event stream.
    ReplayFromOffset { stream: String, offset: u64 },
    /// Requires quorum of peers before serving.
    RequireQuorum { min_peers: u32 },
}

// ---------------------------------------------------------------------------
// Data governance types
// ---------------------------------------------------------------------------

/// Data classification lattice: Public < Internal < Confidential < Pii.
/// Taint propagation follows this lattice (INV-S4).
/// Multi-input workloads inherit the union (most restrictive) of all inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Classification {
    /// Unrestricted data.
    Public = 0,
    /// Organization-internal only.
    Internal = 1,
    /// Sensitive business data.
    Confidential = 2,
    /// Personally identifiable information -- most restrictive.
    Pii = 3,
}

/// Provenance chain for a data unit (INV-D1).
/// Links back to the producing workload and input data units.
/// References to units not yet in the local graph are marked pending
/// (causal buffering per INV-C4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// The workload unit that produced this data.
    pub produced_by: UnitId,
    /// The input data units consumed by the producing workload.
    pub inputs: Vec<UnitId>,
    /// When this data was produced.
    pub produced_at: Timestamp,
    /// Policies in effect at production time.
    pub governing_policies: Vec<UnitId>,
}

/// Retention policy for a data unit (INV-D2).
/// Expired data units are eligible for compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// How long to retain this data.
    pub duration: Option<Duration>,
    /// Legal basis for retention (e.g., "GDPR Art. 6(1)(f)", "contractual obligation").
    pub legal_basis: String,
    /// Whether retention is mandatory (must keep) or permissive (may delete).
    pub mandatory: bool,
}

/// What a data unit may be used for.
/// Expressed as purpose qualifiers on provided capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentScope {
    /// The permitted purpose (e.g., "analytics", "personalization", "billing").
    pub purpose: Purpose,
    /// Whether consent was explicit or derived.
    pub consent_type: ConsentType,
}

/// A named purpose for data usage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Purpose(pub String);

/// How consent was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConsentType {
    /// User explicitly consented.
    Explicit,
    /// Consent derived from a legal basis.
    LegalBasis,
    /// Consent inherited from parent data unit.
    Inherited,
}

/// Typed schema declaration for a data unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSchema {
    /// Schema format (e.g., "json-schema", "protobuf", "avro").
    pub format: String,
    /// The schema definition (inline or reference).
    pub definition: String,
}

/// Storage requirements for a data unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageRequirements {
    /// Whether data must be encrypted at rest.
    pub encrypted_at_rest: bool,
    /// Jurisdiction constraints (e.g., "EU", "CH").
    pub jurisdictions: Vec<String>,
    /// Minimum replication factor (if applicable beyond erasure coding).
    pub min_replicas: Option<u32>,
}

/// Position of a data unit in its hierarchy (INV-D3).
/// Children exist only where constraints diverge from parent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataHierarchy {
    /// This unit's position in the hierarchy.
    pub depth: u8,
    /// Direct children of this data unit.
    pub children: Vec<UnitId>,
    /// Whether this unit narrows or widens parent constraints (INV-S7).
    pub constraint_relation: ConstraintRelation,
}

/// Relationship of a child data unit's constraints to its parent (INV-S7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConstraintRelation {
    /// Same constraints as parent (should not exist per INV-D3, but valid during authoring).
    Identical,
    /// More restrictive than parent -- always allowed.
    Narrowed,
    /// Less restrictive than parent -- requires explicit policy.
    Widened,
}

/// Result of computing taint propagation through the provenance graph (INV-S4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintResult {
    /// The unit whose taint was computed.
    pub unit_id: UnitId,
    /// The computed classification after taint propagation.
    pub effective_classification: Classification,
    /// The input units that contributed to this classification.
    pub contributing_inputs: Vec<UnitId>,
    /// Whether declassification policy was applied.
    pub declassified: bool,
    /// The policy that authorized declassification, if any.
    pub declassification_policy: Option<UnitId>,
}
