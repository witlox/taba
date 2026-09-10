//! CRDT merge semantics: how conflicting states are resolved.
//!
//! Any implementation MUST satisfy INV-C2:
//!
//! - **Commutative**: `merge(A, B) == merge(B, A)`
//! - **Associative**: `merge(merge(A, B), C) == merge(A, merge(B, C))`
//! - **Idempotent**: `merge(A, A) == A`
//!
//! These properties are load-bearing for partition tolerance (FM-03).
//! Property-based tests in [`crate::crdt`] verify all three at the
//! `CompositionGraphData` level.
//!
//! In M2, signature verification is a conceptual gate but actual
//! crypto is deferred — [`DefaultMergePolicy`] performs structural
//! validation only.

use taba_core::{DefaultValidator, Unit, UnitValidator};

use crate::crdt::GraphDelta;
use crate::entry::GraphEntry;
use crate::error::GraphError;
use crate::snapshot::GraphSnapshot;

// ===========================================================================
// MergePolicy trait
// ===========================================================================

/// CRDT merge semantics. Defines how conflicting states are resolved.
///
/// Any implementation MUST satisfy INV-C2:
/// - Commutative: `merge(A, B) == merge(B, A)`
/// - Associative: `merge(merge(A, B), C) == merge(A, merge(B, C))`
/// - Idempotent: `merge(A, A) == A`
///
/// The unit identity tuple is `(UnitId, Author, CreationTimestamp)`.
/// Units with the same identity are deduplicated (idempotent). Units
/// with different identities are unioned (commutative). Ordering does
/// not matter (associative).
///
/// Signature verification is a synchronous gate: every unit in both
/// inputs must be verified before merge proceeds. In M2, this is
/// structural validation only — actual crypto is deferred.
pub trait MergePolicy {
    /// Merge two graph states, producing a new graph delta.
    ///
    /// Given a local snapshot and a remote delta, computes the set of
    /// entries in `remote` that are not yet in `local`. This is the
    /// δ-state CRDT (DL-012): only changes are shipped.
    ///
    /// Each entry in `remote` is structurally validated. Entries that
    /// fail validation are excluded from the result without affecting
    /// the remaining valid entries. Entries already present in `local`
    /// (same [`taba_common::UnitId`]) are treated as duplicates
    /// (idempotent) and excluded.
    ///
    /// # Errors
    ///
    /// Returns `Err(GraphError::MergeConflict)` only for true
    /// violations (e.g., two different units claim the same
    /// [`taba_common::UnitId`] — Byzantine). Normal capability
    /// conflicts are NOT merge conflicts; they are surfaced by the
    /// solver.
    fn merge(&self, local: &GraphSnapshot, remote: &GraphDelta) -> Result<GraphDelta, GraphError>;

    /// Check whether two graph states are equivalent after merge.
    ///
    /// Used in property tests to verify CRDT laws. Two states are
    /// equivalent if they contain the same set of unit identity tuples
    /// (same set of [`taba_common::UnitId`] keys).
    fn is_equivalent(&self, a: &GraphSnapshot, b: &GraphSnapshot) -> bool;
}

// ===========================================================================
// DefaultMergePolicy
// ===========================================================================

/// Default implementation of [`MergePolicy`].
///
/// Performs structural validation of units using [`DefaultValidator`]
/// from taba-core. In M2, actual cryptographic signature verification
/// is deferred — the validator checks well-formedness only (INV-S3
/// is a conceptual gate).
///
/// The merge is deterministic: the same inputs always produce the
/// same output. This is guaranteed because:
///
/// 1. [`BTreeMap`](std::collections::BTreeMap) iteration is ordered.
/// 2. Deduplication is by [`taba_common::UnitId`] (content-addressed).
/// 3. No randomness or floating-point in the merge path.
#[derive(Debug, Clone)]
pub struct DefaultMergePolicy {
    /// Structural validator for units.
    validator: DefaultValidator,
}

impl DefaultMergePolicy {
    /// Creates a new `DefaultMergePolicy` with the given validator.
    ///
    /// The validator is used for structural well-formedness checks on
    /// units in the remote delta. Use [`DefaultValidator::empty`] for
    /// structural-only validation (no author scope checks).
    #[must_use]
    pub const fn new(validator: DefaultValidator) -> Self {
        Self { validator }
    }

    /// Creates a new `DefaultMergePolicy` with an empty validator
    /// (structural validation only, no author scope checks).
    #[must_use]
    pub fn structural() -> Self {
        Self::new(DefaultValidator::empty())
    }

