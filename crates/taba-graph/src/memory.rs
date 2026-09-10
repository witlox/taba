//! Memory monitoring for the composition graph (INV-R6).
//!
//! The [`MemoryMonitor`] trait tracks graph memory usage against a
//! configured limit. Auto-compaction triggers at 80% of the limit
//! (INV-R6). At 100%, the node enters degraded mode: it refuses new
//! placements until compaction completes.

use std::sync::{Arc, Mutex};

use crate::crdt::CompositionGraphData;

// ===========================================================================
// Constants
// ===========================================================================

/// Auto-compaction trigger: 80% of memory limit (INV-R6).
pub const COMPACTION_THRESHOLD_PCT: u8 = 80;

/// Degraded mode trigger: 100% of memory limit (INV-R6).
pub const DEGRADED_THRESHOLD_PCT: u8 = 100;

// ===========================================================================
// MemoryMonitor trait
// ===========================================================================

/// Monitors graph memory usage and triggers compaction (INV-R6).
///
/// Auto-compaction at 80% of limit. Degraded mode at 100%.
///
/// The pressure percentage is computed as
/// `(current_bytes / limit_bytes) * 100`, saturating at `u8::MAX` if
/// the limit is exceeded by more than 255×.
pub trait MemoryMonitor {
    /// Check current memory usage against configured limit.
    ///
    /// Returns `(current_bytes, limit_bytes, pressure_pct)` where
    /// `pressure_pct` is `current_bytes * 100 / limit_bytes`,
    /// saturating at `u8::MAX` if the limit is zero or the ratio
    /// exceeds 255.
    fn check_usage(&self) -> (u64, u64, u8);

    /// Returns `true` if memory usage exceeds 80% of limit
    /// (compaction trigger).
    fn is_compaction_needed(&self) -> bool;

    /// Returns `true` if memory usage exceeds 100% of limit
    /// (degraded trigger).
    fn is_degraded_needed(&self) -> bool;
}

// ===========================================================================
// DefaultMemoryMonitor
// ===========================================================================

/// Default implementation of [`MemoryMonitor`].
///
/// Shares graph state via `Arc<Mutex<...>>` — the same lock used by
/// [`crate::DefaultGraph`]. The lock is held only for the duration of
/// each method call (no `await` points).
pub struct DefaultMemoryMonitor {
    /// Shared graph state.
    state: Arc<Mutex<CompositionGraphData>>,
    /// Configured memory limit in bytes (INV-R6).
    memory_limit_bytes: u64,
}

impl DefaultMemoryMonitor {
    /// Creates a new `DefaultMemoryMonitor` sharing the given graph
    /// state.
    ///
    /// The `memory_limit_bytes` is the maximum allowed graph memory.
    /// Auto-compaction triggers at 80%; degraded mode at 100%.
    #[must_use]
    pub const fn new(state: Arc<Mutex<CompositionGraphData>>, memory_limit_bytes: u64) -> Self {
        Self {
            state,
            memory_limit_bytes,
        }
    }

    /// Returns the configured memory limit in bytes.
    #[must_use]
    pub const fn memory_limit_bytes(&self) -> u64 {
        self.memory_limit_bytes
    }

    /// Computes the pressure percentage.
    ///
    /// `pressure = current_bytes * 100 / limit_bytes`, saturating at
    /// `u8::MAX` if the limit is zero or the ratio exceeds 255.
    fn pressure_pct(current: u64, limit: u64) -> u8 {
        if limit == 0 {
            return u8::MAX;
        }
        let ratio = current.saturating_mul(100) / limit;
        u8::try_from(ratio).unwrap_or(u8::MAX)
    }
}

impl MemoryMonitor for DefaultMemoryMonitor {
    fn check_usage(&self) -> (u64, u64, u8) {
        let state = self
            .state
            .lock()
            .expect("graph mutex should not be poisoned");
        let current = state.memory_estimate_bytes;
        let pressure = Self::pressure_pct(current, self.memory_limit_bytes);
        (current, self.memory_limit_bytes, pressure)
    }

    fn is_compaction_needed(&self) -> bool {
        let (_, _, pressure) = self.check_usage();
        pressure >= COMPACTION_THRESHOLD_PCT
    }

