//! Solver error taxonomy.
//!
//! All error variants that the solver can produce. Every error is
//! actionable — it says what went wrong and (where possible) what the
//! caller should do next. No `unwrap()` or panics in solver paths;
//! every failure mode is represented here (INV-C3, FM-004).
//!
//! Errors are categorized at the type level: placement failures,
//! conflict failures, security failures, and internal errors. This
//! mirrors `specs/architecture/error-taxonomy.md`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use taba_common::UnitId;
use taba_core::Capability;

use crate::placement::{Conflict, RecoveryCycle};

// ===========================================================================
// SolverError
// ===========================================================================

/// The complete error type for the solver.
///
/// Every variant is actionable and carries enough context for the
/// caller to respond appropriately. The solver never panics — all
/// failure modes are represented as `SolverError` and collected in
/// [`crate::SolverResult::unplaceable`].
///
/// # Determinism
///
/// `SolverError` derives `PartialEq` so that solver results can be
/// compared for determinism testing (INV-C3). However, the
/// `InternalError` and `Security` variants carry free-form `String`
/// fields; their equality is structural, not semantic.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
pub enum SolverError {
    /// No node in the membership snapshot can satisfy the unit's
    /// capability needs. The `unmet` list contains every capability
    /// that had no matching provider.
    ///
    /// Action: add nodes with the required capabilities, or remove
    /// the unit from the graph.
    #[error("no capable node for unit {unit:?}: unmet capabilities {unmet:?}")]
    NoCapableNode {
        /// The unit that could not be placed.
        unit: UnitId,
        /// Capabilities that no node could satisfy.
        unmet: Vec<Capability>,
    },

    /// An unresolved conflict blocks composition. The solver fails
    /// closed (INV-S2) until explicit policy resolves the conflict.
    ///
    /// Action: author a policy unit that resolves the conflict, or
    /// remove one of the conflicting units.
    #[error("unresolved conflict: {conflict:?}")]
    UnresolvedConflict {
        /// The conflict that could not be resolved.
        conflict: Conflict,
    },

    /// A circular recovery dependency was detected (INV-K5). Cycles
    /// in recovery relationships are unresolvable without explicit
    /// policy declaring restart priority.
    ///
    /// Action: author a policy unit declaring restart priority for
    /// the units in the cycle, or break the cycle by removing a
    /// recovery relationship.
    #[error("cyclic recovery dependency: {cycle:?}")]
    CyclicDependency {
        /// The cycle that was detected.
        cycle: RecoveryCycle,
    },

    /// A unit's tolerance declarations cannot be met by any node
    /// (INV-K3). For example, no node has low enough latency or
    /// enough resources.
    ///
    /// Action: add nodes that meet the tolerance, or relax the
    /// unit's tolerance declarations.
    #[error("tolerance violation for unit {unit:?}: {constraint} ({reason})")]
    ToleranceViolation {
        /// The unit whose tolerance could not be met.
        unit: UnitId,
        /// Which tolerance constraint was violated (e.g., `"max_latency"`).
        constraint: String,
        /// Why the constraint could not be met.
        reason: String,
    },

    /// The policy supersession chain is broken — a policy references
    /// a non-existent predecessor, or the chain contains a gap.
    ///
    /// Action: repair the supersession chain by authoring the
    /// missing policy version or removing the broken reference.
    #[error("broken supersession chain: {reason}")]
    BrokenSupersession {
        /// Why the chain is broken.
        reason: String,
    },

    /// Security error during taint computation or capability check.
    /// The solver fails closed on all security errors (INV-S2).
    ///
    /// Action: review the security policy and trust declarations
    /// for the affected units.
    #[error("security error: {reason}")]
    Security {
        /// Why the security check failed.
        reason: String,
    },

    /// A policy conflict requires human judgment. This occurs when
    /// two non-revoked policies for the same conflict tuple reach
    /// different decisions (INV-S8a), or when legal vs. consent
    /// requirements conflict (FM-010).
    ///
    /// Action: resolve the policy conflict through governance —
    /// supersede one policy or reconcile the decisions.
    #[error("policy conflict between units {units:?}: {reason}")]
    PolicyConflict {
        /// The units involved in the conflict.
        units: Vec<UnitId>,
        /// Why the policies conflict.
        reason: String,
    },