    /// Structurally validates a single [`GraphEntry`].
    ///
    /// In M2, this is the verification gate: it checks that the
    /// wrapped [`Unit`] passes [`DefaultValidator::validate`]. Actual
    /// cryptographic signature verification is deferred to M6.
    ///
    /// Returns `Ok(())` if the entry is valid, or
    /// `Err(GraphError::SignatureRejected)` if validation fails.
    fn validate_entry(&self, entry: &GraphEntry) -> Result<(), GraphError> {
        let unit: &Unit = entry.unit();
        self.validator
            .validate(unit)
            .map_err(|e| GraphError::SignatureRejected {
                unit: unit.id(),
                reason: e.to_string(),
            })
    }
}

impl Default for DefaultMergePolicy {
    fn default() -> Self {
        Self::structural()
    }
}

impl MergePolicy for DefaultMergePolicy {
    fn merge(&self, local: &GraphSnapshot, remote: &GraphDelta) -> Result<GraphDelta, GraphError> {
        let mut result = GraphDelta::new();

        for (id, entry) in &remote.entries {
            // Skip entries already present in local (idempotent).
            if local.entries.contains_key(id) {
                continue;
            }

            // Skip entries that are archived in local — they are
            // remove-set members and should not be re-added.
            if local.entries.get(id).is_some_and(|e| e.archived) {
                continue;
            }

            // Validate the entry (M2: structural only).
            if self.validate_entry(entry).is_err() {
                // Invalid entries are excluded from the result without
                // affecting the remaining valid entries.
                continue;
            }

            // Valid, new entry — include in the result delta.
            result.add_entry(entry.clone());
        }

        // Merge policy chains: union by conflict tuple.
        for (conflict, chain) in &remote.policy_chains {
            if !local.policy_chains.contains_key(conflict) {
                result.add_policy_chain(conflict.clone(), chain.clone());
            }
        }

        Ok(result)
    }

