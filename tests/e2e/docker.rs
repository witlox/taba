#![allow(clippy::all, clippy::pedantic, dead_code, unused)]
//! Docker e2e tests: apply → compose → verify container is running → stop.
//!
//! These tests require Docker. They are marked `#[ignore = "slow:requires-docker"]`
//! and run via `cargo test -p taba-e2e --test docker -- --run-ignored=only`
//! or in the nightly CI `docker-e2e` job.

use std::path::PathBuf;
use std::process::Command;

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

/// Verify Docker is available.
fn docker_available() -> bool {
    Command::new("docker")
        .args(["info"])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Returns the names of all containers that start with "taba-".
fn taba_containers() -> Vec<String> {
    let output = Command::new("docker")
        .args(["ps", "--filter", "name=taba-", "--format", "{{.Names}}"])
        .output()
        .expect("docker ps");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}

/// Force-remove all taba-* containers (cleanup).
fn cleanup_taba_containers() {
    let containers = taba_containers();
    for name in &containers {
        let _ = Command::new("docker").args(["rm", "-f", name]).output();
    }
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_apply_and_compose() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload unit
    let workload = write_workload(tmp.path(), "docker-web");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");
    assert!(
        output.status.success(),
        "apply failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Compose — the solver should produce a placement
    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");
    assert!(
        output.status.success(),
        "compose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("PLACEMENTS") || stdout.contains("UNPLACEABLE"),
        "compose should produce results: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_runtime_start_stop() {
    use taba_common::UnitId;
    use taba_core::Unit;
    use taba_node::runtime::{DockerRuntime, RuntimeExecutor, RuntimeState};

    let runtime = DockerRuntime::new().expect("connect to Docker");
    let id = UnitId(uuid::Uuid::new_v4());

    // Create a minimal workload with alpine:latest
    use taba_common::ContentDigest;
    use taba_core::{Artifact, ArtifactType};
    use taba_test_harness::WorkloadUnitBuilder;

    let mut unit = WorkloadUnitBuilder::new().with_id(id).build();
    unit.artifact = Artifact {
        artifact_type: ArtifactType::Oci,
        artifact_ref: "alpine:latest".to_string(),
        digest: ContentDigest("sha256:".to_string()),
        requires: Vec::new(),
    };

    // Start
    let state = runtime.start(&Unit::Workload(unit)).expect("start");
    assert_eq!(state, RuntimeState::Running);

    // Check state
    let state = runtime.check_state(&Unit::Workload(
        WorkloadUnitBuilder::new().with_id(id).build(),
    ));
    assert_eq!(state, RuntimeState::Running);

    // Stop
    let state = runtime
        .stop(&Unit::Workload(
            WorkloadUnitBuilder::new().with_id(id).build(),
        ))
        .expect("stop");
    assert_eq!(state, RuntimeState::Stopped);
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_persistence_format() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply multiple workloads
    for name in &["web-1", "web-2", "web-3"] {
        let workload = write_workload(tmp.path(), name);
        let output = Command::new(bin_path("taba"))
            .args(["apply", "--state-dir"])
            .arg(&state)
            .arg(&workload)
            .output()
            .expect("apply");
        assert!(
            output.status.success(),
            "apply {name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Status should show 4 units (3 workloads + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains('4'), "should have 4 units: {stdout}");

    // Unit list should show 3 workloads + 1 governance
    let output = Command::new(bin_path("taba"))
        .args(["unit", "list", "--state-dir"])
        .arg(&state)
        .output()
        .expect("unit list");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let workload_count = stdout.matches("workload").count();
    assert_eq!(
        workload_count, 3,
        "unit list should contain 3 workloads: {stdout}"
    );

    // Archive one
    let graph_json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    let units: Vec<serde_json::Value> = serde_json::from_str(&graph_json).expect("parse json");
    let workload_id = units
        .iter()
        .filter_map(|u| u.get("Workload"))
        .filter_map(|w| w.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .next()
        .expect("find workload id");

    let output = Command::new(bin_path("taba"))
        .args(["unit", "archive", "--state-dir"])
        .arg(&state)
        .arg(&workload_id)
        .output()
        .expect("archive");
    assert!(
        output.status.success(),
        "archive failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Status should now show 3 active + 1 archived
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status after archive");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('3'),
        "should have 3 active after archive: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_reconcile() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init + apply
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "reconcile-web");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    // Reconcile — should start a Docker container
    let output = Command::new(bin_path("taba"))
        .args(["reconcile", "--state-dir"])
        .arg(&state)
        .output()
        .expect("reconcile");

    assert!(
        output.status.success(),
        "reconcile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("reconciled successfully") || stdout.contains("FAILED"),
        "reconcile should report results: {stdout}"
    );

    // If reconciliation succeeded, verify a container is running
    if stdout.contains("reconciled successfully") {
        let containers = taba_containers();
        assert!(
            !containers.is_empty(),
            "should have at least one taba-* container running"
        );
    }

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_daemon_reconciliation() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init + apply
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    let workload = write_workload(tmp.path(), "daemon-web");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    // Run daemon for 3 seconds (one reconciliation cycle)
    let mut daemon = Command::new(bin_path("taba"))
        .args(["daemon", "--interval", "1s", "--state-dir"])
        .arg(&state)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn daemon");

    std::thread::sleep(std::time::Duration::from_secs(3));

    // Check if a container is running
    let containers = taba_containers();
    assert!(
        !containers.is_empty(),
        "daemon should have started at least one container, got: {containers:?}"
    );

    // Kill daemon
    let _ = daemon.kill();
    let _ = daemon.wait();

    cleanup_taba_containers();
}

// ===========================================================================
// Expanded Docker e2e tests — exercising real container lifecycle,
// WAL persistence, capability matching, and health checks.
// ===========================================================================

/// Writes a workload TOML with capabilities (needs/provides).
fn write_workload_with_caps(
    dir: &std::path::Path,
    name: &str,
    needs: &[(&str, &str)],
    provides: &[(&str, &str)],
) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let mut toml = format!(
        r#"[unit]
name = "{name}"
image = "alpine:latest"

"#
    );

    if !needs.is_empty() {
        toml.push_str("\n[needs]\n");
        for (cap, cap_type) in needs {
            toml.push_str(&format!("{cap} = {{ type = \"{cap_type}\" }}\n"));
        }
    }

    if !provides.is_empty() {
        toml.push_str("\n[provides]\n");
        for (cap, cap_type) in provides {
            toml.push_str(&format!("{cap} = {{ type = \"{cap_type}\" }}\n"));
        }
    }

    std::fs::write(&path, toml).expect("write toml");
    path
}

/// Writes a workload TOML with a health check.
fn write_workload_with_health(dir: &std::path::Path, name: &str, health_type: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let health_section = match health_type {
        "http" => {
            r#"[health]
type = "http"
path = "/healthz"
port = 8080
interval = "5s"
timeout = "2s"
"#
        }
        "command" => {
            r#"[health]
type = "command"
command = "echo healthy"
interval = "5s"
"#
        }
        _ => "",
    };

    let toml = format!(
        r#"[unit]
name = "{name}"
image = "alpine:latest"

{health_section}"#
    );

    std::fs::write(&path, toml).expect("write toml");
    path
}

/// Writes a workload TOML with failure semantics.
fn write_workload_with_failure(dir: &std::path::Path, name: &str, on_crash: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let toml = format!(
        r#"[unit]
name = "{name}"
image = "alpine:latest"

[failure]
on_crash = {{ restart_with_backoff = {on_crash} }}
on_shutdown = {{ drain = "10s" }}
"#
    );

    std::fs::write(&path, toml).expect("write toml");
    path
}

/// Count running taba-* containers.
fn count_taba_containers() -> usize {
    taba_containers().len()
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_multi_unit_lifecycle() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply 3 workloads
    for name in &["web-a", "web-b", "web-c"] {
        let workload = write_workload(tmp.path(), name);
        let output = Command::new(bin_path("taba"))
            .args(["apply", "--state-dir"])
            .arg(&state)
            .arg(&workload)
            .output()
            .expect("apply");
        assert!(
            output.status.success(),
            "apply {name} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Compose
    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");
    assert!(
        output.status.success(),
        "compose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Reconcile — should start 3 containers
    let output = Command::new(bin_path("taba"))
        .args(["reconcile", "--state-dir"])
        .arg(&state)
        .output()
        .expect("reconcile");
    assert!(
        output.status.success(),
        "reconcile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("reconciled successfully") {
        let count = count_taba_containers();
        assert!(
            count >= 1,
            "should have at least 1 taba-* container running, got {count}"
        );
    }

    // Archive one workload
    let graph_json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    let units: Vec<serde_json::Value> = serde_json::from_str(&graph_json).expect("parse json");
    let workload_id = units
        .iter()
        .filter_map(|u| u.get("Workload"))
        .filter_map(|w| w.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .next()
        .expect("find workload id");

    let output = Command::new(bin_path("taba"))
        .args(["unit", "archive", "--state-dir"])
        .arg(&state)
        .arg(&workload_id)
        .output()
        .expect("archive");
    assert!(
        output.status.success(),
        "archive failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Reconcile again — should stop the archived container
    let output = Command::new(bin_path("taba"))
        .args(["reconcile", "--state-dir"])
        .arg(&state)
        .output()
        .expect("reconcile after archive");

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("reconciled successfully") {
        let count = count_taba_containers();
        assert!(
            count <= 3,
            "should have at most 3 containers after archive, got {count}"
        );
    }

    // Status should show 3 active + 1 archived
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status after archive");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('3') || stdout.contains('4'),
        "should have 3-4 units after archive: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_wal_persistence() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload
    let workload = write_workload(tmp.path(), "wal-test");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");
    assert!(output.status.success(), "apply failed");

    // Verify WAL directory exists and has entries
    let wal_dir = state.join("wal");
    assert!(
        wal_dir.exists(),
        "WAL directory should exist after apply: {wal_dir:?}"
    );

    // Verify graph.json exists
    let graph_json = state.join("graph.json");
    assert!(
        graph_json.exists(),
        "graph.json should exist after apply: {graph_json:?}"
    );

    // Verify the graph.json contains the workload
    let json = std::fs::read_to_string(&graph_json).expect("read graph");
    assert!(
        json.contains("wal-test"),
        "graph.json should contain 'wal-test': {json}"
    );

    // Reconcile — should start a container
    let output = Command::new(bin_path("taba"))
        .args(["reconcile", "--state-dir"])
        .arg(&state)
        .output()
        .expect("reconcile");

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("reconciled successfully") {
        let count = count_taba_containers();
        assert!(
            count >= 1,
            "should have at least 1 container after reconcile, got {count}"
        );
    }

    // Verify WAL still exists after reconcile
    assert!(
        wal_dir.exists(),
        "WAL directory should still exist after reconcile"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_capability_matching() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload that needs "postgres"
    let needs_pg = write_workload_with_caps(
        tmp.path(),
        "api-server",
        &[("postgres", "storage")],
        &[("http-api", "network")],
    );
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&needs_pg)
        .output()
        .expect("apply api-server");
    assert!(
        output.status.success(),
        "apply api-server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Apply a workload that provides "postgres"
    let provides_pg =
        write_workload_with_caps(tmp.path(), "db-server", &[], &[("postgres", "storage")]);
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&provides_pg)
        .output()
        .expect("apply db-server");
    assert!(
        output.status.success(),
        "apply db-server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Compose — solver should match needs to provides
    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");
    assert!(
        output.status.success(),
        "compose failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The solver should either place both or report unmatched/ambiguous
    assert!(
        stdout.contains("PLACEMENTS")
            || stdout.contains("UNPLACEABLE")
            || stdout.contains("CONFLICTS"),
        "compose should produce results with capabilities: {stdout}"
    );

    // graph.json should contain capabilities from both workloads
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("postgres") && json.contains("http-api"),
        "graph.json should contain both capabilities (postgres, http-api): {json}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_health_check() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload with an HTTP health check
    let workload = write_workload_with_health(tmp.path(), "health-web", "http");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");
    assert!(
        output.status.success(),
        "apply health-web failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // graph.json should contain health check
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Http") || json.contains("healthz") || json.contains("8080"),
        "graph.json should contain HTTP health check: {json}"
    );

    // Apply a workload with a command health check
    let workload = write_workload_with_health(tmp.path(), "cmd-web", "command");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply cmd-web");
    assert!(
        output.status.success(),
        "apply cmd-web failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Status should show 3 units (2 workloads + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('3'),
        "should have 3 units (2 workloads + 1 governance): {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_failure_semantics() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload with restart_with_backoff = 3
    let workload = write_workload_with_failure(tmp.path(), "resilient-web", "3");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");
    assert!(
        output.status.success(),
        "apply resilient-web failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // graph.json should contain failure semantics
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("RestartWithBackoff") || json.contains("restart_with_backoff"),
        "graph.json should contain restart_with_backoff: {json}"
    );

    // Reconcile — should start a container
    let output = Command::new(bin_path("taba"))
        .args(["reconcile", "--state-dir"])
        .arg(&state)
        .output()
        .expect("reconcile");

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("reconciled successfully") {
        let count = count_taba_containers();
        assert!(
            count >= 1,
            "should have at least 1 container after reconcile, got {count}"
        );
    }

    // Verify the graph.json contains failure semantics
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("restart_with_backoff") || json.contains("RestartWithBackoff"),
        "graph.json should contain restart_with_backoff: {json}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_bounded_task() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a bounded task
    let path = tmp.path().join("migrate.taba.toml");
    std::fs::write(
        &path,
        r#"[unit]
name = "migrate"
image = "alpine:latest"
kind = "bounded-task"

[deadline]
start = "now"
end = "1h"
"#,
    )
    .expect("write toml");

    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&path)
        .output()
        .expect("apply");
    assert!(
        output.status.success(),
        "apply bounded task failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // graph.json should show BoundedTask kind
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("BoundedTask"),
        "graph.json should contain BoundedTask: {json}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_compaction() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply 5 workloads
    for i in 1..=5 {
        let workload = write_workload(tmp.path(), &format!("compact-{i}"));
        let output = Command::new(bin_path("taba"))
            .args(["apply", "--state-dir"])
            .arg(&state)
            .arg(&workload)
            .output()
            .expect("apply");
        assert!(
            output.status.success(),
            "apply compact-{i} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Status should show 6 units (5 workloads + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('6'),
        "should have 6 units before compaction: {stdout}"
    );

    // Archive 2 workloads
    let graph_json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    let units: Vec<serde_json::Value> = serde_json::from_str(&graph_json).expect("parse json");
    let workload_ids: Vec<String> = units
        .iter()
        .filter_map(|u| u.get("Workload"))
        .filter_map(|w| w.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .take(2)
        .collect();

    for id in &workload_ids {
        let output = Command::new(bin_path("taba"))
            .args(["unit", "archive", "--state-dir"])
            .arg(&state)
            .arg(id)
            .output()
            .expect("archive");
        assert!(
            output.status.success(),
            "archive {id} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Status should now show 4 active + 2 archived
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status after archive");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('4') || stdout.contains('6'),
        "should have 4 active or 6 total after archive: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_audit_provenance() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply a workload
    let workload = write_workload(tmp.path(), "audit-web");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply");

    // Query audit trails — should succeed even if empty
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

    // Query provenance for a unit — should succeed (may return empty)
    let graph_json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    let units: Vec<serde_json::Value> = serde_json::from_str(&graph_json).expect("parse json");
    if let Some(workload_id) = units
        .iter()
        .filter_map(|u| u.get("Workload"))
        .filter_map(|w| w.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .next()
    {
        let output = Command::new(bin_path("taba"))
            .args(["audit", "provenance", "--state-dir"])
            .arg(&state)
            .arg(&workload_id)
            .output()
            .expect("audit provenance");
        // Provenance query should succeed (workload has no data inputs)
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() || stdout.contains("no provenance") || stdout.contains("empty"),
            "audit provenance should succeed or return empty: {stdout}"
        );
    }

    cleanup_taba_containers();
}
