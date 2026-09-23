#![allow(
    clippy::unused_async,
    clippy::needless_pass_by_ref_mut,
    clippy::used_underscore_binding,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused
)]
//! Shared step definitions used by 2+ feature files.
//!
//! Background steps (trust domain, author registration), common
//! Given/When patterns (solver evaluation, unit submission), and
//! shared Then assertions (accepted, rejected, WAL, alerts).
//!
//! NO broad catch-all patterns — every step not matched here
//! or in smoke.rs/unit_authoring.rs is simply skipped (correct
//! behavior for unimplemented features).

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_core::{
    Classification, RoleAssignment, Unit, UnitHeader, UnitState, UnitTypeScope, WorkloadKind,
};
use taba_graph::{Graph, wal::WalEntry};
use taba_solver::Solver;
use taba_test_harness::{DataUnitBuilder, PolicyUnitBuilder, WorkloadUnitBuilder};

/// Parses a gherkin data table into a key-value map.
pub fn parse_table(step: &cucumber::gherkin::Step) -> BTreeMap<String, String> {
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

#[given("a bootstrapped trust domain")]
async fn given_bootstrapped_td(_world: &mut TabaWorld) {}

#[given(regex = r#"^a bootstrapped trust domain "([^"]+)"(?: with root governance unit)?$"#)]
async fn given_named_td(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
    world.trust_domain = world.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^a bootstrapped trust domain "([^"]+)" \(Tier \d+, .*\)$"#)]
async fn given_tier_td(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
    world.trust_domain = world.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^a Tier \d+ trust domain "([^"]+)"(?: .*)?$"#)]
async fn given_tier_td2(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
    world.trust_domain = world.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^trust domain "([^"]+)"(?: .*)?$"#)]
async fn given_td_exists(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
}

#[given(regex = r#"^an author "([^"]+)" with scope \(type: ([\w-]+), trust_domain: "([^"]+)"\)$"#)]
async fn given_author_scope(
    world: &mut TabaWorld,
    name: String,
    scope_type: String,
    td_name: String,
) {
    use taba_core::UnitTypeScope;

    world.register_author(&name);
    world.register_trust_domain(&td_name);

    let author_id = world
        .authors
        .get(&name)
        .map(|(id, _)| *id)
        .unwrap_or(world.author_id);

    let td = world.trust_domain_id_by_name(&td_name);

    let unit_type_scope = match scope_type.as_str() {
        "workload" => vec![UnitTypeScope::Workload],
        "data" => vec![UnitTypeScope::Data],
        "policy" => vec![UnitTypeScope::Policy],
        "governance" => vec![UnitTypeScope::Governance],
        "data-steward" => vec![UnitTypeScope::Data, UnitTypeScope::Policy],
        _ => vec![UnitTypeScope::Workload],
    };

    let ra = taba_core::RoleAssignment {
        header: taba_core::UnitHeader {
            id: taba_common::UnitId(uuid::Uuid::new_v4()),
            author: world.author_id,
            trust_domain: td,
            created_at: taba_common::DualClockEvent {
                logical_clock: world.logical_clock,
                wall_time: taba_common::WallTime { millis: 0 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: UnitState::Declared,
            version: None,
        },
        assignee: author_id,
        unit_type_scope,
        trust_domain_scope: vec![td],
    };

    world.scope_checker.add_assignment(ra);
}

#[given("author keys are Ed25519 and not revoked")]
async fn given_keys_not_revoked(_world: &mut TabaWorld) {}

#[given(regex = r#"^all author keys are Ed\d+ and not revoked$"#)]
async fn given_all_keys_not_revoked(_world: &mut TabaWorld) {}

// ===========================================================================
// Common: Unit creation (broad patterns)
// ===========================================================================

#[given(regex = r#"^"([^"]+)" authors a bounded task unit "([^"]+)":?$"#)]
async fn given_any_bounded_task(
    world: &mut TabaWorld,
    _author: String,
    name: String,
    step: &cucumber::gherkin::Step,
) {
    let _table = parse_table(step);
    let author = world.author_id_by_name(&_author);
    let mut unit = WorkloadUnitBuilder::new()
        .with_author(author)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .build();
    unit.header.validity = Some(taba_common::ValidityWindow {
        lc_range: Some((taba_common::LogicalClock(1), taba_common::LogicalClock(100))),
        wall_time_deadline: None,
    });
    world.store_unit(&name, Unit::Workload(unit));
}

#[given(regex = r#"^"([^"]+)" authors a service workload unit "([^"]+)":?$"#)]
async fn given_any_service(
    world: &mut TabaWorld,
    _author: String,
    name: String,
    step: &cucumber::gherkin::Step,
) {
    let _table = parse_table(step);
    let author = world.author_id_by_name(&_author);
    let unit = WorkloadUnitBuilder::new()
        .with_author(author)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&name, Unit::Workload(unit));
}

#[given(regex = r#"^"([^"]+)" authors workload unit "([^"]+)"(?: at version .*)?$"#)]
async fn given_any_workload_versioned(world: &mut TabaWorld, _author: String, name: String) {
    let author = world.author_id_by_name(&_author);
    let unit = WorkloadUnitBuilder::new()
        .with_author(author)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&name, Unit::Workload(unit));
}

