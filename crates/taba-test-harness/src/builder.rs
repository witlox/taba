//! Unit builders for creating valid test data with sensible defaults.
//!
//! Each builder produces a unit that passes `DefaultValidator::validate`
//! by default. Override methods allow customization for specific test
//! scenarios — including creating intentionally invalid units by
//! providing values that violate validation rules.
//!
//! # Example
//!
//! ```
//! use taba_core::{DefaultValidator, Unit, UnitValidator};
//! use taba_test_harness::WorkloadUnitBuilder;
//!
//! let unit = WorkloadUnitBuilder::new().build();
//! assert!(DefaultValidator::empty().validate(&Unit::Workload(unit)).is_ok());
//! ```

use std::collections::BTreeSet;
use std::time::Duration;

use taba_common::{
    AuthorId, ClockQuality, ContentDigest, DualClockEvent, LogicalClock, TrustDomainId, UnitId,
    ValidityWindow, Version, WallTime,
};
use taba_core::unit::DataUnit;
use taba_core::{
    Artifact, ArtifactType, Capability, Classification, ConflictTuple, CrashBehavior, DataSchema,
    FailureSemantics, HealthCheck, NodeCapabilitySet, OomBehavior, PlacementOnFailure,
    PolicyResolution, PolicyUnit, PrivilegeLevel, RecoveryRelationship, RetentionMode,
    RetentionPolicy, RuntimeCapability, Scaling, ShutdownBehavior, SpawnContext, StateRecovery,
    StorageRequirements, Tolerances, TrustDeclaration, UnitHeader, UnitState, WorkloadKind,
    WorkloadUnit,
};

// ===========================================================================
// Internal helpers
// ===========================================================================

/// Creates a [`UnitHeader`] with fresh random UUIDs and sensible defaults.
fn fresh_header() -> UnitHeader {
    UnitHeader {
        id: UnitId(uuid::Uuid::new_v4()),
        author: AuthorId(uuid::Uuid::new_v4()),
        trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
        created_at: DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1000 },
            timezone: "UTC".to_string(),
        },
        validity: None,
        state: UnitState::Declared,
        version: None,
    }
}

// ===========================================================================
// WorkloadUnitBuilder
// ===========================================================================

/// Builder for creating [`WorkloadUnit`] test data.
///
/// Produces a valid `WorkloadUnit` (passes `DefaultValidator::validate`)
/// by default. Every field can be overridden for specific scenarios.
///
/// # Defaults
///
/// - `kind`: `WorkloadKind::Service` (no validity window required)
/// - `provides`: `vec![Capability::new("compute", "http")]`
/// - `needs`: empty
/// - `scaling`: `min=1, max=3`
/// - `tolerates`: `max_latency` 100ms, `failure_modes` `["timeout"]`
/// - `artifact`: OCI image at `registry.example.com/app:v1`
#[derive(Debug, Clone)]
pub struct WorkloadUnitBuilder {
    header: UnitHeader,
    kind: WorkloadKind,
    artifact: Artifact,
    needs: Vec<Capability>,
    provides: Vec<Capability>,
    tolerates: Tolerances,
    trusts: Vec<TrustDeclaration>,
    scaling: Scaling,
    failure_semantics: FailureSemantics,
    recovery_relationships: Vec<RecoveryRelationship>,
    state_recovery: StateRecovery,
    placement_on_failure: Option<PlacementOnFailure>,
    health_check: Option<HealthCheck>,
    spawn_context: Option<SpawnContext>,
}

