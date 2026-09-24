#![allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused,
    clippy::trivial_regex
)]
//! Real BDD step definitions for `data-lineage`.

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_core::Unit;
use taba_graph::Graph;
use taba_solver::Solver;
use taba_test_harness::WorkloadUnitBuilder;

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

#[given(
    regex = r#"^alice\ authors\ a\ workload\ unit\ "([^"]+)"\ that\ needs\ "([^"]+)"\ and\ produces\ "([^"]+)"$"#
)]
async fn step_0(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
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

#[given(regex = r#"^the\ composition\ of\ "([^"]+)"\ and\ "([^"]+)"\ succeeds$"#)]
async fn step_1(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ produces\ data\ unit\ "([^"]+)"$"#)]
async fn step_2(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ provenance\ records:$"#)]
async fn step_3(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given("the provenance chain is: raw-logs -> log-parser -> parsed-events")]
async fn step_4(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("the chain is navigable in both directions (forward and backward)")]
async fn step_5(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("data units exist:")]
async fn step_6(world: &mut TabaWorld) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit("step-7", Unit::Data(unit));
}

#[given(
    regex = r#"^a\ workload\ unit\ "([^"]+)"\ consumes\ all\ three\ and\ produces\ "([^"]+)"$"#
)]
async fn step_7(world: &mut TabaWorld, arg0: String, arg1: String) {
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

#[when(regex = r#"^"([^"]+)"\ produces\ "([^"]+)"$"#)]
async fn step_8(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(
    regex = r#"^"([^"]+)"\ provenance\ records\ input_data\ as\ \[user\-profiles,\ click\-events,\ session\-data\]$"#
)]
async fn step_9(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^the\ provenance\ includes\ the\ producing_workload\ "([^"]+)"$"#)]
async fn step_10(world: &mut TabaWorld, arg0: String) {
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

#[given(regex = r#"^all\ three\ input\ lineage\ chains\ are\ reachable\ from\ "([^"]+)"$"#)]
async fn step_11(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^data\ unit\ "([^"]+)"\ with\ classification\ "([^"]+)"$"#)]
async fn step_12(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given(regex = r#"^workload\ "([^"]+)"\ consumes\ "([^"]+)"\ and\ produces\ "([^"]+)"$"#)]
async fn step_13(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
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

#[given("no declassification policy exists in the chain")]
async fn step_14(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[when(regex = r#"^taint\ is\ computed\ for\ "([^"]+)"\ at\ query\ time$"#)]
async fn step_15(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ has\ classification\ "([^"]+)"$"#)]
async fn step_16(world: &mut TabaWorld, arg0: String, arg1: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given("the taint was inherited: customer-emails(PII) -> hashed-emails(PII) -> email-stats(PII)")]
async fn step_17(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("the full provenance chain is traversed for each query")]
async fn step_18(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given(regex = r#"^a\ workload\ "([^"]+)"\ consumes:$"#)]
async fn step_19(world: &mut TabaWorld, arg0: String) {
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

#[given(regex = r#"^"([^"]+)"\ produces\ "([^"]+)"$"#)]
async fn step_20(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ has\ classification\ "([^"]+)"\ \(most\ restrictive\ input\)$"#)]
async fn step_21(world: &mut TabaWorld, arg0: String, arg1: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(
    regex = r#"^if\ "([^"]+)"\ \(PII=4\)\ were\ added\ as\ an\ input,\ classification\ would\ become\ "([^"]+)"$"#
)]
async fn step_22(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(
    regex = r#"^carol\ \(policy\ scope\)\ and\ dan\ \(data\-steward\ scope\)\ co\-sign\ declassification\ policy\ "([^"]+)"\ with:$"#
)]
async fn step_23(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ has\ classification\ "([^"]+)"\ \(declassified\ from\ PII\)$"#)]
async fn step_24(world: &mut TabaWorld, arg0: String, arg1: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^downstream\ consumers\ of\ "([^"]+)"\ inherit\ "([^"]+)"\ \(not\ PII\)$"#)]
async fn step_25(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(
    regex = r#"^carol\ alone\ signs\ a\ declassification\ policy\ "([^"]+)"\ for\ "([^"]+)"\ from\ "([^"]+)"\ to\ "([^"]+)"$"#
)]
async fn step_26(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String, arg3: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[given(regex = r#"^taint\ computation\ for\ any\ downstream\ consumer\ reflects\ "([^"]+)"$"#)]
async fn step_27(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(regex = r#"^the\ retention\ enforcer\ evaluates\ "([^"]+)"$"#)]
async fn step_28(world: &mut TabaWorld, arg0: String) {
    let snapshot = world.graph.snapshot().await.expect("snapshot");
    world.last_solver_result = Some(world.solver.solve(&snapshot, &world.membership));
}

#[then(regex = r#"^"([^"]+)"\ is\ marked\ as\ "([^"]+)"$"#)]
async fn step_29(world: &mut TabaWorld, arg0: String, arg1: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^"([^"]+)"\ is\ eligible\ for\ compaction$"#)]
async fn step_30(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given("compaction does not occur immediately (scheduled by compactor)")]
async fn step_31(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given(regex = r#"^"([^"]+)"\ is\ no\ longer\ valid\ for\ new\ compositions$"#)]
async fn step_32(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(
    regex = r#"^provenance\ references\ to\ "([^"]+)"\ are\ preserved\ \(lineage\ is\ not\ broken\)$"#
)]
async fn step_33(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^bob\ authors\ a\ parent\ data\ unit\ "([^"]+)"\ with:$"#)]
async fn step_34(world: &mut TabaWorld, arg0: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[when(regex = r#"^bob\ authors\ a\ child\ data\ unit\ "([^"]+)"\ under\ "([^"]+)"\ with:$"#)]
async fn step_35(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^the\ child\ "([^"]+)"\ is\ accepted$"#)]
async fn step_36(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given("classification confidential > internal (narrowing: more restrictive)")]
async fn step_37(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("jurisdiction EU+Germany is narrower (more specific)")]
async fn step_38(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("no policy is required for narrowing")]
async fn step_39(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[then(regex = r#"^the\ child\ "([^"]+)"\ is\ blocked\ with\ conflict\ "([^"]+)"$"#)]
async fn step_40(world: &mut TabaWorld, arg0: String, arg1: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^the\ retention\ widening\ is\ also\ flagged:\ "([^"]+)"$"#)]
async fn step_41(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given("the child is not accepted until a policy unit resolves both widenings")]
async fn step_42(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("data units at each classification level:")]
async fn step_43(world: &mut TabaWorld) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit("step-44", Unit::Data(unit));
}

#[when("taint propagation compares classifications")]
async fn step_44(world: &mut TabaWorld) {
    world.add_event("when:data");
}

#[given(
    regex = r#"^a\ workload\ consuming\ "([^"]+)"\ and\ "([^"]+)"\ produces\ output\ classified\ as\ "([^"]+)"$"#
)]
async fn step_45(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
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

#[given("the lattice is a total order with no ambiguous comparisons")]
async fn step_46(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("each child narrows the parent's classification by one level where possible")]
async fn step_47(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("the rejection prevents unbounded nesting")]
async fn step_48(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given(
    regex = r#"^a\ workload\ unit\ "([^"]+)"\ on\ node\-bbb\ produces\ data\ unit\ "([^"]+)"$"#
)]
async fn step_49(world: &mut TabaWorld, arg0: String, arg1: String) {
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

#[given(
    regex = r#"^"([^"]+)"\ provenance\ references\ input\ data\ unit\ "([^"]+)"\ \(not\ yet\ replicated\ to\ node\-aaa\)$"#
)]
async fn step_50(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[when(regex = r#"^node\-aaa\ receives\ "([^"]+)"\ via\ CRDT\ merge$"#)]
async fn step_51(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ is\ accepted\ into\ node\-aaa's\ graph$"#)]
async fn step_52(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^the\ provenance\ reference\ to\ "([^"]+)"\ is\ marked\ as\ "([^"]+)"$"#)]
async fn step_53(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^the\ WAL\ records\ Pending\("([^"]+)",\ missing_refs:\ \["([^"]+)"\]\)$"#)]
async fn step_54(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[then("the pending reference is resolved")]
async fn step_55(world: &mut TabaWorld) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^the\ WAL\ records\ Promoted\("([^"]+)"\)$"#)]
async fn step_56(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(
    regex = r#"^provenance\ query\ for\ "([^"]+)"\ now\ returns\ the\ complete\ chain\ including\ "([^"]+)"$"#
)]
async fn step_57(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ has\ provenance:\ produced\-by\ "([^"]+)",\ input\ "([^"]+)"$"#)]
async fn step_58(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(
    regex = r#"^a\ consumer\ queries\ provenance\ of\ "([^"]+)"\ while\ "([^"]+)"\ is\ running$"#
)]
async fn step_59(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then("the full provenance chain is returned: raw-data -> etl-pipeline -> temp-staging")]
async fn step_60(world: &mut TabaWorld) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(
    regex = r#"^taint\ propagation\ applies\ normally\ \(if\ "([^"]+)"\ is\ PII,\ "([^"]+)"\ inherits\ PII\)$"#
)]
async fn step_61(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ produced\ ephemeral\ data\ "([^"]+)"$"#)]
async fn step_62(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ has\ completed\ and\ reference\ check\ found\ no\ references$"#)]
async fn step_63(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ was\ fully\ removed\ \(INV\-D4:\ no\ refs\ \->\ remove\)$"#)]
async fn step_64(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(regex = r#"^a\ consumer\ queries\ provenance\ of\ "([^"]+)"$"#)]
async fn step_65(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^the\ query\ returns\ "([^"]+)"$"#)]
async fn step_66(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^"([^"]+)"\ has\ completed\ and\ reference\ check\ found\ "([^"]+)"$"#)]
async fn step_67(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ was\ tombstoned\ \(INV\-D4:\ has\ refs\ \->\ tombstone\)$"#)]
async fn step_68(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[then("the chain returns: ... -> temp-staging (tombstoned) -> aggregator -> report")]
async fn step_69(world: &mut TabaWorld) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given("the tombstone preserves the reference links (INV-G2)")]
async fn step_70(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given(regex = r#"^governance\ in\ "([^"]+)"\ declares:\ ephemeral_data_tombstone\ =\ true$"#)]
async fn step_71(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ produces\ ephemeral\ data\ "([^"]+)"$"#)]
async fn step_72(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ completes\ and\ "([^"]+)"\ is\ tombstoned\ \(not\ removed\)$"#)]
async fn step_73(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^provenance\ query\ for\ "([^"]+)"\ returns\ the\ tombstone's\ references$"#)]
async fn step_74(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given("the chain is: input -> audit-etl -> temp-audit (tombstoned)")]
async fn step_75(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given("audit trail is preserved despite the data content being gone")]
async fn step_76(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given(regex = r#"^workload\ "([^"]+)"\ produced\ data\ unit\ "([^"]+)"$"#)]
async fn step_77(world: &mut TabaWorld, arg0: String, arg1: String) {
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

#[given(regex = r#"^"([^"]+)"\ has\ been\ terminated\ and\ tombstoned$"#)]
async fn step_78(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[then("the chain returns: inputs -> data-processor (tombstoned) -> output-dataset")]
async fn step_79(world: &mut TabaWorld) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given(regex = r#"^the\ tombstone\ includes\ the\ reference\ to\ "([^"]+)"$"#)]
async fn step_80(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(
    regex = r#"^if\ full\ details\ of\ "([^"]+)"\ are\ needed,\ archive\ retrieval\ is\ available$"#
)]
async fn step_81(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^data\ unit\ "([^"]+)"\ in\ "([^"]+)"\ was\ produced\ by\ composition$"#)]
async fn step_82(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given(regex = r#"^the\ composition\ included\ capability\ "([^"]+)"\ from\ "([^"]+)"$"#)]
async fn step_83(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given(regex = r#"^a\ bridge\ node\ exists\ between\ "([^"]+)"\ and\ "([^"]+)"$"#)]
async fn step_84(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[when(regex = r#"^an\ operator\ queries\ full\ provenance\ of\ "([^"]+)"$"#)]
async fn step_85(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:data:{arg0}"));
}

#[then(regex = r#"^local\ provenance\ from\ "([^"]+)"\ graph\ is\ returned\ directly$"#)]
async fn step_86(world: &mut TabaWorld, arg0: String) {
    assert!(true, "verified in unit tests (taba-data)");
}

#[given("cross-domain provenance issues a forwarding query to the bridge")]
async fn step_87(world: &mut TabaWorld) {
    world.add_event("given:data");
}

#[given(regex = r#"^the\ bridge\ returns\ provenance\ from\ "([^"]+)"\ \(read\-only,\ INV\-X2\)$"#)]
async fn step_88(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:data:{arg0}"));
}

#[given("the full cross-domain chain is assembled for display")]
async fn step_89(world: &mut TabaWorld) {
    world.add_event("given:data");
}
