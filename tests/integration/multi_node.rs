//! Multi-node integration tests.
//!
//! These tests simulate two taba nodes (`LocalClients`) sharing state
//! via the same graph.json file.

use std::path::PathBuf;
use taba_cli::client::LocalClient;
use taba_cli::commands;
use taba_core::{Unit, UnitKind};

fn write_toml(dir: &std::path::Path, name: &str, toml: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    std::fs::write(&path, toml).expect("write toml");
    path
}

#[tokio::test]
async fn test_cross_node_graph_persistence() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state_a = tmp.path().join("node-a-state");
    let state_b = tmp.path().join("node-b-state");

    // 1. Node A: init + apply a workload
    commands::run_init(Some(state_a.clone()), false).expect("node A init");
    let workload_toml = r#"
[unit]
name = "shared-service"
image = "nginx:alpine"
"#;
    let file = write_toml(tmp.path(), "shared-service", workload_toml);
    commands::run_apply(Some(state_a.clone()), &file, false)
        .await
        .expect("node A apply");

    let client_a = LocalClient::load_unverified(Some(state_a.clone()))
        .await
        .expect("node A load");
    assert_eq!(
        client_a.graph_stats().active_units,
        2,
        "node A should have 1 workload + 1 governance"
    );

    // 2. Copy graph.json from node A to node B's state
    std::fs::create_dir_all(&state_b).expect("create node B state dir");
    commands::run_init(Some(state_b.clone()), false).expect("node B init");

    let graph_json_a = state_a.join("graph.json");
    let graph_json_b = state_b.join("graph.json");
    if graph_json_a.exists() {
        std::fs::copy(&graph_json_a, &graph_json_b).expect("copy graph.json from A to B");
    }

    // 3. Node B: load and verify it sees node A's unit
    let client_b = LocalClient::load_unverified(Some(state_b.clone()))
        .await
        .expect("node B load");
    let units_b = client_b.list_units().await.expect("node B list units");
    let non_gov_b: Vec<_> = units_b
        .iter()
        .filter(|u| !matches!(u, Unit::Governance(_)))
        .collect();
    assert_eq!(
        non_gov_b.len(),
        1,
        "node B should see 1 non-governance unit (from node A via graph.json)"
    );
    assert_eq!(non_gov_b[0].kind(), UnitKind::Workload);
}

#[tokio::test]
async fn test_multi_node_merge() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state_a = tmp.path().join("node-a");
    let state_b = tmp.path().join("node-b");

    // 1. Node A: init + apply unit-1
    commands::run_init(Some(state_a.clone()), false).expect("node A init");
    let toml_a = r#"
[unit]
name = "unit-from-a"
image = "app-a:v1"
"#;
    let file_a = write_toml(tmp.path(), "unit-a", toml_a);
    let _result_a = commands::run_apply(Some(state_a.clone()), &file_a, false).await;

    // 2. Node B: init + apply unit-2
    commands::run_init(Some(state_b.clone()), false).expect("node B init");
    let toml_b = r#"
[unit]
name = "unit-from-b"
image = "app-b:v1"
"#;
    let file_b = write_toml(tmp.path(), "unit-b", toml_b);
    let _result_b = commands::run_apply(Some(state_b.clone()), &file_b, false).await;
    // 3. Load both clients
    let client_a = LocalClient::load_unverified(Some(state_a.clone()))
        .await
        .expect("node A load");
    let units_a = client_a.list_units().await.expect("node A units");

    let client_b = LocalClient::load_unverified(Some(state_b.clone()))
        .await
        .expect("node B load");
    let units_b = client_b.list_units().await.expect("node B units");
    assert_eq!(
        units_a.len(),
        2,
        "node A should have 1 workload + 1 governance"
    );
    assert_eq!(
        units_a.len(),
        2,
        "node A should have 1 workload + 1 governance"
    );
    assert_eq!(
        units_b.len(),
        2,
        "node B should have 1 workload + 1 governance"
    );

    // 4. Merge units from B into A's graph (simulating CRDT merge)
    for unit in &units_b {
        client_a
            .insert_unit(unit.clone())
            .await
            .expect("merge unit from B into A");
    }

    // 5. Node A should now have 2 units (merge is additive)
    let units_merged = client_a.list_units().await.expect("merged list");
    assert_eq!(
        units_merged
            .iter()
            .filter(|u| !matches!(u, Unit::Governance(_)))
            .count(),
        2,
        "node A should have 2 non-governance units after merge"
    );

    // 6. Solver should handle both units
    let snapshot = client_a.snapshot().await.expect("snapshot");
    let result = client_a.solve(&snapshot);
    assert!(
        !result.placements.is_empty() || !result.unplaceable.is_empty(),
        "solver should produce results for merged graph"
    );
}

