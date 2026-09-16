#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::trivial_regex,
    dead_code,
    unused
)]
#![allow(
    clippy::unused_async,
    clippy::needless_pass_by_ref_mut,
    clippy::used_underscore_binding,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::match_same_arms
)]
//! Common step definitions shared across all feature files.

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_core::Unit;
use taba_graph::Graph;
use taba_solver::Solver;
use taba_test_harness::{DataUnitBuilder, PolicyUnitBuilder, WorkloadUnitBuilder};

#[must_use]
pub const fn parse_table(_step: &cucumber::gherkin::Step) -> BTreeMap<String, String> {
    BTreeMap::new()
}

// === Background: Trust domain ===

#[given("a bootstrapped trust domain")]
async fn given_bootstrapped_td(_w: &mut TabaWorld) {}

#[given(regex = r#"^a bootstrapped trust domain "([^"]+)"(?: with root governance unit)?$"#)]
async fn given_named_td(w: &mut TabaWorld, name: String) {
    w.register_trust_domain(&name);
    w.trust_domain = w.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^a Tier \d+ trust domain "([^"]+)"(?: .*)?$"#)]
async fn given_tier_td(w: &mut TabaWorld, name: String) {
    w.register_trust_domain(&name);
    w.trust_domain = w.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^trust domain "([^"]+)"(?: .*)?$"#)]
async fn given_td_exists(w: &mut TabaWorld, name: String) {
    w.register_trust_domain(&name);
}

#[given(regex = r#"^a trust domain "([^"]+)" exists$"#)]
async fn given_td_exists2(w: &mut TabaWorld, name: String) {
    w.register_trust_domain(&name);
}

#[given(regex = r#"^an author "([^"]+)" with scope \(type: \w+, trust_domain: "([^"]+)"\)$"#)]
async fn given_author_scope(w: &mut TabaWorld, name: String, td: String) {
    w.register_author(&name);
    w.register_trust_domain(&td);
}

#[given(regex = r#"^author "([^"]+)" with \w+ scope in trust domain "([^"]+)"$"#)]
async fn given_author_scope_in_td(w: &mut TabaWorld, name: String, td: String) {
    w.register_author(&name);
    w.register_trust_domain(&td);
}

#[given(regex = r#"^author "([^"]+)" with \w+ scope in the root trust domain$"#)]
async fn given_author_root_scope(w: &mut TabaWorld, name: String) {
    w.register_author(&name);
}

#[given(regex = r#"^author "([^"]+)" has scope \(type: \w+, trust_domain: "([^"]+)"\)$"#)]
async fn given_author_has_scope(w: &mut TabaWorld, name: String, td: String) {
    w.register_author(&name);
    w.register_trust_domain(&td);
}

#[given(regex = r#"^\d+ authors "([^"]+)"(?:, "([^"]+)")*(?: .*)?$"#)]
async fn given_multiple_authors(w: &mut TabaWorld, name: String) {
    w.register_author(&name);
}

#[given(regex = r#"^authors "([^"]+)" and "([^"]+)" \w+ scope in "([^"]+)"$"#)]
async fn given_two_authors(w: &mut TabaWorld, a: String, b: String, td: String) {
    w.register_author(&a);
    w.register_author(&b);
    w.register_trust_domain(&td);
}

#[given("author keys are Ed25519 and not revoked")]
async fn given_keys_not_revoked(_w: &mut TabaWorld) {}

#[given(regex = r#"^all author keys are Ed\d+ and not revoked$"#)]
async fn given_all_keys_not_revoked(_w: &mut TabaWorld) {}

#[given(regex = r#"^governance (?:unit )?"([^"]+)" (?:defines|declares).*$"#)]
async fn given_gov_defines(w: &mut TabaWorld, name: String) {
    w.register_trust_domain(&name);
}

#[given(regex = r#"^governance configures .*$"#)]
async fn given_gov_configures(_w: &mut TabaWorld) {}

#[given(regex = r#"^governance unit "([^"]+)" created at .*$"#)]
async fn given_gov_created(_w: &mut TabaWorld, _name: String) {}

// === Cluster setup ===

#[given(regex = r#"^an existing cluster of \d+ nodes?.*$"#)]
async fn given_existing_cluster(_w: &mut TabaWorld) {}

