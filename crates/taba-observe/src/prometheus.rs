//! Prometheus exposition format export.
//!
//! The [`PrometheusExporter`] trait renders per-node metrics in the
//! standard Prometheus text exposition format. Metrics cover resource
//! usage, solver activity, gossip, and graph health.
//!
//! ## Metric types
//!
//! - **Gauges**: current values that can go up or down
//!   (`memory_available_bytes`, `cpu_load_ppm`, `workloads_running`,
//!   `artifact_cache_size_bytes`, `decision_trails_stored`)
//! - **Counters**: monotonically increasing cumulative totals
//!   (`solver_runs_total`, `gossip_messages_total`, `compactions_total`)

use serde::{Deserialize, Serialize};

use taba_common::Ppm;

// ===========================================================================
// NodeMetrics
// ===========================================================================

/// Metric types exposed on the Prometheus endpoint.
///
/// All values are point-in-time snapshots taken when the exporter is
/// constructed. The exporter renders them in Prometheus text
/// exposition format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeMetrics {
    /// Memory available in bytes.
    pub memory_available_bytes: u64,
    /// CPU load in parts per million (10^6 = 1.0).
    pub cpu_load_ppm: Ppm,
    /// Number of workloads currently running on this node.
    pub workloads_running: u32,
    /// Cumulative total of solver runs since node start.
    pub solver_runs_total: u64,
    /// Cumulative total of gossip messages sent/received.
    pub gossip_messages_total: u64,
    /// Size of the artifact cache in bytes.
    pub artifact_cache_size_bytes: u64,
    /// Number of decision trails currently stored.
    pub decision_trails_stored: u64,
    /// Cumulative total of graph compactions since node start.
    pub compactions_total: u64,
}

// ===========================================================================
// PrometheusExporter trait
// ===========================================================================

/// Exposes Prometheus-format metrics endpoint.
///
/// The exporter renders all node metrics in the standard Prometheus
/// text exposition format, ready to be served on an HTTP endpoint.
pub trait PrometheusExporter {
    /// Render all metrics in Prometheus exposition format.
    ///
    /// The output includes `# HELP`, `# TYPE`, and value lines for
    /// each metric. The format is:
    ///
    /// ```text
    /// # HELP taba_memory_available_bytes Memory available in bytes
    /// # TYPE taba_memory_available_bytes gauge
    /// taba_memory_available_bytes 12345
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`ObserveError::ExportFailed`] if rendering fails
    /// (e.g., internal formatting error).
    fn render_metrics(&self) -> String;
}

// ===========================================================================
// DefaultPrometheusExporter
// ===========================================================================

/// Default implementation of [`PrometheusExporter`].
///
/// Takes a [`NodeMetrics`] snapshot at construction time and renders
/// it in Prometheus text exposition format. The snapshot is immutable
/// — to update metrics, create a new exporter with a fresh snapshot.
#[derive(Debug, Clone)]
pub struct DefaultPrometheusExporter {
    /// The metrics snapshot to export.
    metrics: NodeMetrics,
}

impl DefaultPrometheusExporter {
    /// Creates a new exporter with the given metrics snapshot.
    ///
    /// # Arguments
    ///
    /// * `metrics` — a [`NodeMetrics`] snapshot to render
    #[must_use]
    pub const fn new(metrics: NodeMetrics) -> Self {
        Self { metrics }
    }
}