    fn is_degraded_needed(&self) -> bool {
        let (_, _, pressure) = self.check_usage();
        pressure >= DEGRADED_THRESHOLD_PCT
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates graph state with the given memory estimate.
    fn state_with_memory(mem: u64) -> Arc<Mutex<CompositionGraphData>> {
        let mut data = CompositionGraphData::new();
        data.memory_estimate_bytes = mem;
        Arc::new(Mutex::new(data))
    }

    #[test]
    fn test_memory_usage_under_limit() {
        // 40% of limit
        let limit = 1_000_000u64;
        let state = state_with_memory(400_000);
        let monitor = DefaultMemoryMonitor::new(state, limit);

        let (current, lim, pressure) = monitor.check_usage();
        assert_eq!(current, 400_000);
        assert_eq!(lim, limit);
        assert_eq!(pressure, 40, "pressure should be 40%");
        assert!(
            pressure < COMPACTION_THRESHOLD_PCT,
            "pressure should be under 80%"
        );
        assert!(
            !monitor.is_compaction_needed(),
            "should not need compaction"
        );
        assert!(!monitor.is_degraded_needed(), "should not be degraded");
    }

    #[test]
    fn test_compaction_needed_at_80_percent() {
        let limit = 1_000_000u64;
        let state = state_with_memory(800_000);
        let monitor = DefaultMemoryMonitor::new(state, limit);

        let (_, _, pressure) = monitor.check_usage();
        assert_eq!(pressure, 80, "pressure should be exactly 80%");

        assert!(
            monitor.is_compaction_needed(),
            "compaction should be needed at 80%"
        );
        assert!(
            !monitor.is_degraded_needed(),
            "should not be degraded at 80%"
        );
    }

    #[test]
    fn test_compaction_needed_above_80_percent() {
        let limit = 1_000_000u64;
        let state = state_with_memory(850_000);
        let monitor = DefaultMemoryMonitor::new(state, limit);

        assert!(
            monitor.is_compaction_needed(),
            "compaction should be needed above 80%"
        );
        assert!(
            !monitor.is_degraded_needed(),
            "should not be degraded at 85%"
        );
    }

    #[test]
    fn test_degraded_needed_at_100_percent() {
        let limit = 1_000_000u64;
        let state = state_with_memory(1_000_000);
        let monitor = DefaultMemoryMonitor::new(state, limit);

        let (_, _, pressure) = monitor.check_usage();
        assert_eq!(pressure, 100, "pressure should be exactly 100%");

        assert!(
            monitor.is_compaction_needed(),
            "compaction should be needed at 100%"
        );
        assert!(monitor.is_degraded_needed(), "should be degraded at 100%");
    }

    #[test]
    fn test_degraded_needed_above_100_percent() {
        let limit = 1_000_000u64;
        let state = state_with_memory(1_500_000);
        let monitor = DefaultMemoryMonitor::new(state, limit);

        assert!(
            monitor.is_degraded_needed(),
            "should be degraded above 100%"
        );
    }

    #[test]
    fn test_memory_zero_limit() {
        let state = state_with_memory(100);
        let monitor = DefaultMemoryMonitor::new(state, 0);

        let (_, _, pressure) = monitor.check_usage();
        assert_eq!(pressure, u8::MAX, "zero limit should saturate pressure");
        assert!(monitor.is_compaction_needed());
        assert!(monitor.is_degraded_needed());
    }

    #[test]
    fn test_memory_empty_graph() {
        let state = Arc::new(Mutex::new(CompositionGraphData::new()));
        let monitor = DefaultMemoryMonitor::new(state, 1_000_000);

        let (current, _, pressure) = monitor.check_usage();
        assert_eq!(current, 0);
        assert_eq!(pressure, 0, "empty graph should have 0% pressure");
        assert!(!monitor.is_compaction_needed());
        assert!(!monitor.is_degraded_needed());
    }

    #[test]
    fn test_pressure_pct_function() {
        assert_eq!(DefaultMemoryMonitor::pressure_pct(0, 100), 0);
        assert_eq!(DefaultMemoryMonitor::pressure_pct(50, 100), 50);
        assert_eq!(DefaultMemoryMonitor::pressure_pct(80, 100), 80);
        assert_eq!(DefaultMemoryMonitor::pressure_pct(100, 100), 100);
        assert_eq!(DefaultMemoryMonitor::pressure_pct(150, 100), 150);
        assert_eq!(
            DefaultMemoryMonitor::pressure_pct(300, 100),
            u8::MAX,
            "300% should saturate"
        );
        assert_eq!(
            DefaultMemoryMonitor::pressure_pct(100, 0),
            u8::MAX,
            "zero limit should saturate"
        );
    }
}
