//! In-memory [`UnitStore`] implementation for testing.
//!
//! [`InMemoryUnitStore`] provides a simple, fast, non-persistent
//! implementation of the [`UnitStore`] trait defined in taba-core.
//! It uses [`std::sync::Mutex`] for interior mutability (since the
//! trait methods take `&self`) and is safe to use in multi-threaded
//! async tests via `#[tokio::test]`.
//!
//! At M1, the store does not check references or implement causal
//! buffering — all inserts go directly to the active map. The
//! `pending` vector exists for future use but starts empty.

use std::collections::HashMap;
use std::sync::Mutex;

use taba_common::UnitId;
use taba_core::{CoreError, Unit, UnitKind, UnitStore};

// ===========================================================================
// InMemoryUnitStore
// ===========================================================================

/// In-memory implementation of [`UnitStore`] for testing.
///
/// Stores units in a [`HashMap`] keyed by [`UnitId`]. Uses
/// [`std::sync::Mutex`] for interior mutability since the trait
/// methods take `&self`. The mutex is never held across an `await`
/// point — all operations are synchronous and the guard is dropped
/// before the async block returns, ensuring the future is `Send`.
///
/// # Example
///
/// ```
/// use taba_core::{Unit, UnitStore};
/// use taba_test_harness::{InMemoryUnitStore, WorkloadUnitBuilder};
///
/// # async fn run() {
/// let store = InMemoryUnitStore::new();
/// let unit = WorkloadUnitBuilder::new().build();
/// store.insert(Unit::Workload(unit)).await.expect("insert");
/// assert!(store.contains(&Unit::Workload(WorkloadUnitBuilder::new().build())
///     .id()).await.expect("contains"));
/// # }
/// ```
pub struct InMemoryUnitStore {
    /// Active (non-archived) units, keyed by ID.
    units: Mutex<HashMap<UnitId, Unit>>,
    /// Units awaiting causal delivery (empty at M1).
    pending: Mutex<Vec<Unit>>,
}

impl InMemoryUnitStore {
    /// Creates a new empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            units: Mutex::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
        }
    }
}

impl Default for InMemoryUnitStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UnitStore for InMemoryUnitStore {
    fn get(
        &self,
        id: &UnitId,
    ) -> impl std::future::Future<Output = Result<Unit, CoreError>> + Send {
        let units = self
            .units
            .lock()
            .expect("units mutex should not be poisoned");
        std::future::ready(
            units
                .get(id)
                .cloned()
                .ok_or(CoreError::UnitNotFound { id: *id }),
        )
    }

    fn contains(
        &self,
        id: &UnitId,
    ) -> impl std::future::Future<Output = Result<bool, CoreError>> + Send {
        let units = self
            .units
            .lock()
            .expect("units mutex should not be poisoned");
        std::future::ready(Ok(units.contains_key(id)))
    }

    fn insert(
        &self,
        unit: Unit,
    ) -> impl std::future::Future<Output = Result<(), CoreError>> + Send {
        {
            let mut units = self
                .units
                .lock()
                .expect("units mutex should not be poisoned");
            units.insert(unit.id(), unit);
        }
        std::future::ready(Ok(()))
    }

    fn archive(
        &self,
        id: &UnitId,
    ) -> impl std::future::Future<Output = Result<(), CoreError>> + Send {
        let result = {
            let mut units = self
                .units
                .lock()
                .expect("units mutex should not be poisoned");
            if units.remove(id).is_some() {
                Ok(())
            } else {
                Err(CoreError::UnitNotFound { id: *id })
            }
        };
        std::future::ready(result)
    }

    fn list_by_kind(
        &self,
        kind: UnitKind,
    ) -> impl std::future::Future<Output = Result<Vec<Unit>, CoreError>> + Send {
        let units = self
            .units
            .lock()
            .expect("units mutex should not be poisoned");
        std::future::ready(Ok(units
            .values()
            .filter(|u| u.kind() == kind)
            .cloned()
            .collect()))
    }

    fn list_pending(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<Unit>, CoreError>> + Send {
        let pending = self
            .pending
            .lock()
            .expect("pending mutex should not be poisoned");
        std::future::ready(Ok(pending.clone()))
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{DataUnitBuilder, PolicyUnitBuilder, WorkloadUnitBuilder};
    use taba_core::Unit;

    /// Helper: builds a [`Unit::Workload`] with a known ID.
    fn workload_unit() -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().build())
    }

    /// Helper: builds a [`Unit::Data`] with a known ID.
    fn data_unit() -> Unit {
        Unit::Data(DataUnitBuilder::new().build())
    }

    /// Helper: builds a [`Unit::Policy`] with a known ID.
    fn policy_unit() -> Unit {
        Unit::Policy(PolicyUnitBuilder::new().build())
    }

    #[tokio::test]
    async fn test_in_memory_store_insert_and_get() {
        let store = InMemoryUnitStore::new();
        let unit = workload_unit();
        let id = unit.id();

        store
            .insert(unit.clone())
            .await
            .expect("insert should succeed");
        let retrieved = store.get(&id).await.expect("get should succeed");

        assert_eq!(retrieved.id(), id, "retrieved unit should have same ID");
        assert_eq!(retrieved.kind(), unit.kind(), "kinds should match");
    }

    #[tokio::test]
    async fn test_in_memory_store_get_not_found() {
        let store = InMemoryUnitStore::new();
        let missing_id = UnitId(uuid::Uuid::new_v4());

        let result = store.get(&missing_id).await;
        assert!(
            matches!(result, Err(CoreError::UnitNotFound { id }) if id == missing_id),
            "get on non-existent unit should return UnitNotFound"
        );
    }

