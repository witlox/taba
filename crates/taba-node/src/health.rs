//! Health reporting — local node health status (INV-R6, FM-07).
//!
//! The [`HealthReporter`] trait produces health snapshots consumed by
//! the gossip protocol (heartbeats) and by the solver (node scoring).
//! Health is determined by resource utilization, operational mode,
//! and WAL status.

use serde::{Deserialize, Serialize};
use taba_common::NodeId;

use crate::mode::{DegradedReason, OperationalMode};

// ---------------------------------------------------------------------------
// HealthStatus
// ---------------------------------------------------------------------------

/// Health status reported by a node.
///
/// This is the payload included in gossip heartbeats and consumed by
/// the solver for node scoring. All fields are observable artifacts
/// that a stub would not produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    /// The node being reported.
    pub node: NodeId,
    /// Current operational mode.
    pub mode: OperationalMode,
    /// Number of units currently running on this node.
    pub units_running: u32,
    /// Number of units that have failed on this node.
    pub units_failed: u32,
    /// Current WAL size in bytes.
    pub wal_size_bytes: u64,
    /// Current graph memory usage in bytes.
    pub graph_memory_bytes: u64,
    /// Graph memory limit in bytes (INV-R6).
    pub graph_memory_limit_bytes: u64,
    /// Percentage of memory limit used (0-100).
    pub memory_pressure_pct: u8,
}

// ---------------------------------------------------------------------------
// HealthReporter trait
// ---------------------------------------------------------------------------

/// Reports local node health status.
///
/// Health information is consumed by the gossip protocol (heartbeats)
/// and by the solver (node scoring considers health). This trait is
/// read-only — it does not modify node state.
pub trait HealthReporter: Send + Sync {
    /// Produce the current health status snapshot.
    ///
    /// Includes operational mode, running/failed unit counts, WAL size,
    /// graph memory usage, and memory pressure percentage.
    fn report(&self) -> HealthStatus;

    /// Check whether the node is approaching memory limit.
    ///
    /// Returns `true` if memory usage exceeds 80% of limit (the
    /// auto-compaction trigger threshold per INV-R6).
    fn is_memory_pressured(&self) -> bool;

    /// Check whether the node should enter degraded mode.
    ///
    /// Returns `Some(reason)` if any degraded-mode trigger condition
    /// is met:
    /// - Erasure threshold exceeded (INV-R4)
    /// - Memory limit exceeded (INV-R6)
    /// - WAL failure detected (FM-07)
    ///
    /// Returns `None` if the node is healthy.
    fn should_degrade(&self) -> Option<DegradedReason>;
}

// ---------------------------------------------------------------------------
// DefaultHealthReporter
// ---------------------------------------------------------------------------

/// Default implementation of [`HealthReporter`].
///
/// Tracks health metrics in memory. The `wal_failure` flag is set
/// when a WAL write fails — the reporter will then return
/// `Some(DegradedReason::WalFailure)` from `should_degrade()`.
///
/// Thread-safe via interior mutability.
#[derive(Debug)]
pub struct DefaultHealthReporter {
    /// The node being reported.
    node: NodeId,
    /// Current operational mode.
    mode: std::sync::Mutex<OperationalMode>,
    /// Number of units currently running.
    units_running: std::sync::atomic::AtomicU32,
    /// Number of units that have failed.
    units_failed: std::sync::atomic::AtomicU32,
    /// Current WAL size in bytes.
    wal_size_bytes: std::sync::atomic::AtomicU64,
    /// Current graph memory usage in bytes.
    graph_memory_bytes: std::sync::atomic::AtomicU64,
    /// Graph memory limit in bytes (INV-R6).
    graph_memory_limit_bytes: u64,
    /// WAL failure flag (FM-07).
    wal_failure: std::sync::atomic::AtomicBool,
}