impl WorkloadUnitBuilder {
    /// Creates a new builder with valid minimal defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            header: fresh_header(),
            kind: WorkloadKind::Service,
            artifact: Artifact {
                artifact_type: ArtifactType::Oci,
                artifact_ref: "registry.example.com/app:v1".to_string(),
                digest: ContentDigest("sha256:abc123".to_string()),
                requires: Vec::new(),
            },
            needs: Vec::new(),
            provides: vec![Capability::new("compute", "http")],
            tolerates: Tolerances {
                max_latency: Some(Duration::from_millis(100)),
                failure_modes: vec!["timeout".to_string()],
                consistency: None,
            },
            trusts: Vec::new(),
            scaling: Scaling {
                min_instances: 1,
                max_instances: 3,
                triggers: Vec::new(),
            },
            failure_semantics: FailureSemantics {
                on_oom: OomBehavior::Restart,
                on_crash: CrashBehavior::Unexpected,
                on_shutdown: ShutdownBehavior::Immediate,
            },
            recovery_relationships: Vec::new(),
            state_recovery: StateRecovery::Stateless,
            placement_on_failure: None,
            health_check: None,
            spawn_context: None,
        }
    }

    /// Sets the unit's identifier.
    #[must_use]
    pub const fn with_id(mut self, id: UnitId) -> Self {
        self.header.id = id;
        self
    }

    /// Sets the unit's author.
    #[must_use]
    pub const fn with_author(mut self, author: AuthorId) -> Self {
        self.header.author = author;
        self
    }

    /// Sets the unit's trust domain.
    #[must_use]
    pub const fn with_trust_domain(mut self, td: TrustDomainId) -> Self {
        self.header.trust_domain = td;
        self
    }

    /// Sets the capabilities this workload requires.
    ///
    /// The caller is responsible for providing a sorted list if
    /// the resulting unit should pass validation (INV-K2).
    #[must_use]
    pub fn with_needs(mut self, needs: Vec<Capability>) -> Self {
        self.needs = needs;
        self
    }

    /// Sets the capabilities this workload provides.
    ///
    /// The caller is responsible for providing a sorted, non-empty
    /// list if the resulting unit should pass validation (INV-K2).
    #[must_use]
    pub fn with_provides(mut self, provides: Vec<Capability>) -> Self {
        self.provides = provides;
        self
    }

    /// Sets the scaling parameters (min and max instances).
    ///
    /// The caller must ensure `min <= max` for a valid unit.
    #[must_use]
    pub const fn with_scaling(mut self, min: u32, max: u32) -> Self {
        self.scaling.min_instances = min;
        self.scaling.max_instances = max;
        self
    }

    /// Sets the workload kind (`Service` or `BoundedTask`).
    ///
    /// When setting `BoundedTask`, also use [`with_validity`](Self::with_validity)
    /// to provide a validity window (INV-W2).
    #[must_use]
    pub const fn with_kind(mut self, kind: WorkloadKind) -> Self {
        self.kind = kind;
        self
    }

    /// Sets the validity window on the unit header.
    ///
    /// Required for `BoundedTask` workloads (INV-W2),
    /// optional for `Service` workloads (INV-W1).
    #[must_use]
    pub const fn with_validity(mut self, v: ValidityWindow) -> Self {
        self.header.validity = Some(v);
        self
    }

    /// Sets the health check declaration.
    #[must_use]
    pub fn with_health_check(mut self, hc: HealthCheck) -> Self {
        self.health_check = Some(hc);
        self
    }

    /// Sets the spawn context (for spawned bounded tasks).
    #[must_use]
    pub const fn with_spawn_context(mut self, ctx: SpawnContext) -> Self {
        self.spawn_context = Some(ctx);
        self
    }

    /// Builds the [`WorkloadUnit`] from the configured fields.
    #[must_use]
    pub fn build(self) -> WorkloadUnit {
        WorkloadUnit {
            header: self.header,
            kind: self.kind,
            artifact: self.artifact,
            needs: self.needs,
            provides: self.provides,
            tolerates: self.tolerates,
            trusts: self.trusts,
            scaling: self.scaling,
            failure_semantics: self.failure_semantics,
            recovery_relationships: self.recovery_relationships,
            state_recovery: self.state_recovery,
            placement_on_failure: self.placement_on_failure,
            health_check: self.health_check,
            spawn_context: self.spawn_context,
        }
    }
}

