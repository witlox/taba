//! Errors produced by taba-graph operations.
//!
//! Every graph operation that can fail returns [`GraphError`]. Variants
//! are designed to be actionable: they identify the unit, author, or
//! conflict involved and provide a human-readable reason.
//!
//! # Categorisation
//!
//! Graph errors map to the three-tab-bucket model (Retriable,
//! Permanent, Security):
//!
//! - **Security**: [`GraphError::SignatureRejected`],
//!   [`GraphError::ScopeViolation`], [`GraphError::DeclassificationDenied`]
//! - **Permanent**: [`GraphError::NotFound`], [`GraphError::Archived`],
//!   [`GraphError::PolicyChainError`], [`GraphError::WouldCreateCycle`]
//! - **Retriable**: [`GraphError::UnsatisfiedReferences`] (pending
//!   units may be promoted later), [`GraphError::MemoryLimitExceeded`]
//!   (compaction may free space), [`GraphError::PersistenceError`]

use taba_common::{AuthorId, UnitId};

use taba_core::ConflictTuple;

// ===========================================================================
// GraphError
// ===========================================================================

/// Errors produced by taba-graph operations.
///
/// All variants carry sufficient context for diagnosis and (where
/// applicable) recovery. Error propagation uses `?`.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// Signature verification failed (synchronous gate per INV-S3).
    #[error("signature rejected for unit {unit:?}: {reason}")]
    SignatureRejected {
        /// The unit whose signature was rejected.
        unit: UnitId,
        /// Human-readable description of why the signature is invalid.
        reason: String,
    },

    /// Unit references entities not yet in the graph (enters pending queue).
    ///
    /// This is not a hard failure — the unit is buffered in the pending
    /// queue and will be promoted when its references arrive (INV-C4).
    #[error("unsatisfied references for unit {unit:?}: missing {missing:?}")]
    UnsatisfiedReferences {
        /// The unit with unsatisfied references.
        unit: UnitId,
        /// References that are not yet present in the local graph.
        missing: Vec<UnitId>,
    },

    /// Merge would violate CRDT properties.
    ///
    /// Returned only for true Byzantine violations (e.g., two different
    /// units claim the same `UnitId`). Normal capability conflicts are NOT
    /// merge conflicts — they are surfaced by the solver.
    #[error("merge conflict: {reason}")]
    MergeConflict {
        /// Human-readable description of the CRDT violation.
        reason: String,
    },

    /// Unit not found in the active set.
    #[error("unit not found: {id:?}")]
    NotFound {
        /// The ID of the unit that was not found.
        id: UnitId,
    },

    /// Policy supersession chain is broken or ambiguous (INV-C7).
    #[error("policy chain error for conflict {conflict:?}: {reason}")]
    PolicyChainError {
        /// The conflict tuple whose policy chain is broken.
        conflict: ConflictTuple,
        /// Human-readable description of the chain error.
        reason: String,
    },

    /// Graph memory limit exceeded (INV-R6). Node should enter degraded mode.
    #[error("memory limit exceeded: used {used} bytes, limit {limit} bytes")]
    MemoryLimitExceeded {
        /// Current memory usage in bytes.
        used: u64,
        /// Configured memory limit in bytes.
        limit: u64,
    },

    /// WAL or storage persistence error.
    #[error("persistence error: {reason}")]
    PersistenceError {
        /// Human-readable description of the persistence failure.
        reason: String,
    },

    /// Query references an archived or compacted unit.
    #[error("unit archived: {id:?}")]
    Archived {
        /// The ID of the archived unit.
        id: UnitId,
    },

    /// Inserting this unit would create a cyclic recovery dependency (INV-K5).
    #[error("would create cycle for unit {unit:?}: {cycle:?}")]
    WouldCreateCycle {
        /// The unit that would create the cycle.
        unit: UnitId,
        /// The cycle path (ordered list of unit IDs).
        cycle: Vec<UnitId>,
    },

    /// Compaction failed (partially or fully).
    #[error("compaction failed: {units_attempted} attempted, {units_failed} failed")]
    CompactionFailed {
        /// Number of units the compaction attempted to process.
        units_attempted: u64,
        /// Number of units that failed during compaction.
        units_failed: u64,
    },

    /// Author scope violation (INV-S5, INV-S8).
    #[error("scope violation by author {author:?}: {reason}")]
    ScopeViolation {
        /// The author who violated scope.
        author: AuthorId,
        /// Human-readable description of the violation.
        reason: String,
    },

    /// Declassification policy missing required multi-party signatures (INV-S9).
    #[error("declassification denied for policy {policy:?}: {reason}")]
    DeclassificationDenied {
        /// The policy that attempted declassification.
        policy: UnitId,
        /// Human-readable description of why declassification was denied.
        reason: String,
    },
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_rejected_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let err = GraphError::SignatureRejected {
            unit: id,
            reason: "bad signature".to_string(),
        };
        assert!(err.to_string().contains("signature rejected"));
        assert!(err.to_string().contains("bad signature"));
    }

    #[test]
    fn test_unsatisfied_references_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let missing = vec![UnitId(uuid::Uuid::new_v4())];
        let err = GraphError::UnsatisfiedReferences {
            unit: id,
            missing: missing.clone(),
        };
        assert!(err.to_string().contains("unsatisfied references"));
        assert!(err.to_string().contains(&format!("{missing:?}")));
    }

    #[test]
    fn test_not_found_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let err = GraphError::NotFound { id };
        assert!(err.to_string().contains("unit not found"));
        assert!(err.to_string().contains(&format!("{id:?}")));
    }

    #[test]
    fn test_memory_limit_exceeded_display() {
        let err = GraphError::MemoryLimitExceeded {
            used: 1_000_000,
            limit: 500_000,
        };
        assert!(err.to_string().contains("memory limit exceeded"));
        assert!(err.to_string().contains("1000000"));
        assert!(err.to_string().contains("500000"));
    }

    #[test]
    fn test_archived_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let err = GraphError::Archived { id };
        assert!(err.to_string().contains("archived"));
        assert!(err.to_string().contains(&format!("{id:?}")));
    }

    #[test]
    fn test_policy_chain_error_display() {
        let conflict = ConflictTuple {
            unit_ids: std::collections::BTreeSet::from([UnitId(uuid::Uuid::new_v4())]),
            capability_name: "storage".to_string(),
        };
        let err = GraphError::PolicyChainError {
            conflict,
            reason: "broken chain".to_string(),
        };
        assert!(err.to_string().contains("policy chain error"));
        assert!(err.to_string().contains("broken chain"));
    }

    #[test]
    fn test_scope_violation_display() {
        let author = AuthorId(uuid::Uuid::new_v4());
        let err = GraphError::ScopeViolation {
            author,
            reason: "unauthorized".to_string(),
        };
        assert!(err.to_string().contains("scope violation"));
        assert!(err.to_string().contains("unauthorized"));
    }

    #[test]
    fn test_declassification_denied_display() {
        let policy = UnitId(uuid::Uuid::new_v4());
        let err = GraphError::DeclassificationDenied {
            policy,
            reason: "missing co-signer".to_string(),
        };
        assert!(err.to_string().contains("declassification denied"));
        assert!(err.to_string().contains("missing co-signer"));
    }

    #[test]
    fn test_would_create_cycle_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let cycle = vec![id, UnitId(uuid::Uuid::new_v4())];
        let err = GraphError::WouldCreateCycle {
            unit: id,
            cycle: cycle.clone(),
        };
        assert!(err.to_string().contains("would create cycle"));
        assert!(err.to_string().contains(&format!("{cycle:?}")));
    }

    #[test]
    fn test_compaction_failed_display() {
        let err = GraphError::CompactionFailed {
            units_attempted: 10,
            units_failed: 3,
        };
        assert!(err.to_string().contains("compaction failed"));
        assert!(err.to_string().contains("10"));
        assert!(err.to_string().contains('3'));
    }

    #[test]
    fn test_merge_conflict_display() {
        let err = GraphError::MergeConflict {
            reason: "duplicate UnitId".to_string(),
        };
        assert!(err.to_string().contains("merge conflict"));
        assert!(err.to_string().contains("duplicate UnitId"));
    }

    #[test]
    fn test_persistence_error_display() {
        let err = GraphError::PersistenceError {
            reason: "disk full".to_string(),
        };
        assert!(err.to_string().contains("persistence error"));
        assert!(err.to_string().contains("disk full"));
    }
}
