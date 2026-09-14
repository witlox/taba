#![allow(clippy::format_push_string)]
//! Conversion report: what was converted, what couldn't be mapped.

use std::collections::BTreeMap;

/// A resource that could not be mapped to a taba unit.
#[derive(Debug, Clone)]
pub struct UnmappableResource {
    /// K8s resource kind.
    pub kind: String,
    /// K8s resource name.
    pub name: String,
    /// Why it couldn't be mapped.
    pub reason: String,
    /// Suggested manual action.
    pub suggestion: String,
}

/// Result of converting K8s manifests to taba units.
#[derive(Debug, Clone, Default)]
pub struct ConversionReport {
    /// Successfully generated taba unit TOML files.
    /// Key: unit name, Value: TOML content.
    pub generated: BTreeMap<String, String>,
    /// Resources that could not be mapped.
    pub unmappable: Vec<UnmappableResource>,
    /// Warnings (non-fatal, but operator should review).
    pub warnings: Vec<String>,
    /// K8s resources that were skipped (e.g., empty `ConfigMaps`).
    pub skipped: Vec<String>,
}

impl ConversionReport {
    /// Creates a new empty report.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Total number of resources processed.
    #[must_use]
    pub fn total_processed(&self) -> usize {
        self.generated.len() + self.unmappable.len() + self.skipped.len()
    }

    /// Number of resources that generated units.
    #[must_use]
    pub fn generated_count(&self) -> usize {
        self.generated.len()
    }

    /// Number of resources that could not be mapped.
    #[must_use]
    pub fn unmappable_count(&self) -> usize {
        self.unmappable.len()
    }

    /// Returns true if all resources were successfully converted.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.unmappable.is_empty() && self.warnings.is_empty()
    }

    /// Formats the report as a human-readable string.
    #[must_use]
    pub fn format(&self) -> String {
        let mut out = String::new();
        out.push_str("=== K8s → taba Conversion Report ===\n\n");

        out.push_str(&format!("Generated: {} units\n", self.generated_count()));
        for name in self.generated.keys() {
            out.push_str(&format!("  ✓ {name}\n"));
        }

        if !self.skipped.is_empty() {
            out.push_str(&format!("\nSkipped: {} resources\n", self.skipped.len()));
            for name in &self.skipped {
                out.push_str(&format!("  ⊘ {name}\n"));
            }
        }

        if !self.warnings.is_empty() {
            out.push_str(&format!("\nWarnings: {}\n", self.warnings.len()));
            for warning in &self.warnings {
                out.push_str(&format!("  ⚠ {warning}\n"));
            }
        }

        if !self.unmappable.is_empty() {
            out.push_str(&format!(
                "\nUnmappable: {} resources\n",
                self.unmappable_count()
            ));
            for resource in &self.unmappable {
                out.push_str(&format!(
                    "  ✗ {}/{}: {}\n    → {}\n",
                    resource.kind, resource.name, resource.reason, resource.suggestion
                ));
            }
        }

        out.push_str(&format!("\nTotal processed: {}\n", self.total_processed()));
        out
    }
}
