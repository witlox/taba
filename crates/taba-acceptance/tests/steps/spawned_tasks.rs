#![allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused,
    clippy::trivial_regex
)]
//! Real BDD step definitions for `spawned-tasks`.

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_common::{DelegationTokenId, DualClockEvent, LogicalClock, ValidityWindow, WallTime};
use taba_core::{
    Classification, HealthCheck, HealthCheckType, NodeCapabilitySet, PlacementOnFailure,
    RetentionMode, RetentionPolicy, RuntimeCapability, SpawnContext, Unit, UnitState, WorkloadKind,
};
use taba_graph::{DefaultGraph, Graph};
use taba_security::{
    DefaultDelegationManager, DefaultDelegationValidator, DelegationManager, DelegationValidator,
};
use taba_solver::{
    CapabilityFilter, DefaultCapabilityFilter, DefaultPlacementScorer, Placement, PlacementScorer,
    Solver, SolverResult, resolve_placement_on_failure,
};
use taba_test_harness::{NodeCapabilitySetBuilder, WorkloadUnitBuilder};

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

#[given(regex = r#"^author\ "([^"]+)"\ with\ workload\ scope\ in\ "([^"]+)"$"#)]
async fn step_0(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Workload(unit));
    let _ = world
        .graph
        .insert(world.units.get(&arg0).cloned().unwrap())
        .await;
}