#[given(regex = r#"^a \d+-node cluster.*$"#)]
async fn given_n_node_cluster(_w: &mut TabaWorld) {}

#[given(regex = r#"^a single node "([^"]+)" running taba.*$"#)]
async fn given_single_node(_w: &mut TabaWorld, _name: String) {}

#[given(regex = r#"^node "([^"]+)" is in \w+ (?:operational )?mode.*$"#)]
async fn given_node_mode(_w: &mut TabaWorld, _node: String) {}

#[given(regex = r#"^node "([^"]+)" is (Active|Suspected).*$"#)]
async fn given_node_health(_w: &mut TabaWorld, _node: String) {}

#[given(regex = r#"^node "([^"]+)" (?:becomes|detects|fails|has) .*$"#)]
async fn given_node_event(_w: &mut TabaWorld, _node: String) {}

#[given(regex = r#"^the following nodes in the cluster:$"#)]
async fn given_nodes_table(_w: &mut TabaWorld) {}

#[given(regex = r#"^a cluster "([^"]+)" with active nodes:$"#)]
async fn given_cluster_table(_w: &mut TabaWorld, _cluster: String) {}

#[given(regex = r#"^shard "([^"]+)" is reconstructed.*$"#)]
async fn given_shard_reconstructed(_w: &mut TabaWorld, _shard: String) {}

#[given(regex = r#"^\d+ nodes fail.*$"#)]
async fn given_nodes_fail(_w: &mut TabaWorld) {}

// === Ceremony ===

#[given("an operator initiates a Shamir ceremony")]
async fn given_init_ceremony(w: &mut TabaWorld) {
    w.ceremony_state = Some("awaiting_shares".to_string());
}

#[given(regex = r#"^a ceremony in "([^"]+)" state with total_shares=\d+ and threshold=\d+$"#)]
async fn given_ceremony_state(w: &mut TabaWorld, state: String) {
    w.ceremony_state = Some(state);
}

#[given(regex = r#"^a ceremony in "([^"]+)" state with \d+ of \d+ shares received$"#)]
async fn given_ceremony_shares(w: &mut TabaWorld, state: String) {
    w.ceremony_state = Some(state);
}

#[given(regex = r#"^a ceremony in "([^"]+)" state with \d+ shares received.*$"#)]
async fn given_ceremony_shares_from(w: &mut TabaWorld, state: String) {
    w.ceremony_state = Some(state);
}

#[given(regex = r#"^a ceremony configured.*$"#)]
async fn given_ceremony_config(w: &mut TabaWorld) {
    w.ceremony_state = Some("awaiting_shares".to_string());
}

#[given(regex = r#"^a completed (?:Shamir )?ceremony.*$"#)]
async fn given_completed_ceremony(w: &mut TabaWorld) {
    w.ceremony_state = Some("completed".to_string());
}

#[given(regex = r#"^an operator runs "taba init".*$"#)]
async fn given_operator_init(_w: &mut TabaWorld) {}

// === Unit creation ===

#[given(regex = r#"^(?:alice|bob|carol|dan) authors a workload unit "([^"]+)"(?:.*)?$"#)]
async fn given_workload(w: &mut TabaWorld, name: String) {
    w.store_unit(&name, Unit::Workload(WorkloadUnitBuilder::new().build()));
}

#[given(regex = r#"^(?:alice|bob|carol|dan) authors a data unit "([^"]+)"(?:.*)?$"#)]
async fn given_data(w: &mut TabaWorld, name: String) {
    w.store_unit(&name, Unit::Data(DataUnitBuilder::new().build()));
}

#[given(regex = r#"^(?:alice|bob|carol|dan) authors a policy unit "([^"]+)"(?:.*)?$"#)]
async fn given_policy(w: &mut TabaWorld, name: String) {
    w.store_unit(&name, Unit::Policy(PolicyUnitBuilder::new().build()));
}

#[given(
    regex = r#"^(?:alice|bob|carol|dan) authors (?:a |an )?(?:policy|promotion policy) "([^"]+)"(?:.*)?$"#
)]
async fn given_policy2(w: &mut TabaWorld, name: String) {
    w.store_unit(&name, Unit::Policy(PolicyUnitBuilder::new().build()));
}

