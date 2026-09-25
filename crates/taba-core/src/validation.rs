//! Unit validation: structural well-formedness and author scope checks.
//!
//! [`UnitValidator`] is the unit's type-checker. It validates structural
//! well-formedness (required fields, sorted capabilities, scaling
//! consistency, recovery self-references) and author scope (INV-S5).
//! It does NOT verify signatures (that is `security::Verifier`) or
//! check graph-level constraints (that is `graph::MergePolicy`).

use thiserror::Error;

use taba_common::{AuthorId, TrustDomainId, UnitId};

use crate::data::{RetentionMode, RetentionPolicy};
use crate::unit::{
    DataUnit, GovernanceUnit, RoleAssignment, Unit, UnitKind, UnitTypeScope, WorkloadUnit,
};

// ===========================================================================
// CoreError
// ===========================================================================

/// Errors produced by taba-core operations.
///
/// Each variant carries sufficient context for diagnosis. Error
/// propagation uses `?`. User-facing errors (surfaced through
/// taba-cli) are actionable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    /// Unit failed structural validation (missing fields, invalid
    /// type, inconsistent scaling, self-referencing recovery, etc.).
    #[error("malformed unit: {reason}")]
    MalformedUnit {
        /// Human-readable description of what is wrong.
        reason: String,
    },

    /// Author scope does not permit creating this unit type in this
    /// trust domain (INV-S5).
    #[error(
        "scope violation: author {author:?} cannot create {unit_kind} units in trust domain {domain:?}"
    )]
    ScopeViolation {
        /// The author who attempted to create the unit.
        author: AuthorId,
        /// The kind of unit that was attempted.
        unit_kind: UnitKind,
        /// The trust domain in which the attempt was made.
        domain: TrustDomainId,
    },

    /// Capability list is not well-formed (duplicates, unsorted, etc.).
    #[error("invalid capability: {reason}")]
    InvalidCapability {
        /// Human-readable description of what is wrong.
        reason: String,
    },

    /// Referenced unit does not exist in the store.
    #[error("unit not found: {id:?}")]
    UnitNotFound {
        /// The ID of the unit that was not found.
        id: UnitId,
    },

    /// Storage backend error.
    #[error("store error: {reason}")]
    StoreError {
        /// Human-readable description of the storage failure.
        reason: String,
    },
}

// ===========================================================================
// Data hierarchy validation (INV-D3, INV-S7)
// ===========================================================================

/// Maximum hierarchy depth for data units (INV-D3).
///
/// Children exist only where constraints diverge from parent. Beyond
/// this depth, the hierarchy is too deep and rejected at merge.
pub const MAX_DATA_HIERARCHY_DEPTH: u8 = 16;

/// Validates that a child data unit's constraints narrow (or are equal
/// to) the parent's constraints (INV-D3, INV-S7).
///
/// A child can narrow parent constraints freely (more restrictive) but
/// can widen (less restrictive) only with explicit policy. This
/// function checks the structural constraint; it does NOT verify
/// policy authorization — callers that need widening must check for a
/// declassification policy separately.
///
/// # Checks
///
/// 1. **Classification** (INV-S7): the child's classification must be
///    ≥ the parent's in the lattice (`Public < Internal < Confidential
///    < Pii`). Narrowing (child more restrictive) is always allowed.
///    Widening (child less restrictive) is rejected.
/// 2. **Retention** (INV-D3): the child's retention must be ≥ the
///    parent's (longer or equal retention is narrowing, always
///    allowed). Shorter retention (widening) is rejected without
///    explicit policy.
///
/// # Errors
///
/// Returns [`CoreError::MalformedUnit`] if the child widens the
/// parent's classification or retention constraints.
pub fn validate_data_hierarchy(parent: &DataUnit, child: &DataUnit) -> Result<(), CoreError> {
    // (b) Classification: child can narrow (more restrictive) but not
    // widen (less restrictive) compared to parent (INV-S7).
    //
    // The Classification enum derives Ord with Public < Internal <
    // Confidential < Pii. A child with classification >= parent is
    // narrowing (or equal) — always allowed.
    if child.classification < parent.classification {
        return Err(CoreError::MalformedUnit {
            reason: format!(
                "data hierarchy violation: child classification {:?} widens parent classification {:?} (INV-S7)",
                child.classification, parent.classification
            ),
        });
    }

    // (c) Retention: child can narrow (longer retention) but not widen
    // (shorter retention) without a policy unit (INV-D3).
    if !retention_narrows_or_equal(&parent.retention, &child.retention) {
        return Err(CoreError::MalformedUnit {
            reason: format!(
                "data hierarchy violation: child retention mode {:?} widens parent retention mode {:?} (shorter without policy, INV-D3)",
                child.retention.mode, parent.retention.mode
            ),
        });
    }

    Ok(())
}

