//! Single-node end-to-end test:
//! init -> apply (workload + data) -> status -> compose -> audit

use std::path::PathBuf;
use taba_cli::client::LocalClient;
use taba_cli::commands;
use taba_core::{Unit, UnitKind};
use taba_k8s::K8sConverter;

fn write_toml(dir: &std::path::Path, name: &str, toml: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    std::fs::write(&path, toml).expect("write toml");
    path
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn test_single_node_full_workflow() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // 1. init
    commands::run_init(Some(state.clone()), false).expect("init should succeed");
    assert!(state.exists(), "state dir should exist after init");
    assert!(state.join("keypair").exists(), "keypair file should exist");
    assert!(
        state.join("config.json").exists(),
        "config.json should exist"
    );

    // 2. apply a minimal workload unit
    let workload_toml = r#"
[unit]
name = "hello-web"
image = "nginx:alpine"
"#;
    let workload_file = write_toml(tmp.path(), "hello-web", workload_toml);
    commands::run_apply(Some(state.clone()), &workload_file, false)
        .await
        .expect("apply workload should succeed");

    // 3. apply a data unit
    let data_toml = r#"
[unit]
name = "app-config"
type = "data"

[schema]
format = "json"
definition = "schemas/app.json"

[classification]
level = "internal"

[retention]
mode = "persistent"
duration = "7y"
legal_basis = "test"
"#;
    let data_file = write_toml(tmp.path(), "app-config", data_toml);
    commands::run_apply(Some(state.clone()), &data_file, false)
        .await
        .expect("apply data unit should succeed");

    // 4. status -- should show 2 units
    let client = LocalClient::load(Some(state.clone()))
        .await
        .expect("load client");
    let graph_stats = client.graph_stats();
    assert_eq!(graph_stats.active_units, 2, "should have 2 active units");
    assert_eq!(graph_stats.pending_units, 0, "should have 0 pending units");

    // 5. list units
    let units = client.list_units().await.expect("list units");
    assert_eq!(units.len(), 2, "list_units should return 2");
    let kinds: Vec<_> = units.iter().map(taba_core::Unit::kind).collect();
    assert!(
        kinds.contains(&UnitKind::Workload),
        "should contain a workload"
    );
    assert!(
        kinds.contains(&UnitKind::Data),
        "should contain a data unit"
    );

    // 6. compose -- run the solver
    let snapshot = client.snapshot().await.expect("snapshot");
    let result = client.solve(&snapshot);
    assert!(
        result.placements.len() + result.unplaceable.len() > 0,
        "solver should produce results for 2 units"
    );

    // 7. audit trails -- should be empty (no trails recorded yet)
    let trails = client.decision_trails().expect("decision trails");
    assert!(trails.is_empty(), "no trails recorded yet");

    // 8. archive one unit
    let workload_id = units
        .iter()
        .find(|u| u.kind() == UnitKind::Workload)
        .map(taba_core::Unit::id)
        .expect("find workload");
    client
        .archive_unit(&workload_id)
        .await
        .expect("archive should succeed");

    // 9. status after archive
    let graph_stats2 = client.graph_stats();
    assert_eq!(
        graph_stats2.active_units, 1,
        "should have 1 active unit after archive"
    );
    assert_eq!(
        graph_stats2.archived_units, 1,
        "should have 1 archived unit"
    );

    // 10. list after archive
    let units2 = client.list_units().await.expect("list units after archive");
    assert_eq!(units2.len(), 1, "list_units should return 1 after archive");

    // 11. Restart -- drop client, create new one from same state
    drop(client);
    let client2 = LocalClient::load(Some(state.clone()))
        .await
        .expect("load client after restart");
    let units3 = client2
        .list_units()
        .await
        .expect("list units after restart");
    assert_eq!(units3.len(), 1, "graph should persist across restart");

    // 12. Dry run -- validate without inserting
    let dry_toml = r#"
[unit]
name = "dry-run-unit"
image = "busybox:latest"
"#;
    let dry_file = write_toml(tmp.path(), "dry-run", dry_toml);
    commands::run_apply(Some(state.clone()), &dry_file, true)
        .await
        .expect("dry run should succeed");

    let graph_stats3 = client2.graph_stats();
    assert_eq!(
        graph_stats3.active_units, 1,
        "dry run should not add a unit"
    );
}

