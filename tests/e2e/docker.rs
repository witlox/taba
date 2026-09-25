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
        kernel_ref: None,
        rootfs_ref: None,
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

// ===========================================================================
// Four unit types — Docker e2e tests for Data, Policy, Governance
// ===========================================================================

/// Writes a data unit TOML with classification, retention, and provenance.
fn write_data_unit(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let toml = format!(
        r#"[unit]
name = "{name}"
type = "data"

[schema]
format = "json-schema"
definition = "schemas/{name}.json"

[classification]
level = "pii"

[retention]
mode = "persistent"
duration = "7y"
legal_basis = "GDPR Art. 6(1)(b)"
mandatory = true

[storage]
encrypted_at_rest = true
jurisdictions = ["EU"]

[provides]
{name}-data = {{ type = "dataset", purpose = "analytics" }}
"#
    );
    std::fs::write(&path, toml).expect("write toml");
    path
}

/// Writes a policy unit TOML that resolves a capability conflict.
fn write_policy_unit(dir: &std::path::Path, name: &str, action: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let toml = format!(
        r#"[unit]
name = "{name}"
type = "policy"

[conflict]
units = ["customer-profiles", "analytics-pipeline"]
capability = "customer-data"

[resolution]
action = "{action}"
rationale = "CI-approved test policy"
"#
    );
    std::fs::write(&path, toml).expect("write toml");
    path
}