/// Validates that a hierarchy depth does not exceed the maximum
/// (INV-D3).
///
/// The maximum hierarchy depth is [`MAX_DATA_HIERARCHY_DEPTH`] (16).
/// This is a standalone function because the depth is computed by
/// traversing the parent chain in the graph, not from a single
/// parent-child pair.
///
/// # Errors
///
/// Returns [`CoreError::MalformedUnit`] if `depth` exceeds
/// [`MAX_DATA_HIERARCHY_DEPTH`].
pub fn validate_hierarchy_depth(depth: u8) -> Result<(), CoreError> {
    if depth > MAX_DATA_HIERARCHY_DEPTH {
        return Err(CoreError::MalformedUnit {
            reason: format!(
                "data hierarchy depth {depth} exceeds maximum {MAX_DATA_HIERARCHY_DEPTH} (INV-D3)"
            ),
        });
    }
    Ok(())
}

/// Returns `true` if the child's retention is at least as restrictive
/// (≥ duration) as the parent's.
///
/// Retention ordering (most to least restrictive):
/// - `Persistent` (with longer or no duration) > `Ephemeral` > `LocalOnly`
///
/// For two `Persistent` policies with explicit durations, the child's
/// duration must be ≥ the parent's.
fn retention_narrows_or_equal(parent: &RetentionPolicy, child: &RetentionPolicy) -> bool {
    // Rank retention modes from least (0) to most (2) restrictive.
    let parent_rank = match parent.mode {
        RetentionMode::Persistent => 2,
        RetentionMode::Ephemeral => 1,
        RetentionMode::LocalOnly => 0,
    };
    let child_rank = match child.mode {
        RetentionMode::Persistent => 2,
        RetentionMode::Ephemeral => 1,
        RetentionMode::LocalOnly => 0,
    };

    // Child must be at least as restrictive as the parent.
    if child_rank < parent_rank {
        return false;
    }

    // If both are Persistent with explicit durations, the child must
    // retain for at least as long as the parent.
    if parent.mode == RetentionMode::Persistent && child.mode == RetentionMode::Persistent {
        if let (Some(p_dur), Some(c_dur)) = (parent.duration, child.duration) {
            return c_dur >= p_dur;
        }
    }

    true
}

// ===========================================================================
// UnitValidator trait
// ===========================================================================

/// Validates that a unit is well-formed before it enters the graph.
///
/// This is a structural check only — it does NOT verify signatures
/// (that is `security::Verifier`) or check graph-level constraints
/// (that is `graph::MergePolicy`). Think of it as the unit's
/// type-checker.
pub trait UnitValidator {
    /// Check that the unit is structurally valid.
    ///
    /// Validates:
    /// - All required fields present for the unit's kind
    /// - Capability lists are well-formed and sorted (INV-K2)
    /// - Data unit hierarchy depth <= 16 (domain-model constraint)
    /// - Recovery relationships contain no self-references
    /// - Scaling parameters are internally consistent (min <= max)
    /// - Retention declarations are internally consistent
    ///
    /// Returns `Ok(())` on success, [`CoreError::MalformedUnit`] or
    /// [`CoreError::InvalidCapability`] on failure.
    fn validate(&self, unit: &Unit) -> Result<(), CoreError>;

    /// Check that the author's scope permits creating this unit.
    ///
    /// Enforces INV-S5: authors cannot create units outside their
    /// (type scope × trust domain scope). Returns
    /// [`CoreError::ScopeViolation`] if the author lacks authority.
    fn validate_author_scope(
        &self,
        author: &AuthorId,
        unit: &Unit,
        domain: &TrustDomainId,
    ) -> Result<(), CoreError>;
}