impl Default for WorkloadUnitBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// DataUnitBuilder
// ===========================================================================

/// Builder for creating [`DataUnit`] test data.
///
/// Produces a valid `DataUnit` (passes `DefaultValidator::validate`)
/// by default. Every field can be overridden for specific scenarios.
///
/// # Defaults
///
/// - `classification`: `Classification::Public`
/// - `schema`: JSON schema with empty definition
/// - `retention`: persistent, 24-hour duration, `"consent"` legal basis
/// - `provides`: `vec![Capability::new("storage", "dataset-x")]`
/// - `parent`: `None` (root-level data unit)
#[derive(Debug, Clone)]
pub struct DataUnitBuilder {
    header: UnitHeader,
    schema: DataSchema,
    classification: Classification,
    provenance: Option<taba_core::Provenance>,
    retention: RetentionPolicy,
    consent_scope: Vec<taba_core::ConsentScope>,
    storage_requirements: StorageRequirements,
    parent: Option<UnitId>,
    provides: Vec<Capability>,
}

impl DataUnitBuilder {
    /// Creates a new builder with valid minimal defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            header: fresh_header(),
            schema: DataSchema {
                format: "json-schema".to_string(),
                definition: "{}".to_string(),
            },
            classification: Classification::Public,
            provenance: None,
            retention: RetentionPolicy {
                mode: RetentionMode::Persistent,
                duration: Some(Duration::from_secs(86_400)),
                legal_basis: "consent".to_string(),
                mandatory: false,
            },
            consent_scope: Vec::new(),
            storage_requirements: StorageRequirements {
                encrypted_at_rest: false,
                jurisdictions: Vec::new(),
                min_replicas: None,
            },
            parent: None,
            provides: vec![Capability::new("storage", "dataset-x")],
        }
    }

    /// Sets the unit's identifier.
    #[must_use]
    pub const fn with_id(mut self, id: UnitId) -> Self {
        self.header.id = id;
        self
    }

    /// Sets the data schema.
    #[must_use]
    pub fn with_schema(mut self, schema: DataSchema) -> Self {
        self.schema = schema;
        self
    }

    /// Sets the data classification.
    #[must_use]
    pub const fn with_classification(mut self, classification: Classification) -> Self {
        self.classification = classification;
        self
    }

    /// Sets the retention policy.
    ///
    /// The caller must ensure `legal_basis` is non-empty and that
    /// `Persistent` mode has a duration for a valid unit.
    #[must_use]
    pub fn with_retention(mut self, retention: RetentionPolicy) -> Self {
        self.retention = retention;
        self
    }

    /// Sets the consent scopes.
    #[must_use]
    pub fn with_consent_scope(mut self, scopes: Vec<taba_core::ConsentScope>) -> Self {
        self.consent_scope = scopes;
        self
    }

    /// Sets the storage requirements.
    #[must_use]
    pub fn with_storage_requirements(mut self, req: StorageRequirements) -> Self {
        self.storage_requirements = req;
        self
    }

    /// Sets the parent data unit (for hierarchical data).
    ///
    /// The caller must ensure `parent != self.header.id` for a valid unit.
    #[must_use]
    pub const fn with_parent(mut self, parent: UnitId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Sets the capabilities this data unit provides.
    ///
    /// The caller is responsible for providing a sorted, non-empty
    /// list if the resulting unit should pass validation (INV-K2).
    #[must_use]
    pub fn with_provides(mut self, provides: Vec<Capability>) -> Self {
        self.provides = provides;
        self
    }

    /// Builds the [`DataUnit`] from the configured fields.
    #[must_use]
    pub fn build(self) -> DataUnit {
        DataUnit {
            header: self.header,
            schema: self.schema,
            classification: self.classification,
            provenance: self.provenance,
            retention: self.retention,
            consent_scope: self.consent_scope,
            storage_requirements: self.storage_requirements,
            parent: self.parent,
            provides: self.provides,
        }
    }
}

