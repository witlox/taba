//! Read-only query operations over the composition graph.
//!
//! [`GraphQuery`] is separated from [`crate::Graph`] to allow
//! read-only access patterns — the solver only needs `GraphQuery`,
//! not mutation. All queries operate over the active set unless
//! explicitly requesting archived or pending units.
//!
//! ## Provenance traversal (INV-D1)
//!
//! [`GraphQuery::traverse_provenance`] walks the provenance chain of
//! a data unit back to its sources. Each [`ProvenanceLink`] records
//! the producing workload, input data units, and the output. The
//! traversal is recursive and cycle-safe.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use taba_common::{AuthorId, DualClockEvent, TrustDomainId, UnitId};
use taba_core::ConflictTuple;

use crate::entry::PolicyChain;
use crate::error::GraphError;

// ===========================================================================
// ProvenanceLink
// ===========================================================================

/// A single link in a data unit's provenance chain (INV-D1).
///
/// Records which workload produced which data unit, from what inputs,
/// and when. Used by [`GraphQuery::traverse_provenance`] to return the
/// full provenance path back to root data (no provenance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceLink {
    /// The workload unit that produced this data.
    pub producer: UnitId,
    /// The input data units consumed by the producing workload.
    pub inputs: Vec<UnitId>,
    /// The data unit that was produced (output of this link).
    pub output: UnitId,
    /// When this data was produced (dual clock — logical for
    /// ordering, wall time for compliance).
    pub timestamp: DualClockEvent,
}

// ===========================================================================
// GraphQuery trait
// ===========================================================================

/// Read-only query operations over the graph.
///
/// Separated from [`crate::Graph`] to allow read-only access patterns
/// (e.g., the solver only needs `GraphQuery`, not mutation). All
/// queries operate over the active set unless explicitly requesting
/// archived or pending units.
///
/// # Error handling
///
/// - [`GraphError::NotFound`]: unit is not in the graph at all.
/// - [`GraphError::Archived`]: unit was archived (soft-deleted).
/// - [`GraphError::PolicyChainError`]: policy chain is broken or
///   ambiguous (INV-C7).
pub trait GraphQuery {
    /// Retrieve a single unit by ID.
    ///
    /// Returns [`GraphError::NotFound`] if the unit is not in the
    /// active set. Returns [`GraphError::Archived`] if the unit was
    /// archived (caller can decide whether to accept archived units).
    fn get(&self, id: &UnitId) -> Result<taba_core::Unit, GraphError>;

    /// Traverse the provenance chain of a data unit (INV-D1).
    ///
    /// Returns the full provenance path: all producing workloads and
    /// their input data units, recursively, back to source data units
    /// with no provenance (root data). Used by security taint
    /// computation.
    ///
    /// Returns [`GraphError::NotFound`] if any link in the chain
    /// references a unit not in the local graph (may be pending
    /// causal delivery).
    fn traverse_provenance(&self, data_unit: &UnitId) -> Result<Vec<ProvenanceLink>, GraphError>;

    /// Get the active (non-revoked) policy for a conflict tuple
    /// (INV-C7).
    ///
    /// Returns exactly zero or one policy. If the supersession chain
    /// is broken, returns [`GraphError::PolicyChainError`].
    ///
    /// Orphaned policies (referencing non-existent conflicts) are
    /// detected here and flagged for archival (INV-C5).
    fn active_policy(
        &self,
        conflict: &ConflictTuple,
    ) -> Result<Option<taba_core::Unit>, GraphError>;

    /// Get the full supersession chain for a conflict tuple.
    ///
    /// Returns all policy versions in order (oldest to newest),
    /// including revoked ones. Used for audit trails.
    fn policy_chain(&self, conflict: &ConflictTuple) -> Result<PolicyChain, GraphError>;

    /// List all active units within a trust domain.
    fn units_in_domain(&self, domain: &TrustDomainId) -> Result<Vec<taba_core::Unit>, GraphError>;

    /// List all active units authored by a specific author.
    fn units_by_author(&self, author: &AuthorId) -> Result<Vec<taba_core::Unit>, GraphError>;

    /// Find all pending units and their missing references.
    ///
    /// Returns a list of `(unit, missing_refs)` pairs for units in
    /// the pending queue awaiting causal delivery (INV-C4).
    fn pending_units(&self) -> Result<Vec<(taba_core::Unit, Vec<UnitId>)>, GraphError>;