// ===========================================================================
// DefaultValidator
// ===========================================================================

/// Default implementation of [`UnitValidator`].
///
/// Performs structural validation of units and author scope checks
/// (INV-S5). Holds a set of [`RoleAssignment`] governance units to
/// verify author authority — the validator must be constructed with
/// the role assignments that are currently active in the local graph.
///
/// All validation methods are pure: no I/O, no side effects. The same
/// input always produces the same output.
#[derive(Debug, Clone, Default)]
pub struct DefaultValidator {
    /// Role assignments that define author scopes, indexed for
    /// lookup by [`AuthorId`].
    role_assignments: Vec<RoleAssignment>,
}

impl DefaultValidator {
    /// Creates a new validator with the given role assignments.
    ///
    /// The role assignments must be the currently active set in the
    /// local graph. The validator uses them to check author authority
    /// (INV-S5).
    #[must_use]
    pub const fn new(role_assignments: Vec<RoleAssignment>) -> Self {
        Self { role_assignments }
    }

    /// Creates a new validator with no role assignments.
    ///
    /// All author scope checks will fail (every author is treated as
    /// unscoped). Useful for structural-only validation or testing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns `true` if the capability list is well-formed: sorted
    /// in strictly increasing lexicographic order with no duplicates
    /// (INV-K2).
    fn is_well_formed(caps: &[crate::capability::Capability]) -> bool {
        caps.windows(2).all(|w| w[0] < w[1])
    }

    /// Validates a [`WorkloadUnit`].
    fn validate_workload(w: &WorkloadUnit) -> Result<(), CoreError> {
        // A workload must declare at least one provided capability.
        if w.provides.is_empty() {
            return Err(CoreError::MalformedUnit {
                reason: "workload must declare at least one provided capability".to_string(),
            });
        }

        // A workload must declare at least one tolerance dimension.
        if !w.tolerates.is_meaningful() {
            return Err(CoreError::MalformedUnit {
                reason: "workload must declare at least one tolerance (max_latency, failure_modes, or consistency)"
                    .to_string(),
            });
        }

        // Capability lists must be well-formed and sorted (INV-K2).
        if !Self::is_well_formed(&w.needs) {
            return Err(CoreError::InvalidCapability {
                reason: "needs list is not sorted or contains duplicates".to_string(),
            });
        }
        if !Self::is_well_formed(&w.provides) {
            return Err(CoreError::InvalidCapability {
                reason: "provides list is not sorted or contains duplicates".to_string(),
            });
        }

        // Scaling parameters: min_instances must not exceed max_instances.
        if w.scaling.min_instances > w.scaling.max_instances {
            return Err(CoreError::MalformedUnit {
                reason: format!(
                    "scaling min_instances ({}) exceeds max_instances ({})",
                    w.scaling.min_instances, w.scaling.max_instances
                ),
            });
        }

        // Recovery relationships must not contain self-references (INV-K5).
        let self_id = w.header.id;
        if w.recovery_relationships
            .iter()
            .any(|rr| rr.depends_on == self_id)
        {
            return Err(CoreError::MalformedUnit {
                reason:
                    "recovery relationship contains a self-reference (depends_on == own unit id)"
                        .to_string(),
            });
        }

        // Bounded tasks must have a validity window (INV-W2).
        if w.kind == crate::unit::WorkloadKind::BoundedTask && w.header.validity.is_none() {
            return Err(CoreError::MalformedUnit {
                reason: "bounded task must declare a validity window (INV-W2)".to_string(),
            });
        }

        Ok(())
    }