    /// The graph snapshot is stale — its generation does not match
    /// the live graph's generation. The solver should re-snapshot
    /// and retry (FM-012).
    ///
    /// Action: take a fresh snapshot and re-run the solver.
    #[error(
        "stale snapshot: expected generation {expected_generation}, actual {actual_generation}"
    )]
    StaleSnapshot {
        /// The generation the solver expected (from the live graph).
        expected_generation: u64,
        /// The generation the snapshot actually has.
        actual_generation: u64,
    },

    /// Internal error that should not occur in a correct
    /// implementation. If this variant is returned, it indicates a
    /// bug in the solver or corrupted graph state.
    ///
    /// Action: report as a bug. Do not retry.
    #[error("internal solver error: {reason}")]
    InternalError {
        /// What went wrong internally.
        reason: String,
    },
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::NodeId;
    use uuid::Uuid;

    #[test]
    fn test_solver_error_no_capable_node() {
        let unit = UnitId(Uuid::new_v4());
        let unmet = vec![Capability::new("storage", "redis")];
        let err = SolverError::NoCapableNode { unit, unmet };
        assert!(err.to_string().contains("no capable node"));
        let cloned = err.clone();
        assert_eq!(err, cloned);
    }

    #[test]
    fn test_solver_error_unresolved_conflict() {
        let conflict = Conflict {
            units: vec![UnitId(Uuid::new_v4())],
            capability: Capability::new("storage", "redis"),
            status: crate::ConflictStatus::Unresolved,
        };
        let err = SolverError::UnresolvedConflict {
            conflict: conflict.clone(),
        };
        assert!(err.to_string().contains("unresolved conflict"));
        assert_eq!(err, SolverError::UnresolvedConflict { conflict });
    }

    #[test]
    fn test_solver_error_cyclic_dependency() {
        let cycle = RecoveryCycle {
            chain: vec![UnitId(Uuid::new_v4()), UnitId(Uuid::new_v4())],
        };
        let err = SolverError::CyclicDependency {
            cycle: cycle.clone(),
        };
        assert!(err.to_string().contains("cyclic recovery dependency"));
        assert_eq!(err, SolverError::CyclicDependency { cycle });
    }

    #[test]
    fn test_solver_error_tolerance_violation() {
        let unit = UnitId(Uuid::new_v4());
        let err = SolverError::ToleranceViolation {
            unit,
            constraint: "max_latency".to_string(),
            reason: "no node within 100ms".to_string(),
        };
        assert!(err.to_string().contains("tolerance violation"));
    }

    #[test]
    fn test_solver_error_broken_supersession() {
        let err = SolverError::BrokenSupersession {
            reason: "missing predecessor".to_string(),
        };
        assert!(err.to_string().contains("broken supersession"));
    }

    #[test]
    fn test_solver_error_security() {
        let err = SolverError::Security {
            reason: "taint check failed".to_string(),
        };
        assert!(err.to_string().contains("security error"));
    }

    #[test]
    fn test_solver_error_policy_conflict() {
        let units = vec![UnitId(Uuid::new_v4()), UnitId(Uuid::new_v4())];
        let err = SolverError::PolicyConflict {
            units,
            reason: "conflicting decisions".to_string(),
        };
        assert!(err.to_string().contains("policy conflict"));
    }

    #[test]
    fn test_solver_error_stale_snapshot() {
        let err = SolverError::StaleSnapshot {
            expected_generation: 5,
            actual_generation: 3,
        };
        assert!(err.to_string().contains("stale snapshot"));
    }

    #[test]
    fn test_solver_error_internal() {
        let err = SolverError::InternalError {
            reason: "unexpected state".to_string(),
        };
        assert!(err.to_string().contains("internal solver error"));
    }

    #[test]
    fn test_solver_error_serialization_roundtrip() {
        let err = SolverError::NoCapableNode {
            unit: UnitId(Uuid::new_v4()),
            unmet: vec![Capability::new("compute", "http")],
        };
        let json = serde_json::to_string(&err).expect("serialize SolverError");
        let decoded: SolverError = serde_json::from_str(&json).expect("deserialize SolverError");
        assert_eq!(err, decoded);
    }

    #[test]
    fn test_solver_error_implements_std_error() {
        let err = SolverError::InternalError {
            reason: "test".to_string(),
        };
        // If it compiles, it implements std::error::Error via thiserror.
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_solver_error_with_node_id_unused_but_imported() {
        // Ensure NodeId compiles in this module's test scope.
        let _node = NodeId(Uuid::new_v4());
    }
}
