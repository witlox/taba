//! Author scope checking and type-dependent uniqueness (INV-S5, INV-S8, INV-S8a).
//!
//! Authors cannot create units outside their (type scope × trust domain
//! scope) — INV-S5. For state-producing unit types (workload, data), no
//! two distinct authors may have identical scope tuples — INV-S8. For
//! decision-making types (policy, governance), overlapping scopes are
//! permitted — INV-S8a.

use std::collections::{BTreeSet, HashSet};

use taba_common::{AuthorId, TrustDomainId};
use taba_core::unit::{RoleAssignment, UnitKind, UnitTypeScope};

use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// ScopeChecker trait
// ---------------------------------------------------------------------------

/// Checks author scope for unit creation (INV-S5, INV-S8, INV-S8a).
///
/// Scope enforcement is a SEPARATE gate from signature verification
/// (INV-S3). Both must pass before a unit can enter the graph.
pub trait ScopeChecker {
    /// Check whether an author has valid scope for the given unit type
    /// and trust domain (INV-S5).
    ///
    /// Returns [`Ok`] if the author has a role assignment whose
    /// `unit_type_scope` contains the target type and whose
    /// `trust_domain_scope` contains the target domain. Returns
    /// [`SecurityError::ScopeViolation`] otherwise.
    fn check_author_scope(
        &self,
        author: &AuthorId,
        unit_type: UnitKind,
        trust_domain: &TrustDomainId,
    ) -> Result<(), SecurityError>;

    /// Validate that no two distinct authors have identical scope tuples
    /// for the given unit type (INV-S8, INV-S8a).
    ///
    /// For state-producing types (workload, data), overlapping scopes
    /// are a violation (INV-S8). For decision-making types (policy,
    /// governance), overlapping scopes are permitted (INV-S8a).
    fn validate_scope_uniqueness(
        &self,
        assignments: &[RoleAssignment],
        unit_type: UnitKind,
    ) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultScopeChecker
// ---------------------------------------------------------------------------

/// Default implementation of [`ScopeChecker`].
///
/// Holds a list of [`RoleAssignment`] governance units that define
/// author permissions. Scope checking is a pure, in-memory operation —
/// no I/O, no side effects.
#[derive(Debug, Clone, Default)]
pub struct DefaultScopeChecker {
    /// Active role assignments scoped by author.
    assignments: Vec<RoleAssignment>,
}

impl DefaultScopeChecker {
    /// Creates a new empty scope checker.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            assignments: Vec::new(),
        }
    }

    /// Creates a scope checker from an existing list of role assignments.
    #[must_use]
    pub const fn from_assignments(assignments: Vec<RoleAssignment>) -> Self {
        Self { assignments }
    }

    /// Adds a role assignment to this checker.
    pub fn add_assignment(&mut self, assignment: RoleAssignment) {
        self.assignments.push(assignment);
    }
}

impl ScopeChecker for DefaultScopeChecker {
    fn check_author_scope(
        &self,
        author: &AuthorId,
        unit_type: UnitKind,
        trust_domain: &TrustDomainId,
    ) -> Result<(), SecurityError> {
        let target_type_scope = UnitTypeScope::from(unit_type);

        let has_scope = self.assignments.iter().any(|a| {
            a.assignee == *author
                && a.unit_type_scope.contains(&target_type_scope)
                && a.trust_domain_scope.contains(trust_domain)
        });

        if has_scope {
            Ok(())
        } else {
            Err(SecurityError::ScopeViolation {
                author: *author,
                reason: format!(
                    "author does not have {unit_type} scope in trust domain {trust_domain:?}"
                ),
            })
        }
    }

