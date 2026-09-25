//! Runtime execution — simulated and Docker-based workload lifecycle.
//!
//! The [`RuntimeExecutor`] trait abstracts how workloads are started,
//! stopped, and monitored. Two implementations are provided:
//!
//! - [`SimulatedRuntime`] — in-memory state machine for fast unit
//!   tests. No real processes are started.
//! - [`DockerRuntime`] — uses `bollard` to start/stop Docker containers.
//!   Integration tests verify the full lifecycle with real containers
//!   (marked `#[ignore = "slow:requires-docker"]`).

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use taba_common::UnitId;
use taba_core::Unit;

use crate::error::NodeError;

// ---------------------------------------------------------------------------
// RuntimeState
// ---------------------------------------------------------------------------

/// Runtime state of a unit on a specific node.
///
/// This is the *observed* state (what is running on this node), NOT
/// the lifecycle state (Declared → Composed → Placed etc., defined in
/// `taba_core::UnitState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RuntimeState {
    /// Placement received, not yet started.
    Pending,
    /// Unit is starting (container, microVM, Wasm, etc.).
    Starting,
    /// Unit is running and healthy.
    Running,
    /// Unit is draining (preparing to stop).
    Draining,
    /// Unit has stopped (clean shutdown).
    Stopped,
    /// Unit failed to start or crashed.
    Failed,
    /// Unit is not known to this runtime.
    Unknown,
}

// ---------------------------------------------------------------------------
// RuntimeExecutor trait
// ---------------------------------------------------------------------------

/// Executes workload lifecycle operations on this node.
///
/// Implementations may use Docker (`DockerRuntime`), an in-memory
/// simulation (`SimulatedRuntime`), or any other execution backend.
pub trait RuntimeExecutor: Send + Sync {
    /// Start a unit on this node.
    ///
    /// Transitions the unit from `Pending` → `Starting` → `Running`.
    /// Returns the resulting runtime state.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the unit cannot be
    ///   started (e.g., image pull failure, container creation error).
    fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError>;

    /// Stop a unit on this node (graceful, default timeout).
    ///
    /// Transitions the unit from `Running` → `Draining` → `Stopped`.
    /// Returns the resulting runtime state.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the unit cannot be
    ///   stopped.
    fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError>;

    /// Check the current runtime state of a unit.
    ///
    /// Returns [`RuntimeState::Unknown`] if the unit is not known to
    /// this runtime.
    fn check_state(&self, unit: &Unit) -> RuntimeState;

    /// Drain a unit with a specific timeout (graceful shutdown).
    ///
    /// Transitions the unit from `Running` → `Draining`, waits up to
    /// `timeout`, then transitions to `Stopped`.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the unit cannot be
    ///   drained within the timeout.
    fn drain(&self, unit: &Unit, timeout: Duration) -> Result<RuntimeState, NodeError>;
}

// ---------------------------------------------------------------------------
// SimulatedRuntime
// ---------------------------------------------------------------------------

/// In-memory runtime for fast unit tests.
///
/// Tracks unit state in a [`HashMap`] behind a [`std::sync::Mutex`].
/// All operations are synchronous and complete immediately — no real
/// processes are started. This is the default for unit tests.
#[derive(Debug, Default)]
pub struct SimulatedRuntime {
    states: std::sync::Mutex<HashMap<UnitId, RuntimeState>>,
}

impl SimulatedRuntime {
    /// Creates a new empty `SimulatedRuntime`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the state of a unit directly (for test setup).
    ///
    /// This bypasses the normal state machine transitions and is
    /// intended for test setup only.
    pub fn set_state(&self, unit_id: UnitId, state: RuntimeState) {
        self.states
            .lock()
            .expect("simulated runtime mutex should not be poisoned")
            .insert(unit_id, state);
    }
}

impl RuntimeExecutor for SimulatedRuntime {
    fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let mut states = self
            .states
            .lock()
            .expect("simulated runtime mutex should not be poisoned");

