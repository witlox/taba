//! Fleet-wide operational command propagation (F-A314 rate limited).
//!
//! Signed governance commands are propagated to all nodes via gossip.
//! Each command type is rate-limited: only one command of each type is
//! allowed per configured logical clock (LC) delta. Deduplication is by
//! `(command_type, logical_clock)`.
//!
//! **M4 scope**: this module tracks the last LC at which each command
//! type was propagated and enforces a simple rate limit (one command per
//! type per `rate_limit_delta` LC ticks). Full fleet-wide propagation is
//! exercised once gossip runs in a multi-node test.

use std::collections::HashMap;
use std::sync::Mutex;

use taba_common::{LogicalClock, UnitId};

use crate::error::GossipError;

// ---------------------------------------------------------------------------
// FleetCommandService trait
// ---------------------------------------------------------------------------

/// Fleet-wide operational command propagation (F-A314 rate limited).
///
/// Signed governance commands propagated to all nodes via gossip.
/// Implementations rate-limit by command type: only one command of each
/// type per configured logical clock delta. Deduplication by
/// `(command_type, logical_clock)`.
pub trait FleetCommandService {
    /// Propagate a fleet-wide command.
    ///
    /// Rate-limited: only one command of each type per configured LC
    /// delta. Deduplication by `(command_type, logical_clock)`.
    ///
    /// Returns [`GossipError::FleetCommandRateLimited`] if a command of
    /// the same type was propagated within the current rate window.
    fn propagate_command(
        &self,
        command_type: &str,
        governance_unit_id: &UnitId,
    ) -> Result<(), GossipError>;
}

// ---------------------------------------------------------------------------
// DefaultFleetCommandService
// ---------------------------------------------------------------------------

/// Default implementation of [`FleetCommandService`].
///
/// Tracks the last logical clock at which each command type was
/// propagated in a `Mutex<HashMap>`. A command is allowed if at least
/// `rate_limit_delta` logical clock ticks have elapsed since the last
/// command of the same type.
///
/// # Concurrency
///
/// The command-type → last-LC map is guarded by a `std::sync::Mutex`.
/// The lock is held only for the duration of the rate-limit check and
/// update.
pub struct DefaultFleetCommandService {
    /// `command_type` → last LC at which a command of this type ran.
    last_by_type: Mutex<HashMap<String, LogicalClock>>,
    /// Current logical clock (advanced on each call).
    current_lc: Mutex<LogicalClock>,
    /// Minimum LC delta between two commands of the same type (F-A314).
    rate_limit_delta: u64,
}

impl std::fmt::Debug for DefaultFleetCommandService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultFleetCommandService")
            .field("rate_limit_delta", &self.rate_limit_delta)
            .finish_non_exhaustive()
    }
}

impl DefaultFleetCommandService {
    /// Creates a new fleet command service with the given rate-limit
    /// delta (in logical clock ticks). A value of `1` means only one
    /// command of each type is allowed per LC tick.
    #[must_use]
    pub fn new(rate_limit_delta: u64) -> Self {
        Self {
            last_by_type: Mutex::new(HashMap::new()),
            current_lc: Mutex::new(LogicalClock(0)),
            rate_limit_delta,
        }
    }

    /// Returns the current logical clock value.
    #[must_use]
    pub fn current_lc(&self) -> LogicalClock {
        *self.current_lc.lock().expect("lc lock poisoned")
    }

    /// Advances the logical clock by one tick.
    ///
    /// This models the passage of one gossip convergence window. In a
    /// real deployment, the LC advances as the node processes events.
    pub fn tick(&self) {
        if let Ok(mut lc) = self.current_lc.lock() {
            lc.tick();
        }
    }

    /// Advances the logical clock to a specific value (for tests).
    pub fn set_lc(&self, lc: LogicalClock) {
        if let Ok(mut current) = self.current_lc.lock() {
            *current = lc;
        }
    }
}

impl Default for DefaultFleetCommandService {
    fn default() -> Self {
        // Default: one command of each type per LC tick (F-A314).
        Self::new(1)
    }
}

impl FleetCommandService for DefaultFleetCommandService {
    fn propagate_command(
        &self,
        command_type: &str,
        _governance_unit_id: &UnitId,
    ) -> Result<(), GossipError> {
        let current = *self.current_lc.lock().expect("lc lock poisoned");
        let mut last_map = self
            .last_by_type
            .lock()
            .expect("last_by_type lock poisoned");

        match last_map.get(command_type).copied() {
            Some(last) if current.0 < last.0 + self.rate_limit_delta => {
                // Within the rate window: reject.
                Err(GossipError::FleetCommandRateLimited {
                    command_type: command_type.to_string(),
                })
            }
            _ => {
                // Outside the window (or first command): allow.
                last_map.insert(command_type.to_string(), current);
                tracing::info!(
                    command_type,
                    logical_clock = current.0,
                    "propagated fleet command"
                );
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u128) -> UnitId {
        UnitId(uuid::Uuid::from_u128(n))
    }

    #[test]
    fn test_propagate_command_first() {
        let svc = DefaultFleetCommandService::new(1);
        // First command of this type → Ok.
        svc.propagate_command("refresh-capabilities", &uid(1))
            .expect("first command should be allowed");
    }

    #[test]
    fn test_propagate_command_rate_limited() {
        let svc = DefaultFleetCommandService::new(1);
        svc.propagate_command("refresh-capabilities", &uid(1))
            .expect("first command should be allowed");

        // Second command of the same type, same LC (no tick) → limited.
        let result = svc.propagate_command("refresh-capabilities", &uid(2));
        assert!(
            matches!(&result, Err(GossipError::FleetCommandRateLimited { command_type }) if command_type == "refresh-capabilities"),
            "second command within window should be rate-limited, got: {result:?}"
        );

        // After advancing the LC by the delta, the command is allowed again.
        svc.set_lc(LogicalClock(1));
        svc.propagate_command("refresh-capabilities", &uid(3))
            .expect("command after window should be allowed");
    }

    #[test]
    fn test_propagate_command_different_types() {
        let svc = DefaultFleetCommandService::new(1);
        svc.propagate_command("refresh-capabilities", &uid(1))
            .expect("first refresh should be allowed");

        // A different command type does not interfere with the first.
        svc.propagate_command("rotate-keys", &uid(2))
            .expect("different command type should be allowed");

        // First type is still rate-limited.
        let result = svc.propagate_command("refresh-capabilities", &uid(3));
        assert!(
            matches!(result, Err(GossipError::FleetCommandRateLimited { .. })),
            "first command type should still be rate-limited"
        );

        // Second type is also rate-limited now.
        let result = svc.propagate_command("rotate-keys", &uid(4));
        assert!(
            matches!(result, Err(GossipError::FleetCommandRateLimited { .. })),
            "second command type should be rate-limited after first"
        );
    }

    #[test]
    fn test_tick_advances_clock() {
        let svc = DefaultFleetCommandService::new(1);
        assert_eq!(svc.current_lc(), LogicalClock(0));
        svc.tick();
        assert_eq!(svc.current_lc(), LogicalClock(1));
        svc.tick();
        assert_eq!(svc.current_lc(), LogicalClock(2));
    }
}
