//! Observability error taxonomy.
//!
//! All error variants that the observe crate can produce. Every variant
//! is actionable and carries enough context for diagnosis. Uses
//! `thiserror` for typed error definitions, consistent with the
//! workspace error taxonomy (`specs/architecture/error-taxonomy.md`).

use thiserror::Error;

use crate::trail::DecisionTrailId;

/// The complete error type for the observe crate.
///
/// Every variant carries structured context for diagnosis. Decision
/// trail errors are fatal (the trail is gone or corrupted). Export
/// errors are retryable (external systems may recover).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ObserveError {
    /// Decision trail does not exist for the given ID.
    ///
    /// Action: verify the trail ID. The trail may not have been
    /// recorded yet, or may have been compacted.
    #[error("decision trail not found: {trail_id:?}")]
    TrailNotFound {
        /// The trail ID that was not found.
        trail_id: DecisionTrailId,
    },

    /// Solver replay could not complete. For M3, this is typically
    /// because the graph snapshot is not available in-memory.
    ///
    /// Action: if the reason indicates an M3 limitation, wait for
    /// disk-backed graph support. Otherwise, investigate the replay
    /// inputs.
    #[error("solver replay failed: {reason}")]
    ReplayFailed {
        /// Why the replay failed.
        reason: String,
    },

    /// Integration export to an external system failed (Prometheus,
    /// OpenTelemetry, webhook).
    ///
    /// Action: check the target system's availability and
    /// configuration. May be retried after the external system
    /// recovers.
    #[error("export to {target} failed: {reason}")]
    ExportFailed {
        /// The export target (e.g., "prometheus", "otel", "webhook").
        target: String,
        /// Why the export failed.
        reason: String,
    },

    /// Decision trail was compacted beyond its retention window
    /// (INV-O2). The trail data is no longer available.
    ///
    /// Action: increase the `decision_retention` on the unit or trust
    /// domain to retain trails for longer.
    #[error("decision trail compacted: {trail_id:?}")]
    TrailCompacted {
        /// The trail ID that was compacted.
        trail_id: DecisionTrailId,
    },
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trail_not_found_display() {
        let err = ObserveError::TrailNotFound {
            trail_id: DecisionTrailId(42),
        };
        assert!(err.to_string().contains("decision trail not found"));
        assert!(err.to_string().contains("42"));
    }

    #[test]
    fn test_replay_failed_display() {
        let err = ObserveError::ReplayFailed {
            reason: "snapshot not available".to_string(),
        };
        assert!(err.to_string().contains("solver replay failed"));
        assert!(err.to_string().contains("snapshot not available"));
    }

    #[test]
    fn test_export_failed_display() {
        let err = ObserveError::ExportFailed {
            target: "prometheus".to_string(),
            reason: "connection refused".to_string(),
        };
        assert!(err.to_string().contains("export to prometheus failed"));
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn test_trail_compacted_display() {
        let err = ObserveError::TrailCompacted {
            trail_id: DecisionTrailId(7),
        };
        assert!(err.to_string().contains("compacted"));
        assert!(err.to_string().contains('7'));
    }

    #[test]
    fn test_observe_error_implements_std_error() {
        let err = ObserveError::TrailNotFound {
            trail_id: DecisionTrailId(1),
        };
        // If it compiles, it implements std::error::Error via thiserror.
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn test_observe_error_equality() {
        let a = ObserveError::ReplayFailed {
            reason: "test".to_string(),
        };
        let b = ObserveError::ReplayFailed {
            reason: "test".to_string(),
        };
        assert_eq!(a, b);

        let c = ObserveError::TrailNotFound {
            trail_id: DecisionTrailId(1),
        };
        assert_ne!(a, c);
    }
}
