//! Decision trail recording, storage, and query.
//!
//! Every solver run produces a [`DecisionTrail`] — a queryable record
//! of the solver's inputs (graph snapshot, membership) and outputs
//! (placements, conflicts) — enabling the "why did this happen?"
//! question (INV-O1). Trails are retained since-last-compaction by
//! default (INV-O2).
//!
//! ## Timestamp resolution
//!
//! The spec data-models reference `Timestamp` which does not exist in
//! taba-common. Per M1 convention (A002), trail timestamps use
//! [`DualClockEvent`], consistent with M1+M2.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, LogicalClock, NodeId, Ppm, UnitId, WallTime};
use taba_solver::{Conflict, MembershipSnapshot, Placement, SolverResult};

use crate::error::ObserveError;

// ===========================================================================
// DecisionTrailId
// ===========================================================================

/// Unique identifier for a decision trail.
///
/// Auto-incremented by the [`DecisionTrailRecorder`]. Stable across
/// the lifetime of the in-memory store (M3). In future milestones,
/// trail IDs will be persisted to the WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DecisionTrailId(pub u64);

// ===========================================================================
// DecisionTrail and related types
// ===========================================================================

/// Queryable record of a solver run's inputs and outputs.
///
/// Every solver run produces one of these (INV-O1). Enables solver
/// replay: the deterministic solver produces the exact same result
/// given the same inputs (INV-C3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionTrail {
    /// Unique identifier for this trail.
    pub trail_id: DecisionTrailId,
    /// Graph snapshot used as input.
    pub graph_snapshot_id: String,
    /// Node membership snapshot used as input.
    pub node_membership: Vec<NodeId>,
    /// Resource snapshots used for ranking.
    pub resource_snapshots: Vec<ResourceSnapshotRef>,
    /// Solver version that produced this decision.
    pub solver_version: String,
    /// Placement decisions produced.
    pub placements: Vec<PlacementRecord>,
    /// Conflicts detected.
    pub conflicts: Vec<ConflictRecord>,
    /// When this solver run occurred.
    pub timestamp: DualClockEvent,
}

/// Reference to a resource snapshot used in a solver run.
///
/// Each entry links a [`NodeId`] to the logical clock at which its
/// resource snapshot was taken, allowing replay to reconstruct the
/// resource state the solver observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSnapshotRef {
    /// The node whose resources were snapshotted.
    pub node_id: NodeId,
    /// Logical clock of the resource snapshot.
    pub snapshot_lc: LogicalClock,
}

/// A placement decision recorded in the trail.
///
/// Captures not just *where* a unit was placed, but *why*: the
/// capability filter results and resource scores that led to the
/// decision. This supports audit and debugging (INV-O1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementRecord {
    /// The unit that was placed.
    pub unit_id: UnitId,
    /// The node this unit was placed on.
    pub placed_on: NodeId,
    /// Why this node was chosen (capability match + resource rank).
    pub rationale: String,
    /// Capability filter results: which nodes passed, which didn't.
    pub capability_filter: Vec<(NodeId, bool)>,
    /// Resource ranking scores for eligible nodes.
    pub resource_scores: Vec<(NodeId, Ppm)>,
}

/// A conflict detected during the solver run.
///
/// Conflicts prevent composition until a policy unit resolves them
/// (INV-S2). Recording conflicts in the trail allows operators to
/// audit conflict history and verify resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictRecord {
    /// Type of the conflict (e.g., capability incompatibility).
    pub conflict_type: String,
    /// Units involved in the conflict.
    pub involved_units: Vec<UnitId>,
    /// Human-readable detail about the conflict.
    pub detail: String,
}

// ===========================================================================
// DecisionTrailRecorder trait
// ===========================================================================

/// Records decision trails for every solver run (INV-O1).
///
/// Every solver execution produces a decision trail: the inputs
/// (graph snapshot, membership), outputs (placements, conflicts), and
/// solver version. Trails are stored and become queryable.
pub trait DecisionTrailRecorder {
    /// Record a solver run's inputs and outputs as a decision trail.
    ///
    /// Called by the node after each solver invocation. The trail is
    /// stored in-memory (M3) and becomes queryable via
    /// [`DecisionTrailQuery`].
    ///
    /// # Arguments
    ///
    /// * `graph_snapshot_id` — identifier of the graph snapshot used
    /// * `membership` — node membership snapshot used as solver input
    /// * `result` — the full solver result (placements, conflicts)
    /// * `solver_version` — version string of the solver that ran
    fn record(
        &self,
        graph_snapshot_id: &str,
        membership: &MembershipSnapshot,
        result: &SolverResult,
        solver_version: &str,
    ) -> Result<DecisionTrailId, ObserveError>;
}