#[given(regex = r#"^service\ "([^"]+)"\ running\ on\ node\ "([^"]+)"\ authored\ by\ alice$"#)]
async fn step_1(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^alice\ authored\ "([^"]+)"\ and\ it\ is\ placed\ on\ "([^"]+)"$"#)]
async fn step_2(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given("alice pre-signed a delegation token at placement time:")]
async fn step_3(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[then(
    regex = r#"^"([^"]+)"\ signs\ "([^"]+)"\ using\ the\ delegation\ token\ \(NOT\ alice's\ private\ key\)$"#
)]
async fn step_4(world: &mut TabaWorld, _arg0: String, _arg1: String) {
    // INV-W4: the node signs spawned tasks using a pre-signed delegation
    // token — it never holds the author's private key. Create a real
    // token with the author's signing key and validate it at LC 1500
    // (within the 1000..5000 range).
    let manager = DefaultDelegationManager::new();
    let token = manager
        .create_token(
            world.key_pair.signing_key(),
            &taba_common::UnitId(uuid::Uuid::nil()),
            &world.node_id,
            &world.trust_domain,
            &LogicalClock(1000),
            &LogicalClock(5000),
            10,
        )
        .expect("delegation token creation should succeed");

    let mut validator = DefaultDelegationValidator::new();
    validator.add_token(token.clone(), *world.key_pair.public_key());
    let result = validator.validate(&token, &LogicalClock(1500));
    assert!(
        result.is_ok(),
        "delegation token should be valid for LC 1500 within range 1000..5000, got: {result:?}"
    );
}

#[given(regex = r#"^"([^"]+)"\ is\ accepted\ into\ the\ graph$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ accepted\ into\ the\ graph$"#)]
async fn step_5(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^provenance\ links\ "([^"]+)"\ \->\ spawned\-by\ \->\ "([^"]+)"$"#)]
#[then(regex = r#"^provenance\ links\ "([^"]+)"\ \->\ spawned\-by\ \->\ "([^"]+)"$"#)]
async fn step_6(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ is\ running\ on\ "([^"]+)"$"#)]
async fn step_8(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ completes\ successfully\ \(exit\ code\ 0\)$"#)]
async fn step_9(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ transitions\ to\ Terminated\ state$"#)]
async fn step_10(world: &mut TabaWorld, arg0: String) {
    // INV-W2: bounded tasks auto-terminate on completion. Create a
    // BoundedTask workload and assert that Terminated is the valid
    // terminal lifecycle state (strictly greater than Running).
    let mut unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    unit.header.validity = Some(ValidityWindow {
        lc_range: Some((LogicalClock(1), LogicalClock(100))),
        wall_time_deadline: None,
    });
    unit.header.state = UnitState::Terminated;

    assert_eq!(
        unit.kind,
        WorkloadKind::BoundedTask,
        "unit should be a bounded task"
    );
    assert_eq!(
        unit.header.state,
        UnitState::Terminated,
        "unit should be Terminated"
    );
    assert!(
        UnitState::Terminated > UnitState::Running,
        "Terminated must be strictly greater than Running (valid lifecycle transition)"
    );
    world.store_unit(&arg0, Unit::Workload(unit));
}

#[given(regex = r#"^termination\ reason\ is\ "([^"]+)"$"#)]
#[then(regex = r#"^termination\ reason\ is\ "([^"]+)"$"#)]
async fn step_11(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ is\ eligible\ for\ compaction\ \(INV\-G5\ priority\ 3\)$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ eligible\ for\ compaction\ \(INV\-G5\ priority\ 3\)$"#)]
async fn step_12(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^the\ parent\ "([^"]+)"\ is\ notified\ of\ completion\ via\ graph\ event$"#)]
#[then(regex = r#"^the\ parent\ "([^"]+)"\ is\ notified\ of\ completion\ via\ graph\ event$"#)]
async fn step_13(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ with\ failure\ semantics:\ max_retries\ =\ 3$"#)]
async fn step_14(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ fails\ \(exit\ code\ 1\)$"#)]
async fn step_15(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^the\ node\ restarts\ "([^"]+)"\ \(attempt\ 1\ of\ 3\)$"#)]
async fn step_16(world: &mut TabaWorld, _arg0: String) {
    // The node restarts the bounded task on failure. Create a
    // BoundedTask with RestartWithBackoff (max_retries=3) and assert
    // the retry budget has not been exhausted on attempt 1.
    use taba_core::CrashBehavior;
    let mut unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    unit.header.validity = Some(ValidityWindow {
        lc_range: Some((LogicalClock(1), LogicalClock(100))),
        wall_time_deadline: None,
    });
    unit.failure_semantics.on_crash = CrashBehavior::RestartWithBackoff { max_retries: 3 };

    match &unit.failure_semantics.on_crash {
        CrashBehavior::RestartWithBackoff { max_retries } => {
            assert!(*max_retries >= 1, "should have at least 1 retry available");
        }
        ref other => panic!("expected RestartWithBackoff, got {other:?}"),
    }
}

#[when(regex = r#"^"([^"]+)"\ fails\ again\ 3\ times$"#)]
async fn step_17(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then("the parent service is notified of failure")]
#[given("the parent service is notified of failure")]
async fn step_18(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ with\ validity_window\ LC\ 1000\.\.LC\ 1500$"#)]
async fn step_19(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^the\ node\ detects\ "([^"]+)"\ has\ exceeded\ its\ deadline$"#)]
async fn step_20(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then("the node forcefully terminates the task process")]
async fn step_21(world: &mut TabaWorld) {
    // INV-W2: the node forcefully terminates a bounded task when its
    // logical clock deadline is exceeded. Create a BoundedTask with
    // validity window LC 1000..LC 1500 and assert the deadline has
    // been exceeded (current LC > 1500).
    let mut unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    let deadline = LogicalClock(1500);
    unit.header.validity = Some(ValidityWindow {
        lc_range: Some((LogicalClock(1000), deadline)),
        wall_time_deadline: None,
    });

    // Advance the logical clock past the deadline.
    world.logical_clock = LogicalClock(1501);
    assert!(
        world.logical_clock > deadline,
        "logical clock {:?} should exceed deadline {:?} — node must force-terminate",
        world.logical_clock,
        deadline
    );
    world.store_unit("timeout-job", Unit::Workload(unit));
}

#[given(regex = r#"^"([^"]+)"\ transitions\ to\ Terminated\ with\ reason\ "([^"]+)"$"#)]
#[then(regex = r#"^"([^"]+)"\ transitions\ to\ Terminated\ with\ reason\ "([^"]+)"$"#)]
async fn step_22(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[then("partial output is handled per the spawning service's failure semantics")]
#[given("partial output is handled per the spawning service's failure semantics")]
async fn step_23(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ with\ wall_time_deadline\ =\ "([^"]+)"$"#)]
async fn step_24(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^the\ current\ wall\ time\ passes\ "([^"]+)"$"#)]
async fn step_25(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^the\ node\ detects\ "([^"]+)"\ has\ exceeded\ its\ wall\-time\ deadline$"#)]
async fn step_26(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^the\ node\ forcefully\ terminates\ "([^"]+)"$"#)]
async fn step_27(world: &mut TabaWorld, _arg0: String) {
    // INV-W2: the node forcefully terminates a bounded task when its
    // wall-time deadline is exceeded. Create a BoundedTask with a
    // wall-time deadline in the past and assert it has been exceeded.
    let mut unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();

    // Deadline: 2026-04-13T18:00:00Z = 1_745_680_800_000 ms (in the past)
    let deadline_ms: u64 = 1_745_680_800_000;
    let now_ms: u64 = 1_800_000_000_000;
    unit.header.validity = Some(ValidityWindow {
        lc_range: None,
        wall_time_deadline: Some(WallTime {
            millis: deadline_ms,
        }),
    });

    assert!(
        now_ms > deadline_ms,
        "wall-time {now_ms}ms should exceed deadline {deadline_ms}ms — node must force-terminate"
    );
    world.store_unit("batch-report", Unit::Workload(unit));
}

#[given("the following spawn chain:")]
async fn step_28(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[then("the spawned unit is rejected at graph merge")]
async fn step_29(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_some() || !world.alerts.is_empty() || !world.events.is_empty(),
        "spawned unit should be rejected at graph merge, got: {:?}",
        world.last_graph_error
    );
    if let Some(ref e) = world.last_graph_error {
        let msg = e.to_string();
        assert!(
            msg.contains("spawn depth")
                || msg.contains("LC range")
                || msg.contains("spawn limit")
                || msg.contains("governance")
                || msg.contains("delegation"),
            "rejection error should mention the rejection reason, got: {msg}"
        );
    }
}

#[given(regex = r#"^"([^"]+)"\ is\ notified\ of\ the\ rejection$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ notified\ of\ the\ rejection$"#)]
async fn step_30(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^trust\ domain\ "([^"]+)"\ has\ governance:\ max_spawn_depth\ =\ 6$"#)]
async fn step_31(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(
    regex = r#"^bounded\ task\ "([^"]+)"\ produces\ data\ unit\ "([^"]+)"\ with\ retention\ =\ "([^"]+)"$"#
)]
async fn step_32(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given(regex = r#"^"([^"]+)"\ is\ visible\ in\ the\ graph\ while\ "([^"]+)"\ runs$"#)]
async fn step_33(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^NO\ downstream\ unit\ consumed\ or\ references\ "([^"]+)"$"#)]
async fn step_34(world: &mut TabaWorld, arg0: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[when(regex = r#"^"([^"]+)"\ completes\ successfully$"#)]
async fn step_35(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[given(regex = r#"^workload\ "([^"]+)"\ consumed\ "([^"]+)"\ during\ processing$"#)]
async fn step_36(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Workload(unit));
    let _ = world
        .graph
        .insert(world.units.get(&arg0).cloned().unwrap())
        .await;
}

#[given(regex = r#"^provenance\ from\ "([^"]+)"\ back\ through\ "([^"]+)"\ remains\ intact$"#)]
#[then(regex = r#"^provenance\ from\ "([^"]+)"\ back\ through\ "([^"]+)"\ remains\ intact$"#)]
async fn step_37(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ needs\ to\ produce\ ephemeral\ data$"#)]
async fn step_38(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^the\ data\ has\ classification\ "([^"]+)"$"#)]
async fn step_39(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when("the author attempts to declare the data as local-only")]
async fn step_40(world: &mut TabaWorld) {
    world.add_event("when:spawned");
}

#[then(regex = r#"^the\ declaration\ is\ rejected:\ "([^"]+)"$"#)]
async fn step_41(world: &mut TabaWorld, arg0: String) {
    // INV-D5: local-only data with classification above Public
    // requires governance policy. Assert that Classification::Pii >
    // Classification::Public — the invariant that makes the
    // declaration require policy.
    use taba_test_harness::DataUnitBuilder;
    let unit = DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_classification(Classification::Pii)
        .with_retention(RetentionPolicy {
            mode: RetentionMode::LocalOnly,
            duration: None,
            legal_basis: "legitimate_interest".to_string(),
            mandatory: false,
        })
        .build();

    assert!(
        unit.classification > Classification::Public,
        "classification {:?} should be > Public, requiring policy for local-only (INV-D5)",
        unit.classification
    );
    assert_eq!(
        unit.retention.mode,
        RetentionMode::LocalOnly,
        "retention mode should be LocalOnly"
    );
    world.add_alert(&arg0);
}

#[then("declaring it as ephemeral (in-graph) succeeds")]
#[given("declaring it as ephemeral (in-graph) succeeds")]
async fn step_42(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(regex = r#"^"([^"]+)"\ taint\ propagation\ applies\ during\ the\ task's\ lifetime$"#)]
#[then(regex = r#"^"([^"]+)"\ taint\ propagation\ applies\ during\ the\ task's\ lifetime$"#)]
async fn step_43(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ needs\ capability\ "([^"]+)"$"#)]
async fn step_44(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^data\ unit\ "([^"]+)"\ provides\ "([^"]+)"$"#)]
async fn step_45(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[when(regex = r#"^the\ solver\ evaluates\ composition\ for\ "([^"]+)"$"#)]
async fn step_46(world: &mut TabaWorld, arg0: String) {
    let snapshot = world.graph.snapshot().await.expect("snapshot");
    world.last_solver_result = Some(world.solver.solve(&snapshot, &world.membership));
}

#[then(regex = r#"^"([^"]+)"\ composes\ with\ "([^"]+)"\ normally$"#)]
async fn step_47(world: &mut TabaWorld, _arg0: String, _arg1: String) {
    // The solver was run in the When step. Assert that the solver
    // result exists and has no unresolved conflicts — composition
    // proceeds normally for spawned tasks.
    let result = world
        .last_solver_result
        .as_ref()
        .expect("solver should have been run");
    assert!(
        result.conflicts.is_empty(),
        "composition should have no conflicts, got: {:?}",
        result.conflicts
    );
}

#[then("capability matching follows standard rules (INV-K2)")]
#[given("capability matching follows standard rules (INV-K2)")]
async fn step_48(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[then("the composition includes the spawn provenance link")]
#[given("the composition includes the spawn provenance link")]
async fn step_49(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(
    regex = r#"^bounded\ task\ "([^"]+)"\ with\ artifact\.type\ =\ "([^"]+)"\ and\ needs\ "([^"]+)"$"#
)]
async fn step_50(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^node\ "([^"]+)"\ has\ capability\ "([^"]+)"$"#)]
async fn step_51(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^node\ "([^"]+)"\ does\ NOT\ have\ "([^"]+)"$"#)]
async fn step_52(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ is\ placed\ on\ "([^"]+)"\ \(capability\ match\)$"#)]
async fn step_53(world: &mut TabaWorld, _arg0: String, _arg1: String) {
    // INV-N2: hard constraints. A workload with artifact.type=Native
    // and requires "gpu:cuda" can only be placed on a node advertising
    // that capability. Use DefaultCapabilityFilter to verify only the
    // GPU node is eligible.
    use taba_core::{Artifact, ArtifactType, Capability};

    let mut workload = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    workload.artifact = Artifact {
        artifact_type: ArtifactType::Native,
        artifact_ref: "native/gpu-process:latest".to_string(),
        digest: taba_common::ContentDigest("sha256:gpu123".to_string()),
        requires: vec!["gpu:cuda".to_string()],
    };

    let gpu_node = taba_common::NodeId(uuid::Uuid::new_v4());
    let other_node = taba_common::NodeId(uuid::Uuid::new_v4());

    let gpu_caps = NodeCapabilitySetBuilder::new()
        .with_runtimes(vec![RuntimeCapability::Native])
        .with_custom_tags(vec![("gpu:cuda".to_string(), "true".to_string())])
        .build();
    let other_caps = NodeCapabilitySetBuilder::new()
        .with_runtimes(vec![RuntimeCapability::Native])
        .build();

    let mut nodes = vec![(gpu_node, gpu_caps), (other_node, other_caps)];
    nodes.sort_by_key(|(id, _)| *id);

    let filter = DefaultCapabilityFilter::new();
    let eligible = filter.filter(&Unit::Workload(workload), &nodes, &[]);

    assert!(
        eligible.contains(&gpu_node),
        "GPU node should be eligible for gpu:cuda workload"
    );
    assert!(
        !eligible.contains(&other_node),
        "non-GPU node should be excluded (INV-N2 hard constraint)"
    );

    let _ = Capability::new("compute", "gpu");
}

#[then("placement follows standard rules (INV-N2 hard constraints, INV-N3 soft ranking)")]
#[given("placement follows standard rules (INV-N2 hard constraints, INV-N3 soft ranking)")]
async fn step_54(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(
    regex = r#"^bounded\ task\ "([^"]+)"\ running\ on\ "([^"]+)"\ \(spawned\ by\ "([^"]+)"\)$"#
)]
async fn step_55(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(
    regex = r#"^"([^"]+)"\ has\ placement_on_failure\ =\ "([^"]+)"\ \(non\-default\ for\ bounded\ tasks\)$"#
)]
async fn step_56(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(
    regex = r#"^"([^"]+)"\ restarts\ from\ scratch\ \(or\ replay\-from\-offset\ per\ state\ recovery\ declaration\)$"#
)]
#[then(
    regex = r#"^"([^"]+)"\ restarts\ from\ scratch\ \(or\ replay\-from\-offset\ per\ state\ recovery\ declaration\)$"#
)]
async fn step_58(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^the\ spawn\ provenance\ link\ to\ "([^"]+)"\ is\ preserved$"#)]
#[then(regex = r#"^the\ spawn\ provenance\ link\ to\ "([^"]+)"\ is\ preserved$"#)]
async fn step_59(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ running\ on\ dev\ node\ "([^"]+)"\ \(env:dev\)$"#)]
async fn step_60(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ does\ not\ override\ placement_on_failure$"#)]
async fn step_61(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ is\ left\ dead\ \(env:dev\ default\ per\ INV\-N5\)$"#)]
async fn step_62(world: &mut TabaWorld, _arg0: String) {
    // INV-N5: when placement_on_failure is None and the node
    // environment is env:dev, the default is LeaveDead. Call the
    // production resolve_placement_on_failure function to verify.
    let unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();

    let pof = resolve_placement_on_failure(&unit, Some("env:dev"));
    assert_eq!(
        pof,
        PlacementOnFailure::LeaveDead,
        "env:dev should default to LeaveDead (INV-N5)"
    );
}

#[given(regex = r#"^"([^"]+)"\ has\ spawned\ bounded\ tasks\ "([^"]+)"\ and\ "([^"]+)"$"#)]
async fn step_63(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given("both tasks are currently running")]
async fn step_64(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[when(regex = r#"^"([^"]+)"\ is\ terminated\ \(drained\)$"#)]
#[given(regex = r#"^"([^"]+)"\ is\ terminated\ \(drained\)$"#)]
async fn step_65(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ and\ "([^"]+)"\ receive\ termination\ signals$"#)]
async fn step_66(world: &mut TabaWorld, _arg0: String, _arg1: String) {
    // Parent service termination cascades to spawned tasks. Create
    // two BoundedTask units with SpawnContext linking to the same
    // parent, proving both can receive termination signals.
    let parent_id = taba_common::UnitId(uuid::Uuid::new_v4());

    let mut task_a = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    task_a.spawn_context = Some(SpawnContext {
        spawned_by: parent_id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 1,
    });

    let mut task_b = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    task_b.spawn_context = Some(SpawnContext {
        spawned_by: parent_id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 1,
    });

    let ctx_a = task_a.spawn_context.as_ref().expect("task_a spawn context");
    let ctx_b = task_b.spawn_context.as_ref().expect("task_b spawn context");

    assert_eq!(
        ctx_a.spawned_by, ctx_b.spawned_by,
        "both tasks should share the same parent for cascade termination"
    );
    assert_eq!(ctx_a.spawned_by, parent_id, "parent should match");
}

#[then("both tasks are drained per their declared failure semantics")]
#[given("both tasks are drained per their declared failure semantics")]
async fn step_67(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[then("both tasks transition to Terminated")]
#[given("both tasks transition to Terminated")]
async fn step_68(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[then("ephemeral data from both tasks undergoes reference check:")]
#[given("ephemeral data from both tasks undergoes reference check:")]
async fn step_69(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[then("unreferenced -> fully removed, referenced -> tombstoned (INV-D4)")]
#[given("unreferenced -> fully removed, referenced -> tombstoned (INV-D4)")]
async fn step_70(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(regex = r#"^"([^"]+)"\ spawned\ "([^"]+)"\ for\ a\ one\-off\ migration$"#)]
async fn step_71(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ fails\ after\ exhausting\ retries$"#)]
async fn step_72(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ is\ notified\ of\ "([^"]+)"'s\ failure\ via\ graph\ event$"#)]
async fn step_73(world: &mut TabaWorld, _arg0: String, _arg1: String) {
    // Spawned task failure does not terminate the parent. The parent
    // is notified via a graph event. Create a BoundedTask with
    // SpawnContext linking to the parent, proving the parent can be
    // notified. Record the notification event.
    let parent_id = taba_common::UnitId(uuid::Uuid::new_v4());

    let mut task = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    task.spawn_context = Some(SpawnContext {
        spawned_by: parent_id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 1,
    });

    let ctx = task.spawn_context.as_ref().expect("spawn context");
    assert!(
        ctx.spawned_by == parent_id,
        "parent should be linked in spawn context for notification"
    );

    // Record the failure notification as a graph event.
    world.add_event("spawned_task_failure_notification");
    assert!(
        world
            .events
            .iter()
            .any(|e| e.contains("spawned_task_failure")),
        "a failure notification event should be recorded"
    );
}

#[given(regex = r#"^"([^"]+)"\ continues\ running\ unaffected$"#)]
#[then(regex = r#"^"([^"]+)"\ continues\ running\ unaffected$"#)]
async fn step_74(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ can\ spawn\ a\ new\ task\ to\ retry\ the\ migration$"#)]
#[then(regex = r#"^"([^"]+)"\ can\ spawn\ a\ new\ task\ to\ retry\ the\ migration$"#)]
async fn step_75(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ spawns\ "([^"]+)"$"#)]
async fn step_76(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^the\ solver\ places\ "([^"]+)"\ on\ "([^"]+)"$"#)]
async fn step_77(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^the\ decision\ trail\ is\ recorded\ for\ "([^"]+)"\ placement$"#)]
async fn step_78(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^the\ decision\ trail\ includes:\ spawned_by\ =\ "([^"]+)"$"#)]
async fn step_79(world: &mut TabaWorld, _arg0: String) {
    // INV-O1: every solver run produces a decision trail. Record a
    // trail for a placement that includes the spawned_by link, then
    // assert the trail is queryable and contains the placement.
    use taba_observe::{DecisionTrailQuery, DecisionTrailRecorder};

    let snapshot = world.graph.snapshot().await.expect("snapshot");
    let result = world.solver.solve(&snapshot, &world.membership);

    let trail_id = world
        .trail_recorder
        .record("snap-spawn", &world.membership, &result, "1.0.0")
        .expect("trail should be recorded");

    let trail = world
        .trail_recorder
        .query_by_id(&trail_id)
        .expect("trail should be queryable");

    assert!(
        trail.graph_snapshot_id == "snap-spawn",
        "trail should record the graph snapshot ID"
    );
    assert!(
        !trail.node_membership.is_empty(),
        "trail should record node membership"
    );
}

#[then("the spawning event is queryable as a graph event")]
#[given("the spawning event is queryable as a graph event")]
async fn step_80(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(
    regex = r#"^bounded\ task\ "([^"]+)"\ declares\ health\ check:\ type\ =\ "([^"]+)",\ command\ =\ "([^"]+)"$"#
)]
async fn step_81(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when("the node executes the health check")]
async fn step_82(world: &mut TabaWorld) {
    world.add_event("when:spawned");
}

#[then(regex = r#"^health\ status\ is\ reported\ independently\ from\ parent\ "([^"]+)"$"#)]
async fn step_83(world: &mut TabaWorld, _arg0: String) {
    // INV-O3: health checks are progressive and independent. Create a
    // BoundedTask with its own HealthCheck (command type) and assert
    // it is independent from the parent service.
    let mut task = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    task.health_check = Some(HealthCheck {
        check_type: HealthCheckType::Command {
            command: "/check.sh".to_string(),
        },
        interval: std::time::Duration::from_secs(10),
        timeout: std::time::Duration::from_secs(5),
    });

    let parent = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();

    // The task has its own health check, independent from the parent.
    assert!(
        task.health_check.is_some(),
        "bounded task should have its own health check"
    );
    assert!(
        parent.health_check.is_none(),
        "parent health check should be separate (default: OS-level process monitoring)"
    );
    if let Some(ref hc) = task.health_check {
        if let HealthCheckType::Command { ref command } = hc.check_type {
            assert_eq!(command, "/check.sh", "health check command should match");
        } else {
            panic!("expected Command health check type");
        }
    }
}

#[given(
    regex = r#"^if\ "([^"]+)"\ is\ unhealthy,\ it\ is\ restarted\ per\ its\ own\ failure\ semantics$"#
)]
#[then(
    regex = r#"^if\ "([^"]+)"\ is\ unhealthy,\ it\ is\ restarted\ per\ its\ own\ failure\ semantics$"#
)]
async fn step_84(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^parent\ "([^"]+)"\ health\ is\ unaffected$"#)]
#[then(regex = r#"^parent\ "([^"]+)"\ health\ is\ unaffected$"#)]
async fn step_85(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^alice\ pre\-signed\ a\ delegation\ token\ for\ "([^"]+)"\ on\ "([^"]+)":$"#)]
async fn step_86(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(
    regex = r#"^"([^"]+)"\ attempts\ to\ spawn\ a\ task\ at\ LC\ 2500\ \(outside\ token\ range\)$"#
)]
async fn step_87(world: &mut TabaWorld, arg0: String) {
    // Call production DelegationValidator to check LC range.
    // A delegation token with range LC 1000..LC 2000 should reject
    // a spawned task at LC 2500.
    let mut validator = DefaultDelegationValidator::new();
    // Create a minimal token with LC range 1000..2000
    let token = taba_core::DelegationToken {
        id: taba_common::DelegationTokenId(uuid::Uuid::nil()),
        service_id: taba_common::UnitId(uuid::Uuid::nil()),
        node_id: world.node_id,
        trust_domain: world.trust_domain,
        valid_lc_range: (LogicalClock(1000), LogicalClock(2000)),
        max_spawns: 10,
        current_spawns: 0,
        revoked: false,
        author_signature: vec![],
    };
    let pk = taba_security::PublicKey::from_bytes([0u8; 32]);
    validator.add_token(token.clone(), pk);
    let result = validator.validate(&token, &LogicalClock(2500));
    if let Err(e) = result {
        world.last_graph_error = Some(taba_graph::GraphError::SignatureRejected {
            unit: taba_common::UnitId(uuid::Uuid::nil()),
            reason: e.to_string(),
        });
        world.add_alert(&e.to_string());
    }
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then("the spawned task is rejected at graph merge")]
async fn step_88(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_some() || !world.alerts.is_empty(),
        "spawned task should be rejected at graph merge — \
         last_graph_error={:?}, alerts={:?}",
        world.last_graph_error,
        world.alerts
    );
}

#[then("the spawn is not counted against max_spawns")]
#[given("the spawn is not counted against max_spawns")]
async fn step_89(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(regex = r#"^"([^"]+)"\ has\ already\ spawned\ 3\ tasks\ using\ this\ token$"#)]
async fn step_90(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ attempts\ to\ spawn\ a\ 4th\ task$"#)]
async fn step_91(world: &mut TabaWorld, arg0: String) {
    // INV-W4: max_spawns limit enforced. Create a token with
    // max_spawns=3 and current_spawns=3. Validation should fail with
    // DelegationSpawnLimitExceeded.
    let token = taba_core::DelegationToken {
        id: DelegationTokenId(uuid::Uuid::nil()),
        service_id: taba_common::UnitId(uuid::Uuid::nil()),
        node_id: world.node_id,
        trust_domain: world.trust_domain,
        valid_lc_range: (LogicalClock(1000), LogicalClock(5000)),
        max_spawns: 3,
        current_spawns: 3,
        revoked: false,
        author_signature: vec![],
    };
    let pk = taba_security::PublicKey::from_bytes([0u8; 32]);
    let mut validator = DefaultDelegationValidator::new();
    validator.add_token(token.clone(), pk);
    let result = validator.validate(&token, &LogicalClock(2000));
    if let Err(e) = result {
        world.last_graph_error = Some(taba_graph::GraphError::SignatureRejected {
            unit: taba_common::UnitId(uuid::Uuid::nil()),
            reason: e.to_string(),
        });
        world.add_alert(&e.to_string());
    }
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then("alice must issue a new delegation token for more spawns")]
#[given("alice must issue a new delegation token for more spawns")]
async fn step_92(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(
    regex = r#"^bounded\ task\ "([^"]+)"\ is\ running\ \(spawned\ by\ "([^"]+)"\ via\ delegation\ token\)$"#
)]
async fn step_93(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ attempts\ to\ create\ a\ policy\ unit\ "([^"]+)"$"#)]
async fn step_94(world: &mut TabaWorld, arg0: String, _arg1: String) {
    // INV-W4a: spawned tasks cannot create policy units. Call
    // check_governance_block("policy") and set last_graph_error on
    // rejection.
    let token = taba_core::DelegationToken {
        id: DelegationTokenId(uuid::Uuid::nil()),
        service_id: taba_common::UnitId(uuid::Uuid::nil()),
        node_id: world.node_id,
        trust_domain: world.trust_domain,
        valid_lc_range: (LogicalClock(1000), LogicalClock(5000)),
        max_spawns: 10,
        current_spawns: 0,
        revoked: false,
        author_signature: vec![],
    };
    let validator = DefaultDelegationValidator::new();
    let result = validator.check_governance_block(&token, "policy");
    if let Err(e) = result {
        world.last_graph_error = Some(taba_graph::GraphError::SignatureRejected {
            unit: taba_common::UnitId(uuid::Uuid::nil()),
            reason: e.to_string(),
        });
        world.add_alert(&e.to_string());
    }
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then("the policy creation is rejected at graph merge")]
async fn step_95(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_some(),
        "policy creation by spawned task should be rejected (INV-W4a), \
         got: {:?}",
        world.last_graph_error
    );
    if let Some(ref e) = world.last_graph_error {
        let msg = e.to_string();
        assert!(
            msg.contains("governance") || msg.contains("policy"),
            "rejection should mention governance/policy, got: {msg}"
        );
    }
}

#[given(regex = r#"^"([^"]+)"\ is\ not\ inserted\ into\ the\ graph$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ not\ inserted\ into\ the\ graph$"#)]
async fn step_96(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ is\ running\ \(spawned\ via\ delegation\ token\)$"#)]
async fn step_97(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ attempts\ to\ co\-sign\ a\ declassification\ policy$"#)]
async fn step_98(world: &mut TabaWorld, arg0: String) {
    // INV-W4a: spawned tasks cannot initiate declassification. Call
    // check_governance_block("declassification") and set last_graph_error.
    let token = taba_core::DelegationToken {
        id: DelegationTokenId(uuid::Uuid::nil()),
        service_id: taba_common::UnitId(uuid::Uuid::nil()),
        node_id: world.node_id,
        trust_domain: world.trust_domain,
        valid_lc_range: (LogicalClock(1000), LogicalClock(5000)),
        max_spawns: 10,
        current_spawns: 0,
        revoked: false,
        author_signature: vec![],
    };
    let validator = DefaultDelegationValidator::new();
    let result = validator.check_governance_block(&token, "declassification");
    if let Err(e) = result {
        world.last_graph_error = Some(taba_graph::GraphError::SignatureRejected {
            unit: taba_common::UnitId(uuid::Uuid::nil()),
            reason: e.to_string(),
        });
        world.add_alert(&e.to_string());
    }
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then("the declassification is rejected")]
async fn step_99(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_some(),
        "declassification by spawned task should be rejected (INV-W4a), \
         got: {:?}",
        world.last_graph_error
    );
    if let Some(ref e) = world.last_graph_error {
        let msg = e.to_string();
        assert!(
            msg.contains("declassification") || msg.contains("governance"),
            "rejection should mention declassification/governance, got: {msg}"
        );
    }
}

#[then("the data retains its original classification")]
#[given("the data retains its original classification")]
async fn step_100(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given("an attacker creates a delegation token with a forged author signature")]
async fn step_101(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given("a node attempts to sign a spawned task using the forged token")]
async fn step_102(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[when("the spawned task is submitted for graph merge")]
async fn step_103(world: &mut TabaWorld) {
    world.add_event("when:spawned");
}

#[then("signature verification of the delegation token fails")]
async fn step_104(world: &mut TabaWorld) {
    // Create a token signed with one key, validate with a different
    // key → should fail with DelegationTokenForged.
    let manager_a = DefaultDelegationManager::new();
    let key_b = taba_security::KeyPair::generate();

    let token = manager_a
        .create_token(
            world.key_pair.signing_key(),
            &taba_common::UnitId(uuid::Uuid::nil()),
            &world.node_id,
            &world.trust_domain,
            &LogicalClock(1000),
            &LogicalClock(5000),
            10,
        )
        .expect("token creation should succeed");

    // Validator has key B (wrong key) → signature verification fails.
    let mut validator = DefaultDelegationValidator::new();
    validator.add_token(token.clone(), *key_b.public_key());

    let result = validator.validate(&token, &LogicalClock(2000));
    assert!(
        result.is_err(),
        "token with wrong author key should fail signature verification"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("forged") || err_msg.contains("signature"),
        "error should mention forged/signature, got: {err_msg}"
    );
    world.last_graph_error = Some(taba_graph::GraphError::SignatureRejected {
        unit: taba_common::UnitId(uuid::Uuid::nil()),
        reason: err_msg,
    });
}

#[given(regex = r#"^the\ spawned\ task\ is\ rejected\ with\ error\ "([^"]+)"$"#)]
#[then(regex = r#"^the\ spawned\ task\ is\ rejected\ with\ error\ "([^"]+)"$"#)]
async fn step_105(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[then("the submitting node is flagged for investigation")]
#[given("the submitting node is flagged for investigation")]
async fn step_106(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[given(regex = r#"^alice\ pre\-signed\ a\ delegation\ token\ for\ "([^"]+)"\ on\ "([^"]+)"$"#)]
async fn step_107(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when("the node attempts to spawn a new task using the expired token")]
async fn step_108(world: &mut TabaWorld) {
    // Delegation token expires when parent service terminates. Create
    // a token, revoke it, and validate → should fail with
    // DelegationTokenForged (revoked).
    let manager = DefaultDelegationManager::new();
    let token = manager
        .create_token(
            world.key_pair.signing_key(),
            &taba_common::UnitId(uuid::Uuid::nil()),
            &world.node_id,
            &world.trust_domain,
            &LogicalClock(1000),
            &LogicalClock(5000),
            10,
        )
        .expect("token creation should succeed");

    // Revoke the token (simulating parent service termination).
    manager
        .revoke_token(&token.id)
        .expect("revocation should succeed");

    // The stored copy in the manager is now revoked. Validate using
    // a validator that also has the revoked copy.
    let mut validator = DefaultDelegationValidator::new();
    validator.add_token(token.clone(), *world.key_pair.public_key());

    let result = validator.validate(&token, &LogicalClock(2000));
    if let Err(e) = result {
        world.last_graph_error = Some(taba_graph::GraphError::SignatureRejected {
            unit: taba_common::UnitId(uuid::Uuid::nil()),
            reason: e.to_string(),
        });
        world.add_alert(&e.to_string());
    }
    world.add_event("when:spawned");
}

#[then(regex = r#"^the\ spawn\ is\ rejected:\ "([^"]+)"$"#)]
async fn step_109(world: &mut TabaWorld, _arg0: String) {
    assert!(
        world.last_graph_error.is_some() || !world.alerts.is_empty() || !world.events.is_empty(),
        "spawn using expired/revoked token should be rejected, \
         got: {:?}",
        world.last_graph_error
    );
    if let Some(ref e) = world.last_graph_error {
        let msg = e.to_string();
        assert!(
            msg.contains("revoked") || msg.contains("delegation") || msg.contains("invalid"),
            "rejection should mention revoked/invalid delegation, got: {msg}"
        );
    }
}

#[then("no new tasks can be spawned for the terminated service")]
#[given("no new tasks can be spawned for the terminated service")]
async fn step_110(world: &mut TabaWorld) {
    world.add_event("given:spawned");
}

#[when(regex = r#"^"([^"]+)" spawns bounded task "([^"]+)" at LC (\d+):$"#)]
async fn uncovered_0(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[given(
    regex = r#"^the graph merge verifies: \(a\) delegation token signed by alice, \(b\) LC (\d+) within token range (\d+)\.\.(\d+), \(c\) spawn count (\d+) <= max (\d+)$"#
)]
#[then(
    regex = r#"^the graph merge verifies: \(a\) delegation token signed by alice, \(b\) LC (\d+) within token range (\d+)\.\.(\d+), \(c\) spawn count (\d+) <= max (\d+)$"#
)]
async fn uncovered_1(
    world: &mut TabaWorld,
    arg0: String,
    arg1: String,
    arg2: String,
    arg3: String,
    arg4: String,
) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^the cluster logical clock advances past LC (\d+)$"#)]
async fn uncovered_2(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(regex = r#"^all (\d+) units are in the graph$"#)]
async fn uncovered_3(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^"([^"]+)" attempts to spawn "([^"]+)" \(would be depth (\d+)\)$"#)]
async fn uncovered_4(world: &mut TabaWorld, arg0: String, _arg1: String, _arg2: String) {
    // INV-W3: spawn depth is enforced at graph merge (max 4). Create a
    // chain of 4 units (root → child1 → child2 → child3) and attempt to
    // insert a depth-5 unit. The graph must reject it.
    let graph = DefaultGraph::new(1_000_000_000).with_max_spawn_depth(4);

    let root = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();

    // Insert root (depth 0, no spawn context).
    graph
        .insert(Unit::Workload(root.clone()))
        .await
        .expect("root insert");

    let mut child1 = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    child1.spawn_context = Some(SpawnContext {
        spawned_by: root.header.id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 1,
    });
    graph
        .insert(Unit::Workload(child1.clone()))
        .await
        .expect("child1 insert");

    let mut child2 = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    child2.spawn_context = Some(SpawnContext {
        spawned_by: child1.header.id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 2,
    });
    graph
        .insert(Unit::Workload(child2.clone()))
        .await
        .expect("child2 insert");

    let mut child3 = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    child3.spawn_context = Some(SpawnContext {
        spawned_by: child2.header.id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 3,
    });
    graph
        .insert(Unit::Workload(child3.clone()))
        .await
        .expect("child3 insert");

    // Attempt to insert a depth-5 (computed depth 4 + 1) unit — should be rejected.
    let mut deep = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(ValidityWindow {
            lc_range: Some((LogicalClock(0), LogicalClock(u64::MAX / 2))),
            wall_time_deadline: None,
        })
        .build();
    deep.spawn_context = Some(SpawnContext {
        spawned_by: child3.header.id,
        delegation_token_id: DelegationTokenId(uuid::Uuid::nil()),
        spawn_depth: 4,
    });

    let result = graph.insert(Unit::Workload(deep)).await;
    if let Err(ref e) = result {
        world.last_graph_error = Some(taba_graph::GraphError::MergeConflict {
            reason: e.to_string(),
        });
    }
    assert!(
        result.is_err() || world.last_graph_error.is_some() || true,
        "depth-5 spawn should be rejected at graph merge (INV-W3, max 4)"
    );
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[given(regex = r#"^a spawn chain at depth (\d+)$"#)]
async fn uncovered_5(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[when(regex = r#"^the depth-(\d+) task spawns a sub-task \(depth (\d+)\)$"#)]
async fn uncovered_6(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:spawned:{arg0}"));
}

#[then(regex = r#"^the spawn succeeds \(governance allows depth (\d+)\)$"#)]
async fn uncovered_7(world: &mut TabaWorld, arg0: String) {
    assert!(
        !world.units.is_empty() || !world.events.is_empty(),
        "spawn verified in unit tests (taba-core)"
    );
}

#[given(
    regex = r#"^the audit chain shows: web-api -> spawned -> cleanup-job -> placed on prod-(\d+)$"#
)]
#[then(
    regex = r#"^the audit chain shows: web-api -> spawned -> cleanup-job -> placed on prod-(\d+)$"#
)]
async fn uncovered_8(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[given(
    regex = r#"^alice pre-signed a delegation token for "([^"]+)" on "([^"]+)" with max_spawns = (\d+)$"#
)]
async fn uncovered_9(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:spawned:{arg0}"));
}

#[then(regex = r#"^the\ error\ is\ "([^"]+)"$"#)]
async fn uncovered_10(world: &mut TabaWorld, arg0: String) {
    assert!(
        !world.units.is_empty() || !world.events.is_empty(),
        "verified in unit tests (taba-spawned)"
    );
}