impl DefaultHealthReporter {
    /// Creates a new `DefaultHealthReporter` for the given node and
    /// graph memory limit.
    ///
    /// Starts in Normal mode with zero units, zero WAL size, and zero
    /// graph memory.
    #[must_use]
    pub const fn new(node: NodeId, graph_memory_limit_bytes: u64) -> Self {
        Self {
            node,
            mode: std::sync::Mutex::new(OperationalMode::Normal),
            units_running: std::sync::atomic::AtomicU32::new(0),
            units_failed: std::sync::atomic::AtomicU32::new(0),
            wal_size_bytes: std::sync::atomic::AtomicU64::new(0),
            graph_memory_bytes: std::sync::atomic::AtomicU64::new(0),
            graph_memory_limit_bytes,
            wal_failure: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Sets the current operational mode.
    pub fn set_mode(&self, mode: OperationalMode) {
        *self
            .mode
            .lock()
            .expect("health reporter mode mutex should not be poisoned") = mode;
    }

    /// Sets the number of running units.
    pub fn set_units_running(&self, count: u32) {
        self.units_running
            .store(count, std::sync::atomic::Ordering::Relaxed);
    }

    /// Sets the number of failed units.
    pub fn set_units_failed(&self, count: u32) {
        self.units_failed
            .store(count, std::sync::atomic::Ordering::Relaxed);
    }

    /// Sets the WAL size in bytes.
    pub fn set_wal_size_bytes(&self, size: u64) {
        self.wal_size_bytes
            .store(size, std::sync::atomic::Ordering::Relaxed);
    }

    /// Sets the graph memory usage in bytes.
    pub fn set_graph_memory_bytes(&self, bytes: u64) {
        self.graph_memory_bytes
            .store(bytes, std::sync::atomic::Ordering::Relaxed);
    }

    /// Sets the WAL failure flag (FM-07).
    ///
    /// When set, `should_degrade()` will return
    /// `Some(DegradedReason::WalFailure)`.
    pub fn set_wal_failure(&self, failed: bool) {
        self.wal_failure
            .store(failed, std::sync::atomic::Ordering::Relaxed);
    }

    /// Computes memory pressure as a percentage (0-100).
    fn memory_pressure_pct(&self) -> u8 {
        if self.graph_memory_limit_bytes == 0 {
            return 0;
        }
        let used = self
            .graph_memory_bytes
            .load(std::sync::atomic::Ordering::Relaxed);
        #[allow(clippy::cast_possible_truncation)]
        {
            ((used * 100) / self.graph_memory_limit_bytes).min(100) as u8
        }
    }
}

impl HealthReporter for DefaultHealthReporter {
    fn report(&self) -> HealthStatus {
        let mode = self
            .mode
            .lock()
            .expect("health reporter mode mutex should not be poisoned")
            .clone();

        HealthStatus {
            node: self.node,
            mode,
            units_running: self
                .units_running
                .load(std::sync::atomic::Ordering::Relaxed),
            units_failed: self.units_failed.load(std::sync::atomic::Ordering::Relaxed),
            wal_size_bytes: self
                .wal_size_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            graph_memory_bytes: self
                .graph_memory_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            graph_memory_limit_bytes: self.graph_memory_limit_bytes,
            memory_pressure_pct: self.memory_pressure_pct(),
        }
    }

    fn is_memory_pressured(&self) -> bool {
        self.memory_pressure_pct() >= 80
    }

    fn should_degrade(&self) -> Option<DegradedReason> {
        // WAL failure (FM-07).
        if self.wal_failure.load(std::sync::atomic::Ordering::Relaxed) {
            return Some(DegradedReason::WalFailure);
        }

        // Memory limit exceeded (INV-R6).
        if self.memory_pressure_pct() >= 100 {
            return Some(DegradedReason::MemoryLimitExceeded);
        }

        // Erasure threshold exceeded (INV-R4) is determined by the
        // gossip layer, not by local health. For M3, we return None
        // here — the caller can check separately.

        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_normal() {
        let node = NodeId(uuid::Uuid::new_v4());
        let reporter = DefaultHealthReporter::new(node, 1_000_000);

        reporter.set_units_running(5);
        reporter.set_units_failed(1);
        reporter.set_wal_size_bytes(4096);
        reporter.set_graph_memory_bytes(500_000);

        let status = reporter.report();

        assert_eq!(status.node, node);
        assert!(status.mode.is_normal());
        assert_eq!(status.units_running, 5);
        assert_eq!(status.units_failed, 1);
        assert_eq!(status.wal_size_bytes, 4096);
        assert_eq!(status.graph_memory_bytes, 500_000);
        assert_eq!(status.graph_memory_limit_bytes, 1_000_000);
        assert_eq!(status.memory_pressure_pct, 50);
    }

    #[test]
    fn test_memory_pressured() {
        let node = NodeId(uuid::Uuid::new_v4());
        let reporter = DefaultHealthReporter::new(node, 1_000_000);

        // Below 80% — not pressured.
        reporter.set_graph_memory_bytes(700_000);
        assert!(!reporter.is_memory_pressured());

        // At 80% — pressured.
        reporter.set_graph_memory_bytes(800_000);
        assert!(reporter.is_memory_pressured());

        // Above 80% — pressured.
        reporter.set_graph_memory_bytes(950_000);
        assert!(reporter.is_memory_pressured());
    }

    #[test]
    fn test_should_degrade_memory() {
        let node = NodeId(uuid::Uuid::new_v4());
        let reporter = DefaultHealthReporter::new(node, 1_000_000);

        // Below 100% — no degrade.
        reporter.set_graph_memory_bytes(950_000);
        assert_eq!(reporter.should_degrade(), None);

        // At 100% — degrade.
        reporter.set_graph_memory_bytes(1_000_000);
        assert_eq!(
            reporter.should_degrade(),
            Some(DegradedReason::MemoryLimitExceeded)
        );

        // Above 100% — degrade.
        reporter.set_graph_memory_bytes(2_000_000);
        assert_eq!(
            reporter.should_degrade(),
            Some(DegradedReason::MemoryLimitExceeded)
        );
    }

    #[test]
    fn test_should_degrade_wal() {
        let node = NodeId(uuid::Uuid::new_v4());
        let reporter = DefaultHealthReporter::new(node, 1_000_000);

        // No WAL failure — no degrade.
        assert_eq!(reporter.should_degrade(), None);

        // WAL failure — degrade.
        reporter.set_wal_failure(true);
        assert_eq!(reporter.should_degrade(), Some(DegradedReason::WalFailure));

        // WAL failure cleared — no degrade.
        reporter.set_wal_failure(false);
        assert_eq!(reporter.should_degrade(), None);
    }

    #[test]
    fn test_should_degrade_none() {
        let node = NodeId(uuid::Uuid::new_v4());
        let reporter = DefaultHealthReporter::new(node, 1_000_000);

        reporter.set_units_running(10);
        reporter.set_units_failed(0);
        reporter.set_wal_size_bytes(100_000);
        reporter.set_graph_memory_bytes(100_000);
        reporter.set_wal_failure(false);

        assert_eq!(reporter.should_degrade(), None);
        assert!(!reporter.is_memory_pressured());
    }

    #[test]
    fn test_zero_limit_no_pressure() {
        let node = NodeId(uuid::Uuid::new_v4());
        let reporter = DefaultHealthReporter::new(node, 0);

        reporter.set_graph_memory_bytes(1_000_000);
        assert!(!reporter.is_memory_pressured());
        assert_eq!(reporter.should_degrade(), None);
    }
}