// ===========================================================================
// DecisionTrailQuery trait
// ===========================================================================

/// Queries decision trails (INV-O1, INV-O2).
///
/// Supports three query modes: by trail ID (for replay), by unit ID
/// ("why was unit X placed here?"), and by logical clock range
/// (retention window queries).
pub trait DecisionTrailQuery {
    /// Look up a single trail by its ID.
    ///
    /// Returns [`ObserveError::TrailNotFound`] if no trail with the
    /// given ID exists. Used primarily by [`SolverReplay`](crate::SolverReplay).
    fn query_by_id(&self, trail_id: &DecisionTrailId) -> Result<DecisionTrail, ObserveError>;

    /// Query trails for a specific unit placement.
    ///
    /// "Why was unit X placed on node Y?" — returns the trail(s) that
    /// contain placement decisions for the given unit.
    fn query_by_unit(&self, unit_id: &UnitId) -> Result<Vec<DecisionTrail>, ObserveError>;

    /// Query trails within a logical clock range.
    ///
    /// Retention: since-last-compaction by default (INV-O2).
    /// Returns trails in chronological order (by logical clock).
    fn query_by_range(
        &self,
        from_lc: &LogicalClock,
        to_lc: &LogicalClock,
    ) -> Result<Vec<DecisionTrail>, ObserveError>;
}

// ===========================================================================
// DefaultDecisionTrailRecorder
// ===========================================================================

/// Default in-memory implementation of decision trail recording and
/// querying.
///
/// Trails are stored in a <code>[Mutex]&lt;[Vec]&lt;[DecisionTrail]&gt;&gt;</code>.
/// The trail ID and logical clock are auto-incremented using atomic
/// counters. For M3, all state is in-memory and lost on restart.
/// Future milestones will persist trails via the WAL.
#[derive(Debug)]
pub struct DefaultDecisionTrailRecorder {
    trails: Mutex<Vec<DecisionTrail>>,
    next_id: AtomicU64,
    next_lc: AtomicU64,
}

impl Default for DefaultDecisionTrailRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultDecisionTrailRecorder {
    /// Creates a new, empty in-memory decision trail recorder.
    ///
    /// Trail IDs start at 1. Logical clocks start at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            trails: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            next_lc: AtomicU64::new(1),
        }
    }

    /// Allocates the next trail ID.
    fn alloc_id(&self) -> DecisionTrailId {
        DecisionTrailId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Allocates the next logical clock for a trail timestamp.
    fn alloc_timestamp(&self) -> DualClockEvent {
        let lc = self.next_lc.fetch_add(1, Ordering::SeqCst);
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        }
    }

    /// Converts a solver [`Placement`] into a [`PlacementRecord`].
    ///
    /// For M3, the simplified solver `Placement` (unit, node, score)
    /// is expanded with a basic rationale and the score recorded as
    /// the resource score for the chosen node. Full capability filter
    /// results are not available from the simplified placement and are
    /// left empty until M5 adds detailed placement decisions.
    fn placement_to_record(p: &Placement) -> PlacementRecord {
        PlacementRecord {
            unit_id: p.unit,
            placed_on: p.node,
            rationale: format!("placed with score {}", p.score.as_raw()),
            capability_filter: Vec::new(),
            resource_scores: vec![(p.node, p.score)],
        }
    }

    /// Converts a solver [`Conflict`] into a [`ConflictRecord`].
    fn conflict_to_record(c: &Conflict) -> ConflictRecord {
        ConflictRecord {
            conflict_type: format!("{:?}", c.capability),
            involved_units: c.units.clone(),
            detail: format!("{:?}", c.status),
        }
    }
}