    #[tokio::test]
    async fn test_in_memory_store_contains() {
        let store = InMemoryUnitStore::new();
        let unit = workload_unit();
        let id = unit.id();

        // Before insert: not present
        assert!(
            !store.contains(&id).await.expect("contains should succeed"),
            "unit should not be present before insert"
        );

        // After insert: present
        store.insert(unit).await.expect("insert should succeed");
        assert!(
            store.contains(&id).await.expect("contains should succeed"),
            "unit should be present after insert"
        );

        // A different ID: not present
        let other_id = UnitId(uuid::Uuid::new_v4());
        assert!(
            !store
                .contains(&other_id)
                .await
                .expect("contains should succeed"),
            "different ID should not be present"
        );
    }

    #[tokio::test]
    async fn test_in_memory_store_archive() {
        let store = InMemoryUnitStore::new();
        let unit = workload_unit();
        let id = unit.id();

        store.insert(unit).await.expect("insert should succeed");
        assert!(store.contains(&id).await.expect("contains after insert"));

        // Archive the unit
        store.archive(&id).await.expect("archive should succeed");

        // After archive: not found
        assert!(
            !store.contains(&id).await.expect("contains after archive"),
            "unit should not be present after archive"
        );
        let result = store.get(&id).await;
        assert!(
            matches!(result, Err(CoreError::UnitNotFound { .. })),
            "get after archive should return UnitNotFound"
        );
    }

    #[tokio::test]
    async fn test_in_memory_store_archive_not_found() {
        let store = InMemoryUnitStore::new();
        let missing_id = UnitId(uuid::Uuid::new_v4());

        let result = store.archive(&missing_id).await;
        assert!(
            matches!(result, Err(CoreError::UnitNotFound { id }) if id == missing_id),
            "archive on non-existent unit should return UnitNotFound"
        );
    }

    #[tokio::test]
    async fn test_in_memory_store_list_by_kind() {
        let store = InMemoryUnitStore::new();

        // Insert one of each kind
        let w = workload_unit();
        let d = data_unit();
        let p = policy_unit();
        store.insert(w.clone()).await.expect("insert workload");
        store.insert(d.clone()).await.expect("insert data");
        store.insert(p.clone()).await.expect("insert policy");

        // List workloads
        let workloads = store
            .list_by_kind(UnitKind::Workload)
            .await
            .expect("list workloads");
        assert_eq!(workloads.len(), 1, "should have exactly 1 workload");
        assert_eq!(workloads[0].id(), w.id());

        // List data
        let data = store.list_by_kind(UnitKind::Data).await.expect("list data");
        assert_eq!(data.len(), 1, "should have exactly 1 data unit");
        assert_eq!(data[0].id(), d.id());

        // List policies
        let policies = store
            .list_by_kind(UnitKind::Policy)
            .await
            .expect("list policies");
        assert_eq!(policies.len(), 1, "should have exactly 1 policy");
        assert_eq!(policies[0].id(), p.id());

        // List governance (none inserted)
        let governance = store
            .list_by_kind(UnitKind::Governance)
            .await
            .expect("list governance");
        assert!(governance.is_empty(), "should have no governance units");
    }

    #[tokio::test]
    async fn test_in_memory_store_list_pending() {
        let store = InMemoryUnitStore::new();
        let pending = store.list_pending().await.expect("list pending");
        assert!(
            pending.is_empty(),
            "pending queue should be empty initially (M1)"
        );
    }

    #[tokio::test]
    async fn test_in_memory_store_multiple_units_same_kind() {
        let store = InMemoryUnitStore::new();

        let w1 = workload_unit();
        let w2 = workload_unit();
        let w3 = workload_unit();

        store.insert(w1.clone()).await.expect("insert w1");
        store.insert(w2.clone()).await.expect("insert w2");
        store.insert(w3.clone()).await.expect("insert w3");

        let workloads = store
            .list_by_kind(UnitKind::Workload)
            .await
            .expect("list workloads");
        assert_eq!(workloads.len(), 3, "should have 3 workloads");

        // Verify all three are present
        let ids: std::collections::HashSet<_> = workloads.iter().map(Unit::id).collect();
        assert!(ids.contains(&w1.id()));
        assert!(ids.contains(&w2.id()));
        assert!(ids.contains(&w3.id()));
    }

    #[tokio::test]
    async fn test_in_memory_store_insert_overwrites() {
        let store = InMemoryUnitStore::new();

        // Insert a workload
        let w = workload_unit();
        let id = w.id();
        store.insert(w).await.expect("insert");

        // Insert a data unit with the same ID (overwrites)
        let d = Unit::Data(DataUnitBuilder::new().with_id(id).build());
        store.insert(d).await.expect("insert overwrite");

        // Should now be a data unit
        let retrieved = store.get(&id).await.expect("get");
        assert_eq!(
            retrieved.kind(),
            UnitKind::Data,
            "overwritten unit should be data"
        );

        // Should have only 1 unit total
        let data = store.list_by_kind(UnitKind::Data).await.expect("list data");
        assert_eq!(data.len(), 1, "should have exactly 1 data unit");

        let workloads = store
            .list_by_kind(UnitKind::Workload)
            .await
            .expect("list workloads");
        assert!(
            workloads.is_empty(),
            "should have no workloads after overwrite"
        );
    }
}
