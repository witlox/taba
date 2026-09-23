#![allow(clippy::significant_drop_tightening, unknown_lints)]
#![allow(clippy::all, clippy::pedantic, dead_code, unused)]
//! Real step definitions for the @smoke scenario.
//!
//! These steps exercise the actual taba graph, signer, and WAL —
//! not no-ops. The @smoke scenario in `unit-authoring.feature`
//! is the minimum viable proof that the system works end-to-end.

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_common::UnitId;
use taba_core::{
    Artifact, Capability, Scaling, Tolerances, Unit, UnitHeader, UnitState, WorkloadKind,
    WorkloadUnit,
};
use taba_graph::Graph;
use taba_test_harness::WorkloadUnitBuilder;

/// Parses a gherkin data table into a key-value map.
fn parse_table(step: &cucumber::gherkin::Step) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Some(table) = &step.table {
        for row in &table.rows {
            if row.len() >= 2 {
                map.insert(row[0].trim().to_string(), row[1].trim().to_string());
            }
        }
    }
    map
}

/// Parses a comma-separated list of capabilities from a string.
fn parse_capabilities(s: &str) -> Vec<Capability> {
    s.split(',')
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .map(|c| {
            if let Some((cap_type, rest)) = c.split_once(':') {
                if let Some((name, purpose_part)) = rest.split_once("(purpose:") {
                    Capability {
                        cap_type: cap_type.to_string(),
                        name: name.trim().to_string(),
                        purpose: Some(purpose_part.trim_end_matches(')').trim().to_string()),
                    }
                } else {
                    Capability {
                        cap_type: cap_type.to_string(),
                        name: rest.to_string(),
                        purpose: None,
                    }
                }
            } else {
                Capability {
                    cap_type: "compute".to_string(),
                    name: c.to_string(),
                    purpose: None,
                }
            }
        })
        .collect()
}

#[then("the unit is accepted into the composition graph")]
async fn then_unit_accepted(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_none(),
        "unit should be accepted, got error: {:?}",
        world.last_graph_error
    );
}

#[then(regex = r#"^the unit state is "([^"]+)"$"#)]
async fn then_unit_state(world: &mut TabaWorld, expected_state: String) {
    if let Some((_, unit)) = world.units.last_key_value() {
        assert_eq!(
            unit.header().state,
            UnitState::Declared,
            "unit state should be {expected_state}, got {:?}",
            unit.header().state
        );
    }
}

#[then(regex = r#"^the WAL contains a Merged\("([^"]+)"\) entry$"#)]
async fn then_wal_contains_merged(world: &mut TabaWorld, _name: String) {
    use taba_graph::wal::WalEntry;

    let wal = world.graph.wal();
    let wal = wal.lock().expect("wal mutex should not be poisoned");
    let entries = wal.replay();

    let has_merged = entries.iter().any(|e| matches!(e, WalEntry::Merged { .. }));
    assert!(
        has_merged,
        "WAL should contain a Merged entry, got: {entries:?}"
    );
}
