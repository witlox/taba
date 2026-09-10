//! Operational mode management — Normal, Degraded, Recovery state machine.
//!
//! The operational mode is system-wide and affects which operations are
//! permitted. Transitions follow a strict state machine:
//!
//! - Normal → Degraded (any trigger condition)
//! - Degraded → Recovery (when trigger condition resolves, re-coding starts)
//! - Recovery → Normal (when re-coding completes)
//! - Recovery → Degraded (if a new trigger fires during recovery)
//! - Degraded → Normal (only via operator override — not automatic)

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::NodeError;

// ---------------------------------------------------------------------------
// OperationalMode
// ---------------------------------------------------------------------------

/// The operational mode of a node, system-wide.
///
/// In Normal mode, all operations are permitted. In Degraded mode, only
/// drain and evacuation are allowed — authoring, composition, and
/// placement are frozen. In Recovery mode, placement is throttled while
/// re-coding completes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationalMode {
    /// All operations permitted.
    #[default]
    Normal,
    /// Authoring/composition/placement frozen. Drain and evacuation only.
    ///
    /// Entered when erasure threshold exceeded (INV-R4), memory limit
    /// exceeded (INV-R6), WAL failure detected (FM-07), or operator-triggered.
    Degraded {
        /// What triggered degraded mode.
        reason: DegradedReason,
    },
    /// Gradual re-coding underway. Placement throttled.
    ///
    /// Auto-transitions to Normal when recovery completes.
    Recovery,
}

impl OperationalMode {
    /// Returns `true` if this is Normal mode.
    #[must_use]
    pub const fn is_normal(&self) -> bool {
        matches!(self, Self::Normal)
    }

    /// Returns `true` if this is Degraded mode (any trigger).
    #[must_use]
    pub const fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded { .. })
    }

    /// Returns `true` if this is Recovery mode.
    #[must_use]
    pub const fn is_recovery(&self) -> bool {
        matches!(self, Self::Recovery)
    }
}

// ---------------------------------------------------------------------------
// DegradedReason
// ---------------------------------------------------------------------------

/// Why the node entered degraded mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DegradedReason {
    /// Erasure threshold exceeded — too many node failures (INV-R4).
    ErasureThresholdExceeded,
    /// Graph memory limit exceeded on this node (INV-R6).
    MemoryLimitExceeded,
    /// WAL corruption or disk full detected (FM-07).
    WalFailure,
    /// Operator explicitly triggered degraded mode.
    OperatorTriggered,
}

// ---------------------------------------------------------------------------
// ModeManager trait
// ---------------------------------------------------------------------------

/// Manages operational mode transitions.
///
/// Mode transitions follow a state machine:
/// - Normal → Degraded (any trigger condition)
/// - Normal → Recovery (not valid — recovery is entered from Degraded)
/// - Degraded → Recovery (when trigger condition resolves, re-coding starts)
/// - Recovery → Normal (when re-coding completes)
/// - Recovery → Degraded (if a new trigger fires during recovery)
/// - Degraded → Normal (only via operator override — not automatic)
pub trait ModeManager {
    /// Get the current operational mode.
    fn current_mode(&self) -> OperationalMode;

    /// Transition to a new operational mode.
    ///
    /// Validates the transition against the state machine. Returns
    /// [`NodeError::InvalidModeTransition`] if the transition is not
    /// permitted from the current state.
    ///
    /// # Errors
    ///
    /// - [`NodeError::InvalidModeTransition`] if the transition is not
    ///   valid from the current mode.
    fn transition(&self, to: OperationalMode) -> Result<(), NodeError>;

    /// Check whether a specific operation is permitted in the current mode.
    ///
    /// In Degraded mode, only `drain` and `evacuate` are permitted.
    /// In Recovery mode, `placement` is throttled but still allowed;
    /// `drain` and `evacuate` are always permitted.
    /// In Normal mode, everything is permitted.
    fn is_operation_permitted(&self, operation: &str) -> bool;
}

// ---------------------------------------------------------------------------
// DefaultModeManager
// ---------------------------------------------------------------------------

/// Default implementation of [`ModeManager`].
///
/// Holds the current operational mode behind a [`Mutex`] for interior
/// mutability. All operations are synchronous — the mutex is never held
/// across an `await` point.
#[derive(Debug)]
pub struct DefaultModeManager {
    mode: Mutex<OperationalMode>,
}

impl Default for DefaultModeManager {
    fn default() -> Self {
        Self {
            mode: Mutex::new(OperationalMode::Normal),
        }
    }
}

impl DefaultModeManager {
    /// Creates a new `DefaultModeManager` starting in Normal mode.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new `DefaultModeManager` starting in the given mode.
    ///
    /// Used in tests to set up a specific starting state.
    #[must_use]
    pub const fn with_mode(mode: OperationalMode) -> Self {
        Self {
            mode: Mutex::new(mode),
        }
    }
}

impl ModeManager for DefaultModeManager {
    fn current_mode(&self) -> OperationalMode {
        self.mode
            .lock()
            .expect("mode manager mutex should not be poisoned")
            .clone()
    }

    fn transition(&self, to: OperationalMode) -> Result<(), NodeError> {
        let mut mode = self
            .mode
            .lock()
            .expect("mode manager mutex should not be poisoned");

        let from = mode.clone();

        // Validate transition against the state machine.
        // Invalid: Normal → Recovery (must go through Degraded),
        //          Degraded → Normal (requires operator override).
        // All other transitions are valid.
        let valid = !matches!(
            (&from, &to),
            (OperationalMode::Normal, OperationalMode::Recovery)
                | (OperationalMode::Degraded { .. }, OperationalMode::Normal)
        );

        if !valid {
            return Err(NodeError::InvalidModeTransition { from, to });
        }

        *mode = to;
        Ok(())
    }

