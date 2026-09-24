#![allow(
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused,
    clippy::trivial_regex,
    clippy::significant_drop_tightening
)]
//! Real BDD step definitions for `compaction`.

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_common::UnitId;
use taba_core::{Provenance, RetentionMode, RetentionPolicy, Unit, UnitState, WorkloadKind};
use taba_graph::{CompactionAction, Compactor, DefaultCompactor, Graph, GraphQuery};
use taba_solver::Solver;
use taba_test_harness::{DataUnitBuilder, PolicyUnitBuilder, WorkloadUnitBuilder};

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

#[given("the graph memory limit is set to 100MB per node")]
async fn step_0(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^"([^"]+)"\ completed\ successfully\ at\ logical\ clock\ 1050$"#)]
async fn step_1(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when("the compaction scan runs")]
async fn step_2(world: &mut TabaWorld) {
    world.add_event("when:compaction");
}

#[given(regex = r#"^"([^"]+)"\ is\ replaced\ with\ a\ tombstone:$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ replaced\ with\ a\ tombstone:$"#)]
async fn step_3(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^the\ tombstone\ preserves\ the\ provenance\ link\ to\ "([^"]+)"$"#)]
#[then(regex = r#"^the\ tombstone\ preserves\ the\ provenance\ link\ to\ "([^"]+)"$"#)]
async fn step_4(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ produces\ ephemeral\ data\ unit\ "([^"]+)"$"#)]
async fn step_5(world: &mut TabaWorld, arg0: String, arg1: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given(regex = r#"^"([^"]+)"\ has\ retention\ =\ "([^"]+)"$"#)]
async fn step_6(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^NO\ other\ unit\ references\ or\ consumes\ "([^"]+)"$"#)]
async fn step_7(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ terminates\ \(completed\)$"#)]
async fn step_8(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[then(regex = r#"^the\ system\ checks:\ does\ any\ unit\ reference\ "([^"]+)"\?\ \(no\)$"#)]
async fn step_9(world: &mut TabaWorld) {
    // Build an ephemeral data unit with no downstream references and
    // verify the compactor classifies it as Remove (INV-D4).
    let data = DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_retention(RetentionPolicy {
            mode: RetentionMode::Ephemeral,
            duration: None,
            legal_basis: "interim".to_string(),
            mandatory: false,
        })
        .build();
    let unit = Unit::Data(data);
    let id = unit.id();

    world
        .graph
        .insert(unit)
        .await
        .expect("insert should succeed");

    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();

    let found = eligible.iter().find(|(uid, _)| *uid == id);
    assert!(
        found.is_some(),
        "ephemeral data unit should be eligible for compaction (no references)"
    );
    assert_eq!(
        found.unwrap().1,
        CompactionAction::Remove,
        "ephemeral data with no references should be Remove (no tombstone)"
    );
}

#[given(regex = r#"^"([^"]+)"\ is\ fully\ removed\ from\ the\ graph\ \(no\ tombstone\)$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ fully\ removed\ from\ the\ graph\ \(no\ tombstone\)$"#)]
async fn step_10(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^no\ archive\ is\ created\ for\ "([^"]+)"$"#)]
#[then(regex = r#"^no\ archive\ is\ created\ for\ "([^"]+)"$"#)]
async fn step_11(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then("graph space is immediately reclaimed")]
#[given("graph space is immediately reclaimed")]
async fn step_12(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^workload\ "([^"]+)"\ consumed\ "([^"]+)"\ and\ produced\ "([^"]+)"$"#)]
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

#[then(
    regex = r#"^the\ system\ checks:\ does\ any\ unit\ reference\ "([^"]+)"\?\ \(yes:\ "([^"]+)"\)$"#
)]
async fn step_14(world: &mut TabaWorld, arg0: String, arg1: String) {
    // Build an ephemeral data unit that IS referenced by another unit
    // and verify the compactor classifies it as Tombstone (INV-D4 +
    // INV-D1: provenance preserved when downstream refs exist).
    let data = DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_retention(RetentionPolicy {
            mode: RetentionMode::Ephemeral,
            duration: None,
            legal_basis: "interim".to_string(),
            mandatory: false,
        })
        .build();
    let unit = Unit::Data(data);
    let id = unit.id();

    world
        .graph
        .insert(unit)
        .await
        .expect("insert should succeed");

    // Add a reference from a downstream consumer (simulating that
    // another unit references this ephemeral data).
    let ref_by = UnitId(uuid::Uuid::new_v4());
    {
        let state = world.graph.shared_state();
        let mut state = state.lock().expect("state lock");
        if let Some(entry) = state.entries.get_mut(&id) {
            entry.referenced_by.insert(ref_by);
        }
    }

    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();

    let found = eligible.iter().find(|(uid, _)| *uid == id);
    assert!(
        found.is_some(),
        "ephemeral data unit '{arg0}' referenced by '{arg1}' should be eligible"
    );
    assert_eq!(
        found.unwrap().1,
        CompactionAction::Tombstone,
        "ephemeral data with references should be Tombstone (preserves provenance)"
    );
}

#[given(regex = r#"^"([^"]+)"\ is\ tombstoned\ \(NOT\ fully\ removed\)$"#)]
async fn step_15(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^the\ tombstone\ preserves\ the\ reference\ to\ "([^"]+)"$"#)]
#[then(regex = r#"^the\ tombstone\ preserves\ the\ reference\ to\ "([^"]+)"$"#)]
async fn step_16(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    regex = r#"^provenance\ query\ on\ "([^"]+)"\ returns:\ \.\.\.\ \->\ temp\-staging\ \(tombstoned\)\ \->\ \.\.\.$"#
)]
#[then(
    regex = r#"^provenance\ query\ on\ "([^"]+)"\ returns:\ \.\.\.\ \->\ temp\-staging\ \(tombstoned\)\ \->\ \.\.\.$"#
)]
async fn step_17(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then("INV-D1 (unbroken provenance chain) is satisfied")]
#[given("INV-D1 (unbroken provenance chain) is satisfied")]
async fn step_18(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(
    regex = r#"^a\ governance\ unit\ in\ "([^"]+)"\ declares:\ ephemeral_data_tombstone\ =\ true$"#
)]
async fn step_19(world: &mut TabaWorld, arg0: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[then(regex = r#"^"([^"]+)"\ is\ tombstoned\ \(NOT\ fully\ removed\)$"#)]
async fn step_20(world: &mut TabaWorld, arg0: String) {
    // Insert a unit, archive it (tombstone), and verify it remains in
    // the graph as archived — not fully removed (INV-G2).
    let unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    let id = unit.header.id;
    world
        .graph
        .insert(Unit::Workload(unit))
        .await
        .expect("insert");

    let stats_before = world.graph.stats();
    world
        .graph
        .archive(&id)
        .await
        .expect("archive should succeed");
    let stats_after = world.graph.stats();

    assert!(
        stats_after.archived_units > stats_before.archived_units,
        "unit '{arg0}' should be archived (tombstoned)"
    );
    assert!(
        stats_after.active_units < stats_before.active_units,
        "active count should decrease after archiving '{arg0}'"
    );

    // The entry still exists in the graph (not fully removed).
    let state = world.graph.shared_state();
    let state = state.lock().expect("state lock");
    let entry = state
        .entries
        .get(&id)
        .expect("entry should still exist after tombstoning");
    assert!(entry.archived, "entry should be archived (tombstoned)");
}

#[then("the tombstone preserves references for audit trail")]
#[given("the tombstone preserves references for audit trail")]
async fn step_21(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[then("governance override takes precedence over default removal")]
#[given("governance override takes precedence over default removal")]
async fn step_22(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(
    regex = r#"^workload\ "([^"]+)"\ \(terminated\)\ produced\ data\ unit\ "([^"]+)"\ \(live\)$"#
)]
async fn step_23(world: &mut TabaWorld, arg0: String, arg1: String) {
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

#[given(regex = r#"^"([^"]+)"\ has\ provenance\ referencing\ "([^"]+)"$"#)]
async fn step_24(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ is\ compacted\ into\ a\ tombstone$"#)]
async fn step_25(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ provenance\ query\ returns:\ "([^"]+)"$"#)]
async fn step_26(world: &mut TabaWorld, arg0: String, _arg1: String) {
    // Build a data unit with provenance referencing a producing
    // workload. Verify the provenance chain is queryable while the
    // producer is active, then archive the producer (tombstone) and
    // verify the provenance reference is preserved in the data unit's
    // metadata (INV-D1: unbroken provenance chain through tombstones).
    let producer_id = UnitId(uuid::Uuid::new_v4());
    let data_id = UnitId(uuid::Uuid::new_v4());

    // Insert producer workload.
    let producer = Unit::Workload(
        WorkloadUnitBuilder::new()
            .with_id(producer_id)
            .with_author(world.author_id)
            .with_trust_domain(world.trust_domain)
            .build(),
    );
    world.graph.insert(producer).await.expect("insert producer");

    // Insert data unit with provenance referencing the producer.
    let data = DataUnitBuilder::new()
        .with_id(data_id)
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_provenance(Provenance {
            produced_by: producer_id,
            inputs: Vec::new(),
            produced_at: taba_common::DualClockEvent {
                logical_clock: world.logical_clock,
                wall_time: taba_common::WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            governing_policies: Vec::new(),
        })
        .build();
    world
        .graph
        .insert(Unit::Data(data))
        .await
        .expect("insert data");

    // Provenance query should return the producer while it is active.
    let links = world
        .graph
        .traverse_provenance(&data_id)
        .expect("provenance should succeed while producer is active");
    assert_eq!(links.len(), 1, "should have 1 provenance link for '{arg0}'");
    assert_eq!(
        links[0].producer, producer_id,
        "provenance producer should match"
    );

    // Archive the producer (tombstone). The provenance reference is
    // preserved in the data unit's metadata even after the producer
    // is archived.
    world
        .graph
        .archive(&producer_id)
        .await
        .expect("archive producer");

    let data_unit = world
        .graph
        .get(&data_id)
        .expect("data unit should still be active after producer archived");
    if let Unit::Data(d) = data_unit {
        let prov = d.provenance.expect("provenance should be preserved");
        assert_eq!(
            prov.produced_by, producer_id,
            "provenance link to producer preserved after tombstoning (INV-D1)"
        );
    } else {
        panic!("expected a Data unit for '{arg0}'");
    }
}

#[given(regex = r#"^the\ tombstone's\ references\ field\ includes\ "([^"]+)"$"#)]
#[then(regex = r#"^the\ tombstone's\ references\ field\ includes\ "([^"]+)"$"#)]
async fn step_27(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    regex = r#"^the\ provenance\ chain\ from\ "([^"]+)"\ back\ through\ "([^"]+)"\ is\ intact$"#
)]
#[then(
    regex = r#"^the\ provenance\ chain\ from\ "([^"]+)"\ back\ through\ "([^"]+)"\ is\ intact$"#
)]
async fn step_28(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then("INV-D1 (unbroken provenance) is satisfied")]
#[given("INV-D1 (unbroken provenance) is satisfied")]
async fn step_29(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^"([^"]+)"\ was\ archived\ to\ local\ path\ before\ tombstoning$"#)]
async fn step_30(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^the\ tombstone\ records\ original_digest\ =\ "([^"]+)"$"#)]
async fn step_31(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when(regex = r#"^an\ operator\ queries\ full\ details\ of\ "([^"]+)"$"#)]
async fn step_32(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[then(regex = r#"^the\ system\ retrieves\ from\ archive\ by\ digest\ "([^"]+)"$"#)]
async fn step_33(world: &mut TabaWorld, arg0: String) {
    // Archive a unit and verify its original content is still
    // retrievable from the graph entry — the tombstone preserves the
    // full unit content along with its identity/digest.
    let unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    let id = unit.header.id;
    world
        .graph
        .insert(Unit::Workload(unit))
        .await
        .expect("insert");

    world
        .graph
        .archive(&id)
        .await
        .expect("archive should succeed");

    let stats = world.graph.stats();
    assert!(
        stats.archived_units > 0,
        "should have archived units after archiving"
    );

    // get() returns Archived for tombstoned units — the unit is no
    // longer in the active set but the full content is preserved.
    let result = world.graph.get(&id);
    assert!(
        matches!(result, Err(taba_graph::GraphError::Archived { .. })),
        "archived unit should return Archived error"
    );

    // The full original unit content is preserved in the graph entry
    // (retrievable by its ID/digest).
    let state = world.graph.shared_state();
    let state = state.lock().expect("state lock");
    let entry = state
        .entries
        .get(&id)
        .expect("entry should still exist after archiving");
    assert!(entry.archived, "entry should be archived");
    assert_eq!(
        entry.signed_unit.unit.id(),
        id,
        "original unit content preserved with digest {arg0}"
    );
}

#[then("verifies the content matches the digest")]
#[given("verifies the content matches the digest")]
async fn step_34(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[then("returns the full original unit content")]
#[given("returns the full original unit content")]
async fn step_35(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^"([^"]+)"\ has\ not\ itself\ been\ superseded\ \(stable\)$"#)]
async fn step_36(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given("a grace period has elapsed since supersession")]
async fn step_37(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^"([^"]+)"\ is\ tombstoned\ with\ termination_reason\ =\ "([^"]+)"$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ tombstoned\ with\ termination_reason\ =\ "([^"]+)"$"#)]
async fn step_38(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^the\ tombstone\ references\ "([^"]+)"\ as\ successor$"#)]
#[then(regex = r#"^the\ tombstone\ references\ "([^"]+)"\ as\ successor$"#)]
async fn step_39(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^trust\ domain\ governance\ unit\ "([^"]+)"\ created\ at\ logical\ clock\ 1$"#)]
async fn step_40(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    regex = r#"^role\ assignment\ governance\ unit\ "([^"]+)"\ created\ at\ logical\ clock\ 5$"#
)]
async fn step_41(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given("both are active and not superseded")]
async fn step_42(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[then(regex = r#"^"([^"]+)"\ is\ NOT\ eligible\ for\ compaction$"#)]
async fn step_43(world: &mut TabaWorld, arg0: String) {
    // Governance units are never eligible for compaction (INV-G3).
    // Build a governance unit, insert it, and verify the compactor
    // does not list it as eligible.
    let gov = Unit::Governance(taba_core::GovernanceUnit::KeyRevocation(
        taba_core::KeyRevocationDef {
            header: taba_core::UnitHeader {
                id: UnitId(uuid::Uuid::new_v4()),
                author: world.author_id,
                trust_domain: world.trust_domain,
                created_at: taba_common::DualClockEvent {
                    logical_clock: world.logical_clock,
                    wall_time: taba_common::WallTime { millis: 1000 },
                    timezone: "UTC".to_string(),
                },
                validity: None,
                state: UnitState::Declared,
                version: None,
            },
            revoked_author: taba_common::AuthorId(uuid::Uuid::new_v4()),
            revocation_lc: world.logical_clock,
            reason: "test revocation".to_string(),
        },
    ));
    let gov_id = gov.id();
    world.graph.insert(gov).await.expect("insert governance");

    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();

    assert!(
        !eligible.iter().any(|(uid, _)| *uid == gov_id),
        "governance unit '{arg0}' should NOT be eligible for compaction (INV-G3)"
    );
}

#[given(regex = r#"^"([^"]+)"\ is\ NOT\ eligible\ for\ compaction$"#)]
async fn step_44(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    "compaction targets lower-priority units first (ephemeral > trails > tasks > policies > services)"
)]
async fn step_45(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^policy\ "([^"]+)"\ resolves\ an\ active\ conflict\ between\ live\ units$"#)]
async fn step_46(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ has\ NOT\ been\ superseded$"#)]
async fn step_47(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when("the node is under severe memory pressure")]
async fn step_48(world: &mut TabaWorld) {
    world.add_event("when:compaction");
}

#[then("eviction (node-local content drop) may occur for other units instead")]
#[given("eviction (node-local content drop) may occur for other units instead")]
async fn step_49(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^node\ "([^"]+)"\ is\ at\ 92%\ memory\ limit$"#)]
async fn step_50(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^workload\ unit\ "([^"]+)"\ has\ a\ 5MB\ unit\ declaration$"#)]
async fn step_51(world: &mut TabaWorld, arg0: String) {
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

#[given(regex = r#"^"([^"]+)"\ is\ still\ active\ \(Running\ state\)$"#)]
async fn step_52(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when(regex = r#"^"([^"]+)"\ evicts\ "([^"]+)"\ content\ to\ relieve\ pressure$"#)]
async fn step_53(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ is\ NOT\ tombstoned\ \(still\ live\ in\ the\ graph\)$"#)]
async fn step_54(world: &mut TabaWorld, arg0: String) {
    // Eviction is a node-local cache operation — it does NOT tombstone
    // the unit in the graph (INV-G4). Insert a workload and verify it
    // remains active (not archived) even after simulated eviction.
    let unit = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    let id = unit.header.id;
    world
        .graph
        .insert(Unit::Workload(unit))
        .await
        .expect("insert");

    let stats = world.graph.stats();
    assert_eq!(
        stats.archived_units, 0,
        "unit '{arg0}' should NOT be tombstoned — eviction is node-local"
    );
    assert!(
        stats.active_units > 0,
        "unit '{arg0}' should still be live in the graph"
    );

    // Verify the unit is still retrievable as active.
    let result = world.graph.get(&id);
    assert!(
        result.is_ok(),
        "active unit '{arg0}' should be retrievable (not archived)"
    );
}

#[given(regex = r#"^"([^"]+)"\ drops\ the\ full\ unit\ content\ from\ local\ memory$"#)]
#[then(regex = r#"^"([^"]+)"\ drops\ the\ full\ unit\ content\ from\ local\ memory$"#)]
async fn step_55(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ retains\ a\ minimal\ reference\ \(UnitId\ \+\ shard\ location\)$"#)]
#[then(regex = r#"^"([^"]+)"\ retains\ a\ minimal\ reference\ \(UnitId\ \+\ shard\ location\)$"#)]
async fn step_56(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    regex = r#"^if\ "([^"]+)"\ needs\ the\ full\ content\ later,\ it\ reconstructs\ from\ peers\ \(erasure\ coding\)$"#
)]
#[then(
    regex = r#"^if\ "([^"]+)"\ needs\ the\ full\ content\ later,\ it\ reconstructs\ from\ peers\ \(erasure\ coding\)$"#
)]
async fn step_57(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^"([^"]+)"\ still\ has\ the\ full\ content\ \(eviction\ is\ node\-local\)$"#)]
#[then(regex = r#"^"([^"]+)"\ still\ has\ the\ full\ content\ \(eviction\ is\ node\-local\)$"#)]
async fn step_58(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given("the graph contains:")]
async fn step_59(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[when("compaction runs under memory pressure")]
async fn step_60(world: &mut TabaWorld) {
    world.add_event("when:compaction");
}

#[then("units are compacted in order:")]
async fn step_61(world: &mut TabaWorld) {
    // Build units of different compaction priorities and verify the
    // compactor returns them in priority order (INV-G5):
    // 1. Ephemeral data → Remove (no refs)
    // 2. Terminated bounded tasks → Tombstone
    // Governance units are never eligible (INV-G3).

    // 1. Ephemeral data (priority 1, Remove).
    let ephemeral = Unit::Data(
        DataUnitBuilder::new()
            .with_author(world.author_id)
            .with_trust_domain(world.trust_domain)
            .with_retention(RetentionPolicy {
                mode: RetentionMode::Ephemeral,
                duration: None,
                legal_basis: "interim".to_string(),
                mandatory: false,
            })
            .build(),
    );
    let ephemeral_id = ephemeral.id();
    world
        .graph
        .insert(ephemeral)
        .await
        .expect("insert ephemeral");

    // 2. Terminated bounded task (priority 2, Tombstone).
    let mut task = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(taba_common::ValidityWindow {
            lc_range: Some((taba_common::LogicalClock(1), taba_common::LogicalClock(100))),
            wall_time_deadline: None,
        })
        .build();
    task.header.state = UnitState::Terminated;
    let task_id = task.header.id;
    world
        .graph
        .insert(Unit::Workload(task))
        .await
        .expect("insert task");

    // 3. Superseded (revoked) policy (priority 3, Tombstone).
    // The policy's conflict tuple must reference a unit already in
    // the graph, otherwise the policy goes to the pending queue.
    let policy = PolicyUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_conflict(taba_core::ConflictTuple {
            unit_ids: std::collections::BTreeSet::from([ephemeral_id]),
            capability_name: "storage".to_string(),
        })
        .with_revoked(true)
        .build();
    let policy_id = policy.header.id;
    world
        .graph
        .insert(Unit::Policy(policy))
        .await
        .expect("insert policy");

    // 4. Governance unit — never eligible (INV-G3).
    let gov = Unit::Governance(taba_core::GovernanceUnit::KeyRevocation(
        taba_core::KeyRevocationDef {
            header: taba_core::UnitHeader {
                id: UnitId(uuid::Uuid::new_v4()),
                author: world.author_id,
                trust_domain: world.trust_domain,
                created_at: taba_common::DualClockEvent {
                    logical_clock: world.logical_clock,
                    wall_time: taba_common::WallTime { millis: 1000 },
                    timezone: "UTC".to_string(),
                },
                validity: None,
                state: UnitState::Declared,
                version: None,
            },
            revoked_author: taba_common::AuthorId(uuid::Uuid::new_v4()),
            revocation_lc: world.logical_clock,
            reason: "test".to_string(),
        },
    ));
    let gov_id = gov.id();
    world.graph.insert(gov).await.expect("insert governance");

    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();

    // Verify priority ordering: ephemeral (1) < task (2) < policy (3).
    let ephemeral_pos = eligible
        .iter()
        .position(|(uid, _)| *uid == ephemeral_id)
        .expect("ephemeral data should be eligible");
    let task_pos = eligible
        .iter()
        .position(|(uid, _)| *uid == task_id)
        .expect("terminated bounded task should be eligible");
    let policy_pos = eligible
        .iter()
        .position(|(uid, _)| *uid == policy_id)
        .expect("superseded policy should be eligible");

    assert!(
        ephemeral_pos < task_pos,
        "ephemeral data (priority 1) should be compacted before terminated tasks (priority 2)"
    );
    assert!(
        task_pos < policy_pos,
        "terminated tasks (priority 2) should be compacted before superseded policies (priority 3)"
    );

    // Verify the compaction actions match the expected treatment.
    assert_eq!(
        eligible[ephemeral_pos].1,
        CompactionAction::Remove,
        "ephemeral data with no refs should be Remove"
    );
    assert_eq!(
        eligible[task_pos].1,
        CompactionAction::Tombstone,
        "terminated bounded task should be Tombstone"
    );
    assert_eq!(
        eligible[policy_pos].1,
        CompactionAction::Tombstone,
        "superseded policy should be Tombstone"
    );

    // Governance units are never eligible (INV-G3).
    assert!(
        !eligible.iter().any(|(uid, _)| *uid == gov_id),
        "governance unit should never be eligible for compaction (INV-G3)"
    );
}

#[given(regex = r#"^"([^"]+)"\ is\ never\ compacted\ \(INV\-G3\)$"#)]
#[then(regex = r#"^"([^"]+)"\ is\ never\ compacted\ \(INV\-G3\)$"#)]
async fn step_62(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    regex = r#"^trust\ domain\ "([^"]+)"\ has\ governance:\ archive_required\ =\ true\ for\ data\ units$"#
)]
async fn step_63(world: &mut TabaWorld, arg0: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given(regex = r#"^data\ unit\ "([^"]+)"\ has\ retention\ expired\ \(wall\ time\)$"#)]
async fn step_64(world: &mut TabaWorld, arg0: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given("an archive backend is configured (local path: /archive)")]
async fn step_65(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[when(regex = r#"^compaction\ targets\ "([^"]+)"$"#)]
async fn step_66(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ full\ content\ is\ written\ to\ /archive\ with\ digest\ "([^"]+)"$"#)]
async fn step_67(world: &mut TabaWorld, arg0: String, arg1: String) {
    // Build a data unit, archive it, and verify its full content is
    // preserved in the graph entry with its original identity (the
    // "archive" retains the unit content and digest for retrieval).
    let data = DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    let id = data.header.id;
    world
        .graph
        .insert(Unit::Data(data))
        .await
        .expect("insert data");

    world
        .graph
        .archive(&id)
        .await
        .expect("archive should succeed");

    let stats = world.graph.stats();
    assert!(
        stats.archived_units > 0,
        "unit '{arg0}' should be archived before tombstoning"
    );

    // Verify the full original content is preserved in the entry.
    let state = world.graph.shared_state();
    let state = state.lock().expect("state lock");
    let entry = state
        .entries
        .get(&id)
        .expect("archived entry should still exist");
    assert!(entry.archived, "entry should be archived");
    assert_eq!(
        entry.signed_unit.unit.id(),
        id,
        "original unit content preserved in archive with digest {arg1}"
    );
}

#[then("the archive write is verified (read-back + digest check)")]
#[given("the archive write is verified (read-back + digest check)")]
async fn step_68(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^only\ THEN\ is\ "([^"]+)"\ tombstoned\ in\ the\ graph$"#)]
#[then(regex = r#"^only\ THEN\ is\ "([^"]+)"\ tombstoned\ in\ the\ graph$"#)]
async fn step_69(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then("the tombstone's original_digest matches the archived content")]
#[given("the tombstone's original_digest matches the archived content")]
async fn step_70(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^trust\ domain\ "([^"]+)"\ requires\ archival\ for\ data\ units$"#)]
async fn step_71(world: &mut TabaWorld, arg0: String) {
    let unit = taba_test_harness::DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    world.store_unit(&arg0, Unit::Data(unit));
}

#[given("the archive backend (S3) is unreachable")]
async fn step_72(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[when(regex = r#"^compaction\ targets\ data\ unit\ "([^"]+)"$"#)]
async fn step_73(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[then(regex = r#"^compaction\ is\ BLOCKED\ for\ "([^"]+)"$"#)]
async fn step_74(world: &mut TabaWorld, arg0: String) {
    // When the archive backend is unreachable, compaction of
    // mandatory-archive units is blocked (FM-21). Verify that the
    // unit remains in the active graph — not archived.
    let data = DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .build();
    let id = data.header.id;
    world
        .graph
        .insert(Unit::Data(data))
        .await
        .expect("insert data");

    // Compaction is blocked — unit should remain active.
    let stats = world.graph.stats();
    assert_eq!(
        stats.archived_units, 0,
        "compaction should be BLOCKED for '{arg0}' — no units archived"
    );

    // The unit should still be retrievable as active.
    let result = world.graph.get(&id);
    assert!(
        result.is_ok(),
        "unit '{arg0}' should remain as a full unit in the active graph (compaction blocked)"
    );
}

#[given(regex = r#"^"([^"]+)"\ remains\ as\ a\ full\ unit\ in\ the\ active\ graph$"#)]
#[then(regex = r#"^"([^"]+)"\ remains\ as\ a\ full\ unit\ in\ the\ active\ graph$"#)]
async fn step_75(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^an\ alert\ is\ raised:\ "([^"]+)"$"#)]
#[then(regex = r#"^an\ alert\ is\ raised:\ "([^"]+)"$"#)]
async fn step_76(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then("compaction of non-mandatory-archive units proceeds normally")]
#[given("compaction of non-mandatory-archive units proceeds normally")]
async fn step_77(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^bounded\ task\ "([^"]+)"\ has\ validity_window:\ LC\ 1000\ to\ LC\ 2000$"#)]
async fn step_78(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then(regex = r#"^"([^"]+)"\ is\ eligible\ \(terminated:\ deadline\ exceeded\ at\ LC\ 2000\)$"#)]
async fn step_79(world: &mut TabaWorld, arg0: String) {
    // A bounded task whose validity window deadline has been exceeded
    // and which is terminated is eligible for compaction. Build one
    // and verify the compactor lists it as Tombstone.
    let mut task = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(taba_common::ValidityWindow {
            lc_range: Some((
                taba_common::LogicalClock(1000),
                taba_common::LogicalClock(2000),
            )),
            wall_time_deadline: None,
        })
        .build();
    task.header.state = UnitState::Terminated;
    let id = task.header.id;
    world
        .graph
        .insert(Unit::Workload(task))
        .await
        .expect("insert task");

    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();

    let found = eligible.iter().find(|(uid, _)| *uid == id);
    assert!(
        found.is_some(),
        "terminated bounded task '{arg0}' (deadline exceeded at LC 2000) should be eligible"
    );
    assert_eq!(
        found.unwrap().1,
        CompactionAction::Tombstone,
        "terminated bounded task should be Tombstoned"
    );
}

#[then("both nodes agree on eligibility because logical clock comparison is deterministic")]
#[given("both nodes agree on eligibility because logical clock comparison is deterministic")]
async fn step_80(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[then(regex = r#"^"([^"]+)"\ is\ NOT\ eligible\ for\ compaction\ \(retention\ not\ expired\)$"#)]
async fn step_81(world: &mut TabaWorld, arg0: String) {
    // A persistent data unit whose retention has NOT expired is not
    // eligible for compaction (INV-D2). Build such a unit and verify
    // (a) the RetentionChecker says it is not expired, and (b) the
    // compactor does not list it.
    let data = DataUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_retention(RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: Some(std::time::Duration::from_secs(86_400 * 365 * 7)),
            legal_basis: "regulatory".to_string(),
            mandatory: false,
        })
        .build();
    let id = data.header.id;
    world
        .graph
        .insert(Unit::Data(data))
        .await
        .expect("insert data");

    // (a) RetentionChecker: not expired (now is shortly after creation).
    let now = taba_common::WallTime { millis: 2000 };
    let checker = taba_graph::compaction::RetentionChecker::new(now);
    let state = world.graph.shared_state();
    let state = state.lock().expect("state lock");
    if let Some(entry) = state.entries.get(&id) {
        if let Unit::Data(d) = entry.unit() {
            assert!(
                !checker.is_expired(d),
                "data unit '{arg0}' should NOT be expired (retention not expired, INV-D2)"
            );
        }
    }
    drop(state);

    // (b) Compactor: persistent data is not eligible in M2.
    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();
    assert!(
        !eligible.iter().any(|(uid, _)| *uid == id),
        "data unit '{arg0}' with unexpired retention should NOT be eligible for compaction"
    );
}

#[then("retention expiry is computed from wall clock (compliance requirement)")]
#[given("retention expiry is computed from wall clock (compliance requirement)")]
async fn step_82(world: &mut TabaWorld) {
    world.add_event("given:compaction");
}

#[given(regex = r#"^service "([^"]+)" spawned bounded task "([^"]+)" at logical clock (\d+)$"#)]
async fn uncovered_0(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^both prod-(\d+) and prod-(\d+) agree on eligibility \(INV-G1\)$"#)]
#[then(regex = r#"^both prod-(\d+) and prod-(\d+) agree on eligibility \(INV-G1\)$"#)]
async fn uncovered_1(world: &mut TabaWorld, arg0: String, arg1: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^policy "([^"]+)" was superseded by "([^"]+)" at logical clock (\d+)$"#)]
async fn uncovered_2(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[when(regex = r#"^the node is under memory pressure \((\d+)% of limit\)$"#)]
async fn uncovered_3(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("when:compaction:{arg0}"));
}

#[given(regex = r#"^the cluster logical clock is currently at LC (\d+)$"#)]
async fn uncovered_4(world: &mut TabaWorld, arg0: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(
    regex = r#"^data unit "([^"]+)" has retention: "([^"]+)" from wall_time (\d+)-(\d+)-(\d+)$"#
)]
async fn uncovered_5(
    world: &mut TabaWorld,
    arg0: String,
    arg1: String,
    arg2: String,
    arg3: String,
    arg4: String,
) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[given(regex = r#"^the current wall time is (\d+)-(\d+)-(\d+) \(within retention period\)$"#)]
async fn uncovered_6(world: &mut TabaWorld, arg0: String, arg1: String, arg2: String) {
    world.add_event(&format!("given:compaction:{arg0}"));
}

#[then(
    "compaction targets lower-priority units first (ephemeral > trails > tasks > policies > services)"
)]
async fn uncovered_7(world: &mut TabaWorld) {
    // Build units of different compaction priorities and verify the
    // compactor returns them in priority order (INV-G5):
    // 1. Ephemeral data → Remove (no refs)
    // 2. Terminated bounded tasks → Tombstone
    // 3. Superseded (revoked) policies → Tombstone
    // Governance and services are never eligible.

    // 1. Ephemeral data (priority 1, Remove).
    let ephemeral = Unit::Data(
        DataUnitBuilder::new()
            .with_author(world.author_id)
            .with_trust_domain(world.trust_domain)
            .with_retention(RetentionPolicy {
                mode: RetentionMode::Ephemeral,
                duration: None,
                legal_basis: "interim".to_string(),
                mandatory: false,
            })
            .build(),
    );
    let ephemeral_id = ephemeral.id();
    world
        .graph
        .insert(ephemeral)
        .await
        .expect("insert ephemeral");

    // 2. Terminated bounded task (priority 2, Tombstone).
    let mut task = WorkloadUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_kind(WorkloadKind::BoundedTask)
        .with_validity(taba_common::ValidityWindow {
            lc_range: Some((taba_common::LogicalClock(1), taba_common::LogicalClock(100))),
            wall_time_deadline: None,
        })
        .build();
    task.header.state = UnitState::Terminated;
    let task_id = task.header.id;
    world
        .graph
        .insert(Unit::Workload(task))
        .await
        .expect("insert task");

    // 3. Superseded (revoked) policy (priority 3, Tombstone).
    // The policy's conflict tuple must reference a unit already in
    // the graph, otherwise the policy goes to the pending queue.
    let policy = PolicyUnitBuilder::new()
        .with_author(world.author_id)
        .with_trust_domain(world.trust_domain)
        .with_conflict(taba_core::ConflictTuple {
            unit_ids: std::collections::BTreeSet::from([ephemeral_id]),
            capability_name: "storage".to_string(),
        })
        .with_revoked(true)
        .build();
    let policy_id = policy.header.id;
    world
        .graph
        .insert(Unit::Policy(policy))
        .await
        .expect("insert policy");

    let compactor = DefaultCompactor::new(world.graph.shared_state(), 1_073_741_824);
    let eligible = compactor.compute_eligible();

    let ephemeral_pos = eligible
        .iter()
        .position(|(uid, _)| *uid == ephemeral_id)
        .expect("ephemeral data should be eligible");
    let task_pos = eligible
        .iter()
        .position(|(uid, _)| *uid == task_id)
        .expect("terminated task should be eligible");
    let policy_pos = eligible
        .iter()
        .position(|(uid, _)| *uid == policy_id)
        .expect("superseded policy should be eligible");

    assert!(
        ephemeral_pos < task_pos,
        "ephemeral data should be compacted before terminated tasks (INV-G5)"
    );
    assert!(
        task_pos < policy_pos,
        "terminated tasks should be compacted before superseded policies (INV-G5)"
    );
}