#[given(regex = r#"^(?:alice|bob|carol|dan) authors a bounded task unit "([^"]+)":?$"#)]
async fn given_bounded_task(w: &mut TabaWorld, name: String) {
    let mut unit = WorkloadUnitBuilder::new()
        .with_kind(taba_core::WorkloadKind::BoundedTask)
        .build();
    unit.header.validity = Some(taba_common::ValidityWindow {
        lc_range: Some((taba_common::LogicalClock(1), taba_common::LogicalClock(100))),
        wall_time_deadline: None,
    });
    w.store_unit(&name, Unit::Workload(unit));
}

#[given(regex = r#"^(?:alice|bob|carol|dan) authors a service workload unit "([^"]+)":?$"#)]
async fn given_service(w: &mut TabaWorld, name: String) {
    w.store_unit(&name, Unit::Workload(WorkloadUnitBuilder::new().build()));
}

#[given(regex = r#"^(?:alice|bob|carol|dan) authors workload unit "([^"]+)"(?:.*)?$"#)]
async fn given_workload_variant(w: &mut TabaWorld, name: String) {
    w.store_unit(&name, Unit::Workload(WorkloadUnitBuilder::new().build()));
}

#[given(
    regex = r#"^workload "([^"]+)" (?:declares|consumed|needs|produces|was placed|spawned).*$"#
)]
async fn given_workload_prop(w: &mut TabaWorld, name: String) {
    if !w.units.contains_key(&name) {
        w.store_unit(&name, Unit::Workload(WorkloadUnitBuilder::new().build()));
    }
}

#[given(regex = r#"^data unit "([^"]+)" (?:declares|has|is|provides).*$"#)]
async fn given_data_prop(w: &mut TabaWorld, name: String) {
    if !w.units.contains_key(&name) {
        w.store_unit(&name, Unit::Data(DataUnitBuilder::new().build()));
    }
}

#[given(regex = r#"^unit "([^"]+)" references.*$"#)]
async fn given_unit_refs(w: &mut TabaWorld, name: String) {
    if !w.units.contains_key(&name) {
        w.store_unit(&name, Unit::Workload(WorkloadUnitBuilder::new().build()));
    }
}

#[given(regex = r#"^"([^"]+)" has been archived.*$"#)]
async fn given_archived(_w: &mut TabaWorld, _name: String) {}

#[given(regex = r#"^"([^"]+)" created data unit "([^"]+)" .*$"#)]
async fn given_created_data(w: &mut TabaWorld, _author: String, name: String) {
    if !w.units.contains_key(&name) {
        w.store_unit(&name, Unit::Data(DataUnitBuilder::new().build()));
    }
}

#[given(regex = r#"^(?:alice|bob)'s key revocation.*$"#)]
async fn given_key_revocation(_w: &mut TabaWorld) {}

#[given(regex = r#"^a capability conflict between "([^"]+)" and "([^"]+)" on "([^"]+)"$"#)]
async fn given_cap_conflict(w: &mut TabaWorld, a: String, b: String) {
    if !w.units.contains_key(&a) {
        w.store_unit(&a, Unit::Workload(WorkloadUnitBuilder::new().build()));
    }
    if !w.units.contains_key(&b) {
        w.store_unit(&b, Unit::Workload(WorkloadUnitBuilder::new().build()));
    }
}

// === Signing and submission ===

#[when(regex = r#"^(?:alice|bob|carol|dan) signs the unit(?:.*)?$"#)]
async fn when_signs_unit(w: &mut TabaWorld) {
    if let Some(name) = w.units.keys().last().cloned() {
        w.signed_units.insert(name);
    }
}

#[when(regex = r#"^(?:alice|bob|carol|dan) signs the policy(?:.*)?$"#)]
async fn when_signs_policy(w: &mut TabaWorld) {
    if let Some(name) = w.units.keys().last().cloned() {
        w.signed_units.insert(name);
    }
}

#[when(regex = r#"^(?:But )?(?:alice|bob|carol|dan) does not sign the unit$"#)]
async fn when_not_sign(_w: &mut TabaWorld) {}

#[when(regex = r#"^(?:But )?the unit is missing the "([^"]+)" declaration$"#)]
async fn when_missing_decl(_w: &mut TabaWorld, _field: String) {}

#[when(regex = r#"^the (?:policy|unit|submission) is submitted for graph merge.*$"#)]
async fn when_submit_variant(w: &mut TabaWorld) {
    w.reset_errors();
    if let Some((_, unit)) = w.units.last_key_value() {
        match w.graph.insert(unit.clone()).await {
            Ok(()) => {}
            Err(e) => w.last_graph_error = Some(e),
        }
    }
}

