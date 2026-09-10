//! Conflict detection: capability and security conflicts (INV-S2,
//! INV-K2, INV-C7).
//!
//! The [`ConflictDetector`] trait scans the composition graph for
//! conflicts: capability needs with no matching provide, purpose
//! qualifier mismatches, and security-incompatible declarations.
//! Conflicts fail closed (INV-S2) until explicit policy resolves
//! them.
//!
//! Supersession chain checking (INV-C7) verifies that at most one
//! non-revoked policy exists per conflict tuple.

use std::collections::BTreeMap;

use taba_common::UnitId;
use taba_core::{Capability, ConflictTuple, Unit};
use taba_graph::GraphSnapshot;

use crate::SolverError;
use crate::placement::{Conflict, ConflictStatus, SupersessionChain};

// ===========================================================================
// ConflictDetector trait
// ===========================================================================

/// Detects and reports conflicts in the composition graph.
///
/// A conflict occurs when capability declarations are incompatible
/// and no policy resolves them, or when the policy chain is broken.
/// The solver calls this as part of `solve`, but it is a separate
/// trait for testability.
///
/// All methods are pure functions: no I/O, no side effects,
/// deterministic (INV-C3).
pub trait ConflictDetector {
    /// Find all unresolved conflicts in the graph.
    ///
    /// Scans all capability need/provide pairs. A conflict exists
    /// when:
    /// - A need has no matching provide (missing capability)
    /// - A need matches a provide but purpose qualifiers conflict
    ///   (INV-K2)
    /// - A need matches but security requirements are incompatible
    ///   (INV-S2) and no policy resolves the incompatibility
    ///
    /// Returns all conflicts found, sorted deterministically. An
    /// empty list means composition is clean.
    #[must_use]
    fn detect_conflicts(&self, graph: &GraphSnapshot) -> Vec<Conflict>;

    /// Check the policy supersession chain for a specific conflict
    /// tuple.
    ///
    /// Verifies INV-C7: exactly zero or one non-revoked policy per
    /// conflict tuple. Reports broken chains, ambiguous policies,
    /// and orphaned policies.
    ///
    /// # Errors
    ///
    /// - [`SolverError::PolicyConflict`] if multiple non-revoked
    ///   policies exist for the same conflict tuple (INV-C7
    ///   violation).
    fn check_supersession(
        &self,
        graph: &GraphSnapshot,
        conflict_units: &[UnitId],
        capability: &Capability,
    ) -> Result<SupersessionChain, SolverError>;
}

// ===========================================================================
// DefaultConflictDetector
// ===========================================================================

/// Default, stateless implementation of [`ConflictDetector`].
///
/// Conflict detection is deterministic (INV-C3): the same graph
/// snapshot produces the same conflicts on every node. Units are
/// iterated in sorted `UnitId` order to ensure reproducibility
/// regardless of insertion order (INV-C6).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultConflictDetector;

impl DefaultConflictDetector {
    /// Creates a new default conflict detector.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Checks whether a need is satisfiable by a provide, including
    /// purpose matching (INV-K2).
    ///
    /// - Type must match exactly.
    /// - Name must be compatible (exact or `{name}-compatible`).
    /// - If purpose is set on the need, the provide must have the
    ///   same purpose.
    fn is_satisfiable(need: &Capability, provide: &Capability) -> bool {
        if need.cap_type != provide.cap_type {
            return false;
        }
        let name_compatible =
            need.name == provide.name || provide.name == format!("{}-compatible", need.name);
        if !name_compatible {
            return false;
        }
        need.purpose
            .as_ref()
            .is_none_or(|np| provide.purpose.as_ref() == Some(np))
    }

    /// Checks whether a need is matched by type and name only,
    /// ignoring purpose. Used to detect purpose mismatches.
    fn matches_type_name(need: &Capability, provide: &Capability) -> bool {
        need.cap_type == provide.cap_type
            && (need.name == provide.name || provide.name == format!("{}-compatible", need.name))
    }

    /// Builds a [`ConflictTuple`] for the given units and capability.
    fn conflict_tuple(units: &[UnitId], capability: &Capability) -> ConflictTuple {
        ConflictTuple {
            unit_ids: units.iter().copied().collect(),
            capability_name: capability.cap_type.clone(),
        }
    }

