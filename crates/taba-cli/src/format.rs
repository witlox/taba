#![allow(clippy::format_push_string)]
//! Output formatting for CLI display.
//!
//! Provides [`OutputFormat`] selector (table or JSON) and formatting
//! functions for units, graph stats, solver results, provenance
//! chains, and decision trails.
//!
//! ## Table format
//!
//! Uses simple aligned columns with headers. Column widths are
//! determined by the longest value in each column.
//!
//! ## JSON format
//!
//! Uses [`serde_json::to_string_pretty`] for human-readable JSON.

use clap::ValueEnum;
use taba_core::Unit;
use taba_graph::{GraphStats, ProvenanceLink};
use taba_observe::DecisionTrail;
use taba_solver::SolverResult;

// ===========================================================================
// OutputFormat
// ===========================================================================

/// Output format selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum OutputFormat {
    /// Human-readable table.
    #[default]
    Table,
    /// JSON (pretty-printed).
    Json,
}

// ===========================================================================
// Formatting functions
// ===========================================================================

/// Format a list of units for display.
pub fn format_units(units: &[Unit], format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_units_table(units),
        OutputFormat::Json => serde_json::to_string_pretty(units)
            .unwrap_or_else(|e| format!("Error formatting units as JSON: {e}")),
    }
}

/// Format graph stats for display.
pub fn format_stats(stats: &GraphStats, format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_stats_table(stats),
        OutputFormat::Json => serde_json::to_string_pretty(stats)
            .unwrap_or_else(|e| format!("Error formatting stats as JSON: {e}")),
    }
}

/// Format solver result for display.
pub fn format_solver_result(result: &SolverResult, format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_solver_result_table(result),
        OutputFormat::Json => serde_json::to_string_pretty(result)
            .unwrap_or_else(|e| format!("Error formatting solver result as JSON: {e}")),
    }
}

/// Format a single unit for display.
pub fn format_unit(unit: &Unit, format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_single_unit_table(unit),
        OutputFormat::Json => serde_json::to_string_pretty(unit)
            .unwrap_or_else(|e| format!("Error formatting unit as JSON: {e}")),
    }
}

/// Format provenance links for display.
pub fn format_provenance(links: &[ProvenanceLink], format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_provenance_table(links),
        OutputFormat::Json => serde_json::to_string_pretty(links)
            .unwrap_or_else(|e| format!("Error formatting provenance as JSON: {e}")),
    }
}

/// Format decision trails for display.
pub fn format_trails(trails: &[DecisionTrail], format: OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_trails_table(trails),
        OutputFormat::Json => serde_json::to_string_pretty(trails)
            .unwrap_or_else(|e| format!("Error formatting trails as JSON: {e}")),
    }
}

// ===========================================================================
// Table implementations
// ===========================================================================

/// Formats units as a table with columns: ID, TYPE, STATE, AUTHOR.
fn format_units_table(units: &[Unit]) -> String {
    let mut rows: Vec<[String; 4]> = Vec::with_capacity(units.len());

    for unit in units {
        let header = unit.header();
        rows.push([
            header.id.to_string(),
            unit.kind().to_string(),
            format!("{:?}", header.state),
            header.author.to_string(),
        ]);
    }

    format_table(&["ID", "TYPE", "STATE", "AUTHOR"], &rows)
}

/// Formats graph stats as a table.
fn format_stats_table(stats: &GraphStats) -> String {
    format_table(
        &["METRIC", "VALUE"],
        &[
            ["active_units".to_string(), stats.active_units.to_string()],
            ["pending_units".to_string(), stats.pending_units.to_string()],
            [
                "archived_units".to_string(),
                stats.archived_units.to_string(),
            ],
            ["memory_bytes".to_string(), stats.memory_bytes.to_string()],
            [
                "memory_limit_bytes".to_string(),
                stats.memory_limit_bytes.to_string(),
            ],
        ],
    )
}