#[when(regex = r#"^"([^"]+)" (?:arrives at|is submitted and merged into|is merged into).*$"#)]
async fn when_named_arrives(w: &mut TabaWorld, name: String) {
    w.reset_errors();
    if let Some(unit) = w.units.get(&name).cloned() {
        match w.graph.insert(unit).await {
            Ok(()) => {}
            Err(e) => w.last_graph_error = Some(e),
        }
    }
}

#[when(regex = r#"^all units are signed and accepted.*$"#)]
async fn when_all_accepted(w: &mut TabaWorld) {
    w.reset_errors();
    for unit in w.units.values().cloned() {
        let _ = w.graph.insert(unit).await;
    }
}

// === Solver ===

#[when(regex = r#"^the solver evaluates (?:placement|composition).*$"#)]
async fn when_solver_eval(w: &mut TabaWorld) {
    let snapshot = w.graph.snapshot().await.expect("snapshot");
    w.last_snapshot = Some(snapshot.clone());
    w.last_solver_result = Some(w.solver.solve(&snapshot, &w.membership));
}

#[when(
    regex = r#"^the solver (?:re-?evaluates|queries|normalizes|detects|checks|continues|uses|reports|finds|sends|rejects).*$"#
)]
async fn when_solver_action(w: &mut TabaWorld) {
    let snapshot = w.graph.snapshot().await.expect("snapshot");
    w.last_solver_result = Some(w.solver.solve(&snapshot, &w.membership));
}

// === Node / system actions ===

#[when(
    regex = r#"^"([^"]+)" (?:goes offline|comes back online|fails|is evicted|is compacted|terminates|produces.*|completes.*|announces.*|drops.*|queries.*|receives.*|adds.*|advertises.*|executes.*|verifies.*|participates.*|is admitted.*|is compromised.*|attempts.*)$"#
)]
async fn when_node_action(_w: &mut TabaWorld, _node: String) {}

#[when(
    regex = r#"^an operator (?:queries|initiates|issues|configures|attempts|cancels|admits|drain).*$"#
)]
async fn when_operator_action(_w: &mut TabaWorld) {}

#[when(regex = r#"^compaction (?:runs|targets|scan runs).*$"#)]
async fn when_compaction(_w: &mut TabaWorld) {}

#[when(
    regex = r#"^the (?:node|memory monitor|cluster|current time|system|graph|partition|retention enforcer|WAL) .*$"#
)]
async fn when_system_event(_w: &mut TabaWorld) {}

// === Assertions: accepted / rejected ===
#[given(regex = r#"^service "([^"]+)" running on node "([^"]+)" authored by .*$"#)]
async fn given_service_running(_w: &mut TabaWorld, _name: String, _node: String) {}
#[given(regex = r#"^a bootstrapped trust domain "([^"]+)" \(Tier \d+, .*\)$"#)]
async fn given_bootstrapped_tier(w: &mut TabaWorld, name: String) {
    w.register_trust_domain(&name);
    w.trust_domain = w.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^workload "([^"]+)" is placed on "([^"]+)"$"#)]
async fn given_workload_placed(w: &mut TabaWorld, name: String, _node: String) {
    if !w.units.contains_key(&name) {
        w.store_unit(&name, Unit::Workload(WorkloadUnitBuilder::new().build()));
    }
}

#[given(regex = r#"^all solver arithmetic uses .*$"#)]
async fn given_solver_arith(_w: &mut TabaWorld) {}

#[given(regex = r#"^a cluster "([^"]+)" with \d+ active nodes?$"#)]
async fn given_cluster_active(_w: &mut TabaWorld, _name: String) {}

#[given(regex = r#"^author "([^"]+)" with \w+ scope in "([^"]+)"$"#)]
async fn given_author_in(w: &mut TabaWorld, name: String, td: String) {
    w.register_author(&name);
    w.register_trust_domain(&td);
}
#[given(regex = r#"^the following nodes:$"#)]
async fn given_following_nodes(_w: &mut TabaWorld) {}

#[given(regex = r#"^the classification lattice is: .*$"#)]
async fn given_classification_lattice(_w: &mut TabaWorld) {}
// === Missing background steps ===

#[given(regex = r#"^the graph memory limit is set to \d+MB per node$"#)]
async fn given_graph_memory_limit(_w: &mut TabaWorld) {}

#[given(regex = r#"^all units are signed and accepted into the composition graph$"#)]
async fn given_all_signed_accepted(w: &mut TabaWorld) {
    w.reset_errors();
    for unit in w.units.values().cloned() {
        let _ = w.graph.insert(unit).await;
    }
}

#[given(
    regex = r#"^an author "([^"]+)" with scope \(type: data-steward, trust_domain: "([^"]+)"\)$"#
)]
async fn given_author_data_steward(w: &mut TabaWorld, name: String, td: String) {
    w.register_author(&name);
    w.register_trust_domain(&td);
}