impl DecisionTrailRecorder for DefaultDecisionTrailRecorder {
    fn record(
        &self,
        graph_snapshot_id: &str,
        membership: &MembershipSnapshot,
        result: &SolverResult,
        solver_version: &str,
    ) -> Result<DecisionTrailId, ObserveError> {
        let trail_id = self.alloc_id();
        let timestamp = self.alloc_timestamp();

        let node_membership: Vec<NodeId> = membership.nodes.iter().map(|(id, _, _)| *id).collect();

        let resource_snapshots: Vec<ResourceSnapshotRef> = membership
            .nodes
            .iter()
            .map(|(id, _, _)| ResourceSnapshotRef {
                node_id: *id,
                snapshot_lc: LogicalClock(membership.generation),
            })
            .collect();

        let placements: Vec<PlacementRecord> = result
            .placements
            .iter()
            .map(Self::placement_to_record)
            .collect();

        let conflicts: Vec<ConflictRecord> = result
            .conflicts
            .iter()
            .map(Self::conflict_to_record)
            .collect();

        let trail = DecisionTrail {
            trail_id,
            graph_snapshot_id: graph_snapshot_id.to_string(),
            node_membership,
            resource_snapshots,
            solver_version: solver_version.to_string(),
            placements,
            conflicts,
            timestamp,
        };

        self.trails
            .lock()
            .expect("trails mutex should not be poisoned")
            .push(trail);

        Ok(trail_id)
    }
}

impl DecisionTrailQuery for DefaultDecisionTrailRecorder {
    fn query_by_id(&self, trail_id: &DecisionTrailId) -> Result<DecisionTrail, ObserveError> {
        self.trails
            .lock()
            .expect("trails mutex should not be poisoned")
            .iter()
            .find(|t| t.trail_id == *trail_id)
            .cloned()
            .ok_or(ObserveError::TrailNotFound {
                trail_id: *trail_id,
            })
    }

    fn query_by_unit(&self, unit_id: &UnitId) -> Result<Vec<DecisionTrail>, ObserveError> {
        let matching: Vec<DecisionTrail> = self
            .trails
            .lock()
            .expect("trails mutex should not be poisoned")
            .iter()
            .filter(|t| t.placements.iter().any(|p| p.unit_id == *unit_id))
            .cloned()
            .collect();
        Ok(matching)
    }

    fn query_by_range(
        &self,
        from_lc: &LogicalClock,
        to_lc: &LogicalClock,
    ) -> Result<Vec<DecisionTrail>, ObserveError> {
        let mut matching: Vec<DecisionTrail> = self
            .trails
            .lock()
            .expect("trails mutex should not be poisoned")
            .iter()
            .filter(|t| {
                let lc = t.timestamp.logical_clock;
                lc >= *from_lc && lc <= *to_lc
            })
            .cloned()
            .collect();
        // Sort by logical clock for chronological order.
        matching.sort_by_key(|t| t.timestamp.logical_clock);
        Ok(matching)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_unit_id() -> UnitId {
        UnitId(Uuid::new_v4())
    }

    fn test_node_id() -> NodeId {
        NodeId(Uuid::new_v4())
    }

    fn empty_membership() -> MembershipSnapshot {
        MembershipSnapshot::empty(1)
    }

    fn empty_solver_result() -> SolverResult {
        SolverResult::empty()
    }

    fn solver_result_with_placement(unit_id: UnitId, node_id: NodeId) -> SolverResult {
        SolverResult {
            placements: vec![Placement {
                unit: unit_id,
                node: node_id,
                score: Ppm(750_000),
            }],
            ..SolverResult::empty()
        }
    }

    fn test_dual_clock(lc: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(lc),
            wall_time: WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        }
    }

    // -- DecisionTrailRecorder tests ----------------------------------------

    #[test]
    fn test_record_creates_trail() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let result = empty_solver_result();

        let trail_id = recorder
            .record("snapshot-1", &empty_membership(), &result, "0.1.0")
            .expect("record should succeed");

        // Trail ID starts at 1.
        assert_eq!(trail_id, DecisionTrailId(1));