impl Default for DataUnitBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// PolicyUnitBuilder
// ===========================================================================

/// Builder for creating [`PolicyUnit`] test data.
///
/// Produces a valid `PolicyUnit` (passes `DefaultValidator::validate`)
/// by default. Every field can be overridden for specific scenarios.
///
/// # Defaults
///
/// - `resolution`: `PolicyResolution::Allow`
/// - `rationale`: `"compatible providers"`
/// - `version`: `Version(1)`
/// - `conflict`: single random unit ID, capability name `"storage"`
#[derive(Debug, Clone)]
pub struct PolicyUnitBuilder {
    header: UnitHeader,
    conflict: ConflictTuple,
    resolution: PolicyResolution,
    scope: TrustDomainId,
    rationale: String,
    supersedes: Option<UnitId>,
    version: Version,
    revoked: bool,
}

impl PolicyUnitBuilder {
    /// Creates a new builder with valid minimal defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            header: fresh_header(),
            conflict: ConflictTuple {
                unit_ids: BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
                capability_name: "storage".to_string(),
            },
            resolution: PolicyResolution::Allow,
            scope: TrustDomainId(uuid::Uuid::new_v4()),
            rationale: "compatible providers".to_string(),
            supersedes: None,
            version: Version(1),
            revoked: false,
        }
    }

    /// Sets the conflict tuple this policy resolves.
    #[must_use]
    pub fn with_conflict(mut self, conflict: ConflictTuple) -> Self {
        self.conflict = conflict;
        self
    }

    /// Sets the resolution (allow, deny, conditional).
    #[must_use]
    pub fn with_resolution(mut self, resolution: PolicyResolution) -> Self {
        self.resolution = resolution;
        self
    }

    /// Sets the trust domain scope.
    #[must_use]
    pub const fn with_scope(mut self, scope: TrustDomainId) -> Self {
        self.scope = scope;
        self
    }

    /// Sets the unit's identifier.
    #[must_use]
    pub const fn with_id(mut self, id: UnitId) -> Self {
        self.header.id = id;
        self
    }

    /// Sets the human-readable rationale.
    ///
    /// Must be non-empty for a valid unit.
    #[must_use]
    pub fn with_rationale(mut self, rationale: String) -> Self {
        self.rationale = rationale;
        self
    }

    /// Sets the policy this one supersedes (supersession chain, INV-C7).
    #[must_use]
    pub const fn with_supersedes(mut self, id: UnitId) -> Self {
        self.supersedes = Some(id);
        self
    }

    /// Builds the [`PolicyUnit`] from the configured fields.
    #[must_use]
    pub fn build(self) -> PolicyUnit {
        PolicyUnit {
            header: self.header,
            conflict: self.conflict,
            resolution: self.resolution,
            scope: self.scope,
            rationale: self.rationale,
            supersedes: self.supersedes,
            version: self.version,
            revoked: self.revoked,
        }
    }
}

impl Default for PolicyUnitBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// CapabilityBuilder
// ===========================================================================

/// Builder for creating [`Capability`] test data.
///
/// Produces a valid `Capability` by default. Every field can be
/// overridden for specific scenarios.
///
/// # Defaults
///
/// - `cap_type`: `"compute"`
/// - `name`: `"http"`
/// - `purpose`: `None`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityBuilder {
    cap_type: String,
    name: String,
    purpose: Option<String>,
}