/// Formats solver result as a table.
fn format_solver_result_table(result: &SolverResult) -> String {
    let mut out = String::new();

    out.push_str("PLACEMENTS\n");
    if result.placements.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let rows: Vec<[String; 3]> = result
            .placements
            .iter()
            .map(|p| {
                [
                    p.unit.to_string(),
                    p.node.to_string(),
                    format!("{} ppm", p.score.as_raw()),
                ]
            })
            .collect();
        out.push_str(&format_table(&["UNIT", "NODE", "SCORE"], &rows));
    }

    out.push('\n');
    out.push_str("CONFLICTS\n");
    if result.conflicts.is_empty() {
        out.push_str("  (none)\n");
    } else {
        let rows: Vec<[String; 3]> = result
            .conflicts
            .iter()
            .map(|c| {
                let units = c
                    .units
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                [
                    units,
                    format!("{}:{}", c.capability.cap_type, c.capability.name),
                    format!("{:?}", c.status),
                ]
            })
            .collect();
        out.push_str(&format_table(&["UNITS", "CAPABILITY", "STATUS"], &rows));
    }

    if !result.unplaceable.is_empty() {
        out.push('\n');
        out.push_str("UNPLACEABLE\n");
        for (id, err) in &result.unplaceable {
            out.push_str(&format!("  {id}: {err}\n"));
        }
    }

    out
}

/// Formats a single unit as a detailed table.
fn format_single_unit_table(unit: &Unit) -> String {
    let header = unit.header();
    let mut rows = vec![
        ["id".to_string(), header.id.to_string()],
        ["type".to_string(), unit.kind().to_string()],
        ["state".to_string(), format!("{:?}", header.state)],
        ["author".to_string(), header.author.to_string()],
        ["trust_domain".to_string(), header.trust_domain.to_string()],
    ];

    match unit {
        Unit::Workload(w) => {
            rows.push(["kind".to_string(), format!("{:?}", w.kind)]);
            rows.push([
                "artifact_type".to_string(),
                format!("{:?}", w.artifact.artifact_type),
            ]);
            rows.push(["artifact_ref".to_string(), w.artifact.artifact_ref.clone()]);
            rows.push([
                "scaling_min".to_string(),
                w.scaling.min_instances.to_string(),
            ]);
            rows.push([
                "scaling_max".to_string(),
                w.scaling.max_instances.to_string(),
            ]);
            rows.push([
                "needs".to_string(),
                w.needs
                    .iter()
                    .map(|c| format!("{}:{}", c.cap_type, c.name))
                    .collect::<Vec<_>>()
                    .join(", "),
            ]);
            rows.push([
                "provides".to_string(),
                w.provides
                    .iter()
                    .map(|c| format!("{}:{}", c.cap_type, c.name))
                    .collect::<Vec<_>>()
                    .join(", "),
            ]);
        }
        Unit::Data(d) => {
            rows.push([
                "classification".to_string(),
                format!("{:?}", d.classification),
            ]);
            rows.push(["schema_format".to_string(), d.schema.format.clone()]);
            rows.push([
                "retention_mode".to_string(),
                format!("{:?}", d.retention.mode),
            ]);
        }
        Unit::Policy(p) => {
            rows.push(["resolution".to_string(), format!("{:?}", p.resolution)]);
            rows.push(["rationale".to_string(), p.rationale.clone()]);
        }
        Unit::Governance(g) => {
            rows.push([
                "governance_type".to_string(),
                format!("{:?}", std::mem::discriminant(g)),
            ]);
        }
    }

    format_table(&["FIELD", "VALUE"], &rows)
}

/// Formats provenance links as a table.
fn format_provenance_table(links: &[ProvenanceLink]) -> String {
    let rows: Vec<[String; 3]> = links
        .iter()
        .map(|l| {
            let inputs = l
                .inputs
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            [l.producer.to_string(), inputs, l.output.to_string()]
        })
        .collect();

    format_table(&["PRODUCER", "INPUTS", "OUTPUT"], &rows)
}

/// Formats decision trails as a table.
fn format_trails_table(trails: &[DecisionTrail]) -> String {
    let rows: Vec<[String; 3]> = trails
        .iter()
        .map(|t| {
            [
                format!("{:?}", t.trail_id),
                t.graph_snapshot_id.clone(),
                format!("{} placements", t.placements.len()),
            ]
        })
        .collect();

    format_table(&["TRAIL_ID", "SNAPSHOT", "SUMMARY"], &rows)
}

