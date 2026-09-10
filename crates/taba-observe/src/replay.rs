//! Solver replay from historical decision trails (INV-C3).
//!
//! The deterministic solver produces the exact same result given the
//! same inputs. [`SolverReplay`] enables "show me the solver's view at
//! time T" by reconstructing the inputs from a [`DecisionTrail`] and
//! re-invoking the solver.
//!
//! ## M3 limitation
//!
//! In M3, the graph snapshot is not persisted to disk. Since replay
//! requires the original graph snapshot (not just the trail), replay
//! always returns [`ObserveError::ReplayFailed`] with the reason
//! "snapshot not available in M3 (requires disk-backed graph)". Full
//! replay will be available in M3+ (disk-backed graph) or M5
//! (Usable).

use taba_solver::{Solver, SolverResult};

use crate::error::ObserveError;
use crate::trail::{DecisionTrailId, DecisionTrailQuery};

// ===========================================================================
// SolverReplay trait
// ===========================================================================

/// Replays a solver run from a historical decision trail (INV-C3).
///
/// Deterministic solver means replay produces the exact same result.
/// Used for debugging: "show me the solver's view at time T."
///
/// # Determinism
///
/// If the replayed result differs from the recorded placements, this
/// indicates a determinism regression (FM-11) and should be
/// investigated immediately.
pub trait SolverReplay {
    /// Replay the solver using inputs from a recorded trail.
    ///
    /// Reconstructs the graph snapshot and membership from the trail,
    /// invokes the solver, and verifies the output matches the recorded
    /// placements. Divergence indicates a determinism regression
    /// (FM-11).
    ///
    /// # Errors
    ///
    /// - [`ObserveError::TrailNotFound`] if no trail exists for the
    ///   given ID.
    /// - [`ObserveError::ReplayFailed`] if the graph snapshot is not
    ///   available (M3 limitation) or the replay could not complete.
    fn replay(&self, trail_id: &DecisionTrailId) -> Result<SolverResult, ObserveError>;
}

// ===========================================================================
// DefaultSolverReplayer
// ===========================================================================

/// Default implementation of [`SolverReplay`].
///
/// Holds a reference to a [`DecisionTrailQuery`] implementation (for
/// looking up trails by ID) and a [`Solver`] (for re-running the
/// solver once the graph snapshot is available).
///
/// ## M3 behavior
///
/// The `Solver` reference is stored but not invoked in M3, because the
/// graph snapshot cannot be reconstructed from the in-memory trail
/// alone. The `replay()` method always returns
/// [`ObserveError::ReplayFailed`] with the reason
/// `"snapshot not available in M3 (requires disk-backed graph)"`.
///
/// In future milestones, `replay()` will:
/// 1. Look up the trail by ID.
/// 2. Reconstruct the graph snapshot and membership from the trail.
/// 3. Invoke the solver with the reconstructed inputs.
/// 4. Verify the output matches the recorded placements.
/// 5. Return the replayed [`SolverResult`] (or
///    [`ObserveError::ReplayFailed`] on divergence).
pub struct DefaultSolverReplayer<'a, Q, S>
where
    Q: DecisionTrailQuery,
    S: Solver,
{
    /// Query interface for looking up trails by ID.
    query: &'a Q,
    /// Solver to invoke during replay (unused in M3).
    solver: &'a S,
}

impl<'a, Q, S> DefaultSolverReplayer<'a, Q, S>
where
    Q: DecisionTrailQuery,
    S: Solver,
{
    /// Creates a new replayer with the given query interface and solver.
    ///
    /// # Arguments
    ///
    /// * `query` — a [`DecisionTrailQuery`] implementation for
    ///   looking up trails by ID
    /// * `solver` — a [`Solver`] implementation for re-running the
    ///   solver (stored but not invoked in M3)
    #[must_use]
    pub const fn new(query: &'a Q, solver: &'a S) -> Self {
        Self { query, solver }
    }
}

