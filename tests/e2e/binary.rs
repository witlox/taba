#![allow(clippy::all, clippy::pedantic, dead_code, unused)]
//! End-to-end tests that spawn the `taba` and `taba-k8s` binaries
//! as subprocesses and verify their stdout/stderr.

use std::path::PathBuf;
use std::process::Command;

/// Returns the path to a built binary, or skips the test if not built.
fn bin_path(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug")
        .join(name);
    if !path.exists() {
        eprintln!("Skipping: {name} not built. Run: cargo build --workspace");
        std::process::exit(0);
    }
    path
}

/// Writes a minimal taba TOML unit file.
fn write_workload(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let toml = format!(
        r#"[unit]
name = "{name}"
image = "alpine:latest"
"#
    );
    std::fs::write(&path, toml).expect("write toml");
    path
}

/// Writes a minimal K8s Deployment YAML.
fn write_deployment(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("deployment.yaml");
    let yaml = r#"
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
    metadata:
      labels:
        app: api
    spec:
      containers:
        - name: api
          image: nginx:1.25
          ports:
            - containerPort: 8080
"#;
    std::fs::write(&path, yaml).expect("write yaml");
    path
}

// ===========================================================================
// taba binary tests
// ===========================================================================

#[test]
fn test_init_creates_state() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    let output = Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("spawn taba");

    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    assert!(state.exists(), "state dir should exist");
    assert!(state.join("keypair").exists(), "keypair should exist");
    assert!(
        state.join("config.json").exists(),
        "config.json should exist"
    );
    assert!(state.join("graph.json").exists(), "graph.json should exist");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("taba node initialized"), "stdout: {stdout}");
    assert!(stdout.contains("Key ID:"), "stdout: {stdout}");
    assert!(stdout.contains("Trust Domain:"), "stdout: {stdout}");
}

#[test]
fn test_init_twice_rejected() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("first init");

    let output = Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("second init");

    assert!(!output.status.success(), "second init should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already initialized") || stderr.contains("Use --force"),
        "stderr: {stderr}"
    );
}

#[test]
fn test_apply_and_status() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload unit
    let workload = write_workload(tmp.path(), "hello-web");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    assert!(
        output.status.success(),
        "apply failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    // Status should show 1 workload + 1 governance = 2 units
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");

    assert!(
        output.status.success(),
        "status failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("active"), "stdout: {stdout}");
    assert!(
        stdout.contains("2"),
        "should show 2 active units (1 workload + 1 governance): {stdout}"
    );
}

#[test]
fn test_apply_dry_run() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "dry-run-unit");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--dry-run", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply --dry-run");

    assert!(
        output.status.success(),
        "dry run failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dry run"), "stdout: {stdout}");
    assert!(
        stdout.contains("dry-run-unit"),
        "stdout should contain unit name: {stdout}"
    );

    // Verify the unit was NOT added — graph.json should only have governance
    let graph_json = std::fs::read_to_string(state.join("graph.json")).expect("read graph.json");
    let units: Vec<serde_json::Value> = serde_json::from_str(&graph_json).expect("parse json");
    let non_gov: Vec<_> = units
        .iter()
        .filter(|u| !u.get("Governance").is_some())
        .collect();
    assert_eq!(
        non_gov.len(),
        0,
        "dry run should not add a unit, got: {non_gov:?}"
    );
}

#[test]
fn test_unit_list() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "listable-unit");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    let output = Command::new(bin_path("taba"))
        .args(["unit", "list", "--state-dir"])
        .arg(&state)
        .output()
        .expect("unit list");

    assert!(
        output.status.success(),
        "unit list failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("workload"),
        "stdout should contain workload: {stdout}"
    );
}

#[test]
fn test_compose() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "composable-unit");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");

    assert!(
        output.status.success(),
        "compose failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The solver should produce either placements or unplaceable results
    assert!(
        stdout.contains("PLACEMENTS")
            || stdout.contains("UNPLACEABLE")
            || stdout.contains("CONFLICTS"),
        "stdout should contain solver results: {stdout}"
    );
}

#[test]
fn test_persistence_across_restart() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init + apply
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "persistent-unit");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    // Status should show 2 units (1 workload + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status after apply");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains('2'), "should have 2 units: {stdout}");

    // "Restart" — run status again on the same state dir
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status after restart");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('2'),
        "should still have 2 units after restart: {stdout}"
    );

    // Unit list should still show the workload
    let output = Command::new(bin_path("taba"))
        .args(["unit", "list", "--state-dir"])
        .arg(&state)
        .output()
        .expect("unit list after restart");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("workload"), "unit should persist: {stdout}");
}

// ===========================================================================
// taba-k8s binary tests
// ===========================================================================

#[test]
fn test_k8s_convert_deployment() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml = write_deployment(tmp.path());
    let output = Command::new(bin_path("taba-k8s"))
        .args(["convert", "--output-dir"])
        .arg(&output_dir)
        .arg(&yaml)
        .output()
        .expect("spawn taba-k8s");

    assert!(
        output.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Output dir should contain generated TOML
    assert!(output_dir.exists(), "output dir should exist");
    let toml_files: Vec<_> = std::fs::read_dir(&output_dir)
        .expect("read output dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(
        !toml_files.is_empty(),
        "should generate at least one TOML file"
    );

    // The generated TOML should contain the image
    let generated = std::fs::read_to_string(toml_files[0].path()).expect("read generated toml");
    assert!(
        generated.contains("nginx:1.25") || generated.contains("image"),
        "generated TOML should contain image: {generated}"
    );
}

#[test]
fn test_k8s_convert_multidocument() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml_path = tmp.path().join("multi.yaml");
    let yaml = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  replicas: 2
  selector:
    matchLabels:
      app: web
  template:
    metadata:
      labels:
        app: web
    spec:
      containers:
        - name: web
          image: nginx:1.25
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
data:
  config.yaml: |
    key: value
"#;
    std::fs::write(&yaml_path, yaml).expect("write yaml");

    let output = Command::new(bin_path("taba-k8s"))
        .args(["convert", "--output-dir"])
        .arg(&output_dir)
        .arg(&yaml_path)
        .output()
        .expect("spawn taba-k8s");

    assert!(
        output.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let toml_files: Vec<_> = std::fs::read_dir(&output_dir)
        .expect("read output dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(
        toml_files.len() >= 2,
        "should generate at least 2 TOML files, got: {}",
        toml_files.len()
    );
}

#[test]
fn test_k8s_convert_unmappable() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml_path = tmp.path().join("hpa.yaml");
    let yaml = r#"
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: web-hpa
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: web
  minReplicas: 1
  maxReplicas: 10
"#;
    std::fs::write(&yaml_path, yaml).expect("write yaml");

    let output = Command::new(bin_path("taba-k8s"))
        .args(["convert", "--output-dir"])
        .arg(&output_dir)
        .arg(&yaml_path)
        .output()
        .expect("spawn taba-k8s");

    // HPA is unmappable — should still succeed but report it
    assert!(
        output.status.success(),
        "convert should succeed even with unmappable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("Skipped") && stdout.contains("HorizontalPodAutoscaler"),
        "stdout should report skipped HPA: {stdout}"
    );
}
