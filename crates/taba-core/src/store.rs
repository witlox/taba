//! Abstract unit storage and graph/membership snapshot placeholders.
//!
//! [`UnitStore`] is an abstract CRUD interface for units, decoupling
//! core domain logic from the CRDT graph implementation (taba-graph)
//! and in-memory test fakes (taba-test-harness).
//!
//! [`GraphSnapshot`] and [`MembershipSnapshot`] are opaque types
//! defined here for DAG cleanliness — their concrete implementations
//! live in taba-graph and taba-gossip respectively.

use serde::{Deserialize, Serialize};

use taba_common::UnitId;

use crate::unit::{Unit, UnitKind};
use crate::validation::CoreError;

// ===========================================================================
// Placeholder types (implemented by other crates)
// ===========================================================================

/// Immutable point-in-time view of the composition graph.
///
/// Defined in taba-core for DAG cleanliness, **implemented by
/// taba-graph**. Consumed by taba-solver (wrapped in `Arc` for safe
/// concurrent access — the snapshot is frozen at creation and never
/// mutated). This struct is an opaque placeholder; the real type is
/// provided when taba-graph is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSnapshot {
    /// Opaque blob — replaced by real graph state in taba-graph.
    ///
    /// Serialization is provided so the placeholder roundtrips
    /// cleanly in tests that exercise the abstract trait surface.
    #[doc(hidden)]
    pub opaque: Vec<u8>,
}

/// Current cluster membership view.
///
/// Defined in taba-core for DAG cleanliness, **implemented by
/// taba-gossip**. Consumed by taba-solver for placement decisions.
/// This struct is an opaque placeholder; the real type is provided
/// when taba-gossip is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipSnapshot {
    /// Opaque blob — replaced by real membership state in taba-gossip.
    ///
    /// Serialization is provided so the placeholder roundtrips
    /// cleanly in tests that exercise the abstract trait surface.
    #[doc(hidden)]
    pub opaque: Vec<u8>,
}

// ===========================================================================
// UnitStore trait
// ===========================================================================

/// Abstract CRUD storage for units.
///
/// This trait decouples core unit logic from the CRDT graph
/// implementation. The concrete implementation lives in **taba-graph**
/// (persistent, distributed). In tests, this can be backed by an
/// in-memory map (taba-test-harness).
///
/// All mutating operations are async because the backing store may
/// involve WAL writes (INV-C4) or network I/O. Implementors should
/// ensure returned futures are `Send` when used in a multi-threaded
/// tokio runtime.
///
/// # Caller responsibilities
///
/// The caller MUST have passed [`UnitValidator::validate`](crate::UnitValidator::validate)
/// and `security::Verifier::verify` before calling [`insert`](Self::insert).
/// This method does not re-validate — it trusts the caller.
pub trait UnitStore {
    /// Retrieve a unit by its ID.
    ///
    /// Returns [`CoreError::UnitNotFound`] if no unit with that ID
    /// exists in the store. Does NOT return archived or compacted
    /// units.
    fn get(&self, id: &UnitId)
    -> impl std::future::Future<Output = Result<Unit, CoreError>> + Send;

    /// Check whether a unit exists in the store (including pending state).
    fn contains(
        &self,
        id: &UnitId,
    ) -> impl std::future::Future<Output = Result<bool, CoreError>> + Send;

    /// Insert a unit into the store.
    ///
    /// The unit MUST have passed validation and signature verification
    /// before calling this method. If the unit has unsatisfied
    /// references, it enters the pending queue (causal buffering per
    /// INV-C4). Otherwise it is immediately active.
    ///
    /// Returns [`CoreError::StoreError`] on persistence failure.
    fn insert(&self, unit: Unit)
    -> impl std::future::Future<Output = Result<(), CoreError>> + Send;

    /// Remove a unit from the active store (mark for archival).
    ///
    /// The unit is not physically deleted — it transitions to
    /// archived state. Archived units are eligible for compaction.
    /// Returns [`CoreError::UnitNotFound`] if the unit does not exist.
    fn archive(
        &self,
        id: &UnitId,
    ) -> impl std::future::Future<Output = Result<(), CoreError>> + Send;

    /// List all active (non-archived, non-pending) units of a given kind.
    fn list_by_kind(
        &self,
        kind: UnitKind,
    ) -> impl std::future::Future<Output = Result<Vec<Unit>, CoreError>> + Send;

    /// List all units currently in the pending queue awaiting causal delivery.
    fn list_pending(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<Unit>, CoreError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_snapshot_serialization_roundtrip() {
        let snapshot = GraphSnapshot {
            opaque: vec![0u8, 1u8, 2u8, 3u8],
        };

        let json = serde_json::to_string(&snapshot).expect("serialize GraphSnapshot");
        let decoded: GraphSnapshot =
            serde_json::from_str(&json).expect("deserialize GraphSnapshot");
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn test_membership_snapshot_serialization_roundtrip() {
        let snapshot = MembershipSnapshot {
            opaque: vec![0u8, 1u8, 2u8, 3u8],
        };

        let json = serde_json::to_string(&snapshot).expect("serialize MembershipSnapshot");
        let decoded: MembershipSnapshot =
            serde_json::from_str(&json).expect("deserialize MembershipSnapshot");
        assert_eq!(snapshot, decoded);
    }
}