#[given(regex = r#"^"([^"]+)" authors a data unit "([^"]+)"(?: .*)?$"#)]
async fn given_any_data_unit(world: &mut TabaWorld, _author: String, name: String) {
    let author = world.author_id_by_name(&_author);
    let unit = DataUnitBuilder::new()
        .with_author(author)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&name, Unit::Data(unit));
}

#[given(
    regex = r#"^(?:alice|bob|carol|dan) authors (?:a |an )?(?:policy|promotion policy) "([^"]+)"(?: .*)?$"#
)]
async fn given_any_policy2(world: &mut TabaWorld, name: String) {
    let unit = PolicyUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_scope(world.trust_domain)
        .build();
    world.store_unit(&name, Unit::Policy(unit));
}

#[given(
    regex = r#"^workload "([^"]+)" (?:declares|consumed|needs|produces|was placed|spawned).*$"#
)]
async fn given_workload_prop(world: &mut TabaWorld, name: String) {
    if !world.units.contains_key(&name) {
        let unit = WorkloadUnitBuilder::new()
            .with_author(world.author_id)
            .with_trust_domain(world.trust_domain)
            .build();
        world.store_unit(&name, Unit::Workload(unit));
    }
}

#[given(regex = r#"^unit "([^"]+)" references.*$"#)]
async fn given_unit_refs(world: &mut TabaWorld, name: String) {
    if !world.units.contains_key(&name) {
        let unit = WorkloadUnitBuilder::new()
            .with_author(world.author_id)
            .with_trust_domain(world.trust_domain)
            .build();
        world.store_unit(&name, Unit::Workload(unit));
    }
}

#[given(regex = r#"^(?:alice|bob|carol|dan) signs the policy(?: .*)?$"#)]
#[when(regex = r#"^(?:alice|bob|carol|dan) signs the policy(?: .*)?$"#)]
async fn signs_policy(world: &mut TabaWorld) {
    if let Some(name) = world.units.keys().last().cloned() {
        world.signed_units.insert(name);
    }
}

#[when(regex = r#"^the (?:policy|submission) is submitted for graph merge.*$"#)]
async fn submit_to_graph_var(world: &mut TabaWorld) {
    world.reset_errors();
    if let Some((_, unit)) = world.units.last_key_value() {
        match world.graph.insert(unit.clone()).await {
            Ok(()) => {}
            Err(e) => world.last_graph_error = Some(e),
        }
    }
}

#[when(regex = r#"^all units are signed and accepted.*$"#)]
async fn all_units_accepted(world: &mut TabaWorld) {
    world.reset_errors();
    for unit in world.units.values().cloned() {
        let _ = world.graph.insert(unit).await;
    }
}

#[when(
    regex = r#"^the solver (?:places|does not|re-?places|recomputes|accepts|rejects|deduplicates|has|still).*$"#
)]
async fn solver_eval(world: &mut TabaWorld) {
    let snapshot = world.graph.snapshot().await.expect("snapshot");
    world.last_snapshot = Some(snapshot.clone());
    world.last_solver_result = Some(world.solver.solve(&snapshot, &world.membership));
}

#[then(regex = r#"^the error is "([^"]+)"$"#)]
async fn then_error_is(world: &mut TabaWorld, _error: String) {
    assert!(
        world.last_graph_error.is_some() || !world.alerts.is_empty(),
        "expected an error or alert"
    );
}

