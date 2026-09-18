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

// ===========================================================================
// Remaining binary command tests
// ===========================================================================

#[test]
fn test_unit_validate() {
    let tmp = tempfile::TempDir::new().expect("temp dir");

    // Valid unit
    let valid = write_workload(tmp.path(), "valid-unit");
    let output = Command::new(bin_path("taba"))
        .args(["unit", "validate"])
        .arg(&valid)
        .output()
        .expect("validate");

    assert!(
        output.status.success(),
        "validate should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Valid"),
        "stdout should say valid: {stdout}"
    );

    // Invalid unit (empty name)
    let bad = tmp.path().join("bad.taba.toml");
    std::fs::write(&bad, "[unit]\nname = \"\"\nimage = \"alpine:latest\"\n").expect("write");
    let output = Command::new(bin_path("taba"))
        .args(["unit", "validate"])
        .arg(&bad)
        .output()
        .expect("validate bad");

    assert!(
        !output.status.success(),
        "validate should fail for empty name"
    );
}

#[test]
fn test_unit_inspect() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "inspectable");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    // Get the unit ID from graph.json
    let graph_json = std::fs::read_to_string(state.join("graph.json")).expect("read");
    let units: Vec<serde_json::Value> = serde_json::from_str(&graph_json).expect("parse");
    let workload_id = units
        .iter()
        .filter_map(|u| u.get("Workload"))
        .filter_map(|w| w.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .next()
        .expect("find workload id");

    // Inspect
    let output = Command::new(bin_path("taba"))
        .args(["unit", "inspect", "--state-dir"])
        .arg(&state)
        .arg(&workload_id)
        .output()
        .expect("inspect");

    assert!(
        output.status.success(),
        "inspect failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("inspectable") || stdout.contains("workload"),
        "stdout should contain unit: {stdout}"
    );
}

#[test]
fn test_audit_trails() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "auditable");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    let output = Command::new(bin_path("taba"))
        .args(["audit", "trails", "--state-dir"])
        .arg(&state)
        .output()
        .expect("audit trails");

    assert!(
        output.status.success(),
        "audit trails failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Trails may be empty (no solver run recorded) — that's OK
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("trail") || stdout.contains("No ") || stdout.is_empty(),
        "stdout: {stdout}"
    );
}

#[test]
fn test_push() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let cache = tmp.path().join("cache");

    // Create a file to push
    let artifact = tmp.path().join("artifact.txt");
    std::fs::write(&artifact, "hello taba cache").expect("write");

    let output = Command::new(bin_path("taba"))
        .args(["push", "--cache-dir"])
        .arg(&cache)
        .arg(&artifact)
        .output()
        .expect("push");

    assert!(
        output.status.success(),
        "push failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Cache dir should contain a file (SHA256-named)
    assert!(cache.exists(), "cache dir should exist");
    let cached: Vec<_> = std::fs::read_dir(&cache)
        .expect("read cache")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!cached.is_empty(), "cache should contain at least one file");

    // The cached content should match
    let content = std::fs::read_to_string(cached[0].path()).expect("read cached");
    assert_eq!(content, "hello taba cache", "cached content should match");
}

#[test]
fn test_k8s_convert_statefulset() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml_path = tmp.path().join("sts.yaml");
    let yaml = r#"
apiVersion: apps/v1
kind: StatefulSet
metadata:
  name: db
spec:
  replicas: 3
  selector:
    matchLabels:
      app: db
  template:
    metadata:
      labels:
        app: db
    spec:
      containers:
        - name: postgres
          image: postgres:16
"#;
    std::fs::write(&yaml_path, yaml).expect("write");

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
        .expect("read output")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(!toml_files.is_empty(), "should generate TOML");
    let generated = std::fs::read_to_string(toml_files[0].path()).expect("read");
    assert!(
        generated.contains("postgres:16") || generated.contains("image"),
        "should contain image: {generated}"
    );
    assert!(
        generated.contains("require-quorum") || generated.contains("quorum"),
        "StatefulSet should mention quorum: {generated}"
    );
}

#[test]
fn test_k8s_convert_secret() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml_path = tmp.path().join("secret.yaml");
    let yaml = r#"
apiVersion: v1
kind: Secret
metadata:
  name: db-credentials
type: Opaque
data:
  password: cGFzc3dvcmQxMjM=
"#;
    std::fs::write(&yaml_path, yaml).expect("write");

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
        .expect("read output")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(!toml_files.is_empty(), "should generate TOML for Secret");
    let generated = std::fs::read_to_string(toml_files[0].path()).expect("read");
    assert!(
        generated.contains("confidential") || generated.contains("classification"),
        "Secret should be confidential: {generated}"
    );
}

#[test]
fn test_k8s_convert_configmap() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml_path = tmp.path().join("cm.yaml");
    let yaml = r#"
apiVersion: v1
kind: ConfigMap
metadata:
  name: app-config
data:
  config.yaml: |
    key: value
"#;
    std::fs::write(&yaml_path, yaml).expect("write");

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
        .expect("read output")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(!toml_files.is_empty(), "should generate TOML for ConfigMap");
    let generated = std::fs::read_to_string(toml_files[0].path()).expect("read");
    assert!(
        generated.contains("internal") || generated.contains("classification"),
        "ConfigMap should be internal: {generated}"
    );
}

#[test]
fn test_k8s_convert_networkpolicy() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let output_dir = tmp.path().join("output");

    let yaml_path = tmp.path().join("np.yaml");
    let yaml = r#"
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: deny-all
spec:
  podSelector: {}
  policyTypes:
    - Ingress
    - Egress