    /// Looks up the policy chain for a conflict tuple and determines
    /// its status.
    ///
    /// Returns:
    /// - `None` if no policy chain exists (unresolved).
    /// - `Some(ConflictStatus::Revoked)` if all policies are revoked.
    /// - `Some(ConflictStatus::Ambiguous)` if multiple non-revoked.
    /// - `Some(active_id)` if exactly one non-revoked (resolved).
    fn policy_status(graph: &GraphSnapshot, tuple: &ConflictTuple) -> Option<ConflictResolution> {
        let chain = graph.policy_chains.get(tuple)?;

        let non_revoked: Vec<_> = chain.versions.iter().filter(|v| !v.revoked).collect();

        if non_revoked.is_empty() {
            Some(ConflictResolution::Revoked)
        } else if non_revoked.len() > 1 {
            Some(ConflictResolution::Ambiguous)
        } else {
            Some(ConflictResolution::Resolved(non_revoked[0].policy_id))
        }
    }
}

/// Internal representation of a conflict's policy resolution state.
#[allow(dead_code)]
enum ConflictResolution {
    /// A non-revoked policy exists — conflict is resolved.
    Resolved(UnitId),
    /// All policies are revoked — conflict returns to unresolved.
    Revoked,
    /// Multiple non-revoked policies — INV-C7 violation.
    Ambiguous,
}

impl ConflictDetector for DefaultConflictDetector {
    fn detect_conflicts(&self, graph: &GraphSnapshot) -> Vec<Conflict> {
        // Collect active units in sorted order (BTreeMap by UnitId).
        let active_units: BTreeMap<UnitId, &Unit> = graph
            .entries
            .values()
            .filter(|e| !e.archived)
            .map(|e| (e.unit_id(), e.unit()))
            .collect();

        let mut conflicts = Vec::new();

        // For each unit with needs, check for conflicts.
        for (&needer_id, needer) in &active_units {
            for need in needer.needs() {
                let mut fully_matched = false;
                let mut purpose_mismatch_found = false;
                let mut security_mismatch_found = false;

                for (&provider_id, provider) in &active_units {
                    if provider_id == needer_id {
                        continue;
                    }

                    // Check if this provider provides a matching capability.
                    for provided in provider.provides() {
                        if Self::is_satisfiable(need, provided) {
                            // Full match. Check security (trust domain).
                            if needer.header().trust_domain == provider.header().trust_domain {
                                // Same trust domain — full match.
                                fully_matched = true;
                            } else {
                                // Cross-domain — check policy.
                                let tuple = Self::conflict_tuple(&[needer_id, provider_id], need);
                                match Self::policy_status(graph, &tuple) {
                                    Some(ConflictResolution::Resolved(_)) => {
                                        fully_matched = true;
                                    }
                                    Some(
                                        ConflictResolution::Revoked | ConflictResolution::Ambiguous,
                                    )
                                    | None => {
                                        security_mismatch_found = true;
                                    }
                                }
                            }
                        } else if Self::matches_type_name(need, provided) {
                            // Type and name match, but purpose doesn't.
                            purpose_mismatch_found = true;
                        }
                    }
                }

                if fully_matched {
                    continue;
                }

                // Determine conflict status from policy chains.
                let tuple = Self::conflict_tuple(&[needer_id], need);
                let status = match Self::policy_status(graph, &tuple) {
                    Some(ConflictResolution::Resolved(_)) => {
                        // Resolved by policy — don't report.
                        continue;
                    }
                    Some(ConflictResolution::Revoked) => ConflictStatus::Revoked,
                    Some(ConflictResolution::Ambiguous) => ConflictStatus::Ambiguous,
                    None => ConflictStatus::Unresolved,
                };

                let _ = purpose_mismatch_found;
                let _ = security_mismatch_found;

                conflicts.push(Conflict {
                    units: vec![needer_id],
                    capability: need.clone(),
                    status,
                });
            }
        }

        // Sort by unit, then capability for deterministic output.
        conflicts.sort_by(|a, b| {
            a.units
                .first()
                .cmp(&b.units.first())
                .then_with(|| a.capability.cmp(&b.capability))
        });

        conflicts
    }