        let id = unit.id();
        // Transition: Pending → Starting → Running
        states.insert(id, RuntimeState::Starting);
        states.insert(id, RuntimeState::Running);
        Ok(RuntimeState::Running)
    }

    fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let mut states = self
            .states
            .lock()
            .expect("simulated runtime mutex should not be poisoned");

        let id = unit.id();
        // Transition: Running → Draining → Stopped
        states.insert(id, RuntimeState::Draining);
        states.insert(id, RuntimeState::Stopped);
        Ok(RuntimeState::Stopped)
    }

    fn check_state(&self, unit: &Unit) -> RuntimeState {
        let states = self
            .states
            .lock()
            .expect("simulated runtime mutex should not be poisoned");

        states
            .get(&unit.id())
            .copied()
            .unwrap_or(RuntimeState::Unknown)
    }

    fn drain(&self, unit: &Unit, _timeout: Duration) -> Result<RuntimeState, NodeError> {
        let mut states = self
            .states
            .lock()
            .expect("simulated runtime mutex should not be poisoned");

        let id = unit.id();
        // Transition: Running → Draining → Stopped
        states.insert(id, RuntimeState::Draining);
        states.insert(id, RuntimeState::Stopped);
        Ok(RuntimeState::Stopped)
    }
}

// ---------------------------------------------------------------------------
// DockerRuntime
// ---------------------------------------------------------------------------

/// Docker-based runtime using `bollard`.
///
/// Starts and stops Docker containers to execute workload units. The
/// container name is derived from the unit ID: `taba-{unit_id}`.
///
/// For M3, this is a basic implementation: it pulls images, creates
/// and starts containers, and inspects their state. Error handling
/// covers missing containers and Docker daemon failures.
#[derive(Debug, Clone)]
pub struct DockerRuntime {
    docker: bollard::Docker,
}

impl DockerRuntime {
    /// Creates a new `DockerRuntime` connected to the local Docker daemon.
    ///
    /// Uses `bollard::Docker::connect_with_defaults()` which tries
    /// the standard Docker socket paths.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the Docker daemon
    ///   cannot be reached.
    pub fn new() -> Result<Self, NodeError> {
        let docker = bollard::Docker::connect_with_defaults().map_err(|e| {
            NodeError::ReconciliationFailed {
                unit: UnitId(uuid::Uuid::nil()),
                reason: format!("failed to connect to Docker daemon: {e}"),
            }
        })?;
        Ok(Self { docker })
    }

    /// Derives a Docker container name from a unit ID.
    fn container_name(unit: &Unit) -> String {
        format!("taba-{}", unit.id().0)
    }

    /// Extracts the image reference from a workload unit's artifact.
    ///
    /// For non-workload units, returns a default `nginx:alpine` image
    /// (M3 basic implementation).
    fn image_for(unit: &Unit) -> String {
        match unit {
            Unit::Workload(w) => w.artifact.artifact_ref.clone(),
            _ => "nginx:alpine".to_string(),
        }
    }

    /// Maps a Docker container status to [`RuntimeState`].
    fn map_status(state: &bollard::models::ContainerState) -> RuntimeState {
        if state.running == Some(true) || state.paused == Some(true) {
            RuntimeState::Running
        } else if state.restarting == Some(true) {
            RuntimeState::Starting
        } else if state.dead == Some(true) {
            RuntimeState::Failed
        } else if state.exit_code == Some(0) {
            RuntimeState::Stopped
        } else if state.status == Some(bollard::models::ContainerStateStatusEnum::CREATED) {
            RuntimeState::Pending
        } else {
            RuntimeState::Stopped
        }
    }
}

impl RuntimeExecutor for DockerRuntime {
    fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let name = Self::container_name(unit);
        let image = Self::image_for(unit);

        // Use tokio runtime to execute async bollard calls.
        let rt = tokio::runtime::Runtime::new().map_err(|e| NodeError::ReconciliationFailed {
            unit: unit.id(),
            reason: format!("failed to create tokio runtime: {e}"),
        })?;