    /// Find all data units that are children of a given parent
    /// (INV-D3).
    ///
    /// Returns only direct children (one level). Used for validating
    /// hierarchical constraint inheritance (INV-S7).
    fn child_data_units(&self, parent: &UnitId) -> Result<Vec<taba_core::Unit>, GraphError>;
}

// ===========================================================================
// Provenance traversal helper
// ===========================================================================

/// Performs a recursive provenance traversal over a map of entries.
///
/// This is a free function that can be called by any `GraphQuery`
/// implementation that has access to a `UnitId → GraphEntry` map.
/// It is cycle-safe: visited units are tracked to prevent infinite
/// recursion.
///
/// # Errors
///
/// Returns [`GraphError::NotFound`] if any provenance reference is not
/// in the `entries` map.
pub(crate) fn traverse_provenance_impl(
    entries: &std::collections::BTreeMap<UnitId, crate::entry::GraphEntry>,
    data_unit: &UnitId,
) -> Result<Vec<ProvenanceLink>, GraphError> {
    let mut links = Vec::new();
    let mut visited = BTreeSet::new();
    traverse_provenance_recursive(entries, data_unit, &mut links, &mut visited)?;
    Ok(links)
}

/// Recursive helper for provenance traversal.
fn traverse_provenance_recursive(
    entries: &std::collections::BTreeMap<UnitId, crate::entry::GraphEntry>,
    data_unit_id: &UnitId,
    links: &mut Vec<ProvenanceLink>,
    visited: &mut BTreeSet<UnitId>,
) -> Result<(), GraphError> {
    // Prevent infinite recursion on cycles.
    if visited.contains(data_unit_id) {
        return Ok(());
    }
    visited.insert(*data_unit_id);

    // Find the data unit in the entries.
    let entry = entries
        .get(data_unit_id)
        .ok_or(GraphError::NotFound { id: *data_unit_id })?;

    // Only data units have provenance.
    let taba_core::Unit::Data(data_unit) = entry.unit() else {
        return Ok(()); // Non-data units have no provenance.
    };

    // If the data unit has no provenance, it's root data — no links.
    let Some(provenance) = &data_unit.provenance else {
        return Ok(());
    };

    // Verify the producing workload exists (INV-D1: unbroken chain).
    if !entries.contains_key(&provenance.produced_by) {
        return Err(GraphError::NotFound {
            id: provenance.produced_by,
        });
    }

    // Create a provenance link for this data unit.
    links.push(ProvenanceLink {
        producer: provenance.produced_by,
        inputs: provenance.inputs.clone(),
        output: *data_unit_id,
        timestamp: provenance.produced_at.clone(),
    });

    // Recursively traverse input data units.
    for input_id in &provenance.inputs {
        traverse_provenance_recursive(entries, input_id, links, visited)?;
    }

    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{DefaultGraph, Graph};
    use std::collections::BTreeSet;
    use taba_common::{LogicalClock, WallTime};
    use taba_core::Unit;
    use taba_test_harness::{DataUnitBuilder, PolicyUnitBuilder, WorkloadUnitBuilder};

    /// Creates a minimal [`DualClockEvent`] for testing.
    fn test_dual_clock(lc: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: WallTime { millis: lc * 1000 },
            timezone: "UTC".to_string(),
        }
    }

    /// Creates a [`Unit::Workload`] with the given ID and trust domain.
    fn workload_unit(id: UnitId, td: TrustDomainId, author: AuthorId) -> Unit {
        Unit::Workload(
            WorkloadUnitBuilder::new()
                .with_id(id)
                .with_author(author)
                .with_trust_domain(td)
                .build(),
        )
    }

    /// Creates a [`Unit::Data`] with the given ID and no provenance.
    fn root_data_unit(id: UnitId, td: TrustDomainId, author: AuthorId) -> Unit {
        Unit::Data(
            DataUnitBuilder::new()
                .with_id(id)
                .with_author(author)
                .with_trust_domain(td)
                .build(),
        )
    }

    /// Creates a [`Unit::Data`] with provenance pointing to a producer and inputs.
    fn derived_data_unit(
        id: UnitId,
        td: TrustDomainId,
        author: AuthorId,
        produced_by: UnitId,
        inputs: Vec<UnitId>,
        produced_at: DualClockEvent,
    ) -> Unit {
        Unit::Data(
            DataUnitBuilder::new()
                .with_id(id)
                .with_author(author)
                .with_trust_domain(td)
                .with_provenance(taba_core::Provenance {
                    produced_by,
                    inputs,
                    produced_at,
                    governing_policies: Vec::new(),
                })
                .build(),
        )
    }

    // I need to check if DataUnitBuilder has with_provenance and with_author
    // Let me adjust if needed after compile.

    // -- get ----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_existing_unit() {
        let graph = DefaultGraph::new(1_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        let unit = workload_unit(id, td, author);

        graph.insert(unit).await.expect("insert should succeed");

        let result = graph.get(&id);
        assert!(result.is_ok(), "get on existing unit should return Ok");
        assert_eq!(result.expect("unit").id(), id);
    }

    #[tokio::test]
    async fn test_get_nonexistent_unit() {
        let graph = DefaultGraph::new(1_000_000);
        let missing_id = UnitId(uuid::Uuid::new_v4());

        let result = graph.get(&missing_id);
        assert!(
            matches!(result, Err(GraphError::NotFound { id }) if id == missing_id),
            "get on non-existent unit should return NotFound"
        );
    }

    #[tokio::test]
    async fn test_get_archived_unit() {
        let graph = DefaultGraph::new(1_000_000);
        let id = UnitId(uuid::Uuid::new_v4());
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        let unit = workload_unit(id, td, author);

        graph.insert(unit).await.expect("insert should succeed");
        graph.archive(&id).await.expect("archive should succeed");

        let result = graph.get(&id);
        assert!(
            matches!(result, Err(GraphError::Archived { id }) if id == id),
            "get on archived unit should return Archived"
        );
    }

    // -- traverse_provenance ------------------------------------------------

    #[tokio::test]
    async fn test_traverse_provenance_simple() {
        // One level: producer (workload) → output (data)
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let producer_id = UnitId(uuid::Uuid::new_v4());
        let output_id = UnitId(uuid::Uuid::new_v4());

        graph
            .insert(workload_unit(producer_id, td, author))
            .await
            .expect("insert producer");
        graph
            .insert(derived_data_unit(
                output_id,
                td,
                author,
                producer_id,
                Vec::new(), // no inputs
                test_dual_clock(2),
            ))
            .await
            .expect("insert output data");

        let links = graph
            .traverse_provenance(&output_id)
            .expect("traverse should succeed");

        assert_eq!(links.len(), 1, "should have 1 provenance link");
        assert_eq!(links[0].producer, producer_id);
        assert_eq!(links[0].output, output_id);
        assert!(links[0].inputs.is_empty());
    }

    #[tokio::test]
    async fn test_traverse_provenance_multi_level() {
        // Two levels: producer → output → producer2 → output2
        // output2 has provenance: produced_by=producer2, inputs=[output]
        // output has provenance: produced_by=producer, inputs=[]
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let producer1_id = UnitId(uuid::Uuid::new_v4());
        let output1_id = UnitId(uuid::Uuid::new_v4());
        let producer2_id = UnitId(uuid::Uuid::new_v4());
        let output2_id = UnitId(uuid::Uuid::new_v4());

        graph
            .insert(workload_unit(producer1_id, td, author))
            .await
            .expect("insert producer1");
        graph
            .insert(workload_unit(producer2_id, td, author))
            .await
            .expect("insert producer2");

        // output1: produced by producer1, no inputs (root data)
        graph
            .insert(derived_data_unit(
                output1_id,
                td,
                author,
                producer1_id,
                Vec::new(),
                test_dual_clock(2),
            ))
            .await
            .expect("insert output1");

        // output2: produced by producer2, inputs=[output1]
        graph
            .insert(derived_data_unit(
                output2_id,
                td,
                author,
                producer2_id,
                vec![output1_id],
                test_dual_clock(4),
            ))
            .await
            .expect("insert output2");

        let links = graph
            .traverse_provenance(&output2_id)
            .expect("traverse should succeed");

        // Should have 2 links: output2 → output1
        assert_eq!(
            links.len(),
            2,
            "should have 2 provenance links (multi-level)"
        );

        // The first link should be for output2 (produced by producer2, input output1)
        assert_eq!(links[0].output, output2_id);
        assert_eq!(links[0].producer, producer2_id);
        assert_eq!(links[0].inputs, vec![output1_id]);

        // The second link should be for output1 (produced by producer1, no inputs)
        assert_eq!(links[1].output, output1_id);
        assert_eq!(links[1].producer, producer1_id);
        assert!(links[1].inputs.is_empty());
    }

    #[tokio::test]
    async fn test_traverse_provenance_root_data() {
        // Root data (no provenance) → empty links
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());
        let id = UnitId(uuid::Uuid::new_v4());

        graph
            .insert(root_data_unit(id, td, author))
            .await
            .expect("insert root data");

        let links = graph
            .traverse_provenance(&id)
            .expect("traverse should succeed");
        assert!(links.is_empty(), "root data has no provenance links");
    }

    #[tokio::test]
    async fn test_traverse_provenance_not_found() {
        let graph = DefaultGraph::new(1_000_000);
        let missing_id = UnitId(uuid::Uuid::new_v4());

        let result = graph.traverse_provenance(&missing_id);
        assert!(
            matches!(result, Err(GraphError::NotFound { id }) if id == missing_id),
            "traverse on non-existent unit should return NotFound"
        );
    }

    // -- active_policy ------------------------------------------------------

    #[tokio::test]
    async fn test_active_policy_returns_correct_one() {
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());

        // Insert a unit to serve as conflict participant
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let unit_id = unit.id();
        graph.insert(unit).await.expect("insert unit");

        let conflict = ConflictTuple {
            unit_ids: BTreeSet::from([unit_id]),
            capability_name: "storage".to_string(),
        };

        // Insert a policy that resolves the conflict
        let policy = PolicyUnitBuilder::new()
            .with_conflict(conflict.clone())
            .with_resolution(taba_core::PolicyResolution::Allow)
            .with_trust_domain(td)
            .build();
        let policy_id = policy.header.id;

        graph
            .insert(Unit::Policy(policy))
            .await
            .expect("insert policy");

        let active = graph
            .active_policy(&conflict)
            .expect("active_policy should succeed");
        assert!(active.is_some(), "should have an active policy");
        assert_eq!(active.expect("policy").id(), policy_id);
    }

    #[tokio::test]
    async fn test_active_policy_none_when_all_revoked() {
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());

        // Insert a unit to serve as conflict participant
        let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
        let unit_id = unit.id();
        graph.insert(unit).await.expect("insert unit");

        let conflict = ConflictTuple {
            unit_ids: BTreeSet::from([unit_id]),
            capability_name: "storage".to_string(),
        };

        // Insert a revoked policy
        let policy = PolicyUnitBuilder::new()
            .with_conflict(conflict.clone())
            .with_trust_domain(td)
            .with_revoked(true)
            .build();

        graph
            .insert(Unit::Policy(policy))
            .await
            .expect("insert policy");

        let active = graph
            .active_policy(&conflict)
            .expect("active_policy should succeed");
        assert!(
            active.is_none(),
            "should return None when all policies are revoked"
        );
    }

    // -- units_in_domain ----------------------------------------------------

    #[tokio::test]
    async fn test_units_in_domain() {
        let graph = DefaultGraph::new(1_000_000);
        let td_a = TrustDomainId(uuid::Uuid::new_v4());
        let td_b = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        // Insert 2 units in domain A, 1 in domain B
        graph
            .insert(workload_unit(UnitId(uuid::Uuid::new_v4()), td_a, author))
            .await
            .expect("insert 1");
        graph
            .insert(workload_unit(UnitId(uuid::Uuid::new_v4()), td_a, author))
            .await
            .expect("insert 2");
        graph
            .insert(workload_unit(UnitId(uuid::Uuid::new_v4()), td_b, author))
            .await
            .expect("insert 3");

        let units_a = graph
            .units_in_domain(&td_a)
            .expect("units_in_domain should succeed");
        assert_eq!(units_a.len(), 2, "should have 2 units in domain A");

        let units_b = graph
            .units_in_domain(&td_b)
            .expect("units_in_domain should succeed");
        assert_eq!(units_b.len(), 1, "should have 1 unit in domain B");

        // Empty domain
        let td_c = TrustDomainId(uuid::Uuid::new_v4());
        let units_c = graph
            .units_in_domain(&td_c)
            .expect("units_in_domain should succeed");
        assert!(units_c.is_empty(), "should have 0 units in domain C");
    }

    // -- units_by_author ----------------------------------------------------

    #[tokio::test]
    async fn test_units_by_author() {
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author_a = AuthorId(uuid::Uuid::new_v4());
        let author_b = AuthorId(uuid::Uuid::new_v4());

        // Insert 2 units by author A, 1 by author B
        graph
            .insert(workload_unit(UnitId(uuid::Uuid::new_v4()), td, author_a))
            .await
            .expect("insert 1");
        graph
            .insert(workload_unit(UnitId(uuid::Uuid::new_v4()), td, author_a))
            .await
            .expect("insert 2");
        graph
            .insert(workload_unit(UnitId(uuid::Uuid::new_v4()), td, author_b))
            .await
            .expect("insert 3");

        let units_a = graph
            .units_by_author(&author_a)
            .expect("units_by_author should succeed");
        assert_eq!(units_a.len(), 2, "should have 2 units by author A");

        let units_b = graph
            .units_by_author(&author_b)
            .expect("units_by_author should succeed");
        assert_eq!(units_b.len(), 1, "should have 1 unit by author B");
    }

    // -- pending_units ------------------------------------------------------

    #[tokio::test]
    async fn test_pending_units_with_missing_refs() {
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        // Insert a workload that references a missing unit via recovery_relationships
        let missing_ref = UnitId(uuid::Uuid::new_v4());
        let mut workload = WorkloadUnitBuilder::new()
            .with_trust_domain(td)
            .with_author(author)
            .build();
        workload.recovery_relationships = vec![taba_core::RecoveryRelationship {
            depends_on: missing_ref,
            action: taba_core::RecoveryAction::DrainFirst,
        }];

        graph
            .insert(Unit::Workload(workload))
            .await
            .expect("insert should succeed (goes to pending)");

        let pending = graph.pending_units().expect("pending_units should succeed");
        assert_eq!(pending.len(), 1, "should have 1 pending unit");
        assert!(
            pending[0].1.contains(&missing_ref),
            "pending unit should list missing_ref as missing"
        );
    }

    // -- child_data_units ---------------------------------------------------

    #[tokio::test]
    async fn test_child_data_units() {
        let graph = DefaultGraph::new(1_000_000);
        let td = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let parent_id = UnitId(uuid::Uuid::new_v4());
        let child1_id = UnitId(uuid::Uuid::new_v4());
        let child2_id = UnitId(uuid::Uuid::new_v4());

        // Insert parent (root data)
        graph
            .insert(root_data_unit(parent_id, td, author))
            .await
            .expect("insert parent");

        // Insert children with parent set
        graph
            .insert(Unit::Data(
                DataUnitBuilder::new()
                    .with_id(child1_id)
                    .with_author(author)
                    .with_trust_domain(td)
                    .with_parent(parent_id)
                    .build(),
            ))
            .await
            .expect("insert child1");

        graph
            .insert(Unit::Data(
                DataUnitBuilder::new()
                    .with_id(child2_id)
                    .with_author(author)
                    .with_trust_domain(td)
                    .with_parent(parent_id)
                    .build(),
            ))
            .await
            .expect("insert child2");

        // Insert a data unit with no parent (not a child)
        graph
            .insert(root_data_unit(UnitId(uuid::Uuid::new_v4()), td, author))
            .await
            .expect("insert non-child");

        let children = graph
            .child_data_units(&parent_id)
            .expect("child_data_units should succeed");
        assert_eq!(children.len(), 2, "should have 2 children");

        let child_ids: BTreeSet<UnitId> = children.iter().map(taba_core::Unit::id).collect();
        assert!(child_ids.contains(&child1_id));
        assert!(child_ids.contains(&child2_id));
    }

    // -- ProvenanceLink serialization ---------------------------------------

    #[test]
    fn test_provenance_link_serialization_roundtrip() {
        let link = ProvenanceLink {
            producer: UnitId(uuid::Uuid::new_v4()),
            inputs: vec![UnitId(uuid::Uuid::new_v4())],
            output: UnitId(uuid::Uuid::new_v4()),
            timestamp: test_dual_clock(5),
        };

        let json = serde_json::to_string(&link).expect("serialize ProvenanceLink");
        let decoded: ProvenanceLink =
            serde_json::from_str(&json).expect("deserialize ProvenanceLink");
        assert_eq!(link, decoded);
    }

    // -- traverse_provenance_impl unit tests (pure function) ----------------

    #[test]
    fn test_traverse_provenance_impl_empty_entries() {
        let entries = std::collections::BTreeMap::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let result = traverse_provenance_impl(&entries, &id);
        assert!(matches!(result, Err(GraphError::NotFound { .. })));
    }
}