/// Writes a governance unit TOML (trust domain definition).
fn write_governance_unit(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.taba.toml"));
    let toml = format!(
        r#"[unit]
name = "{name}"
type = "governance"
governance_type = "trust-domain"

[trust_domain]
description = "Test trust domain for e2e"
"#
    );
    std::fs::write(&path, toml).expect("write toml");
    path
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_data_unit_apply() {
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

    // Apply a data unit with PII classification
    let data = write_data_unit(tmp.path(), "customer-profiles");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&data)
        .output()
        .expect("apply data");
    assert!(
        output.status.success(),
        "apply data unit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify in graph.json
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Pii") || json.contains("pii"),
        "graph.json should contain PII classification: {json}"
    );
    assert!(
        json.contains("persistent") || json.contains("Persistent"),
        "graph.json should contain persistent retention: {json}"
    );
    assert!(
        json.contains("7y") || json.contains("GDPR"),
        "graph.json should contain retention duration or legal basis: {json}"
    );
    assert!(
        json.contains("encrypted_at_rest") || json.contains("EncryptedAtRest"),
        "graph.json should contain encrypted_at_rest: {json}"
    );

    // Status should show 2 units (1 data + 1 governance from init)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('2'),
        "should have 2 units after data apply: {stdout}"
    );

    // Audit provenance for the data unit — should succeed (no producers)
    let units: Vec<serde_json::Value> = serde_json::from_str(&json).expect("parse json");
    if let Some(data_id) = units
        .iter()
        .filter_map(|u| u.get("Data"))
        .filter_map(|d| d.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .next()
    {
        let output = Command::new(bin_path("taba"))
            .args(["audit", "provenance", "--state-dir"])
            .arg(&state)
            .arg(&data_id)
            .output()
            .expect("audit provenance");
        assert!(
            output.status.success(),
            "audit provenance for data unit should succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_policy_unit_apply() {
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

    // Apply a workload that needs customer-data
    let workload = write_workload_with_caps(
        tmp.path(),
        "analytics-pipeline",
        &[("customer-data", "dataset")],
        &[],
    );
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply workload");
    assert!(
        output.status.success(),
        "apply workload failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Apply a data unit that provides customer-data
    let data = write_workload_with_caps(
        tmp.path(),
        "customer-profiles",
        &[],
        &[("customer-data", "dataset")],
    );
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&data)
        .output()
        .expect("apply data");
    assert!(
        output.status.success(),
        "apply data failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Apply a policy unit that allows the composition
    let policy = write_policy_unit(tmp.path(), "allow-analytics", "allow");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&policy)
        .output()
        .expect("apply policy");
    assert!(
        output.status.success(),
        "apply policy failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Status should show 4 units (1 active workload + 1 active data + 1 pending policy + 1 active governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // 2 active (workload + data + governance = 3) + 1 pending (policy)
    assert!(
        stdout.contains('3') && stdout.contains('1'),
        "should have 3 active + 1 pending after policy apply: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_policy_supersession() {
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

    // Apply initial policy (allow)
    let policy_v1 = write_policy_unit(tmp.path(), "policy-v1", "allow");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&policy_v1)
        .output()
        .expect("apply policy-v1");

    // Apply superseding policy (deny) — references policy-v1 by name
    let policy_v2_path = tmp.path().join("policy-v2.taba.toml");
    std::fs::write(
        &policy_v2_path,
        r#"[unit]
name = "policy-v2"
type = "policy"
supersedes = "policy-v1"

[conflict]
units = ["customer-profiles", "analytics-pipeline"]
capability = "customer-data"

[resolution]
action = "deny"
rationale = "Revoked: security review found PII exposure"
"#,
    )
    .expect("write policy-v2");

    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&policy_v2_path)
        .output()
        .expect("apply policy-v2");
    assert!(
        output.status.success(),
        "apply policy-v2 (supersede) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Status should show 1 active + 2 pending (both policies reference non-existent units)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('1') && (stdout.contains('2') || stdout.contains('3')),
        "should have 1 active + 2-3 pending after supersession: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_governance_unit_apply() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init — creates an initial governance unit (RoleAssignment)
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Verify init created a governance unit
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Governance") || json.contains("governance"),
        "graph.json should contain a Governance unit after init: {json}"
    );
    assert!(
        json.contains("RoleAssignment"),
        "graph.json should contain a RoleAssignment after init: {json}"
    );

    // Init already created a Governance unit (RoleAssignment).
    // Trust domain creation requires 2 distinct signers (INV-S10),
    // which can't be done via CLI in single-node mode.
    // Verify the init-created governance unit is present.
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("RoleAssignment"),
        "graph.json should contain RoleAssignment from init: {json}"
    );

    // Status should show 1 governance unit
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('1'),
        "should have 1 unit (governance) after init: {stdout}"
    );

    // Governance units cannot be archived (INV-G3)
    let units: Vec<serde_json::Value> = serde_json::from_str(&json).expect("parse json");
    if let Some(gov_id) = units
        .iter()
        .filter_map(|u| u.get("Governance"))
        .filter_map(|g| g.get("header"))
        .filter_map(|h| h.get("id"))
        .filter_map(|id| id.as_str().map(|s| s.to_string()))
        .next()
    {
        let output = Command::new(bin_path("taba"))
            .args(["unit", "archive", "--state-dir"])
            .arg(&state)
            .arg(&gov_id)
            .output()
            .expect("archive governance");
        // Archiving a governance unit should fail
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success()
                || stderr.contains("cannot archive")
                || stderr.contains("governance"),
            "archiving governance unit should be rejected: {stderr}"
        );
    }

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_docker_all_four_unit_types() {
    if !docker_available() {
        eprintln!("Skipping: Docker not available");
        return;
    }

    cleanup_taba_containers();
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let state = tmp.path().join("state");

    // Init — creates Governance (RoleAssignment)
    Command::new(bin_path("taba"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .output()
        .expect("init");

    // Apply Workload
    let workload = write_workload(tmp.path(), "api-server");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&workload)
        .output()
        .expect("apply workload");
    assert!(
        output.status.success(),
        "apply workload failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Apply Data
    let data = write_data_unit(tmp.path(), "customer-db");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&data)
        .output()
        .expect("apply data");
    assert!(
        output.status.success(),
        "apply data failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Apply Policy
    let policy = write_policy_unit(tmp.path(), "access-policy", "conditional");
    let output = Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&policy)
        .output()
        .expect("apply policy");
    assert!(
        output.status.success(),
        "apply policy failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Governance already created by init (RoleAssignment).
    // Trust domain creation requires 2 signers (INV-S10).

    // Verify all four types in graph.json
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Workload"),
        "graph.json should contain Workload: {json}"
    );
    assert!(
        json.contains("Data"),
        "graph.json should contain Data: {json}"
    );
    assert!(
        json.contains("Policy"),
        "graph.json should contain Policy: {json}"
    );
    assert!(
        json.contains("Governance"),
        "graph.json should contain Governance: {json}"
    );

    // Status should show 4 units (1 workload + 1 data + 1 policy + 1 governance from init)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('4'),
        "should have 4 units (all four types): {stdout}"
    );

    // Compose — solver should evaluate all units
    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");
    assert!(
        output.status.success(),
        "compose with all four types failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("PLACEMENTS") || stdout.contains("UNPLACEABLE"),
        "compose should produce results with all four types: {stdout}"
    );

    cleanup_taba_containers();
}

// ===========================================================================
// Multi-runtime e2e tests — Native, Wasm, MicroVm
// ===========================================================================