/// Generic table formatter with aligned columns.
///
/// Computes column widths from the longest value in each column
/// (including the header), then prints rows with padding.
fn format_table<const N: usize>(headers: &[&str; N], rows: &[[String; N]]) -> String {
    let mut widths = [0usize; N];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.len();
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }

    let mut out = String::new();

    // Header
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&format!("{:width$}", h, width = widths[i]));
    }
    out.push('\n');

    // Separator
    for (i, _) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        out.push_str(&"-".repeat(widths[i]));
    }
    out.push('\n');

    // Rows
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            out.push_str(&format!("{:width$}", cell, width = widths[i]));
        }
        out.push('\n');
    }

    out
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_test_harness::WorkloadUnitBuilder;

    fn test_unit() -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().build())
    }

    #[test]
    fn test_format_units_table() {
        let unit = test_unit();
        let out = format_units(&[unit], OutputFormat::Table);
        assert!(out.contains("TYPE"), "table should contain TYPE header");
        assert!(out.contains("ID"), "table should contain ID header");
        assert!(
            out.contains("workload"),
            "table should contain workload type"
        );
    }

    #[test]
    fn test_format_units_json() {
        let unit = test_unit();
        let out = format_units(&[unit], OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
        assert!(parsed.is_array(), "units JSON should be an array");
    }

    #[test]
    fn test_format_stats_table() {
        let stats = GraphStats {
            active_units: 5,
            pending_units: 2,
            archived_units: 1,
            memory_bytes: 1024,
            memory_limit_bytes: 4096,
        };
        let out = format_stats(&stats, OutputFormat::Table);
        assert!(
            out.contains("active_units"),
            "table should contain active_units"
        );
        assert!(out.contains('5'), "table should contain the value 5");
    }

    #[test]
    fn test_format_stats_json() {
        let stats = GraphStats {
            active_units: 3,
            pending_units: 0,
            archived_units: 0,
            memory_bytes: 512,
            memory_limit_bytes: 4096,
        };
        let out = format_stats(&stats, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
        assert_eq!(parsed["active_units"], 3);
    }

    #[test]
    fn test_format_solver_result_table() {
        let result = SolverResult::empty();
        let out = format_solver_result(&result, OutputFormat::Table);
        assert!(
            out.contains("PLACEMENTS"),
            "table should contain placements section"
        );
        assert!(
            out.contains("CONFLICTS"),
            "table should contain conflicts section"
        );
    }

    #[test]
    fn test_format_solver_result_json() {
        let result = SolverResult::empty();
        let out = format_solver_result(&result, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
        assert!(parsed["placements"].is_array());
    }

    #[test]
    fn test_format_single_unit_json() {
        let unit = test_unit();
        let out = format_unit(&unit, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
        assert!(
            parsed.get("Workload").is_some(),
            "JSON should contain Workload variant"
        );
    }

    #[test]
    fn test_format_single_unit_table() {
        let unit = test_unit();
        let out = format_unit(&unit, OutputFormat::Table);
        assert!(out.contains("id"), "table should contain id field");
        assert!(out.contains("type"), "table should contain type field");
    }

    #[test]
    fn test_format_provenance_table() {
        let links = vec![ProvenanceLink {
            producer: taba_common::UnitId(uuid::Uuid::new_v4()),
            inputs: vec![taba_common::UnitId(uuid::Uuid::new_v4())],
            output: taba_common::UnitId(uuid::Uuid::new_v4()),
            timestamp: taba_common::DualClockEvent {
                logical_clock: taba_common::LogicalClock(1),
                wall_time: taba_common::WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
        }];
        let out = format_provenance(&links, OutputFormat::Table);
        assert!(
            out.contains("PRODUCER"),
            "table should contain PRODUCER header"
        );
    }

    #[test]
    fn test_format_trails_json() {
        let trails: Vec<DecisionTrail> = vec![];
        let out = format_trails(&trails, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("should be valid JSON");
        assert!(parsed.is_array(), "trails JSON should be an array");
    }

    #[test]
    fn test_format_units_empty_table() {
        let out = format_units(&[], OutputFormat::Table);
        assert!(out.contains("ID"), "even empty table should have headers");
    }
}