        // Recording again gives a new ID.
        let trail_id2 = recorder
            .record("snapshot-2", &empty_membership(), &result, "0.1.0")
            .expect("record should succeed");
        assert_eq!(trail_id2, DecisionTrailId(2));
    }

    #[test]
    fn test_record_stores_full_trail() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let unit_id = test_unit_id();
        let node_id = test_node_id();
        let result = solver_result_with_placement(unit_id, node_id);

        let trail_id = recorder
            .record("snap-abc", &empty_membership(), &result, "0.2.0")
            .expect("record should succeed");

        let trail = recorder.query_by_id(&trail_id).expect("trail should exist");
        assert_eq!(trail.trail_id, trail_id);
        assert_eq!(trail.graph_snapshot_id, "snap-abc");
        assert_eq!(trail.solver_version, "0.2.0");
        assert_eq!(trail.placements.len(), 1);
        assert_eq!(trail.placements[0].unit_id, unit_id);
        assert_eq!(trail.placements[0].placed_on, node_id);
    }

    // -- DecisionTrailQuery.query_by_unit tests -----------------------------

    #[test]
    fn test_query_by_unit_finds_trail() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let unit_id = test_unit_id();
        let node_id = test_node_id();
        let result = solver_result_with_placement(unit_id, node_id);

        recorder
            .record("snap-1", &empty_membership(), &result, "0.1.0")
            .expect("record should succeed");

        let trails = recorder
            .query_by_unit(&unit_id)
            .expect("query should succeed");
        assert_eq!(trails.len(), 1, "should find one trail for this unit");
        assert_eq!(trails[0].placements[0].unit_id, unit_id);
    }

    #[test]
    fn test_query_by_unit_no_match() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let unit_id = test_unit_id();
        let other_id = test_unit_id();
        let result = solver_result_with_placement(unit_id, test_node_id());

        recorder
            .record("snap-1", &empty_membership(), &result, "0.1.0")
            .expect("record should succeed");

        let trails = recorder
            .query_by_unit(&other_id)
            .expect("query should succeed");
        assert!(trails.is_empty(), "should find no trails for unknown unit");
    }

    // -- DecisionTrailQuery.query_by_range tests ----------------------------

    #[test]
    fn test_query_by_range() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let result = empty_solver_result();

        // Record three trails. Their logical clocks will be 1, 2, 3
        // (auto-incremented starting from 1).
        let id1 = recorder
            .record("s1", &empty_membership(), &result, "v1")
            .expect("record should succeed");
        let _id2 = recorder
            .record("s2", &empty_membership(), &result, "v1")
            .expect("record should succeed");
        let id3 = recorder
            .record("s3", &empty_membership(), &result, "v1")
            .expect("record should succeed");

        // Query range [2, 3] — should return trails 2 and 3, not 1.
        let trails = recorder
            .query_by_range(&LogicalClock(2), &LogicalClock(3))
            .expect("query should succeed");
        assert_eq!(trails.len(), 2, "should find 2 trails in range [2,3]");
        assert_eq!(trails[0].trail_id, DecisionTrailId(2));
        assert_eq!(trails[1].trail_id, id3);

        // Trail 1 should be excluded.
        assert!(
            !trails.iter().any(|t| t.trail_id == id1),
            "trail 1 should be outside range [2,3]"
        );

        // Query range [1, 3] — all three.
        let all = recorder
            .query_by_range(&LogicalClock(1), &LogicalClock(3))
            .expect("query should succeed");
        assert_eq!(all.len(), 3, "should find all 3 trails in range [1,3]");

        // Query range [4, 10] — none.
        let none = recorder
            .query_by_range(&LogicalClock(4), &LogicalClock(10))
            .expect("query should succeed");
        assert!(none.is_empty(), "should find no trails in range [4,10]");
    }

    #[test]
    fn test_query_by_id_not_found() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let result = recorder.query_by_id(&DecisionTrailId(999));
        assert!(
            matches!(result, Err(ObserveError::TrailNotFound { trail_id }) if trail_id == DecisionTrailId(999))
        );
    }

    // -- Serialization roundtrip tests --------------------------------------

    #[test]
    fn test_decision_trail_serialization_roundtrip() {
        let unit_id = test_unit_id();
        let node_id = test_node_id();

        let trail = DecisionTrail {
            trail_id: DecisionTrailId(42),
            graph_snapshot_id: "snap-xyz".to_string(),
            node_membership: vec![node_id],
            resource_snapshots: vec![ResourceSnapshotRef {
                node_id,
                snapshot_lc: LogicalClock(10),
            }],
            solver_version: "0.3.0".to_string(),
            placements: vec![PlacementRecord {
                unit_id,
                placed_on: node_id,
                rationale: "highest score".to_string(),
                capability_filter: vec![(node_id, true)],
                resource_scores: vec![(node_id, Ppm(900_000))],
            }],
            conflicts: vec![ConflictRecord {
                conflict_type: "SecurityIncompatible".to_string(),
                involved_units: vec![unit_id],
                detail: "unresolved".to_string(),
            }],
            timestamp: test_dual_clock(5),
        };

        let json = serde_json::to_string(&trail).expect("serialize DecisionTrail");
        let decoded: DecisionTrail =
            serde_json::from_str(&json).expect("deserialize DecisionTrail");
        assert_eq!(trail, decoded);
    }

    #[test]
    fn test_placement_record_serialization_roundtrip() {
        let record = PlacementRecord {
            unit_id: test_unit_id(),
            placed_on: test_node_id(),
            rationale: "best score with capability match".to_string(),
            capability_filter: vec![(test_node_id(), true), (test_node_id(), false)],
            resource_scores: vec![
                (test_node_id(), Ppm(800_000)),
                (test_node_id(), Ppm(600_000)),
            ],
        };

        let json = serde_json::to_string(&record).expect("serialize PlacementRecord");
        let decoded: PlacementRecord =
            serde_json::from_str(&json).expect("deserialize PlacementRecord");
        assert_eq!(record, decoded);
    }

    #[test]
    fn test_conflict_record_serialization_roundtrip() {
        let record = ConflictRecord {
            conflict_type: "PurposeMismatch".to_string(),
            involved_units: vec![test_unit_id(), test_unit_id()],
            detail: "two units claim storage:redis with different purposes".to_string(),
        };

        let json = serde_json::to_string(&record).expect("serialize ConflictRecord");
        let decoded: ConflictRecord =
            serde_json::from_str(&json).expect("deserialize ConflictRecord");
        assert_eq!(record, decoded);
    }

    #[test]
    fn test_resource_snapshot_ref_serialization_roundtrip() {
        let ref_ = ResourceSnapshotRef {
            node_id: test_node_id(),
            snapshot_lc: LogicalClock(42),
        };

        let json = serde_json::to_string(&ref_).expect("serialize ResourceSnapshotRef");
        let decoded: ResourceSnapshotRef =
            serde_json::from_str(&json).expect("deserialize ResourceSnapshotRef");
        assert_eq!(ref_, decoded);
    }

    #[test]
    fn test_decision_trail_id_serialization_roundtrip() {
        let id = DecisionTrailId(123);
        let json = serde_json::to_string(&id).expect("serialize DecisionTrailId");
        let decoded: DecisionTrailId =
            serde_json::from_str(&json).expect("deserialize DecisionTrailId");
        assert_eq!(id, decoded);
    }

    // -- Property tests -----------------------------------------------------

    use proptest::prelude::*;

    fn arb_decision_trail(trail_id: u64) -> DecisionTrail {
        DecisionTrail {
            trail_id: DecisionTrailId(trail_id),
            graph_snapshot_id: format!("snap-{trail_id}"),
            node_membership: vec![NodeId(Uuid::nil())],
            resource_snapshots: vec![ResourceSnapshotRef {
                node_id: NodeId(Uuid::nil()),
                snapshot_lc: LogicalClock(trail_id),
            }],
            solver_version: "0.1.0".to_string(),
            placements: vec![PlacementRecord {
                unit_id: UnitId(Uuid::nil()),
                placed_on: NodeId(Uuid::nil()),
                rationale: "test".to_string(),
                capability_filter: Vec::new(),
                resource_scores: vec![(NodeId(Uuid::nil()), Ppm(500_000))],
            }],
            conflicts: Vec::new(),
            timestamp: test_dual_clock(trail_id),
        }
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_decision_trail_serialization_roundtrip(trail_id in 0u64..1_000_000) {
            let trail = arb_decision_trail(trail_id);
            let json = serde_json::to_string(&trail).expect("serialize");
            let decoded: DecisionTrail = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(trail, decoded);
        }
    }
}