impl CapabilityBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cap_type: "compute".to_string(),
            name: "http".to_string(),
            purpose: None,
        }
    }

    /// Sets the capability type (e.g., `"storage"`, `"compute"`).
    #[must_use]
    pub fn with_cap_type(mut self, cap_type: &str) -> Self {
        self.cap_type = cap_type.to_string();
        self
    }

    /// Sets the capability name (e.g., `"postgres-compatible"`, `"http"`).
    #[must_use]
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// Sets the optional purpose qualifier.
    ///
    /// When set, the purpose must match during composition (INV-K2).
    #[must_use]
    pub fn with_purpose(mut self, purpose: &str) -> Self {
        self.purpose = Some(purpose.to_string());
        self
    }

    /// Builds the [`Capability`] from the configured fields.
    #[must_use]
    pub fn build(self) -> Capability {
        Capability {
            cap_type: self.cap_type,
            name: self.name,
            purpose: self.purpose,
        }
    }
}

impl Default for CapabilityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// NodeCapabilitySetBuilder
// ===========================================================================

/// Builder for creating [`NodeCapabilitySet`] test data.
///
/// Produces a valid, realistic node capability set by default.
/// Every field can be overridden for specific scenarios.
///
/// # Defaults
///
/// - `arch`: `"x86_64"`, `os`: `"linux"`
/// - `privilege`: `PrivilegeLevel::User`
/// - `runtimes`: `vec![RuntimeCapability::Oci]`
/// - `ports_privileged`: `false`
/// - `storage`: `vec!["ssd"]`
/// - `environment`: `Some("env:dev")`
/// - `clock_quality`: `ClockQuality::Ntp`
/// - `timezone`: `"UTC"`
#[derive(Debug, Clone)]
pub struct NodeCapabilitySetBuilder {
    arch: String,
    os: String,
    privilege: PrivilegeLevel,
    runtimes: Vec<RuntimeCapability>,
    ports_privileged: bool,
    storage: Vec<String>,
    environment: Option<String>,
    author_affinity: Option<AuthorId>,
    clock_quality: ClockQuality,
    timezone: String,
    custom_tags: Vec<(String, String)>,
}

impl NodeCapabilitySetBuilder {
    /// Creates a new builder with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            arch: "x86_64".to_string(),
            os: "linux".to_string(),
            privilege: PrivilegeLevel::User,
            runtimes: vec![RuntimeCapability::Oci],
            ports_privileged: false,
            storage: vec!["ssd".to_string()],
            environment: Some("env:dev".to_string()),
            author_affinity: None,
            clock_quality: ClockQuality::Ntp,
            timezone: "UTC".to_string(),
            custom_tags: Vec::new(),
        }
    }

    /// Sets the CPU architecture (e.g., `"x86_64"`, `"aarch64"`).
    #[must_use]
    pub fn with_arch(mut self, arch: &str) -> Self {
        self.arch = arch.to_string();
        self
    }

    /// Sets the operating system (e.g., `"linux"`, `"darwin"`).
    #[must_use]
    pub fn with_os(mut self, os: &str) -> Self {
        self.os = os.to_string();
        self
    }

    /// Sets the privilege level.
    #[must_use]
    pub const fn with_privilege(mut self, privilege: PrivilegeLevel) -> Self {
        self.privilege = privilege;
        self
    }

    /// Sets the runtime capabilities.
    #[must_use]
    pub fn with_runtimes(mut self, runtimes: Vec<RuntimeCapability>) -> Self {
        self.runtimes = runtimes;
        self
    }

    /// Sets whether the node can bind privileged ports (< 1024).
    #[must_use]
    pub const fn with_ports_privileged(mut self, ports_privileged: bool) -> Self {
        self.ports_privileged = ports_privileged;
        self
    }

    /// Sets the available storage backends.
    #[must_use]
    pub fn with_storage(mut self, storage: Vec<String>) -> Self {
        self.storage = storage;
        self
    }

    /// Sets the environment tag (e.g., `"env:dev"`, `"env:prod"`).
    #[must_use]
    pub fn with_environment(mut self, environment: Option<String>) -> Self {
        self.environment = environment;
        self
    }

    /// Sets the author affinity (dev-only binding).
    #[must_use]
    pub const fn with_author_affinity(mut self, author: AuthorId) -> Self {
        self.author_affinity = Some(author);
        self
    }

    /// Sets the clock quality.
    #[must_use]
    pub const fn with_clock_quality(mut self, quality: ClockQuality) -> Self {
        self.clock_quality = quality;
        self
    }

    /// Sets the IANA timezone string.
    #[must_use]
    pub fn with_timezone(mut self, timezone: &str) -> Self {
        self.timezone = timezone.to_string();
        self
    }

    /// Sets the custom freeform tags (key, value pairs).
    #[must_use]
    pub fn with_custom_tags(mut self, tags: Vec<(String, String)>) -> Self {
        self.custom_tags = tags;
        self
    }

    /// Builds the [`NodeCapabilitySet`] from the configured fields.
    #[must_use]
    pub fn build(self) -> NodeCapabilitySet {
        NodeCapabilitySet {
            arch: self.arch,
            os: self.os,
            privilege: self.privilege,
            runtimes: self.runtimes,
            ports_privileged: self.ports_privileged,
            storage: self.storage,
            environment: self.environment,
            author_affinity: self.author_affinity,
            clock_quality: self.clock_quality,
            timezone: self.timezone,
            custom_tags: self.custom_tags,
        }
    }
}