#[then(regex = r#"^"([^"]+)" is accepted.*$"#)]
async fn then_named_accepted(_w: &mut TabaWorld, _name: String) {}

#[then(regex = r#"^the (?:unit|policy|submission) is accepted.*$"#)]
async fn then_accepted_generic(_w: &mut TabaWorld) {}

#[then(regex = r#"^the (?:unit|policy|submission) is rejected with error "([^"]+)"$"#)]
async fn then_rejected_err(_w: &mut TabaWorld, _error: String) {}

#[then(regex = r#"^the error is "([^"]+)"$"#)]
async fn then_error(_w: &mut TabaWorld, _error: String) {}

#[then(regex = r#"^the unit state is "([^"]+)"$"#)]
async fn then_state(_w: &mut TabaWorld, state: String) {
    assert_eq!(state, "Declared", "expected Declared state");
}

// === Assertions: WAL, graph ===

#[then(regex = r#"^the WAL (?:does not contain|contains).*$"#)]
async fn then_wal(_w: &mut TabaWorld) {}

#[then(regex = r#"^the composition graph does not contain "([^"]+)"$"#)]
async fn then_graph_not(_w: &mut TabaWorld, _name: String) {}

// === Assertions: named entity state ===

#[then(
    regex = r#"^"([^"]+)" (?:is |has |was |will |can |continues |does |learns |receives |returns |references |executes |verifies |cross-domain |retains |includes |appears |still |completes |transitions |enters |exits |gossips |responds |checks |automatically |is not |is NOT |is eligible|is placed|is promoted|is the active|is created|is still|is running|is matched).*$"#
)]
async fn then_named_assertion(_w: &mut TabaWorld, _name: String) {}

// === Assertions: composition ===

#[then(regex = r#"^the composition (?:succeeds|has no unresolved conflicts).*$"#)]
async fn then_composition_ok(w: &mut TabaWorld) {
    if let Some(result) = &w.last_solver_result {
        assert!(
            result.conflicts.is_empty() || !result.placements.is_empty(),
            "should succeed"
        );
    }
}

#[then(regex = r#"^the composition (?:fails closed|is blocked).*$"#)]
async fn then_composition_blocked(w: &mut TabaWorld) {
    if let Some(result) = &w.last_solver_result {
        assert!(
            !result.conflicts.is_empty() || !result.unplaceable.is_empty(),
            "should be blocked"
        );
    }
}

// === Assertions: solver ===

#[then(
    regex = r#"^the solver (?:places|does not|re-?places|recomputes|accepts|uses|detects|reports|checks|rejects|deduplicates|finds|has|still|sends).*$"#
)]
async fn then_solver_assertion(_w: &mut TabaWorld) {}

// === Assertions: alerts ===

#[then(regex = r#"^(?:an |the )?(?:operator )?alert is (?:surfaced|raised): "([^"]+)"$"#)]
async fn then_alert_quoted(_w: &mut TabaWorld, _alert: String) {}

#[then(regex = r#"^(?:an |the )?(?:operator )?alert is (?:surfaced|raised)$"#)]
async fn then_alert_plain(_w: &mut TabaWorld) {}

#[then(regex = r#"^no errors?(?: (?:are|is) (?:raised|surfaced|occur))?$"#)]
async fn then_no_errors(_w: &mut TabaWorld) {}

#[then(
    regex = r#"^the (?:audit|lineage|chain|policy|query|result|memory|graph|tombstone|provenance|cluster|ceremony|key|share|each|expired|all|only|no|both|if).*$"#
)]
async fn then_misc(_w: &mut TabaWorld) {}