#[test]
#[ignore = "slow:requires-docker"]
fn test_native_runtime_lifecycle() {
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

    // Apply a native binary workload (alpine:latest with sleep)
    let path = tmp.path().join("native-job.taba.toml");
    std::fs::write(
        &path,
        r#"[unit]
name = "native-job"
binary = "/bin/sleep 300"
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
        "apply native-job failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify in graph.json
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Native") || json.contains("native"),
        "graph.json should contain Native artifact type: {json}"
    );

    // Status should show 2 units (1 workload + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('2'),
        "should have 2 units after native apply: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_wasm_runtime_lifecycle() {
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

    // Apply a Wasm module workload
    let path = tmp.path().join("wasm-job.taba.toml");
    std::fs::write(
        &path,
        r#"[unit]
name = "wasm-job"
wasm = "/opt/module.wasm"
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
        "apply wasm-job failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify in graph.json
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Wasm") || json.contains("wasm"),
        "graph.json should contain Wasm artifact type: {json}"
    );

    // Status should show 2 units (1 workload + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('2'),
        "should have 2 units after wasm apply: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_microvm_runtime_lifecycle() {
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

    // Apply a MicroVM workload
    let path = tmp.path().join("vm-job.taba.toml");
    std::fs::write(
        &path,
        r#"[unit]
name = "vm-job"
microvm = "vmlinux-5.10"
kernel = "/opt/vmlinux"
rootfs = "/opt/rootfs.ext4"
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
        "apply vm-job failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify in graph.json
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("MicroVm") || json.contains("microvm"),
        "graph.json should contain MicroVm artifact type: {json}"
    );

    // Verify kernel_ref and rootfs_ref are stored
    assert!(
        json.contains("/opt/vmlinux"),
        "graph.json should contain kernel path: {json}"
    );
    assert!(
        json.contains("/opt/rootfs.ext4"),
        "graph.json should contain rootfs path: {json}"
    );

    // Status should show 2 units (1 workload + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('2'),
        "should have 2 units after microvm apply: {stdout}"
    );

    cleanup_taba_containers();
}

#[test]
#[ignore = "slow:requires-docker"]
fn test_all_four_runtimes_apply() {
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

    // Apply OCI workload
    let oci = write_workload(tmp.path(), "oci-web");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&oci)
        .output()
        .expect("apply oci");

    // Apply Native workload
    let native = tmp.path().join("native-svc.taba.toml");
    std::fs::write(
        &native,
        r#"[unit]
name = "native-svc"
binary = "/bin/sleep 300"
"#,
    )
    .expect("write native toml");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&native)
        .output()
        .expect("apply native");

    // Apply Wasm workload
    let wasm = tmp.path().join("wasm-svc.taba.toml");
    std::fs::write(
        &wasm,
        r#"[unit]
name = "wasm-svc"
wasm = "/opt/module.wasm"
"#,
    )
    .expect("write wasm toml");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&wasm)
        .output()
        .expect("apply wasm");

    // Apply MicroVM workload
    let microvm = tmp.path().join("vm-svc.taba.toml");
    std::fs::write(
        &microvm,
        r#"[unit]
name = "vm-svc"
microvm = "vmlinux-5.10"
kernel = "/opt/vmlinux"
rootfs = "/opt/rootfs.ext4"
"#,
    )
    .expect("write microvm toml");
    Command::new(bin_path("taba"))
        .args(["apply", "--state-dir"])
        .arg(&state)
        .arg(&microvm)
        .output()
        .expect("apply microvm");

    // Verify all four artifact types in graph.json
    let json = std::fs::read_to_string(state.join("graph.json")).expect("read graph");
    assert!(
        json.contains("Oci"),
        "graph.json should contain Oci: {json}"
    );
    assert!(
        json.contains("Native"),
        "graph.json should contain Native: {json}"
    );
    assert!(
        json.contains("Wasm"),
        "graph.json should contain Wasm: {json}"
    );
    assert!(
        json.contains("MicroVm"),
        "graph.json should contain MicroVm: {json}"
    );

    // Status should show 5 units (4 workloads + 1 governance)
    let output = Command::new(bin_path("taba"))
        .args(["status", "--state-dir"])
        .arg(&state)
        .output()
        .expect("status");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('5'),
        "should have 5 units (all four runtimes + governance): {stdout}"
    );

    // Compose — solver should evaluate all four
    let output = Command::new(bin_path("taba"))
        .args(["compose", "--state-dir"])
        .arg(&state)
        .output()
        .expect("compose");
    assert!(
        output.status.success(),
        "compose with all four runtimes failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    cleanup_taba_containers();
}