    /// Validates a [`crate::unit::DataUnit`].
    fn validate_data(d: &crate::unit::DataUnit) -> Result<(), CoreError> {
        // A data unit must declare at least one provided capability.
        if d.provides.is_empty() {
            return Err(CoreError::MalformedUnit {
                reason: "data unit must declare at least one provided capability".to_string(),
            });
        }

        // Capability lists must be well-formed and sorted (INV-K2).
        if !Self::is_well_formed(&d.provides) {
            return Err(CoreError::InvalidCapability {
                reason: "provides list is not sorted or contains duplicates".to_string(),
            });
        }

        // Parent must not be the unit itself (no self-referencing parent).
        if d.parent == Some(d.header.id) {
            return Err(CoreError::MalformedUnit {
                reason: "data unit parent cannot be the unit itself".to_string(),
            });
        }

        // Retention declarations must be internally consistent.
        // Persistent mode requires a duration; legal_basis must be non-empty.
        if d.retention.legal_basis.is_empty() {
            return Err(CoreError::MalformedUnit {
                reason: "retention policy must declare a legal basis".to_string(),
            });
        }
        if d.retention.mode == crate::data::RetentionMode::Persistent
            && d.retention.duration.is_none()
        {
            return Err(CoreError::MalformedUnit {
                reason: "persistent retention mode requires a duration".to_string(),
            });
        }

        Ok(())
    }

    /// Validates a [`crate::unit::PolicyUnit`].
    fn validate_policy(p: &crate::unit::PolicyUnit) -> Result<(), CoreError> {
        // A policy must reference at least one unit in its conflict tuple.
        if p.conflict.unit_ids.is_empty() {
            return Err(CoreError::MalformedUnit {
                reason: "policy conflict tuple must reference at least one unit".to_string(),
            });
        }

        // A policy must have a non-empty rationale.
        if p.rationale.is_empty() {
            return Err(CoreError::MalformedUnit {
                reason: "policy must declare a non-empty rationale".to_string(),
            });
        }

        Ok(())
    }

    /// Validates a [`GovernanceUnit`].
    fn validate_governance(g: &GovernanceUnit) -> Result<(), CoreError> {
        match g {
            GovernanceUnit::TrustDomainDef(td) => {
                // Trust domain creation requires multi-party signing (INV-S10).
                let distinct_signers: std::collections::HashSet<_> = td.signers.iter().collect();
                if distinct_signers.len() < 2 {
                    return Err(CoreError::MalformedUnit {
                        reason: format!(
                            "trust domain creation requires at least 2 distinct signers (INV-S10), found {}",
                            distinct_signers.len()
                        ),
                    });
                }
                if td.name.is_empty() {
                    return Err(CoreError::MalformedUnit {
                        reason: "trust domain must declare a non-empty name".to_string(),
                    });
                }
                Ok(())
            }
            GovernanceUnit::RoleAssignment(ra) => {
                // An author must have at least one unit type scope and
                // at least one trust domain scope.
                if ra.unit_type_scope.is_empty() {
                    return Err(CoreError::MalformedUnit {
                        reason: "role assignment must declare at least one unit type scope"
                            .to_string(),
                    });
                }
                if ra.trust_domain_scope.is_empty() {
                    return Err(CoreError::MalformedUnit {
                        reason: "role assignment must declare at least one trust domain scope"
                            .to_string(),
                    });
                }
                Ok(())
            }
            GovernanceUnit::OperationalCommand(oc) => match &oc.command_type {
                crate::unit::OperationalCommandType::EnterDegraded { reason }
                    if reason.is_empty() =>
                {
                    Err(CoreError::MalformedUnit {
                        reason: "enter-degraded command must declare a non-empty reason"
                            .to_string(),
                    })
                }
                _ => Ok(()),
            },
            GovernanceUnit::PromotionGate(pg) => {
                // A promotion gate must have at least one transition.
                if pg.transitions.is_empty() {
                    return Err(CoreError::MalformedUnit {
                        reason: "promotion gate must declare at least one transition".to_string(),
                    });
                }
                Ok(())
            }
            // Certifications, cross-domain capabilities, and key revocations
            // are validated structurally — all required fields are typed.
            _ => Ok(()),
        }
    }
}

impl UnitValidator for DefaultValidator {
    fn validate(&self, unit: &Unit) -> Result<(), CoreError> {
        match unit {
            Unit::Workload(w) => Self::validate_workload(w),
            Unit::Data(d) => Self::validate_data(d),
            Unit::Policy(p) => Self::validate_policy(p),
            Unit::Governance(g) => Self::validate_governance(g),
        }
    }

