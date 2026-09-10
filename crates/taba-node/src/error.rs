//! Errors produced by taba-node operations.
//!
//! Every error that can occur during WAL management, reconciliation,
//! health reporting, or operational mode transitions is represented by
//! a variant of [`NodeError`]. Variants are designed to be actionable:
//! they identify the unit, position, or mode involved and provide a
//! human-readable reason.
//!
//! # Categorisation
//!
//! - **Retriable**: [`NodeError::WalWriteFailed`] (disk may free up),
//!   [`NodeError::ReconciliationFailed`] (transient runtime issue).
//! - **Permanent**: [`NodeError::WalCorrupted`] (data loss),
//!   [`NodeError::UnitNotFound`].
//! - **Security/Policy**: [`NodeError::InvalidModeTransition`],
//!   [`NodeError::DegradedModeRestriction`] (fail closed).

use crate::mode::OperationalMode;
use taba_common::UnitId;

// ---------------------------------------------------------------------------
// WalPosition
// ---------------------------------------------------------------------------

/// A byte position within the write-ahead log (DL-014).
///
/// Used as a replay cursor and compaction boundary. Positions are
/// monotonically increasing within a single node's WAL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct WalPosition(pub u64);

impl WalPosition {
    /// The zero position — start of the WAL.
    pub const ZERO: Self = Self(0);

    /// Returns the underlying byte offset.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for WalPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wal:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// NodeError
// ---------------------------------------------------------------------------

/// Errors produced by taba-node operations.
///
/// All variants carry sufficient context for diagnosis and (where
/// applicable) recovery. Error propagation uses `?`.
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    /// Reconciliation failed for a specific unit.
    ///
    /// The unit's desired state could not be reached — typically because
    /// the runtime executor returned an error during start, stop, or
    /// drain.
    #[error("reconciliation failed for unit {unit:?}: {reason}")]
    ReconciliationFailed {
        /// The unit whose reconciliation failed.
        unit: UnitId,
        /// Human-readable description of the failure.
        reason: String,
    },

    /// WAL write failed (disk full, I/O error, etc.).
    ///
    /// The node should enter degraded mode when this error is
    /// encountered (FM-07).
    #[error("WAL write failed: {reason}")]
    WalWriteFailed {
        /// Human-readable description of the write failure.
        reason: String,
    },

    /// WAL replay encountered a corrupt entry (FM-07).
    ///
    /// The CRC32C checksum did not match the payload, indicating
    /// on-disk corruption. The position identifies where the
    /// corruption was detected.
    #[error("WAL corrupted at {position}: {reason}")]
    WalCorrupted {
        /// The byte position of the corrupt entry.
        position: WalPosition,
        /// Human-readable description of the corruption.
        reason: String,
    },

    /// Mode transition is not permitted from the current state.
    ///
    /// The operational mode state machine (Normal → Degraded → Recovery
    /// → Normal) enforces specific transition rules. See [`ModeManager`](crate::mode::ModeManager).
    #[error("invalid mode transition from {from:?} to {to:?}")]
    InvalidModeTransition {
        /// The mode the node is currently in.
        from: OperationalMode,
        /// The mode that was requested.
        to: OperationalMode,
    },

    /// Node is in degraded mode; operation not permitted.
    ///
    /// In Degraded mode, only drain and evacuation are permitted.
    /// Authoring, composition, and placement are frozen.
    #[error("degraded mode restriction: {operation} not permitted")]
    DegradedModeRestriction {
        /// The operation that was attempted.
        operation: String,
    },

    /// Unit not found on this node.
    ///
    /// The unit ID was not present in the local actual state or the
    /// runtime executor has no record of it.
    #[error("unit not found: {id:?}")]
    UnitNotFound {
        /// The ID of the unit that was not found.
        id: UnitId,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wal_position_zero() {
        assert_eq!(WalPosition::ZERO, WalPosition(0));
        assert_eq!(WalPosition::ZERO.as_u64(), 0);
    }

    #[test]
    fn test_wal_position_display() {
        let pos = WalPosition(12345);
        assert_eq!(pos.to_string(), "wal:12345");
    }

    #[test]
    fn test_wal_position_ordering() {
        assert!(WalPosition(1) < WalPosition(2));
        assert!(WalPosition(100) > WalPosition(99));
    }

    #[test]
    fn test_node_error_reconciliation_failed_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let err = NodeError::ReconciliationFailed {
            unit: id,
            reason: "start timeout".to_string(),
        };
        assert!(err.to_string().contains("reconciliation failed"));
        assert!(err.to_string().contains("start timeout"));
    }

    #[test]
    fn test_node_error_wal_write_failed_display() {
        let err = NodeError::WalWriteFailed {
            reason: "disk full".to_string(),
        };
        assert!(err.to_string().contains("WAL write failed"));
        assert!(err.to_string().contains("disk full"));
    }

    #[test]
    fn test_node_error_wal_corrupted_display() {
        let err = NodeError::WalCorrupted {
            position: WalPosition(4096),
            reason: "CRC mismatch".to_string(),
        };
        assert!(err.to_string().contains("WAL corrupted"));
        assert!(err.to_string().contains("wal:4096"));
        assert!(err.to_string().contains("CRC mismatch"));
    }

    #[test]
    fn test_node_error_invalid_mode_transition_display() {
        let err = NodeError::InvalidModeTransition {
            from: OperationalMode::Normal,
            to: OperationalMode::Recovery,
        };
        assert!(err.to_string().contains("invalid mode transition"));
    }

    #[test]
    fn test_node_error_degraded_restriction_display() {
        let err = NodeError::DegradedModeRestriction {
            operation: "author".to_string(),
        };
        assert!(err.to_string().contains("degraded mode restriction"));
        assert!(err.to_string().contains("author"));
    }

    #[test]
    fn test_node_error_unit_not_found_display() {
        let id = UnitId(uuid::Uuid::new_v4());
        let err = NodeError::UnitNotFound { id };
        assert!(err.to_string().contains("unit not found"));
    }
}
