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
    /// Author who signed this unit (or delegation token for spawned tasks).
    pub author: AuthorId,
    /// Trust domain this unit belongs to.
    pub trust_domain: TrustDomainId,
    /// When this unit was created (dual clock).
    pub created_at: DualClockEvent,
    /// Validity window. Optional for services (INV-W1), set for bounded tasks (INV-W2).
    pub validity: Option<ValidityWindow>,
    /// Current lifecycle state.
    pub state: UnitState,
    /// Git-native version ref (commit SHA or tag). Optional for non-git sources (A9).
    pub version: Option<String>,
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
/// Runtime-agnostic: the solver matches artifact.type to node runtime
/// capabilities (INV-N2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadUnit {
    /// Common unit metadata.
    pub header: UnitHeader,
    /// Service (indefinite) or BoundedTask (lifecycle-limited).
    pub kind: WorkloadKind,
    /// Artifact packaging: OCI image, native binary, Wasm module, K8s manifest.
    pub artifact: Artifact,
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
    /// What to do when the hosting node fails (INV-N5).
    pub placement_on_failure: Option<PlacementOnFailure>,
    /// Optional health check endpoint (INV-O3, progressive).
    pub health_check: Option<HealthCheck>,
    /// If this is a spawned bounded task, the parent service and delegation token.
    pub spawn_context: Option<SpawnContext>,
}

/// Whether a workload is a long-running service or a bounded task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WorkloadKind {
    /// Long-running, indefinite lifetime. No validity window (INV-W1).
    Service,
    /// Lifecycle-limited. Auto-terminates on completion/failure/deadline (INV-W2).
    BoundedTask,
}

/// Artifact packaging for a workload unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Type of artifact (determines which runtime capability is needed).
    pub artifact_type: ArtifactType,
    /// Content reference (OCI image tag, binary URL, file path, etc.).
    pub artifact_ref: String,
    /// SHA256 content hash for integrity and dedup (INV-A1).
    pub digest: ContentDigest,
    /// Additional runtime requirements (e.g., ["windows", "dotnet-4.8"]).
    pub requires: Vec<String>,
}

/// Artifact type — matched against node runtime capabilities by the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ArtifactType {
    /// OCI container image (Docker, Podman).
    Oci,
    /// Native binary or package installer (MSI, RPM, DEB).
    Native,
    /// WebAssembly module.
    Wasm,
    /// Kubernetes manifest (pod spec).
    K8sManifest,
}

/// What happens when the hosting node fails (INV-N5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlacementOnFailure {
    /// Solver re-places the workload to another eligible node.
    Replace,
    /// Workload is left dead (not re-placed). Dev default.
    LeaveDead,
}

/// Health check declaration for progressive monitoring (INV-O3).
/// Default (None): OS-level process monitoring. Declared: explicit check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Type of health check.
    pub check_type: HealthCheckType,
    /// How often to run the check.
    pub interval: Duration,
    /// Maximum time to wait for a response.
    pub timeout: Duration,
}

/// Type of health check (progressive disclosure).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HealthCheckType {
    /// HTTP GET to a path. 2xx = healthy.
    Http { path: String, port: u16 },
    /// TCP connect to a port. Success = healthy.
    Tcp { port: u16 },
    /// Execute a command. Exit 0 = healthy.
    Command { command: String },
}

/// Context for a spawned bounded task (INV-W4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnContext {
    /// The parent service that spawned this task.
    pub spawned_by: UnitId,
    /// The delegation token authorizing this spawn.
    pub delegation_token_id: DelegationTokenId,
    /// Depth in the spawn chain (1 = direct spawn from service, max 4 per INV-W3).
    pub spawn_depth: u8,
}

/// Delegation token for spawned task signing (INV-W4).
/// Pre-signed by the author at service placement time.
/// The node uses this token to sign spawned tasks — it never holds the
/// author's private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationToken {
    /// Unique identifier for this token.
    pub id: DelegationTokenId,
    /// The service this token authorizes spawning for.
    pub service_id: UnitId,
    /// The node authorized to use this token.
    pub node_id: NodeId,
    /// Trust domain scope.
    pub trust_domain: TrustDomainId,
    /// Logical clock range during which this token is valid.
    pub valid_lc_range: (LogicalClock, LogicalClock),
    /// Maximum number of tasks this token can spawn.
    pub max_spawns: u32,
    /// Current spawn count (tracked by the node, verified at merge).
    pub current_spawns: u32,
    /// Author's signature over this token.
    pub author_signature: Vec<u8>,
    /// Whether this token has been revoked.
    pub revoked: bool,
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

/// Trust domain definitions, role scope assignments, certification
/// attestations, operational commands, and promotion gates.
/// Created through multi-party agreement (INV-S6, INV-S10) or
/// self-signed in Tier 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GovernanceUnit {
    /// Defines a trust domain boundary.
    TrustDomainDef(TrustDomainDef),
    /// Assigns an author to a scoped role.
    RoleAssignment(RoleAssignment),
    /// Attests that a composition meets a standard.
    Certification(Certification),
    /// Fleet-wide administrative instruction (refresh-capabilities, etc.).
    OperationalCommand(OperationalCommand),
    /// Declares auto-promote vs human-approval per environment transition.
    PromotionGate(PromotionGateDef),
    /// Publishes a capability for cross-domain consumption (INV-X5).
    CrossDomainCapability(CrossDomainCapabilityDef),
    /// Key revocation (causal model, INV-S3).
    KeyRevocation(KeyRevocationDef),
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