    fn validate_author_scope(
        &self,
        author: &AuthorId,
        unit: &Unit,
        domain: &TrustDomainId,
    ) -> Result<(), CoreError> {
        let unit_kind = unit.kind();
        let type_scope: UnitTypeScope = unit_kind.into();

        // Find all role assignments for this author.
        let author_assignments: Vec<&RoleAssignment> = self
            .role_assignments
            .iter()
            .filter(|ra| &ra.assignee == author)
            .collect();

        if author_assignments.is_empty() {
            return Err(CoreError::ScopeViolation {
                author: *author,
                unit_kind,
                domain: *domain,
            });
        }

        // Check type scope: at least one assignment must include the
        // unit's kind in its unit_type_scope.
        let has_type_scope = author_assignments
            .iter()
            .any(|ra| ra.unit_type_scope.contains(&type_scope));

        // Check domain scope: at least one assignment must include the
        // given domain in its trust_domain_scope.
        let has_domain_scope = author_assignments
            .iter()
            .any(|ra| ra.trust_domain_scope.contains(domain));

        if !has_type_scope || !has_domain_scope {
            return Err(CoreError::ScopeViolation {
                author: *author,
                unit_kind,
                domain: *domain,
            });
        }

        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{Artifact, ArtifactType};
    use crate::capability::Capability;
    use crate::contract::{
        CrashBehavior, FailureSemantics, OomBehavior, PlacementOnFailure, RecoveryAction,
        RecoveryRelationship, Scaling, StateRecovery, Tolerances,
    };
    use crate::data::{
        Classification, DataSchema, RetentionMode, RetentionPolicy, StorageRequirements,
    };
    use crate::unit::{
        ConflictTuple, PolicyResolution, PolicyUnit, UnitHeader, UnitState, UnitTypeScope,
        WorkloadKind, WorkloadUnit,
    };
    use std::collections::BTreeSet;
    use taba_common::{
        AuthorId, DualClockEvent, LogicalClock, TrustDomainId, UnitId, ValidityWindow, Version,
        WallTime,
    };

    /// Creates a minimal [`UnitHeader`] for testing.
    fn test_header() -> UnitHeader {
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

    /// Creates a minimal valid [`WorkloadUnit`] (Service kind).
    fn valid_workload() -> WorkloadUnit {
        WorkloadUnit {
            header: test_header(),
            kind: WorkloadKind::Service,
            artifact: Artifact {
                artifact_type: ArtifactType::Oci,
                artifact_ref: "registry.example.com/app:v1".to_string(),
                digest: taba_common::ContentDigest("sha256:abc123".to_string()),
                requires: Vec::new(),
                kernel_ref: None,
                rootfs_ref: None,
            },
            needs: Vec::new(),
            provides: vec![
                Capability::new("compute", "http"),
                Capability::new("storage", "postgres"),
            ],
            tolerates: Tolerances {
                max_latency: Some(std::time::Duration::from_millis(100)),
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
                on_shutdown: crate::contract::ShutdownBehavior::Immediate,
            },
            recovery_relationships: Vec::new(),
            state_recovery: StateRecovery::Stateless,
            placement_on_failure: Some(PlacementOnFailure::Replace),
            health_check: None,
            spawn_context: None,
        }
    }

    /// Creates a minimal valid [`Unit`] wrapping a [`WorkloadUnit`].
    fn valid_workload_unit() -> Unit {
        Unit::Workload(valid_workload())
    }

    // -- validate: valid workload -----------------------------------------

    #[test]
    fn test_validate_valid_workload_unit() {
        let validator = DefaultValidator::empty();
        let unit = valid_workload_unit();
        assert!(
            validator.validate(&unit).is_ok(),
            "well-formed unit should pass"
        );
    }

    #[test]
    fn test_validate_missing_provides() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        workload.provides = Vec::new();

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "empty provides should be rejected with MalformedUnit"
        );
    }