    fn is_equivalent(&self, a: &GraphSnapshot, b: &GraphSnapshot) -> bool {
        a.entry_ids() == b.entry_ids()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use taba_common::{
        ClusterId, DualClockEvent, LogicalClock, TrustDomainId, ValidityWindow, WallTime,
    };
    use taba_core::Unit;
    use taba_security::{PublicKey, Signature, SignatureContext};
    use taba_test_harness::WorkloadUnitBuilder;

    /// Creates a minimal [`DualClockEvent`] for testing.
    fn test_dual_clock() -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1000 },
            timezone: "UTC".to_string(),
        }
    }

    /// Wraps a [`Unit`] in a [`SignedUnit`] with placeholder crypto.
    fn wrap_signed(unit: Unit) -> taba_security::SignedUnit<Unit> {
        taba_security::SignedUnit {
            unit,
            signature: Signature([0u8; 64]),
            context: SignatureContext {
                trust_domain_id: TrustDomainId(uuid::Uuid::nil()),
                cluster_id: ClusterId(uuid::Uuid::nil()),
                validity_window: ValidityWindow {
                    lc_range: None,
                    wall_time_deadline: None,
                },
            },
            signer: PublicKey([0u8; 32]),
        }
    }

    /// Creates a valid [`GraphEntry`].
    fn valid_entry() -> GraphEntry {
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new())
    }

    /// Creates an INVALID [`GraphEntry`] (empty provides fails validation).
    fn invalid_entry() -> GraphEntry {
        let mut workload = WorkloadUnitBuilder::new().build();
        workload.provides = Vec::new(); // Invalid: empty provides
        let unit = Unit::Workload(workload);
        let signed = wrap_signed(unit);
        GraphEntry::from_signed_unit(signed, test_dual_clock(), BTreeSet::new())
    }

    /// Creates a [`GraphSnapshot`] from a set of entries.
    fn snapshot_with(entries: Vec<GraphEntry>) -> GraphSnapshot {
        let map = entries.into_iter().map(|e| (e.unit_id(), e)).collect();
        GraphSnapshot::new(0, map, std::collections::BTreeMap::new())
    }

    // -- Merge ---------------------------------------------------------------

    #[test]
    fn test_merge_no_conflicts() {
        let policy = DefaultMergePolicy::default();

        let entry_a = valid_entry();
        let entry_b = valid_entry();
        let id_a = entry_a.unit_id();
        let id_b = entry_b.unit_id();

        let local = snapshot_with(vec![entry_a]);
        let remote = GraphDelta::from_entry(entry_b);

        let result = policy.merge(&local, &remote).expect("merge should succeed");

        assert_eq!(result.len(), 1, "should return 1 new entry");
        assert!(result.entries.contains_key(&id_b));
        assert!(
            !result.entries.contains_key(&id_a),
            "existing entry should not be in result"
        );
    }

    #[test]
    fn test_merge_already_present_is_noop() {
        let policy = DefaultMergePolicy::default();

        let entry = valid_entry();
        let id = entry.unit_id();

        let local = snapshot_with(vec![entry.clone()]);
        let remote = GraphDelta::from_entry(entry);

        let result = policy.merge(&local, &remote).expect("merge should succeed");

        assert!(
            result.is_empty(),
            "merging already-present entry should be no-op"
        );
        assert!(!result.entries.contains_key(&id));
    }

    #[test]
    fn test_merge_rejects_invalid_entries() {
        let policy = DefaultMergePolicy::default();

        let valid = valid_entry();
        let invalid = invalid_entry();
        let valid_id = valid.unit_id();
        let invalid_id = invalid.unit_id();

        let local = GraphSnapshot::new(
            0,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
        );

        let mut remote = GraphDelta::new();
        remote.add_entry(valid);
        remote.add_entry(invalid);

        let result = policy.merge(&local, &remote).expect("merge should succeed");

        // Valid entry should be in the result
        assert!(
            result.entries.contains_key(&valid_id),
            "valid entry should be included in result"
        );
        // Invalid entry should NOT be in the result
        assert!(
            !result.entries.contains_key(&invalid_id),
            "invalid entry should be excluded from result"
        );
        assert_eq!(result.len(), 1, "only valid entry should be in result");
    }

    #[test]
    fn test_merge_multiple_valid_entries() {
        let policy = DefaultMergePolicy::default();

        let entries: Vec<GraphEntry> = (0..5).map(|_| valid_entry()).collect();
        let ids: Vec<_> = entries
            .iter()
            .map(super::super::entry::GraphEntry::unit_id)
            .collect();

        let local = GraphSnapshot::new(
            0,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
        );

        let mut remote = GraphDelta::new();
        for entry in entries {
            remote.add_entry(entry);
        }

        let result = policy.merge(&local, &remote).expect("merge should succeed");

        assert_eq!(result.len(), 5);
        for id in &ids {
            assert!(
                result.entries.contains_key(id),
                "entry {id:?} should be in result"
            );
        }
    }

    #[test]
    fn test_merge_archived_entry_excluded() {
        let policy = DefaultMergePolicy::default();

        let mut entry = valid_entry();
        entry.archived = true;
        let id = entry.unit_id();

        let local = snapshot_with(vec![entry.clone()]);
        let remote = GraphDelta::from_entry(entry);

        let result = policy.merge(&local, &remote).expect("merge should succeed");

        assert!(
            result.is_empty(),
            "archived entry already in local should not be re-added"
        );
        let _ = id;
    }

    // -- is_equivalent -------------------------------------------------------

    #[test]
    fn test_is_equivalent_same_entries() {
        let policy = DefaultMergePolicy::default();

        let entry = valid_entry();
        let id = entry.unit_id();

        let snapshot_a = snapshot_with(vec![entry.clone()]);
        let snapshot_b = snapshot_with(vec![entry]);

        assert!(
            policy.is_equivalent(&snapshot_a, &snapshot_b),
            "snapshots with same entries should be equivalent"
        );
        let _ = id;
    }

    #[test]
    fn test_is_equivalent_different_entries() {
        let policy = DefaultMergePolicy::default();

        let entry_a = valid_entry();
        let entry_b = valid_entry();

        let snapshot_a = snapshot_with(vec![entry_a]);
        let snapshot_b = snapshot_with(vec![entry_b]);

        assert!(
            !policy.is_equivalent(&snapshot_a, &snapshot_b),
            "snapshots with different entries should NOT be equivalent"
        );
    }

    #[test]
    fn test_is_equivalent_empty_snapshots() {
        let policy = DefaultMergePolicy::default();

        let snapshot_a = GraphSnapshot::new(
            0,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
        );
        let snapshot_b = GraphSnapshot::new(
            5,
            std::collections::BTreeMap::new(),
            std::collections::BTreeMap::new(),
        );

        assert!(
            policy.is_equivalent(&snapshot_a, &snapshot_b),
            "two empty snapshots should be equivalent (generation doesn't matter)"
        );
    }

    #[test]
    fn test_is_equivalent_same_ids_different_generation() {
        let policy = DefaultMergePolicy::default();

        let entry = valid_entry();
        let id = entry.unit_id();

        let snapshot_a = GraphSnapshot::new(
            1,
            {
                let mut m = std::collections::BTreeMap::new();
                m.insert(id, entry.clone());
                m
            },
            std::collections::BTreeMap::new(),
        );
        let snapshot_b = GraphSnapshot::new(
            99,
            {
                let mut m = std::collections::BTreeMap::new();
                m.insert(id, entry);
                m
            },
            std::collections::BTreeMap::new(),
        );

        assert!(
            policy.is_equivalent(&snapshot_a, &snapshot_b),
            "snapshots with same entry IDs should be equivalent regardless of generation"
        );
    }
}