    fn is_operation_permitted(&self, operation: &str) -> bool {
        let mode = self
            .mode
            .lock()
            .expect("mode manager mutex should not be poisoned");

        match &*mode {
            OperationalMode::Degraded { .. } => {
                // Only drain and evacuation are permitted in Degraded mode.
                matches!(operation, "drain" | "evacuate")
            }
            // Everything is permitted in Normal and Recovery modes.
            // In Recovery, placement is throttled — a policy concern
            // for future phases, not enforced here.
            OperationalMode::Normal | OperationalMode::Recovery => true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_mode_normal() {
        let mgr = DefaultModeManager::new();
        assert!(mgr.current_mode().is_normal());
    }

    #[test]
    fn test_transition_normal_to_degraded() {
        let mgr = DefaultModeManager::new();
        mgr.transition(OperationalMode::Degraded {
            reason: DegradedReason::MemoryLimitExceeded,
        })
        .expect("Normal → Degraded should be valid");
        assert!(mgr.current_mode().is_degraded());
    }

    #[test]
    fn test_transition_normal_to_recovery_err() {
        let mgr = DefaultModeManager::new();
        let result = mgr.transition(OperationalMode::Recovery);
        assert!(
            matches!(result, Err(NodeError::InvalidModeTransition { .. })),
            "Normal → Recovery should be invalid (must go through Degraded)"
        );
        // Mode should be unchanged.
        assert!(mgr.current_mode().is_normal());
    }

    #[test]
    fn test_transition_degraded_to_recovery() {
        let mgr = DefaultModeManager::with_mode(OperationalMode::Degraded {
            reason: DegradedReason::ErasureThresholdExceeded,
        });
        mgr.transition(OperationalMode::Recovery)
            .expect("Degraded → Recovery should be valid");
        assert!(mgr.current_mode().is_recovery());
    }

    #[test]
    fn test_transition_recovery_to_normal() {
        let mgr = DefaultModeManager::with_mode(OperationalMode::Recovery);
        mgr.transition(OperationalMode::Normal)
            .expect("Recovery → Normal should be valid");
        assert!(mgr.current_mode().is_normal());
    }

    #[test]
    fn test_transition_degraded_to_normal_err() {
        let mgr = DefaultModeManager::with_mode(OperationalMode::Degraded {
            reason: DegradedReason::OperatorTriggered,
        });
        let result = mgr.transition(OperationalMode::Normal);
        assert!(
            matches!(result, Err(NodeError::InvalidModeTransition { .. })),
            "Degraded → Normal should be invalid (requires operator override)"
        );
        assert!(mgr.current_mode().is_degraded());
    }

    #[test]
    fn test_transition_recovery_to_degraded() {
        let mgr = DefaultModeManager::with_mode(OperationalMode::Recovery);
        mgr.transition(OperationalMode::Degraded {
            reason: DegradedReason::WalFailure,
        })
        .expect("Recovery → Degraded should be valid");
        assert!(mgr.current_mode().is_degraded());
    }

    #[test]
    fn test_transition_normal_to_normal_noop() {
        let mgr = DefaultModeManager::new();
        mgr.transition(OperationalMode::Normal)
            .expect("Normal → Normal should be valid (no-op)");
        assert!(mgr.current_mode().is_normal());
    }

    #[test]
    fn test_operation_permitted_normal() {
        let mgr = DefaultModeManager::new();
        assert!(mgr.is_operation_permitted("author"));
        assert!(mgr.is_operation_permitted("compose"));
        assert!(mgr.is_operation_permitted("place"));
        assert!(mgr.is_operation_permitted("drain"));
        assert!(mgr.is_operation_permitted("evacuate"));
        assert!(mgr.is_operation_permitted("anything"));
    }

    #[test]
    fn test_operation_permitted_degraded() {
        let mgr = DefaultModeManager::with_mode(OperationalMode::Degraded {
            reason: DegradedReason::MemoryLimitExceeded,
        });
        assert!(!mgr.is_operation_permitted("author"));
        assert!(!mgr.is_operation_permitted("compose"));
        assert!(!mgr.is_operation_permitted("place"));
        assert!(mgr.is_operation_permitted("drain"));
        assert!(mgr.is_operation_permitted("evacuate"));
        assert!(!mgr.is_operation_permitted("anything"));
    }

    #[test]
    fn test_operation_permitted_recovery() {
        let mgr = DefaultModeManager::with_mode(OperationalMode::Recovery);
        // Placement is throttled but still permitted.
        assert!(mgr.is_operation_permitted("place"));
        // Drain and evacuate are always permitted.
        assert!(mgr.is_operation_permitted("drain"));
        assert!(mgr.is_operation_permitted("evacuate"));
        // Other operations are also permitted in Recovery.
        assert!(mgr.is_operation_permitted("author"));
    }

    #[test]
    fn test_degraded_reason_variants() {
        let reasons = [
            DegradedReason::ErasureThresholdExceeded,
            DegradedReason::MemoryLimitExceeded,
            DegradedReason::WalFailure,
            DegradedReason::OperatorTriggered,
        ];
        for (i, r) in reasons.iter().enumerate() {
            for (j, s) in reasons.iter().enumerate() {
                if i == j {
                    assert_eq!(r, s);
                } else {
                    assert_ne!(r, s);
                }
            }
        }
    }
}