/// Retention policy for a data unit (INV-D2, INV-D4).
/// Determines lifecycle: persistent, ephemeral, or local-only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Retention mode (persistent/ephemeral/local-only).
    pub mode: RetentionMode,
    /// How long to retain this data (wall time, for persistent mode).
    pub duration: Option<Duration>,
    /// Legal basis for retention (e.g., "GDPR Art. 6(1)(f)").
    pub legal_basis: String,
    /// Whether retention is mandatory (must keep) or permissive (may delete).
    pub mandatory: bool,
}

/// Data retention mode (progressive disclosure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RetentionMode {
    /// Default. Governed by duration + legal basis. Tombstoned on expiry.
    Persistent,
    /// Auto-removed when producing bounded task terminates.
    /// Reference check: has refs → tombstone, no refs → full remove (INV-D4).
    Ephemeral,
    /// Never enters graph. Node-local scratch only.
    /// Requires policy for classification > Public (INV-D5).
    LocalOnly,
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

// ---------------------------------------------------------------------------
// New governance subtypes
// ---------------------------------------------------------------------------

/// Fleet-wide administrative instruction propagated via gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalCommand {
    pub header: UnitHeader,
    pub command_type: OperationalCommandType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OperationalCommandType {
    /// All nodes re-probe their capabilities.
    RefreshCapabilities,
    /// Operator-triggered degraded mode.
    EnterDegraded { reason: String },
}

/// Environment promotion gate for a trust domain (INV-E3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionGateDef {
    pub header: UnitHeader,
    pub transitions: Vec<PromotionTransition>,
}

/// A single environment transition rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionTransition {
    pub from_env: String,
    pub to_env: String,
    pub mode: PromotionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PromotionMode {
    /// CI or automation can promote automatically.
    Auto,
    /// Human must explicitly approve.
    HumanApproval,
}

/// Promotion policy: gates workload placement by environment (INV-E1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionPolicy {
    /// The policy unit wrapping this promotion.
    pub header: UnitHeader,
    /// The workload unit being promoted.
    pub unit_ref: UnitId,
    /// The version being promoted (git ref or content-addressable ID).
    pub version: String,
    /// The target environment (e.g., "test", "prod").
    pub target_environment: String,
    /// Human-readable rationale.
    pub rationale: String,
}

/// Cross-domain capability advertisement (INV-X5).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDomainCapabilityDef {
    pub header: UnitHeader,
    /// The capability being advertised.
    pub provides: Capability,
    /// Conditions for cross-domain access.
    pub conditions: String,
}

/// Key revocation governance unit (causal model per INV-S3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRevocationDef {
    pub header: UnitHeader,
    /// The author whose key is being revoked.
    pub revoked_author: AuthorId,
    /// Logical clock at which revocation was issued.
    pub revocation_lc: LogicalClock,
    /// Reason for revocation.
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Tombstone (graph compaction, INV-G2)
// ---------------------------------------------------------------------------

/// Minimal record replacing a compacted unit in the graph.
/// Preserves provenance graph structure without content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tombstone {
    /// Original unit ID (preserved).
    pub unit_id: UnitId,
    /// Original author ID (preserved).
    pub author_id: AuthorId,
    /// Original unit type.
    pub unit_type: TombstoneUnitType,
    /// When the original unit was created.
    pub created_at_lc: LogicalClock,
    /// When the unit was terminated/compacted.
    pub terminated_at_lc: LogicalClock,
    /// Why the unit was terminated.
    pub termination_reason: TerminationReason,
    /// References: what the unit consumed/produced (preserves provenance graph).
    pub references: Vec<UnitId>,
    /// SHA256 of the original unit content (for archive retrieval).
    pub original_digest: ContentDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TombstoneUnitType { Workload, Data, Policy }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TerminationReason {
    Completed,
    Failed,
    DeadlineExceeded,
    Superseded,
    RetentionExpired,
    Drained,
}

// ---------------------------------------------------------------------------
// Node capabilities (INV-N1 through INV-N5)
// ---------------------------------------------------------------------------

/// Complete capability set for a node, auto-discovered + operator-declared.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilitySet {
    pub arch: String,
    pub os: String,
    pub privilege: PrivilegeLevel,
    pub runtimes: Vec<RuntimeCapability>,
    pub ports_privileged: bool,
    pub storage: Vec<String>,
    pub environment: Option<String>,
    pub author_affinity: Option<AuthorId>,
    pub clock_quality: ClockQuality,
    pub timezone: String,
    pub custom_tags: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrivilegeLevel { Root, User }

/// Runtime capability that a node can execute (INV-N2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RuntimeCapability {
    /// Docker/Podman with root daemon.
    Oci,
    /// Rootless Docker/Podman (userspace).
    OciRootless,
    /// Kubernetes API access (schedule pods).
    K8s,
    /// WebAssembly runtime (wasmtime/wasmer).
    Wasm,
    /// Native binary/package execution.
    Native,
}

/// Dynamic resource snapshot reported by a node (INV-N3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    pub node_id: NodeId,
    pub logical_clock: LogicalClock,
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub cpu_cores: u32,
    /// CPU load as Ppm (0 = idle, 1_000_000 = fully loaded).
    pub cpu_load_ppm: Ppm,
    pub disk_available_bytes: u64,
    pub gpu_available: u32,
}