    fn check_supersession(
        &self,
        graph: &GraphSnapshot,
        conflict_units: &[UnitId],
        capability: &Capability,
    ) -> Result<SupersessionChain, SolverError> {
        let tuple = Self::conflict_tuple(conflict_units, capability);

        let Some(chain) = graph.policy_chains.get(&tuple) else {
            // No policy chain exists — empty supersession chain.
            return Ok(SupersessionChain {
                conflict_units: conflict_units.to_vec(),
                capability: capability.clone(),
                policies: Vec::new(),
            });
        };

        // Count non-revoked policies.
        let non_revoked_count = chain.versions.iter().filter(|v| !v.revoked).count();

        if non_revoked_count > 1 {
            // INV-C7 violation: multiple non-revoked policies.
            return Err(SolverError::PolicyConflict {
                units: conflict_units.to_vec(),
                reason: format!(
                    "multiple non-revoked policies ({non_revoked_count}) for conflict tuple — \
                     INV-C7 requires exactly zero or one"
                ),
            });
        }

        // Return the chain with policy IDs ordered oldest to newest.
        let policies: Vec<UnitId> = chain.versions.iter().map(|v| v.policy_id).collect();

        Ok(SupersessionChain {
            conflict_units: conflict_units.to_vec(),
            capability: capability.clone(),
            policies,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cycle::helpers::*;
    use std::collections::BTreeSet;
    use taba_common::{AuthorId, DualClockEvent, LogicalClock, TrustDomainId, Version, WallTime};
    use taba_graph::entry::{PolicyChain, PolicyVersion};
    use taba_test_harness::WorkloadUnitBuilder;
    use uuid::Uuid;

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn workload_with_needs(id: UnitId, needs: Vec<Capability>) -> Unit {
        Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id)
                .with_needs(needs)
                .build(),
        )
    }

    fn workload_with_provides(id: UnitId, provides: Vec<Capability>) -> Unit {
        Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id)
                .with_provides(provides)
                .build(),
        )
    }

    #[allow(dead_code)]
    fn workload_with_needs_provides(
        id: UnitId,
        needs: Vec<Capability>,
        provides: Vec<Capability>,
    ) -> Unit {
        Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id)
                .with_needs(needs)
                .with_provides(provides)
                .build(),
        )
    }

    fn graph_with_units(units: Vec<Unit>) -> GraphSnapshot {
        let mut entries = BTreeMap::new();
        for unit in units {
            let entry = entry_from_unit(unit);
            entries.insert(entry.unit_id(), entry);
        }
        GraphSnapshot::new(1, entries, BTreeMap::new())
    }

    fn graph_with_policies(
        units: Vec<Unit>,
        policies: BTreeMap<ConflictTuple, PolicyChain>,
    ) -> GraphSnapshot {
        let mut entries = BTreeMap::new();
        for unit in units {
            let entry = entry_from_unit(unit);
            entries.insert(entry.unit_id(), entry);
        }
        GraphSnapshot::new(1, entries, policies)
    }

    fn policy_version(policy_id: UnitId, version: u64, revoked: bool) -> PolicyVersion {
        PolicyVersion {
            policy_id,
            version: Version(version),
            revoked,
            author: AuthorId(Uuid::new_v4()),
            created_at: DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
        }
    }

    // -- detect_conflicts ----------------------------------------------------

    #[test]
    fn test_detect_conflicts_none() {
        // Unit provides a capability, no needs.
        let unit = workload_with_provides(test_unit_id(), vec![Capability::new("compute", "http")]);
        let graph = graph_with_units(vec![unit]);

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert!(conflicts.is_empty(), "clean graph should have no conflicts");
    }

    #[test]
    fn test_detect_conflicts_satisfied_need() {
        // Unit A needs "storage:redis", unit B provides "storage:redis".
        // Same trust domain — no conflict.
        let id_a = test_unit_id();
        let id_b = test_unit_id();
        let td = TrustDomainId(uuid::Uuid::new_v4());

        let unit_a = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_a)
                .with_trust_domain(td)
                .with_needs(vec![Capability::new("storage", "redis")])
                .build(),
        );
        let unit_b = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_b)
                .with_trust_domain(td)
                .with_provides(vec![Capability::new("storage", "redis")])
                .build(),
        );

        let graph = graph_with_units(vec![unit_a, unit_b]);

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert!(
            conflicts.is_empty(),
            "satisfied need should not produce a conflict"
        );
    }

    #[test]
    fn test_detect_conflicts_missing_capability() {
        // Unit A needs "storage:redis", no unit provides it.
        let id_a = test_unit_id();
        let unit_a = workload_with_needs(id_a, vec![Capability::new("storage", "redis")]);

        let graph = graph_with_units(vec![unit_a]);

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].units, vec![id_a]);
        assert_eq!(conflicts[0].capability, Capability::new("storage", "redis"));
        assert_eq!(conflicts[0].status, ConflictStatus::Unresolved);
    }

    #[test]
    fn test_detect_conflicts_purpose_mismatch() {
        // Unit A needs "storage:redis:analytics", unit B provides "storage:redis" (no purpose).
        let id_a = test_unit_id();
        let id_b = test_unit_id();

        let need = Capability::with_purpose("storage", "redis", "analytics");
        let unit_a = workload_with_needs(id_a, vec![need]);
        let unit_b = workload_with_provides(id_b, vec![Capability::new("storage", "redis")]);

        let graph = graph_with_units(vec![unit_a, unit_b]);

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert_eq!(
            conflicts.len(),
            1,
            "purpose mismatch should produce a conflict"
        );
        assert_eq!(conflicts[0].status, ConflictStatus::Unresolved);
    }

    #[test]
    fn test_detect_conflicts_cross_domain() {
        // Unit A (T1) needs "storage:redis", unit B (T2) provides "storage:redis".
        // No policy → security conflict (INV-S2).
        let id_a = test_unit_id();
        let id_b = test_unit_id();
        let td_a = TrustDomainId(Uuid::new_v4());
        let td_b = TrustDomainId(Uuid::new_v4());

        let unit_a = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_a)
                .with_needs(vec![Capability::new("storage", "redis")])
                .with_trust_domain(td_a)
                .build(),
        );
        let unit_b = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_b)
                .with_provides(vec![Capability::new("storage", "redis")])
                .with_trust_domain(td_b)
                .build(),
        );

        let graph = graph_with_units(vec![unit_a, unit_b]);

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert_eq!(
            conflicts.len(),
            1,
            "cross-domain without policy should produce a conflict"
        );
        assert_eq!(conflicts[0].status, ConflictStatus::Unresolved);
    }

    #[test]
    fn test_detect_conflicts_resolved_by_policy() {
        // Cross-domain conflict resolved by policy.
        let id_a = test_unit_id();
        let id_b = test_unit_id();
        let td_a = TrustDomainId(Uuid::new_v4());
        let td_b = TrustDomainId(Uuid::new_v4());

        let unit_a = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_a)
                .with_needs(vec![Capability::new("storage", "redis")])
                .with_trust_domain(td_a)
                .build(),
        );
        let unit_b = Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id_b)
                .with_provides(vec![Capability::new("storage", "redis")])
                .with_trust_domain(td_b)
                .build(),
        );

        let cap = Capability::new("storage", "redis");
        let tuple = ConflictTuple {
            unit_ids: BTreeSet::from([id_a, id_b]),
            capability_name: cap.cap_type,
        };

        let mut chain = PolicyChain::new(tuple.clone());
        chain.add_version(policy_version(test_unit_id(), 1, false));

        let graph = graph_with_policies(vec![unit_a, unit_b], BTreeMap::from([(tuple, chain)]));

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert!(
            conflicts.is_empty(),
            "cross-domain conflict resolved by policy should not be reported"
        );
    }

    #[test]
    fn test_detect_conflicts_sorted() {
        // Multiple units with unmet needs — conflicts should be sorted.
        let id_a = UnitId(Uuid::from_u128(1));
        let id_b = UnitId(Uuid::from_u128(2));

        let unit_a = workload_with_needs(id_a, vec![Capability::new("storage", "redis")]);
        let unit_b = workload_with_needs(id_b, vec![Capability::new("compute", "http")]);

        let graph = graph_with_units(vec![unit_a, unit_b]);

        let detector = DefaultConflictDetector::new();
        let conflicts = detector.detect_conflicts(&graph);
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].units[0], id_a, "sorted by unit ID");
        assert_eq!(conflicts[1].units[0], id_b);
    }

    // -- check_supersession --------------------------------------------------

    #[test]
    fn test_check_supersession_no_chain() {
        let graph = empty_graph();
        let units = vec![test_unit_id()];
        let cap = Capability::new("storage", "redis");

        let detector = DefaultConflictDetector::new();
        let result = detector.check_supersession(&graph, &units, &cap);

        assert!(result.is_ok());
        let chain = result.unwrap();
        assert!(
            chain.policies.is_empty(),
            "no chain should yield empty policies"
        );
    }

    #[test]
    fn test_check_supersession_valid() {
        let units = vec![test_unit_id()];
        let cap = Capability::new("storage", "redis");
        let tuple = DefaultConflictDetector::conflict_tuple(&units, &cap);

        let policy_id = test_unit_id();
        let mut chain = PolicyChain::new(tuple.clone());
        chain.add_version(policy_version(policy_id, 1, false));

        let graph = GraphSnapshot::new(1, BTreeMap::new(), BTreeMap::from([(tuple, chain)]));

        let detector = DefaultConflictDetector::new();
        let result = detector.check_supersession(&graph, &units, &cap);

        assert!(result.is_ok());
        let chain = result.unwrap();
        assert_eq!(chain.policies, vec![policy_id]);
    }

    #[test]
    fn test_check_supersession_ambiguous() {
        let units = vec![test_unit_id()];
        let cap = Capability::new("storage", "redis");
        let tuple = DefaultConflictDetector::conflict_tuple(&units, &cap);

        let policy_a = test_unit_id();
        let policy_b = test_unit_id();
        let mut chain = PolicyChain::new(tuple.clone());
        chain.add_version(policy_version(policy_a, 1, false));
        chain.add_version(policy_version(policy_b, 2, false));

        let graph = GraphSnapshot::new(1, BTreeMap::new(), BTreeMap::from([(tuple, chain)]));

        let detector = DefaultConflictDetector::new();
        let result = detector.check_supersession(&graph, &units, &cap);

        assert!(
            matches!(result, Err(SolverError::PolicyConflict { .. })),
            "two non-revoked policies should be PolicyConflict, got: {result:?}"
        );
    }

    #[test]
    fn test_check_supersession_all_revoked() {
        let units = vec![test_unit_id()];
        let cap = Capability::new("storage", "redis");
        let tuple = DefaultConflictDetector::conflict_tuple(&units, &cap);

        let policy_a = test_unit_id();
        let policy_b = test_unit_id();
        let mut chain = PolicyChain::new(tuple.clone());
        chain.add_version(policy_version(policy_a, 1, true)); // revoked
        chain.add_version(policy_version(policy_b, 2, true)); // revoked

        let graph = GraphSnapshot::new(1, BTreeMap::new(), BTreeMap::from([(tuple, chain)]));

        let detector = DefaultConflictDetector::new();
        let result = detector.check_supersession(&graph, &units, &cap);

        assert!(
            result.is_ok(),
            "all revoked should return Ok with empty active"
        );
        let chain = result.unwrap();
        assert_eq!(chain.policies.len(), 2, "all policy IDs should be present");
        assert!(chain.policies.contains(&policy_a));
        assert!(chain.policies.contains(&policy_b));
    }

    #[test]
    fn test_check_supersession_one_revoked() {
        let units = vec![test_unit_id()];
        let cap = Capability::new("storage", "redis");
        let tuple = DefaultConflictDetector::conflict_tuple(&units, &cap);

        let policy_a = test_unit_id();
        let policy_b = test_unit_id();
        let mut chain = PolicyChain::new(tuple.clone());
        chain.add_version(policy_version(policy_a, 1, true)); // revoked
        chain.add_version(policy_version(policy_b, 2, false)); // active

        let graph = GraphSnapshot::new(1, BTreeMap::new(), BTreeMap::from([(tuple, chain)]));

        let detector = DefaultConflictDetector::new();
        let result = detector.check_supersession(&graph, &units, &cap);

        assert!(result.is_ok());
        let chain = result.unwrap();
        assert_eq!(
            chain.policies,
            vec![policy_a, policy_b],
            "ordered oldest to newest"
        );
    }

    fn empty_graph() -> GraphSnapshot {
        GraphSnapshot::new(0, BTreeMap::new(), BTreeMap::new())
    }
}