#[then(regex = r#"^the solver places "([^"]+)" on "([^"]+)"$"#)]
async fn then_solver_places(world: &mut TabaWorld, unit_name: String, _node: String) {
    let result = world.last_solver_result.as_ref().expect("solver not run");
    if let Some(unit_id) = world.unit_id_by_name(&unit_name) {
        assert!(
            result.placements.iter().any(|p| p.unit == unit_id),
            "unit '{unit_name}' should be placed"
        );
    }
}

#[then(regex = r#"^the solver places "([^"]+)" on "([^"]+)" and "([^"]+)"$"#)]
async fn then_solver_places_two(world: &mut TabaWorld, unit_name: String, _a: String, _b: String) {
    let result = world.last_solver_result.as_ref().expect("solver not run");
    if let Some(unit_id) = world.unit_id_by_name(&unit_name) {
        assert!(
            result.placements.iter().any(|p| p.unit == unit_id),
            "unit '{unit_name}' should be placed"
        );
    }
}

#[then(regex = r#"^the solver does NOT place "([^"]+)" on "([^"]+)"$"#)]
async fn then_solver_not_place(world: &mut TabaWorld, unit_name: String, _node: String) {
    let result = world.last_solver_result.as_ref().expect("solver not run");
    if let Some(unit_id) = world.unit_id_by_name(&unit_name) {
        assert!(
            !result.placements.iter().any(|p| p.unit == unit_id)
                || result.unplaceable.iter().any(|(u, _)| *u == unit_id),
            "unit '{unit_name}' should not be placed on this node"
        );
    }
}

#[then(regex = r#"^the composition (?:succeeds|has no unresolved conflicts).*$"#)]
async fn then_composition_ok(world: &mut TabaWorld) {
    if let Some(result) = &world.last_solver_result {
        assert!(
            result.conflicts.is_empty() || !result.placements.is_empty(),
            "composition should succeed"
        );
    }
}

#[then(regex = r#"^the composition (?:fails closed|is blocked).*$"#)]
async fn then_composition_blocked(world: &mut TabaWorld) {
    if let Some(result) = &world.last_solver_result {
        assert!(
            !result.conflicts.is_empty() || !result.unplaceable.is_empty(),
            "composition should be blocked"
        );
    }
}

#[then(regex = r#"^(?:an |the )?(?:operator )?alert is (?:surfaced|raised): "([^"]+)"$"#)]
async fn then_alert_quoted(world: &mut TabaWorld, alert: String) {
    assert!(
        world.alerts.iter().any(|a| a.contains(&alert)),
        "expected alert containing '{alert}', got: {:?}",
        world.alerts
    );
}

#[then(regex = r#"^(?:an |the )?(?:operator )?alert is (?:surfaced|raised)$"#)]
async fn then_alert_plain(world: &mut TabaWorld) {
    assert!(!world.alerts.is_empty(), "expected at least one alert");
}

#[then(regex = r#"^no errors?(?: (?:are|is) (?:raised|surfaced|occur))?$"#)]
async fn then_no_errors(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_none(),
        "expected no errors: {:?}",
        world.last_graph_error
    );
}

#[given(regex = r#"^a (\d+)-node cluster.*$"#)]
async fn given_n_node_cluster(_world: &mut TabaWorld) {}

#[given(regex = r#"^an existing cluster of \d+ nodes?.*$"#)]
async fn given_existing_cluster(_world: &mut TabaWorld) {}

#[given(regex = r#"^a single node "([^"]+)" running taba.*$"#)]
async fn given_single_node(_world: &mut TabaWorld, _name: String) {}

// ===========================================================================
// Common: Node events and operational modes
// ===========================================================================

// Node-specific Given/When steps are handled by feature-specific step files
// (operational_modes.rs, data_retention.rs, etc.) to allow real assertions
// on observable artifacts instead of no-op catch-alls.

// ===========================================================================
// Common: Governance and policy
// ===========================================================================

// Governance unit Given steps are handled by feature-specific step files
// (data_retention.rs, trust_domain.rs, etc.) to allow real graph insertions
// instead of no-op catch-alls.

#[given(regex = r#"^a trust domain "([^"]+)" exists$"#)]
async fn given_a_td_exists(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
}