impl PrometheusExporter for DefaultPrometheusExporter {
    fn render_metrics(&self) -> String {
        let mut out = String::with_capacity(1024);

        // Helper macro to append a metric block.
        macro_rules! metric {
            ($name:expr, $help:expr, $type:expr, $value:expr) => {
                out.push_str(&format!(
                    "# HELP {name} {help}\n",
                    name = $name,
                    help = $help
                ));
                out.push_str(&format!("# TYPE {name} {ty}\n", name = $name, ty = $type));
                out.push_str(&format!("{name} {value}\n", name = $name, value = $value));
            };
        }

        metric!(
            "taba_memory_available_bytes",
            "Memory available in bytes",
            "gauge",
            self.metrics.memory_available_bytes
        );
        metric!(
            "taba_cpu_load_ppm",
            "CPU load in parts per million",
            "gauge",
            self.metrics.cpu_load_ppm.as_raw()
        );
        metric!(
            "taba_workloads_running",
            "Number of workloads currently running",
            "gauge",
            self.metrics.workloads_running
        );
        metric!(
            "taba_solver_runs_total",
            "Total solver runs since node start",
            "counter",
            self.metrics.solver_runs_total
        );
        metric!(
            "taba_gossip_messages_total",
            "Total gossip messages sent and received",
            "counter",
            self.metrics.gossip_messages_total
        );
        metric!(
            "taba_artifact_cache_size_bytes",
            "Size of the artifact cache in bytes",
            "gauge",
            self.metrics.artifact_cache_size_bytes
        );
        metric!(
            "taba_decision_trails_stored",
            "Number of decision trails currently stored",
            "gauge",
            self.metrics.decision_trails_stored
        );
        metric!(
            "taba_compactions_total",
            "Total graph compactions since node start",
            "counter",
            self.metrics.compactions_total
        );

        out
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metrics() -> NodeMetrics {
        NodeMetrics {
            memory_available_bytes: 1_073_741_824,
            cpu_load_ppm: Ppm(500_000),
            workloads_running: 7,
            solver_runs_total: 142,
            gossip_messages_total: 9_999,
            artifact_cache_size_bytes: 67_108_864,
            decision_trails_stored: 87,
            compactions_total: 3,
        }
    }

    #[test]
    fn test_render_metrics_contains_all() {
        let exporter = DefaultPrometheusExporter::new(sample_metrics());
        let output = exporter.render_metrics();

        let metric_names = [
            "taba_memory_available_bytes",
            "taba_cpu_load_ppm",
            "taba_workloads_running",
            "taba_solver_runs_total",
            "taba_gossip_messages_total",
            "taba_artifact_cache_size_bytes",
            "taba_decision_trails_stored",
            "taba_compactions_total",
        ];

        for name in &metric_names {
            assert!(
                output.contains(name),
                "output should contain metric name: {name}"
            );
        }
    }

    #[test]
    fn test_render_metrics_format() {
        let exporter = DefaultPrometheusExporter::new(sample_metrics());
        let output = exporter.render_metrics();

        // Each metric should have # HELP and # TYPE lines.
        let metric_names = [
            "taba_memory_available_bytes",
            "taba_cpu_load_ppm",
            "taba_workloads_running",
            "taba_solver_runs_total",
            "taba_gossip_messages_total",
            "taba_artifact_cache_size_bytes",
            "taba_decision_trails_stored",
            "taba_compactions_total",
        ];

        for name in &metric_names {
            assert!(
                output.contains(&format!("# HELP {name}")),
                "output should have # HELP line for {name}"
            );
            assert!(
                output.contains(&format!("# TYPE {name}")),
                "output should have # TYPE line for {name}"
            );
        }

        // Verify gauge and counter type assignments.
        let gauges = [
            "taba_memory_available_bytes",
            "taba_cpu_load_ppm",
            "taba_workloads_running",
            "taba_artifact_cache_size_bytes",
            "taba_decision_trails_stored",
        ];
        let counters = [
            "taba_solver_runs_total",
            "taba_gossip_messages_total",
            "taba_compactions_total",
        ];

        for g in &gauges {
            assert!(
                output.contains(&format!("# TYPE {g} gauge")),
                "{g} should be type gauge"
            );
        }
        for c in &counters {
            assert!(
                output.contains(&format!("# TYPE {c} counter")),
                "{c} should be type counter"
            );
        }
    }

    #[test]
    fn test_render_metrics_values() {
        let metrics = sample_metrics();
        let exporter = DefaultPrometheusExporter::new(metrics.clone());
        let output = exporter.render_metrics();

        assert!(
            output.contains(&format!(
                "taba_memory_available_bytes {}",
                metrics.memory_available_bytes
            )),
            "output should contain memory_available_bytes value"
        );
        assert!(
            output.contains(&format!(
                "taba_cpu_load_ppm {}",
                metrics.cpu_load_ppm.as_raw()
            )),
            "output should contain cpu_load_ppm value"
        );
        assert!(
            output.contains(&format!(
                "taba_workloads_running {}",
                metrics.workloads_running
            )),
            "output should contain workloads_running value"
        );
        assert!(
            output.contains(&format!(
                "taba_solver_runs_total {}",
                metrics.solver_runs_total
            )),
            "output should contain solver_runs_total value"
        );
        assert!(
            output.contains(&format!(
                "taba_gossip_messages_total {}",
                metrics.gossip_messages_total
            )),
            "output should contain gossip_messages_total value"
        );
        assert!(
            output.contains(&format!(
                "taba_artifact_cache_size_bytes {}",
                metrics.artifact_cache_size_bytes
            )),
            "output should contain artifact_cache_size_bytes value"
        );
        assert!(
            output.contains(&format!(
                "taba_decision_trails_stored {}",
                metrics.decision_trails_stored
            )),
            "output should contain decision_trails_stored value"
        );
        assert!(
            output.contains(&format!(
                "taba_compactions_total {}",
                metrics.compactions_total
            )),
            "output should contain compactions_total value"
        );
    }

    #[test]
    fn test_render_metrics_zero_values() {
        let metrics = NodeMetrics {
            memory_available_bytes: 0,
            cpu_load_ppm: Ppm(0),
            workloads_running: 0,
            solver_runs_total: 0,
            gossip_messages_total: 0,
            artifact_cache_size_bytes: 0,
            decision_trails_stored: 0,
            compactions_total: 0,
        };
        let exporter = DefaultPrometheusExporter::new(metrics);
        let output = exporter.render_metrics();

        // All metrics should render even with zero values.
        assert!(output.contains("taba_memory_available_bytes 0\n"));
        assert!(output.contains("taba_cpu_load_ppm 0\n"));
        assert!(output.contains("taba_workloads_running 0\n"));
        assert!(output.contains("taba_solver_runs_total 0\n"));
        assert!(output.contains("taba_gossip_messages_total 0\n"));
        assert!(output.contains("taba_artifact_cache_size_bytes 0\n"));
        assert!(output.contains("taba_decision_trails_stored 0\n"));
        assert!(output.contains("taba_compactions_total 0\n"));
    }

    #[test]
    fn test_node_metrics_serialization_roundtrip() {
        let metrics = sample_metrics();
        let json = serde_json::to_string(&metrics).expect("serialize NodeMetrics");
        let decoded: NodeMetrics = serde_json::from_str(&json).expect("deserialize NodeMetrics");
        assert_eq!(metrics, decoded);
    }
}