    fn validate_scope_uniqueness(
        &self,
        assignments: &[RoleAssignment],
        unit_type: UnitKind,
    ) -> Result<(), SecurityError> {
        // INV-S8a: overlapping scopes are permitted for policy and governance.
        if matches!(unit_type, UnitKind::Policy | UnitKind::Governance) {
            return Ok(());
        }

        // INV-S8: for state-producing types (workload, data), no two distinct
        // authors may have identical (unit_type_scope, trust_domain_scope) tuples.
        let target_type_scope = UnitTypeScope::from(unit_type);

        // Collect authors grouped by their scope tuple.
        // Key: (HashSet<UnitTypeScope>, HashSet<TrustDomainId>) — order-independent
        // Value: set of distinct author IDs
        #[allow(clippy::type_complexity)]
        let mut groups: Vec<(
            (HashSet<UnitTypeScope>, HashSet<TrustDomainId>),
            BTreeSet<AuthorId>,
        )> = Vec::new();

        for assignment in assignments {
            // Only consider assignments that include the target unit type.
            if !assignment.unit_type_scope.contains(&target_type_scope) {
                continue;
            }

            let type_scope: HashSet<UnitTypeScope> =
                assignment.unit_type_scope.iter().copied().collect();
            let domain_scope: HashSet<TrustDomainId> =
                assignment.trust_domain_scope.iter().copied().collect();
            let key = (type_scope, domain_scope);

            if let Some((_, authors)) = groups.iter_mut().find(|(k, _)| *k == key) {
                authors.insert(assignment.assignee);
            } else {
                let mut authors = BTreeSet::new();
                authors.insert(assignment.assignee);
                groups.push((key, authors));
            }
        }

        // Check for groups with 2+ distinct authors.
        for ((type_scope, domain_scope), authors) in &groups {
            if authors.len() >= 2 {
                let first = authors
                    .iter()
                    .next()
                    .copied()
                    .expect("authors set is guaranteed non-empty when len >= 2");
                return Err(SecurityError::ScopeViolation {
                    author: first,
                    reason: format!(
                        "two or more distinct authors have identical scope tuple \
                         (unit_type_scope={type_scope:?}, trust_domain_scope={domain_scope:?}) \
                         for state-producing unit type {unit_type} (INV-S8)"
                    ),
                });
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{DualClockEvent, LogicalClock, UnitId, WallTime};
    use taba_core::unit::{UnitHeader, UnitState};

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

    /// Creates a [`RoleAssignment`] for the given author, type scopes, and
    /// domain scopes.
    fn role_assignment(
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
    fn test_scope_check_correct() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());

        let checker = DefaultScopeChecker::from_assignments(vec![role_assignment(
            author,
            vec![UnitTypeScope::Workload],
            vec![domain],
        )]);

        assert!(
            checker
                .check_author_scope(&author, UnitKind::Workload, &domain)
                .is_ok(),
            "author with correct scope should pass"
        );
    }

    #[test]
    fn test_scope_check_wrong_type() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());

        // Author has workload scope only.
        let checker = DefaultScopeChecker::from_assignments(vec![role_assignment(
            author,
            vec![UnitTypeScope::Workload],
            vec![domain],
        )]);

        // Author tries to create a policy unit — should fail.
        let result = checker.check_author_scope(&author, UnitKind::Policy, &domain);
        assert!(
            matches!(result, Err(SecurityError::ScopeViolation { .. })),
            "author with workload scope creating policy should fail with ScopeViolation, got: {result:?}"
        );
    }

    #[test]
    fn test_scope_check_wrong_domain() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let domain_a = TrustDomainId(uuid::Uuid::new_v4());
        let domain_b = TrustDomainId(uuid::Uuid::new_v4());

        // Author is scoped to domain A only.
        let checker = DefaultScopeChecker::from_assignments(vec![role_assignment(
            author,
            vec![UnitTypeScope::Workload],
            vec![domain_a],
        )]);

        // Author tries to create in domain B — should fail.
        let result = checker.check_author_scope(&author, UnitKind::Workload, &domain_b);
        assert!(
            matches!(result, Err(SecurityError::ScopeViolation { .. })),
            "author scoped to domain A creating in domain B should fail, got: {result:?}"
        );
    }

    #[test]
    fn test_scope_uniqueness_violation() {
        let author1 = AuthorId(uuid::Uuid::new_v4());
        let author2 = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());

        // Two distinct authors with the same (type, domain) for workload.
        let assignments = vec![
            role_assignment(author1, vec![UnitTypeScope::Workload], vec![domain]),
            role_assignment(author2, vec![UnitTypeScope::Workload], vec![domain]),
        ];

        let checker = DefaultScopeChecker::new();
        let result = checker.validate_scope_uniqueness(&assignments, UnitKind::Workload);
        assert!(
            matches!(result, Err(SecurityError::ScopeViolation { .. })),
            "two authors with same (type, domain) for workload should violate INV-S8, got: {result:?}"
        );
    }

    #[test]
    fn test_scope_uniqueness_allowed_for_policy() {
        let author1 = AuthorId(uuid::Uuid::new_v4());
        let author2 = AuthorId(uuid::Uuid::new_v4());
        let domain = TrustDomainId(uuid::Uuid::new_v4());

        // Two distinct authors with the same (type, domain) for policy.
        let assignments = vec![
            role_assignment(author1, vec![UnitTypeScope::Policy], vec![domain]),
            role_assignment(author2, vec![UnitTypeScope::Policy], vec![domain]),
        ];

        let checker = DefaultScopeChecker::new();
        let result = checker.validate_scope_uniqueness(&assignments, UnitKind::Policy);
        assert!(
            result.is_ok(),
            "overlapping scopes for policy should be allowed (INV-S8a), got: {result:?}"
        );
    }
}