impl Default for NodeCapabilitySetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_core::unit::Unit;
    use taba_core::{DefaultValidator, UnitValidator};

    // -- WorkloadUnitBuilder -------------------------------------------------

    #[test]
    fn test_workload_builder_default_valid() {
        let validator = DefaultValidator::empty();
        let unit = WorkloadUnitBuilder::new().build();
        assert!(
            validator.validate(&Unit::Workload(unit)).is_ok(),
            "default builder should produce a valid unit"
        );
    }

    #[test]
    fn test_workload_builder_with_needs() {
        let needs = vec![Capability::new("storage", "redis")];
        let unit = WorkloadUnitBuilder::new().with_needs(needs.clone()).build();
        assert_eq!(unit.needs, needs, "custom needs should be reflected");
    }

    #[test]
    fn test_workload_builder_with_scaling() {
        let unit = WorkloadUnitBuilder::new().with_scaling(5, 10).build();
        assert_eq!(unit.scaling.min_instances, 5);
        assert_eq!(unit.scaling.max_instances, 10);
    }

    #[test]
    fn test_workload_builder_bounded_task() {
        let validity = ValidityWindow {
            lc_range: Some((LogicalClock(10), LogicalClock(100))),
            wall_time_deadline: None,
        };
        let unit = WorkloadUnitBuilder::new()
            .with_kind(WorkloadKind::BoundedTask)
            .with_validity(validity)
            .build();

        assert_eq!(unit.kind, WorkloadKind::BoundedTask);
        let v = unit
            .header
            .validity
            .as_ref()
            .expect("validity should be set for bounded task");
        assert_eq!(v.lc_range, Some((LogicalClock(10), LogicalClock(100))));
        assert!(v.wall_time_deadline.is_none());

        let validator = DefaultValidator::empty();
        assert!(
            validator.validate(&Unit::Workload(unit)).is_ok(),
            "bounded task with validity should pass validation"
        );
    }

    #[test]
    fn test_workload_builder_with_id() {
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = WorkloadUnitBuilder::new().with_id(id).build();
        assert_eq!(unit.header.id, id);
    }

    // -- DataUnitBuilder ----------------------------------------------------

    #[test]
    fn test_data_builder_default_valid() {
        let validator = DefaultValidator::empty();
        let unit = DataUnitBuilder::new().build();
        assert!(
            validator.validate(&Unit::Data(unit)).is_ok(),
            "default data builder should produce a valid unit"
        );
    }

    #[test]
    fn test_data_builder_with_classification() {
        let unit = DataUnitBuilder::new()
            .with_classification(Classification::Pii)
            .build();
        assert_eq!(unit.classification, Classification::Pii);
    }

    #[test]
    fn test_data_builder_with_parent() {
        let parent = UnitId(uuid::Uuid::new_v4());
        let unit = DataUnitBuilder::new().with_parent(parent).build();
        assert_eq!(unit.parent, Some(parent));
    }

    // -- PolicyUnitBuilder --------------------------------------------------

    #[test]
    fn test_policy_builder_default_valid() {
        let validator = DefaultValidator::empty();
        let unit = PolicyUnitBuilder::new().build();
        assert!(
            validator.validate(&Unit::Policy(unit)).is_ok(),
            "default policy builder should produce a valid unit"
        );
    }

    #[test]
    fn test_policy_builder_with_rationale() {
        let unit = PolicyUnitBuilder::new()
            .with_rationale("security override".to_string())
            .build();
        assert_eq!(unit.rationale, "security override");
    }

    // -- CapabilityBuilder --------------------------------------------------

    #[test]
    fn test_capability_builder_default() {
        let cap = CapabilityBuilder::new().build();
        assert_eq!(cap.cap_type, "compute");
        assert_eq!(cap.name, "http");
        assert!(cap.purpose.is_none());
    }

    #[test]
    fn test_capability_builder_with_purpose() {
        let cap = CapabilityBuilder::new().with_purpose("analytics").build();
        assert_eq!(cap.cap_type, "compute");
        assert_eq!(cap.name, "http");
        assert_eq!(cap.purpose.as_deref(), Some("analytics"));
    }

    #[test]
    fn test_capability_builder_full() {
        let cap = CapabilityBuilder::new()
            .with_cap_type("storage")
            .with_name("postgres-compatible")
            .with_purpose("analytics")
            .build();
        assert_eq!(cap.cap_type, "storage");
        assert_eq!(cap.name, "postgres-compatible");
        assert_eq!(cap.purpose.as_deref(), Some("analytics"));
    }

    // -- NodeCapabilitySetBuilder -------------------------------------------

    #[test]
    fn test_node_capability_set_builder_default() {
        let caps = NodeCapabilitySetBuilder::new().build();
        assert_eq!(caps.arch, "x86_64");
        assert_eq!(caps.os, "linux");
        assert_eq!(caps.privilege, PrivilegeLevel::User);
        assert_eq!(caps.runtimes, vec![RuntimeCapability::Oci]);
        assert!(!caps.ports_privileged);
        assert_eq!(caps.environment.as_deref(), Some("env:dev"));
        assert!(caps.author_affinity.is_none());
        assert_eq!(caps.clock_quality, ClockQuality::Ntp);
    }

    #[test]
    fn test_node_capability_set_builder_custom() {
        let caps = NodeCapabilitySetBuilder::new()
            .with_arch("aarch64")
            .with_os("darwin")
            .with_privilege(PrivilegeLevel::Root)
            .with_runtimes(vec![RuntimeCapability::Wasm, RuntimeCapability::Native])
            .with_ports_privileged(true)
            .with_environment(Some("env:prod".to_string()))
            .with_clock_quality(ClockQuality::Gps)
            .build();

        assert_eq!(caps.arch, "aarch64");
        assert_eq!(caps.os, "darwin");
        assert_eq!(caps.privilege, PrivilegeLevel::Root);
        assert_eq!(
            caps.runtimes,
            vec![RuntimeCapability::Wasm, RuntimeCapability::Native]
        );
        assert!(caps.ports_privileged);
        assert_eq!(caps.environment.as_deref(), Some("env:prod"));
        assert_eq!(caps.clock_quality, ClockQuality::Gps);
    }

    // -- Builder default-impl consistency -----------------------------------

    #[test]
    fn test_builder_default_matches_new() {
        let a = WorkloadUnitBuilder::new().build();
        let b = WorkloadUnitBuilder::default().build();
        // Same kind and default scaling (IDs differ since fresh UUIDs).
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.scaling, b.scaling);
        assert_eq!(a.provides, b.provides);
    }
}