        rt.block_on(async {
            // Pull image if needed.
            let create_image_opts = bollard::image::CreateImageOptions {
                from_image: image.as_str(),
                ..Default::default()
            };
            let mut pull_stream = self
                .docker
                .create_image(Some(create_image_opts), None, None);
            while let Some(item) = pull_stream.next().await {
                match item {
                    Ok(_) => {}
                    Err(e) => {
                        return Err(NodeError::ReconciliationFailed {
                            unit: unit.id(),
                            reason: format!("failed to pull image '{image}': {e}"),
                        });
                    }
                }
            }

            // Create container.
            let config = bollard::container::Config {
                image: Some(image.clone()),
                cmd: Some(vec!["sleep".to_string(), "300".to_string()]),
                ..Default::default()
            };

            let create_opts = bollard::container::CreateContainerOptions {
                name: name.as_str(),
                platform: None,
            };

            self.docker
                .create_container(Some(create_opts), config)
                .await
                .map_err(|e| NodeError::ReconciliationFailed {
                    unit: unit.id(),
                    reason: format!("failed to create container '{name}': {e}"),
                })?;

            // Start container.
            self.docker
                .start_container::<String>(&name, None)
                .await
                .map_err(|e| NodeError::ReconciliationFailed {
                    unit: unit.id(),
                    reason: format!("failed to start container '{name}': {e}"),
                })?;

            Ok(RuntimeState::Running)
        })
    }

    fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let name = Self::container_name(unit);

        let rt = tokio::runtime::Runtime::new().map_err(|e| NodeError::ReconciliationFailed {
            unit: unit.id(),
            reason: format!("failed to create tokio runtime: {e}"),
        })?;

        rt.block_on(async {
            // Stop container with default 10-second timeout.
            self.docker.stop_container(&name, None).await.map_err(|e| {
                NodeError::ReconciliationFailed {
                    unit: unit.id(),
                    reason: format!("failed to stop container '{name}': {e}"),
                }
            })?;

            // Small delay to let Docker settle.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            // Remove container (small delay to let Docker settle).
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let remove_opts = bollard::container::RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            // Ignore 404 — container may already be removed.
            let _ = self.docker.remove_container(&name, Some(remove_opts)).await;

            Ok(RuntimeState::Stopped)
        })
    }

    fn check_state(&self, unit: &Unit) -> RuntimeState {
        let name = Self::container_name(unit);

        let Ok(rt) = tokio::runtime::Runtime::new() else {
            return RuntimeState::Unknown;
        };

        rt.block_on(async {
            match self.docker.inspect_container(&name, None).await {
                Ok(info) => info
                    .state
                    .map_or(RuntimeState::Unknown, |state| Self::map_status(&state)),
                Err(_) => RuntimeState::Unknown,
            }
        })
    }

    fn drain(&self, unit: &Unit, timeout: Duration) -> Result<RuntimeState, NodeError> {
        let name = Self::container_name(unit);

        let rt = tokio::runtime::Runtime::new().map_err(|e| NodeError::ReconciliationFailed {
            unit: unit.id(),
            reason: format!("failed to create tokio runtime: {e}"),
        })?;

        rt.block_on(async {
            // Stop container with the specified timeout (seconds).
            let stop_opts = bollard::container::StopContainerOptions {
                t: timeout.as_secs().try_into().unwrap_or(30),
            };

            self.docker
                .stop_container(&name, Some(stop_opts))
                .await
                .map_err(|e| NodeError::ReconciliationFailed {
                    unit: unit.id(),
                    reason: format!("failed to drain container '{name}': {e}"),
                })?;

            // Small delay to let Docker settle.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            // Remove container (small delay to let Docker settle).
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let remove_opts = bollard::container::RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            let _ = self.docker.remove_container(&name, Some(remove_opts)).await;

            Ok(RuntimeState::Stopped)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use taba_test_harness::WorkloadUnitBuilder;

    fn test_workload(id: UnitId) -> Unit {
        Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build())
    }

    /// Test workload with a real Docker image (alpine:latest).
    fn test_docker_workload(id: UnitId) -> Unit {
        use taba_common::ContentDigest;
        use taba_core::{Artifact, ArtifactType};
        let mut unit = WorkloadUnitBuilder::new().with_id(id).build();
        unit.artifact = Artifact {
            artifact_type: ArtifactType::Oci,
            artifact_ref: "alpine:latest".to_string(),
            digest: ContentDigest("sha256:".to_string()),
            requires: Vec::new(),
            kernel_ref: None,
            rootfs_ref: None,
        };
        Unit::Workload(unit)
    }

    // -- SimulatedRuntime tests ---------------------------------------------

    #[test]
    fn test_simulated_start() {
        let runtime = SimulatedRuntime::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_workload(id);

        let state = runtime.start(&unit).expect("start should succeed");
        assert_eq!(state, RuntimeState::Running);
        assert_eq!(runtime.check_state(&unit), RuntimeState::Running);
    }

    #[test]
    fn test_simulated_stop() {
        let runtime = SimulatedRuntime::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_workload(id);

        runtime.start(&unit).expect("start should succeed");
        let state = runtime.stop(&unit).expect("stop should succeed");
        assert_eq!(state, RuntimeState::Stopped);
        assert_eq!(runtime.check_state(&unit), RuntimeState::Stopped);
    }

    #[test]
    fn test_simulated_check_state() {
        let runtime = SimulatedRuntime::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_workload(id);

        // Unknown unit → Unknown.
        assert_eq!(runtime.check_state(&unit), RuntimeState::Unknown);

        // After start → Running.
        runtime.start(&unit).expect("start");
        assert_eq!(runtime.check_state(&unit), RuntimeState::Running);

        // After stop → Stopped.
        runtime.stop(&unit).expect("stop");
        assert_eq!(runtime.check_state(&unit), RuntimeState::Stopped);
    }

    #[test]
    fn test_simulated_drain() {
        let runtime = SimulatedRuntime::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_workload(id);

        runtime.start(&unit).expect("start");
        let state = runtime
            .drain(&unit, Duration::from_secs(30))
            .expect("drain should succeed");
        assert_eq!(state, RuntimeState::Stopped);
        assert_eq!(runtime.check_state(&unit), RuntimeState::Stopped);
    }

    #[test]
    fn test_simulated_unknown_unit() {
        let runtime = SimulatedRuntime::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_workload(id);

        // A unit that was never started should return Unknown.
        assert_eq!(runtime.check_state(&unit), RuntimeState::Unknown);
    }

    #[test]
    fn test_simulated_set_state() {
        let runtime = SimulatedRuntime::new();
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_workload(id);

        runtime.set_state(id, RuntimeState::Failed);
        assert_eq!(runtime.check_state(&unit), RuntimeState::Failed);
    }

    // -- DockerRuntime tests (require Docker) -------------------------------

    #[test]
    #[ignore = "slow:requires-docker"]
    fn test_docker_start_stop() {
        let runtime = DockerRuntime::new().expect("connect to Docker");
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_docker_workload(id);

        let state = runtime.start(&unit).expect("start should succeed");
        assert_eq!(state, RuntimeState::Running);

        let state = runtime.stop(&unit).expect("stop should succeed");
        assert_eq!(state, RuntimeState::Stopped);
    }

    #[test]
    #[ignore = "slow:requires-docker"]
    fn test_docker_check_state() {
        let runtime = DockerRuntime::new().expect("connect to Docker");
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_docker_workload(id);

        runtime.start(&unit).expect("start should succeed");
        assert_eq!(runtime.check_state(&unit), RuntimeState::Running);

        runtime.stop(&unit).expect("stop should succeed");
        assert_eq!(runtime.check_state(&unit), RuntimeState::Unknown);
    }

    #[test]
    #[ignore = "slow:requires-docker"]
    fn test_docker_not_found() {
        let runtime = DockerRuntime::new().expect("connect to Docker");
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = test_docker_workload(id);

        // Container was never created → Unknown.
        assert_eq!(runtime.check_state(&unit), RuntimeState::Unknown);
    }
}