#[tokio::test]
async fn test_k8s_migration_to_apply() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // 1. init
    commands::run_init(Some(state.clone()), false).expect("init should succeed");

    // 2. K8s convert a Deployment YAML
    let k8s_yaml = r"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: api-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: api
  template:
    spec:
      containers:
        - name: api
          image: api:v2.1.0
          ports:
            - containerPort: 8080
";
    let converter = K8sConverter::new();
    let report = converter
        .convert(k8s_yaml)
        .expect("conversion should succeed");

    assert_eq!(report.generated_count(), 1, "should generate 1 unit");
    assert!(report.generated.contains_key("api-server"));

    // 3. Write the generated TOML and apply it
    let toml_content = &report.generated["api-server"];
    let toml_file = write_toml(tmp.path(), "api-server", toml_content);
    commands::run_apply(Some(state.clone()), &toml_file, false)
        .await
        .expect("apply converted unit should succeed");

    // 4. Verify the unit is in the graph
    let client = LocalClient::load(Some(state.clone()))
        .await
        .expect("load client");
    let units = client.list_units().await.expect("list units");
    assert_eq!(units.len(), 1, "should have 1 unit after K8s migration");
    assert_eq!(units[0].kind(), UnitKind::Workload);

    // 5. The generated TOML should contain the image
    let Unit::Workload(workload) = &units[0] else {
        panic!("expected workload")
    };
    assert!(workload.artifact.artifact_ref.contains("api:v2.1.0"));
    assert_eq!(workload.scaling.min_instances, 3);
}

#[tokio::test]
async fn test_k8s_migration_unmappable() {
    let yaml = r"
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: web-ingress
spec:
  rules:
    - host: example.com
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
data:
  key: value
";
    let converter = K8sConverter::new();
    let report = converter.convert(yaml).expect("conversion should succeed");

    assert_eq!(report.unmappable_count(), 1, "Ingress should be unmappable");
    assert_eq!(
        report.generated_count(),
        1,
        "ConfigMap should generate a unit"
    );
    assert_eq!(report.unmappable[0].kind, "Ingress");
    assert!(report.generated.contains_key("app-config"));
}

#[tokio::test]
async fn test_signed_unit_roundtrip() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init should succeed");

    let client = LocalClient::load(Some(state.clone()))
        .await
        .expect("load client");

    let unit = Unit::Workload(
        taba_test_harness::WorkloadUnitBuilder::new()
            .with_id(taba_common::UnitId(uuid::Uuid::new_v4()))
            .build(),
    );

    let signed = client.sign_unit(&unit).expect("sign should succeed");

    assert_ne!(
        signed.signature.0, [0u8; 64],
        "signature should be non-zero (real Ed25519)"
    );

    assert_eq!(signed.signer, *client.public_key());
}

#[tokio::test]
async fn test_graph_persistence_format() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    commands::run_init(Some(state.clone()), false).expect("init should succeed");

    let toml = r#"
[unit]
name = "persist-test"
image = "test:latest"
"#;
    let file = write_toml(tmp.path(), "persist-test", toml);
    commands::run_apply(Some(state.clone()), &file, false)
        .await
        .expect("apply should succeed");

    let graph_json = state.join("graph.json");
    assert!(graph_json.exists(), "graph.json should exist");

    let json_str = std::fs::read_to_string(&graph_json).expect("read graph.json");
    let units: Vec<Unit> =
        serde_json::from_str(&json_str).expect("graph.json should be valid JSON array of units");

    assert_eq!(units.len(), 1, "graph.json should contain 1 unit");
    assert_eq!(units[0].kind(), UnitKind::Workload);
}