impl<Q, S> SolverReplay for DefaultSolverReplayer<'_, Q, S>
where
    Q: DecisionTrailQuery,
    S: Solver,
{
    fn replay(&self, trail_id: &DecisionTrailId) -> Result<SolverResult, ObserveError> {
        // Step 1: Look up the trail by ID.
        // This also verifies the trail exists (not compacted or missing).
        let _trail = self.query.query_by_id(trail_id)?;

        // Step 2: Reconstruct the graph snapshot from the trail.
        //
        // In M3, the graph snapshot is stored only in-memory and is not
        // persisted alongside the trail. The trail records the
        // graph_snapshot_id but not the actual snapshot data. Without
        // the snapshot, the solver cannot be re-invoked.
        //
        // The `solver` reference is stored for future milestones but
        // is intentionally not called here.
        let _ = &self.solver;

        // M3: always return ReplayFailed because the graph snapshot is
        // not available. Future milestones will reconstruct the
        // snapshot from disk-backed storage and invoke the solver.
        Err(ObserveError::ReplayFailed {
            reason: "snapshot not available in M3 (requires disk-backed graph)".to_string(),
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trail::{DecisionTrailRecorder, DefaultDecisionTrailRecorder};
    use taba_common::{NodeId, Ppm, UnitId};
    use taba_solver::{DefaultSolver, MembershipSnapshot, Placement};
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

    // -- Required tests -----------------------------------------------------

    #[test]
    fn test_replay_not_found() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let solver = DefaultSolver::new();
        let replayer = DefaultSolverReplayer::new(&recorder, &solver);

        let result = replayer.replay(&DecisionTrailId(999));
        assert!(
            matches!(result, Err(ObserveError::TrailNotFound { trail_id }) if trail_id == DecisionTrailId(999))
        );
    }

    #[test]
    fn test_replay_no_snapshot() {
        let recorder = DefaultDecisionTrailRecorder::new();
        let solver = DefaultSolver::new();

        // Record a trail so the ID exists.
        let trail_id = recorder
            .record(
                "snap-1",
                &empty_membership(),
                &SolverResult::empty(),
                "0.1.0",
            )
            .expect("record should succeed");

        let replayer = DefaultSolverReplayer::new(&recorder, &solver);
        let result = replayer.replay(&trail_id);

        // M3: graph snapshot is not available in-memory.
        assert!(result.is_err());
        let err = result.expect_err("replay should fail in M3");
        assert!(
            matches!(err, ObserveError::ReplayFailed { ref reason }
                if reason.contains("snapshot not available in M3")),
            "expected ReplayFailed with M3 reason, got: {err:?}"
        );
    }

    #[test]
    fn test_solver_replayer_implements_trait() {
        // Compile-time check that DefaultSolverReplayer implements SolverReplay.
        fn assert_solver_replay<T: SolverReplay>() {}
        let recorder = DefaultDecisionTrailRecorder::new();
        let solver = DefaultSolver::new();
        let replayer = DefaultSolverReplayer::new(&recorder, &solver);
        assert_solver_replay::<
            DefaultSolverReplayer<'_, DefaultDecisionTrailRecorder, DefaultSolver>,
        >();
        // Suppress unused variable warning.
        let _ = replayer;
    }

    // -- Additional test: replay with stored SolverResult -------------------

    #[test]
    fn test_replay_with_stored_solver_result() {
        // Verify that a trail recorded from a SolverResult can be
        // looked up for replay. In M3, replay returns ReplayFailed
        // (graph snapshot not available), but the trail must be
        // found — not TrailNotFound.
        let recorder = DefaultDecisionTrailRecorder::new();
        let solver = DefaultSolver::new();

        let unit_id = test_unit_id();
        let node_id = test_node_id();
        let result = solver_result_with_placement(unit_id, node_id);

        let trail_id = recorder
            .record("snap-stored", &empty_membership(), &result, "0.3.0")
            .expect("record should succeed");

        // Verify the trail was stored with the placement data.
        let trail = recorder.query_by_id(&trail_id).expect("trail should exist");
        assert_eq!(trail.placements.len(), 1);
        assert_eq!(trail.placements[0].unit_id, unit_id);
        assert_eq!(trail.placements[0].placed_on, node_id);

        // Replay: trail is found, but M3 returns ReplayFailed.
        let replayer = DefaultSolverReplayer::new(&recorder, &solver);
        let replay_result = replayer.replay(&trail_id);
        assert!(
            !matches!(replay_result, Err(ObserveError::TrailNotFound { .. })),
            "trail should be found, not TrailNotFound"
        );
        assert!(replay_result.is_err());
        assert!(
            matches!(replay_result, Err(ObserveError::ReplayFailed { .. })),
            "expected ReplayFailed for M3"
        );
    }
}