    #[test]
    fn test_validate_missing_tolerates() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        workload.tolerates = Tolerances {
            max_latency: None,
            failure_modes: Vec::new(),
            consistency: None,
        };

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "empty tolerates should be rejected with MalformedUnit"
        );
    }

    #[test]
    fn test_validate_scaling_min_greater_than_max() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        workload.scaling = Scaling {
            min_instances: 5,
            max_instances: 3,
            triggers: Vec::new(),
        };

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "scaling min > max should be rejected"
        );
    }

    #[test]
    fn test_validate_recovery_self_reference() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        let self_id = workload.header.id;
        workload.recovery_relationships = vec![RecoveryRelationship {
            depends_on: self_id,
            action: RecoveryAction::DrainFirst,
        }];

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "recovery self-reference should be rejected"
        );
    }

    #[test]
    fn test_validate_recovery_no_self_reference() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        workload.recovery_relationships = vec![RecoveryRelationship {
            depends_on: UnitId(uuid::Uuid::new_v4()),
            action: RecoveryAction::DrainFirst,
        }];

        let result = validator.validate(&Unit::Workload(workload));
        assert!(result.is_ok(), "valid recovery relationship should pass");
    }

    #[test]
    fn test_validate_bounded_task_without_validity() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        workload.kind = WorkloadKind::BoundedTask;
        workload.header.validity = None;

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "bounded task without validity window should be rejected (INV-W2)"
        );
    }

    #[test]
    fn test_validate_bounded_task_with_validity() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        workload.kind = WorkloadKind::BoundedTask;
        workload.header.validity = Some(ValidityWindow {
            lc_range: Some((LogicalClock(10), LogicalClock(100))),
            wall_time_deadline: None,
        });

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            result.is_ok(),
            "bounded task with validity window should pass"
        );
    }

    #[test]
    fn test_validate_unsorted_capabilities() {
        let validator = DefaultValidator::empty();
        let mut workload = valid_workload();
        // Deliberately unsorted: "storage" before "compute"
        workload.provides = vec![
            Capability::new("storage", "postgres"),
            Capability::new("compute", "http"),
        ];

        let result = validator.validate(&Unit::Workload(workload));
        assert!(
            matches!(result, Err(CoreError::InvalidCapability { .. })),
            "unsorted capabilities should be rejected with InvalidCapability"
        );
    }

    #[test]
    fn test_validate_data_unit() {
        let validator = DefaultValidator::empty();
        let data = Unit::Data(crate::unit::DataUnit {
            header: test_header(),
            schema: DataSchema {
                format: "json-schema".to_string(),
                definition: "{}".to_string(),
            },
            classification: Classification::Internal,
            provenance: None,
            retention: RetentionPolicy {
                mode: RetentionMode::Persistent,
                duration: Some(std::time::Duration::from_secs(86400)),
                legal_basis: "consent".to_string(),
                mandatory: false,
            },
            consent_scope: Vec::new(),
            storage_requirements: StorageRequirements {
                encrypted_at_rest: true,
                jurisdictions: vec!["EU".to_string()],
                min_replicas: None,
            },
            parent: None,
            provides: vec![Capability::new("storage", "dataset-x")],
        });

        assert!(
            validator.validate(&data).is_ok(),
            "valid data unit should pass"
        );
    }

    #[test]
    fn test_validate_policy_unit() {
        let validator = DefaultValidator::empty();
        let policy = Unit::Policy(PolicyUnit {
            header: test_header(),
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
        });

        assert!(
            validator.validate(&policy).is_ok(),
            "valid policy unit should pass"
        );
    }

    // -- validate_author_scope ---------------------------------------------

    /// Creates a [`RoleAssignment`] for the given author with the
    /// given type and domain scopes.
    fn role_for(
        author: AuthorId,
        type_scopes: Vec<UnitTypeScope>,
        domain_scopes: Vec<TrustDomainId>,
    ) -> RoleAssignment {
        RoleAssignment {
            header: test_header(),
            assignee: author,
            unit_type_scope: type_scopes,
            trust_domain_scope: domain_scopes,
        }
    }

    #[test]
    fn test_validate_author_scope_correct() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());
        let roles = vec![role_for(
            author,
            vec![UnitTypeScope::Workload],
            vec![domain],
        )];
        let validator = DefaultValidator::new(roles);
        let unit = valid_workload_unit();

        let result = validator.validate_author_scope(&author, &unit, &domain);
        assert!(result.is_ok(), "author with correct scope should pass");
    }

    #[test]
    fn test_validate_author_scope_wrong_type() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());
        // Author has Data scope, not Workload scope.
        let roles = vec![role_for(author, vec![UnitTypeScope::Data], vec![domain])];
        let validator = DefaultValidator::new(roles);
        let unit = valid_workload_unit(); // Workload unit

        let result = validator.validate_author_scope(&author, &unit, &domain);
        assert!(
            matches!(result, Err(CoreError::ScopeViolation { .. })),
            "author with wrong type scope should be rejected with ScopeViolation"
        );

        if let Err(CoreError::ScopeViolation {
            author: err_author,
            unit_kind,
            domain: err_domain,
        }) = result
        {
            assert_eq!(err_author, author);
            assert_eq!(unit_kind, UnitKind::Workload);
            assert_eq!(err_domain, domain);
        }
    }

    #[test]
    fn test_validate_author_scope_wrong_domain() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain_a = TrustDomainId(uuid::Uuid::new_v4());
        let domain_b = TrustDomainId(uuid::Uuid::new_v4());
        // Author has Workload scope in domain_a, but we're checking domain_b.
        let roles = vec![role_for(
            author,
            vec![UnitTypeScope::Workload],
            vec![domain_a],
        )];
        let validator = DefaultValidator::new(roles);
        let unit = valid_workload_unit();

        let result = validator.validate_author_scope(&author, &unit, &domain_b);
        assert!(
            matches!(result, Err(CoreError::ScopeViolation { .. })),
            "author with wrong domain scope should be rejected with ScopeViolation"
        );

        if let Err(CoreError::ScopeViolation {
            author: err_author,
            unit_kind,
            domain: err_domain,
        }) = result
        {
            assert_eq!(err_author, author);
            assert_eq!(unit_kind, UnitKind::Workload);
            assert_eq!(err_domain, domain_b);
        }
    }

    #[test]
    fn test_validate_author_scope_no_roles() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());
        let validator = DefaultValidator::empty();
        let unit = valid_workload_unit();

        let result = validator.validate_author_scope(&author, &unit, &domain);
        assert!(
            matches!(result, Err(CoreError::ScopeViolation { .. })),
            "author with no roles should be rejected with ScopeViolation"
        );
    }

    // -- CoreError display -------------------------------------------------

    #[test]
    fn test_core_error_malformed_unit_display() {
        let err = CoreError::MalformedUnit {
            reason: "missing provides".to_string(),
        };
        assert!(err.to_string().contains("malformed unit"));
        assert!(err.to_string().contains("missing provides"));
    }

    #[test]
    fn test_core_error_invalid_capability_display() {
        let err = CoreError::InvalidCapability {
            reason: "unsorted".to_string(),
        };
        assert!(err.to_string().contains("invalid capability"));
    }

    #[test]
    fn test_core_error_unit_not_found_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let err = CoreError::UnitNotFound { id };
        assert!(err.to_string().contains("unit not found"));
    }

    #[test]
    fn test_core_error_store_error_display() {
        let err = CoreError::StoreError {
            reason: "disk full".to_string(),
        };
        assert!(err.to_string().contains("store error"));
        assert!(err.to_string().contains("disk full"));
    }

    // -- validate_data_hierarchy (INV-D3, INV-S7) ---------------------------

    /// Creates a [`DataUnit`] with the given classification and
    /// retention policy.
    fn data_unit_with(
        classification: Classification,
        retention: RetentionPolicy,
    ) -> crate::unit::DataUnit {
        crate::unit::DataUnit {
            header: test_header(),
            schema: DataSchema {
                format: "json-schema".to_string(),
                definition: "{}".to_string(),
            },
            classification,
            provenance: None,
            retention,
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

    fn persistent_retention(duration_secs: u64) -> RetentionPolicy {
        RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: Some(std::time::Duration::from_secs(duration_secs)),
            legal_basis: "consent".to_string(),
            mandatory: false,
        }
    }

    #[test]
    fn scenario_child_pii_narrows_parent_internal_accepted() {
        // INV-S7: A child with PII (more restrictive) narrows a parent
        // with Internal (less restrictive). Narrowing is always allowed.
        let parent = data_unit_with(Classification::Internal, persistent_retention(3600));
        let child = data_unit_with(Classification::Pii, persistent_retention(3600));

        assert!(
            validate_data_hierarchy(&parent, &child).is_ok(),
            "child PII narrowing parent Internal should be accepted (INV-S7)"
        );
    }

    #[test]
    fn scenario_child_public_widens_parent_internal_rejected() {
        // INV-S7: A child with Public (less restrictive) widens a parent
        // with Internal (more restrictive). Widening requires policy and
        // is rejected without one.
        let parent = data_unit_with(Classification::Internal, persistent_retention(3600));
        let child = data_unit_with(Classification::Public, persistent_retention(3600));

        let result = validate_data_hierarchy(&parent, &child);
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "child Public widening parent Internal should be rejected (INV-S7)"
        );
    }

    #[test]
    fn scenario_child_same_classification_accepted() {
        // Equal classification is not widening.
        let parent = data_unit_with(Classification::Internal, persistent_retention(3600));
        let child = data_unit_with(Classification::Internal, persistent_retention(3600));

        assert!(
            validate_data_hierarchy(&parent, &child).is_ok(),
            "child with same classification as parent should be accepted"
        );
    }

    #[test]
    fn scenario_child_retention_longer_accepted() {
        // INV-D3: A child with longer retention (more restrictive)
        // narrows the parent. Always allowed.
        let parent = data_unit_with(
            Classification::Internal,
            persistent_retention(3600), // 1 hour
        );
        let child = data_unit_with(
            Classification::Internal,
            persistent_retention(7200), // 2 hours — longer, narrowing
        );

        assert!(
            validate_data_hierarchy(&parent, &child).is_ok(),
            "child with longer retention should be accepted (narrowing, INV-D3)"
        );
    }

    #[test]
    fn scenario_child_retention_shorter_rejected() {
        // INV-D3: A child with shorter retention (less restrictive)
        // widens the parent. Rejected without policy.
        let parent = data_unit_with(
            Classification::Internal,
            persistent_retention(7200), // 2 hours
        );
        let child = data_unit_with(
            Classification::Internal,
            persistent_retention(3600), // 1 hour — shorter, widening
        );

        let result = validate_data_hierarchy(&parent, &child);
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "child with shorter retention should be rejected (widening, INV-D3)"
        );
    }

    #[test]
    fn scenario_child_ephemeral_widens_parent_persistent_rejected() {
        // Ephemeral has shorter retention than Persistent — widening.
        let parent = data_unit_with(Classification::Internal, persistent_retention(3600));
        let child = data_unit_with(
            Classification::Internal,
            RetentionPolicy {
                mode: RetentionMode::Ephemeral,
                duration: None,
                legal_basis: "task-scratch".to_string(),
                mandatory: false,
            },
        );

        let result = validate_data_hierarchy(&parent, &child);
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "child Ephemeral widening parent Persistent should be rejected (INV-D3)"
        );
    }

    #[test]
    fn scenario_child_persistent_narrows_parent_ephemeral_accepted() {
        // Persistent has longer retention than Ephemeral — narrowing.
        let parent = data_unit_with(
            Classification::Internal,
            RetentionPolicy {
                mode: RetentionMode::Ephemeral,
                duration: None,
                legal_basis: "task-scratch".to_string(),
                mandatory: false,
            },
        );
        let child = data_unit_with(Classification::Internal, persistent_retention(3600));

        assert!(
            validate_data_hierarchy(&parent, &child).is_ok(),
            "child Persistent narrowing parent Ephemeral should be accepted (INV-D3)"
        );
    }

    // -- validate_hierarchy_depth (INV-D3) ----------------------------------

    #[test]
    fn scenario_depth_at_maximum_accepted() {
        assert!(
            validate_hierarchy_depth(MAX_DATA_HIERARCHY_DEPTH).is_ok(),
            "depth at maximum ({MAX_DATA_HIERARCHY_DEPTH}) should be accepted"
        );
    }

    #[test]
    fn scenario_depth_below_maximum_accepted() {
        assert!(
            validate_hierarchy_depth(5).is_ok(),
            "depth below maximum should be accepted"
        );
    }

    #[test]
    fn scenario_depth_exceeds_maximum_rejected() {
        // INV-D3: Hierarchy depth exceeding 16 is rejected at graph merge.
        let result = validate_hierarchy_depth(17);
        assert!(
            matches!(result, Err(CoreError::MalformedUnit { .. })),
            "depth > 16 should be rejected (INV-D3)"
        );
    }
}
