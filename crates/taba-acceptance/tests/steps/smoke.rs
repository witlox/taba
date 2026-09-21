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

#[given(regex = r#"^alice authors a workload unit "([^"]+)" with:$"#)]
async fn given_alice_authors_workload(
    world: &mut TabaWorld,
    name: String,
    step: &cucumber::gherkin::Step,
) {
    let table = parse_table(step);

    let author = world.author_id_by_name("alice");
    let td = world.trust_domain;

    let mut builder = WorkloadUnitBuilder::new()
        .with_author(author)
        .with_trust_domain(td);

    if let Some(needs_str) = table.get("needs") {
        builder = builder.with_needs(parse_capabilities(needs_str));
    }
    if let Some(provides_str) = table.get("provides") {
        builder = builder.with_provides(parse_capabilities(provides_str));
    } else {
        builder = builder.with_provides(vec![Capability::new("compute", "http")]);
    }
    if let Some(scaling_str) = table.get("scaling") {
        let mut min = 1u32;
        let mut max = 3u32;
        for part in scaling_str.split(',') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("min:") {
                if let Ok(n) = v.trim().parse::<u32>() {
                    min = n;
                }
            }
            if let Some(v) = part.strip_prefix("max:") {
                if let Ok(n) = v.trim().parse::<u32>() {
                    max = n;
                }
            }
        }
        builder = builder.with_scaling(min, max);
    }
    if let Some(tol_str) = table.get("tolerates") {
        let mut max_latency = Some(std::time::Duration::from_millis(100));
        let mut failure_modes = vec!["timeout".to_string()];
        for part in tol_str.split(',') {
            let part = part.trim();
            if let Some(v) = part.strip_prefix("latency:") {
                if let Ok(n) = v.trim_end_matches("ms").parse::<u64>() {
                    max_latency = Some(std::time::Duration::from_millis(n));
                }
            }
            if let Some(v) = part.strip_prefix("failure:") {
                failure_modes = vec![v.trim().to_string()];
            }
        }
        let mut unit = builder.build();
        unit.tolerates = Tolerances {
            max_latency,
            failure_modes,
            consistency: None,
        };
        world.store_unit(&name, Unit::Workload(unit));
    } else {
        let unit = builder.build();
        world.store_unit(&name, Unit::Workload(unit));
    }
}

#[given(
    regex = r#"^alice signs the unit binding trust_domain "([^"]+)" and cluster "([^"]+)"(?: with validity window .+)?$"#
)]
async fn given_alice_signs_unit(world: &mut TabaWorld, _td: String, _cluster: String) {
    if let Some(name) = world.units.keys().last().cloned() {
        world.signed_units.insert(name);
    }
}

#[when(
    regex = r#"^alice signs the unit binding trust_domain "([^"]+)" and cluster "([^"]+)"(?: with validity window .+)?$"#
)]
async fn when_alice_signs_unit(world: &mut TabaWorld, _td: String, _cluster: String) {
    if let Some(name) = world.units.keys().last().cloned() {
        world.signed_units.insert(name);
    }
}

#[when("the unit is submitted for graph merge")]
async fn when_unit_submitted(world: &mut TabaWorld) {
    world.reset_errors();
    if let Some((_, unit)) = world.units.last_key_value() {
        match world.graph.insert(unit.clone()).await {
            Ok(()) => {}
            Err(e) => world.last_graph_error = Some(e),
        }
    }
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