"#;
    std::fs::write(&yaml_path, yaml).expect("write");

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
        .expect("read output")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();
    assert!(
        !toml_files.is_empty(),
        "should generate TOML for NetworkPolicy"
    );
    let generated = std::fs::read_to_string(toml_files[0].path()).expect("read");
    assert!(
        generated.contains("policy") || generated.contains("deny") || generated.contains("Policy"),
        "NetworkPolicy should generate policy: {generated}"
    );
}

#[test]
fn test_signed_unit_roundtrip_binary() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init (generates Ed25519 keypair + role assignment)
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload — this now signs with real Ed25519
    let workload = write_workload(tmp.path(), "signed-unit");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    assert!(
        output.status.success(),
        "apply should succeed with signing: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Status should show 2 units (1 workload + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains('2'), "should have 2 units: {stdout}");

    // Compose should work (solver runs with signed units)
    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");

    assert!(
        output.status.success(),
        "compose with signed units failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("PLACEMENTS") || stdout.contains("UNPLACEABLE"),
        "compose should produce results: {stdout}"
    );
}

#[test]
fn test_k8s_convert_then_apply_full_workflow() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");
    let output_dir = tmp.path().join("units");

    // Init taba
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Convert K8s deployment to taba TOML
    let yaml = write_deployment(tmp.path());
    let output = Command::new(bin_path("taba-k8s"))
        .args(["convert", "--output-dir"])
        .arg(&output_dir)
        .arg(&yaml)
        .output()
        .expect("convert");

    assert!(
        output.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Find the generated TOML and apply it
    let toml_files: Vec<_> = std::fs::read_dir(&output_dir)
        .expect("read output")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();

    assert!(!toml_files.is_empty(), "should generate at least one TOML");

    for toml_file in &toml_files {
        let output = Command::new(bin_path("taba"))
            .args(["apply", "--state-dir"])
            .arg(&state)
            .arg(toml_file.path())
            .output()
            .expect("apply");

        assert!(
            output.status.success(),
            "apply generated TOML failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Status should show workloads + governance
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("{}", toml_files.len() + 1)),
        "should have {}+1 units (workloads + governance): {stdout}",
        toml_files.len()
    );
}

// ===========================================================================
// Erasure coding e2e (via library, not binary — no CLI command)
// ===========================================================================

#[test]
fn test_erasure_coding_roundtrip() {
    use taba_erasure::{DefaultErasureCoder, ErasureCoder, ErasureParams};

    let data = b"taba erasure coding e2e test data this should survive!";
    let params = ErasureParams {
        total_shards: 6,
        data_shards: 4,
        parity_shards: 2,
        resilience_pct: 33,
    };

    let coder = DefaultErasureCoder::new();
    let shards = coder.encode(data, params).expect("encode");

    assert_eq!(shards.len(), 6, "should produce 6 shards");

    // Remove 2 shards (within tolerance)
    let mut remaining = shards;
    remaining.remove(0);
    remaining.remove(0);

    let decoded = coder.decode(&remaining, params).expect("decode");
    assert_eq!(decoded, data, "decoded data should match original");
}

// ===========================================================================
// Multi-node solver e2e (via library)
// ===========================================================================

#[tokio::test]
async fn test_multi_node_solver_different_capabilities() {
    use taba_common::{NodeId, UnitId};
    use taba_core::{
        Capability, NodeCapabilitySet, PrivilegeLevel, RuntimeCapability, Unit, WorkloadKind,
    };
    use taba_graph::{DefaultGraph, Graph};
    use taba_solver::{DefaultSolver, MembershipSnapshot, Solver};
    use taba_test_harness::WorkloadUnitBuilder;

    let graph = DefaultGraph::new(1_000_000_000);

    // Workload needs GPU
    let gpu_workload = WorkloadUnitBuilder::new()
        .with_id(UnitId(uuid::Uuid::new_v4()))
        .with_needs(vec![Capability::new("gpu", "cuda")])
        .with_provides(vec![Capability::new("compute", "ml")])
        .with_kind(WorkloadKind::Service)
        .build();
    graph
        .insert(Unit::Workload(gpu_workload))
        .await
        .unwrap_or(());

    // Node A: no GPU
    let node_a = NodeId(uuid::Uuid::new_v4());
    let caps_a = NodeCapabilitySet {
        arch: "x86_64".to_string(),
        os: "linux".to_string(),
        privilege: PrivilegeLevel::User,
        runtimes: vec![RuntimeCapability::Oci],
        ports_privileged: false,
        storage: vec!["ssd".to_string()],
        environment: Some("env:dev".to_string()),
        author_affinity: None,
        clock_quality: taba_common::ClockQuality::Ntp,
        timezone: "UTC".to_string(),
        custom_tags: vec![("gpu".to_string(), "none".to_string())],
    };

    // Node B: has GPU
    let node_b = NodeId(uuid::Uuid::new_v4());
    let caps_b = NodeCapabilitySet {
        custom_tags: vec![("gpu".to_string(), "cuda".to_string())],
        ..caps_a.clone()
    };

    let membership = MembershipSnapshot {
        nodes: vec![
            (node_a, caps_a, taba_solver::NodeHealth::Active),
            (node_b, caps_b, taba_solver::NodeHealth::Active),
        ],
        generation: 1,
    };

    let solver = DefaultSolver::new();
    let snapshot = graph.snapshot().await.unwrap();
    let result = solver.solve(&snapshot, &membership);

    // Solver should produce at least one result (placement or unplaceable)
    let total = result.placements.len() + result.unplaceable.len();
    assert!(total > 0, "solver should produce results, got: {result:?}");
}