#[tokio::test]
async fn test_cross_node_archive() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // 1. init + apply 2 units
    commands::run_init(Some(state.clone()), false).expect("init");

    let toml1 = r#"
[unit]
name = "unit-1"
image = "app1:v1"
"#;
    let file1 = write_toml(tmp.path(), "unit-1", toml1);
    commands::run_apply(Some(state.clone()), &file1, false)
        .await
        .expect("apply unit-1");

    let toml2 = r#"
[unit]
name = "unit-2"
image = "app2:v1"
"#;
    let file2 = write_toml(tmp.path(), "unit-2", toml2);
    commands::run_apply(Some(state.clone()), &file2, false)
        .await
        .expect("apply unit-2");

    // 2. Archive unit-1
    let client = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("load");
    let units = client.list_units().await.expect("list");
    let id_to_archive = units
        .iter()
        .find(|u| !matches!(u, taba_core::Unit::Governance(_)))
        .expect("non-gov unit")
        .id();
    client.archive_unit(&id_to_archive).await.expect("archive");

    // 3. Drop client, reload
    drop(client);
    let client2 = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("reload");

    let stats2 = client2.graph_stats();
    assert_eq!(
        stats2.active_units, 2,
        "should have 1 workload + 1 governance"
    );
    // Archived units are not persisted in M5 (graph.json only contains active units)

    let units2 = client2.list_units().await.expect("list after restart");
    assert_eq!(
        units2
            .iter()
            .filter(|u| !matches!(u, Unit::Governance(_)))
            .count(),
        1,
        "list should show 1 active non-governance unit"
    );

    let archived_in_list = units2.iter().any(|u| u.id() == id_to_archive);
    assert!(
        !archived_in_list,
        "archived unit should not be in active list"
    );
}

#[tokio::test]
async fn test_solver_determinism_across_restart() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // 1. init + apply a workload
    commands::run_init(Some(state.clone()), false).expect("init");

    let toml = r#"
[unit]
name = "deterministic-test"
image = "test:v1"

[provides]
api = { type = "network" }
"#;
    let file = write_toml(tmp.path(), "det-test", toml);
    commands::run_apply(Some(state.clone()), &file, false)
        .await
        .expect("apply");

    // 2. Run solver -- capture result
    let client1 = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("load 1");
    let snapshot1 = client1.snapshot().await.expect("snapshot 1");
    let result1 = client1.solve(&snapshot1);

    // 3. Drop, reload, run solver again
    drop(client1);
    let client2 = LocalClient::load_unverified(Some(state.clone()))
        .await
        .expect("load 2");
    let snapshot2 = client2.snapshot().await.expect("snapshot 2");
    let result2 = client2.solve(&snapshot2);

    // 4. Results should be identical (deterministic solver, INV-C3)
    assert_eq!(
        result1.placements.len(),
        result2.placements.len(),
        "solver should produce same number of placements after restart"
    );

    if !result1.placements.is_empty() {
        assert_eq!(
            result1.placements[0].unit, result2.placements[0].unit,
            "first placement should be the same unit"
        );
        assert_eq!(
            result1.placements[0].node, result2.placements[0].node,
            "first placement should be on the same node"
        );
    }
}
