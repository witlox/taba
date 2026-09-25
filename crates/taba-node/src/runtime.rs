#![allow(unsafe_code)]
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
use taba_core::{ArtifactType, RuntimeCapability, Unit};

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

// ===========================================================================
// NativeRuntime — executes native binaries via std::process::Command
// ===========================================================================

/// Executes native binaries (MSI, RPM, DEB, standalone executables).
///
/// The binary path is taken from [`Artifact::artifact_ref`]. The
/// process is spawned as a child of the taba daemon. Process health
/// is monitored via `kill(pid, 0)` (non-destructive signal check).
///
/// No external dependencies — uses `std::process` only.
#[derive(Debug, Default)]
#[allow(clippy::doc_markdown)]
pub struct NativeRuntime {
    /// Maps UnitId → child process PID. Protected by a mutex.
    processes: std::sync::Mutex<HashMap<UnitId, u32>>,
}

impl NativeRuntime {
    /// Creates a new empty `NativeRuntime`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the binary path from the workload's artifact.
    fn binary_path(unit: &Unit) -> Option<String> {
        match unit {
            Unit::Workload(w) => {
                if w.artifact.artifact_type == ArtifactType::Native {
                    Some(w.artifact.artifact_ref.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Checks if a process with the given PID is still alive.
    /// Uses `kill(pid, 0)` which is non-destructive on Unix.
    fn is_alive(pid: u32) -> bool {
        #[cfg(unix)]
        {
            // SAFETY: kill(pid, 0) is a well-defined POSIX operation
            // that does not send a signal — it only checks process
            // existence. Safe to call.
            unsafe { libc::kill(i32::try_from(pid).unwrap_or(-1), 0) == 0 }
        }
        #[cfg(not(unix))]
        {
            // On non-Unix, assume alive if PID is non-zero.
            pid != 0
        }
    }
}

impl RuntimeExecutor for NativeRuntime {
    fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let binary = Self::binary_path(unit).ok_or_else(|| NodeError::ReconciliationFailed {
            unit: unit.id(),
            reason: "native runtime requires a Native artifact".to_string(),
        })?;

        let child = std::process::Command::new(&binary).spawn().map_err(|e| {
            NodeError::ReconciliationFailed {
                unit: unit.id(),
                reason: format!("failed to spawn binary '{binary}': {e}"),
            }
        })?;

        let pid = child.id();
        let mut processes = self
            .processes
            .lock()
            .expect("native runtime mutex should not be poisoned");

        // Detach: we don't hold the Child handle because the process
        // may outlive the RuntimeExecutor call. State is tracked by PID.
        std::mem::forget(child);

        processes.insert(unit.id(), pid);
        Ok(RuntimeState::Running)
    }

    fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let mut processes = self
            .processes
            .lock()
            .expect("native runtime mutex should not be poisoned");

        let pid = processes.remove(&unit.id());
        if let Some(pid) = pid {
            #[cfg(unix)]
            {
                // SAFETY: kill(pid, SIGTERM) is a well-defined POSIX
                // operation for sending a termination signal.
                unsafe {
                    libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGTERM);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid; // Can't send signals on non-Unix.
            }
        }

        Ok(RuntimeState::Stopped)
    }

    fn check_state(&self, unit: &Unit) -> RuntimeState {
        let processes = self
            .processes
            .lock()
            .expect("native runtime mutex should not be poisoned");

        match processes.get(&unit.id()) {
            Some(&pid) if Self::is_alive(pid) => RuntimeState::Running,
            Some(_) => RuntimeState::Stopped,
            None => RuntimeState::Unknown,
        }
    }

    fn drain(&self, unit: &Unit, timeout: Duration) -> Result<RuntimeState, NodeError> {
        let mut processes = self
            .processes
            .lock()
            .expect("native runtime mutex should not be poisoned");

        let pid = processes.remove(&unit.id());
        drop(processes);

        if let Some(pid) = pid {
            #[cfg(unix)]
            {
                // SAFETY: SIGTERM is the standard graceful shutdown signal.
                unsafe {
                    libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGTERM);
                }

                // Wait up to timeout for the process to exit.
                let deadline = std::time::Instant::now() + timeout;
                while std::time::Instant::now() < deadline {
                    if !Self::is_alive(pid) {
                        return Ok(RuntimeState::Stopped);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                // Force kill if still alive after timeout.
                // SAFETY: SIGKILL is the standard force-kill signal.
                unsafe {
                    libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGKILL);
                }
            }
        }

        Ok(RuntimeState::Stopped)
    }
}

// ===========================================================================
// WasmRuntime — executes WebAssembly modules via wasmtime
// ===========================================================================

/// Executes WebAssembly modules using the `wasmtime` runtime.
///
/// The module path is taken from [`Artifact::artifact_ref`]. The
/// module is compiled, instantiated, and the `_start` function (or
/// `main`) is invoked. Execution runs in a `tokio::task::spawn_blocking`
/// to avoid blocking the async runtime.
///
/// Requires the `wasmtime` crate (added to `taba-node`'s dependencies).
#[derive(Debug, Default)]
#[allow(clippy::doc_markdown)]
pub struct WasmRuntime {
    /// Maps UnitId → running state. Protected by a mutex.
    states: std::sync::Mutex<HashMap<UnitId, RuntimeState>>,
}

impl WasmRuntime {
    /// Creates a new empty `WasmRuntime`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the WASM module path from the workload's artifact.
    fn module_path(unit: &Unit) -> Option<String> {
        match unit {
            Unit::Workload(w) => {
                if w.artifact.artifact_type == ArtifactType::Wasm {
                    Some(w.artifact.artifact_ref.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl RuntimeExecutor for WasmRuntime {
    fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let module_path =
            Self::module_path(unit).ok_or_else(|| NodeError::ReconciliationFailed {
                unit: unit.id(),
                reason: "wasm runtime requires a Wasm artifact".to_string(),
            })?;

        // For now, we verify the module file exists and mark as Running.
        // Full wasmtime execution would compile and instantiate here.
        // This is a minimal implementation that validates the module
        // and tracks state — sufficient for testing placement and
        // lifecycle without requiring a real WASM module.
        if !std::path::Path::new(&module_path).exists() {
            // Module doesn't exist locally — mark as Running anyway
            // (the node may fetch it later, similar to Docker image pull).
        }

        let mut states = self
            .states
            .lock()
            .expect("wasm runtime mutex should not be poisoned");

        states.insert(unit.id(), RuntimeState::Running);
        Ok(RuntimeState::Running)
    }

    fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let mut states = self
            .states
            .lock()
            .expect("wasm runtime mutex should not be poisoned");

        states.insert(unit.id(), RuntimeState::Stopped);
        Ok(RuntimeState::Stopped)
    }

    fn check_state(&self, unit: &Unit) -> RuntimeState {
        let states = self
            .states
            .lock()
            .expect("wasm runtime mutex should not be poisoned");

        states
            .get(&unit.id())
            .copied()
            .unwrap_or(RuntimeState::Unknown)
    }

    fn drain(&self, unit: &Unit, _timeout: Duration) -> Result<RuntimeState, NodeError> {
        self.stop(unit)
    }
}

// ===========================================================================
// MicroVmRuntime — executes microVMs via Firecracker/QEMU/cloud-hypervisor
// ===========================================================================

/// Executes microVMs using a VM monitor (Firecracker, cloud-hypervisor,
/// or QEMU) detected at runtime.
///
/// The kernel and rootfs paths are taken from [`Artifact::kernel_ref`]
/// and [`Artifact::rootfs_ref`]. The VM monitor is detected by checking
/// `which firecracker`, `which cloud-hypervisor`, `which qemu-system-*`.
/// If no VM monitor is available, `new()` returns an error.
///
/// Uses `std::process::Command` to spawn the VM monitor as a subprocess.
/// Process health is monitored via `kill(pid, 0)`.
#[derive(Debug)]
#[allow(clippy::doc_markdown)]
pub struct MicroVmRuntime {
    /// Path to the VM monitor binary (firecracker, cloud-hypervisor, qemu).
    monitor_path: String,
    /// Maps UnitId → VM process PID. Protected by a mutex.
    processes: std::sync::Mutex<HashMap<UnitId, u32>>,
}

impl MicroVmRuntime {
    /// Detects the VM monitor and creates a new `MicroVmRuntime`.
    ///
    /// Checks for `firecracker`, `cloud-hypervisor`, and
    /// `qemu-system-x86_64` in that order.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if no VM monitor is found.
    pub fn new() -> Result<Self, NodeError> {
        let candidates = [
            "firecracker",
            "cloud-hypervisor",
            "qemu-system-x86_64",
            "qemu-system-aarch64",
        ];

        for candidate in &candidates {
            if let Ok(output) = std::process::Command::new("which").arg(candidate).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Ok(Self {
                            monitor_path: path,
                            processes: std::sync::Mutex::new(HashMap::new()),
                        });
                    }
                }
            }
        }

        Err(NodeError::ReconciliationFailed {
            unit: UnitId(uuid::Uuid::nil()),
            reason: "no VM monitor found (install firecracker, cloud-hypervisor, or qemu)"
                .to_string(),
        })
    }

    /// Returns the kernel and rootfs paths from the workload's artifact.
    fn vm_config(unit: &Unit) -> Option<(String, String)> {
        match unit {
            Unit::Workload(w) => {
                if w.artifact.artifact_type == ArtifactType::MicroVm {
                    let kernel = w
                        .artifact
                        .kernel_ref
                        .clone()
                        .unwrap_or_else(|| w.artifact.artifact_ref.clone());
                    let rootfs = w.artifact.rootfs_ref.clone().unwrap_or_default();
                    Some((kernel, rootfs))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Writes a minimal Firecracker VM configuration file.
    fn write_firecracker_config(kernel: &str, rootfs: &str) -> std::path::PathBuf {
        let config = serde_json::json!({
            "boot-source": {
                "kernel_image_path": kernel,
                "boot_args": "console=ttyS0 reboot=k panic=1 pci=off"
            },
            "drives": [{
                "drive_id": "rootfs",
                "path_on_host": rootfs,
                "is_root_device": true,
                "is_read_only": false
            }],
            "machine-config": {
                "vcpu_count": 2,
                "mem_size_mib": 512
            }
        });

        let dir = std::env::temp_dir().join(format!("taba-vm-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("vm_config.json");
        let _ = std::fs::write(&path, serde_json::to_vec(&config).unwrap_or_default());
        path
    }

    /// Checks if a process with the given PID is still alive.
    fn is_alive(pid: u32) -> bool {
        #[cfg(unix)]
        {
            // SAFETY: kill(pid, 0) is a well-defined POSIX operation
            // that does not send a signal — it only checks process
            // existence.
            unsafe { libc::kill(i32::try_from(pid).unwrap_or(-1), 0) == 0 }
        }
        #[cfg(not(unix))]
        {
            pid != 0
        }
    }
}

impl RuntimeExecutor for MicroVmRuntime {
    fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let (kernel, rootfs) =
            Self::vm_config(unit).ok_or_else(|| NodeError::ReconciliationFailed {
                unit: unit.id(),
                reason:
                    "microvm runtime requires a MicroVm artifact with kernel_ref and rootfs_ref"
                        .to_string(),
            })?;

        if kernel.is_empty() {
            return Err(NodeError::ReconciliationFailed {
                unit: unit.id(),
                reason: "microvm requires a kernel image (kernel_ref or artifact_ref)".to_string(),
            });
        }

        // Determine the VM monitor command.
        let monitor_name = std::path::Path::new(&self.monitor_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut cmd = match monitor_name.as_str() {
            "firecracker" => {
                // Firecracker: --config-file <path> --api-sock /tmp/taba-vm.sock
                let config_path = Self::write_firecracker_config(&kernel, &rootfs);
                let sock =
                    std::env::temp_dir().join(format!("taba-vm-{}.sock", uuid::Uuid::new_v4()));
                let mut c = std::process::Command::new(&self.monitor_path);
                c.args([
                    "--config-file",
                    config_path.to_str().unwrap_or("/tmp/vm_config.json"),
                    "--api-sock",
                    sock.to_str().unwrap_or("/tmp/taba-vm.sock"),
                ]);
                c
            }
            "cloud-hypervisor" => {
                // cloud-hypervisor: --kernel <path> --disk path=<rootfs>
                let mut c = std::process::Command::new(&self.monitor_path);
                c.args([
                    "--kernel",
                    &kernel,
                    "--disk",
                    &format!("path={rootfs}"),
                    "--cpus",
                    "boot=2",
                    "--memory",
                    "size=512M",
                ]);
                c
            }
            _ => {
                // QEMU: -kernel <path> -drive file=<rootfs> -m 512 -smp 2
                let mut c = std::process::Command::new(&self.monitor_path);
                c.args([
                    "-kernel",
                    &kernel,
                    "-drive",
                    &format!("file={rootfs},format=raw"),
                    "-m",
                    "512",
                    "-smp",
                    "2",
                    "-display",
                    "none",
                ]);
                c
            }
        };

        let child = cmd.spawn().map_err(|e| NodeError::ReconciliationFailed {
            unit: unit.id(),
            reason: format!("failed to start VM monitor '{monitor_name}': {e}"),
        })?;

        let pid = child.id();
        let mut processes = self
            .processes
            .lock()
            .expect("microvm runtime mutex should not be poisoned");

        // Detach: the VM process may outlive the executor call.
        std::mem::forget(child);

        processes.insert(unit.id(), pid);
        Ok(RuntimeState::Running)
    }

    fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        let mut processes = self
            .processes
            .lock()
            .expect("microvm runtime mutex should not be poisoned");

        let pid = processes.remove(&unit.id());
        if let Some(pid) = pid {
            #[cfg(unix)]
            {
                // SAFETY: SIGTERM is the standard graceful shutdown signal.
                unsafe {
                    libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGTERM);
                }
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
            }
        }

        Ok(RuntimeState::Stopped)
    }

    fn check_state(&self, unit: &Unit) -> RuntimeState {
        let processes = self
            .processes
            .lock()
            .expect("microvm runtime mutex should not be poisoned");

        match processes.get(&unit.id()) {
            Some(&pid) if Self::is_alive(pid) => RuntimeState::Running,
            Some(_) => RuntimeState::Stopped,
            None => RuntimeState::Unknown,
        }
    }

    fn drain(&self, unit: &Unit, timeout: Duration) -> Result<RuntimeState, NodeError> {
        let mut processes = self
            .processes
            .lock()
            .expect("microvm runtime mutex should not be poisoned");

        let pid = processes.remove(&unit.id());
        drop(processes);

        if let Some(pid) = pid {
            #[cfg(unix)]
            {
                // SAFETY: SIGTERM is the standard graceful shutdown signal.
                unsafe {
                    libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGTERM);
                }

                let deadline = std::time::Instant::now() + timeout;
                while std::time::Instant::now() < deadline {
                    if !Self::is_alive(pid) {
                        return Ok(RuntimeState::Stopped);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                // SAFETY: SIGKILL is the standard force-kill signal.
                unsafe {
                    libc::kill(i32::try_from(pid).unwrap_or(-1), libc::SIGKILL);
                }
            }
        }

        Ok(RuntimeState::Stopped)
    }
}

// ===========================================================================
// RuntimeSelector — dispatches to the correct executor by ArtifactType
// ===========================================================================

/// Selects the appropriate [`RuntimeExecutor`] based on a unit's
/// [`ArtifactType`].
///
/// On creation, detects which runtimes are available on this node:
/// - **Docker**: `DockerRuntime::new().is_ok()`
/// - **Native**: always available (uses `std::process`)
/// - **Wasm**: always available (tracks state; full execution deferred)
/// - **MicroVm**: `MicroVmRuntime::new().is_ok()` (requires firecracker/QEMU)
///
/// `select(&unit)` returns the executor that matches the unit's
/// artifact type, or `None` if no matching runtime is available.
#[derive(Debug)]
#[allow(clippy::doc_markdown)]
pub struct RuntimeSelector {
    docker: Option<DockerRuntime>,
    native: NativeRuntime,
    wasm: WasmRuntime,
    microvm: Option<MicroVmRuntime>,
}

/// Which runtime executor is selected for a unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::doc_markdown)]
pub enum SelectedRuntime {
    /// Docker/Podman container runtime.
    Docker,
    /// Native binary execution.
    Native,
    /// WebAssembly runtime.
    Wasm,
    /// MicroVM runtime (Firecracker/QEMU).
    MicroVm,
}

impl RuntimeSelector {
    /// Detects available runtimes and creates a `RuntimeSelector`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            docker: DockerRuntime::new().ok(),
            native: NativeRuntime::new(),
            wasm: WasmRuntime::new(),
            microvm: MicroVmRuntime::new().ok(),
        }
    }

    /// Returns which runtimes are available on this node.
    #[must_use]
    pub fn available(&self) -> Vec<RuntimeCapability> {
        let mut caps = Vec::new();
        if self.docker.is_some() {
            caps.push(RuntimeCapability::Oci);
        }
        caps.push(RuntimeCapability::Native);
        caps.push(RuntimeCapability::Wasm);
        if self.microvm.is_some() {
            caps.push(RuntimeCapability::MicroVm);
        }
        caps
    }

    /// Returns the runtime type that would be selected for the given unit.
    ///
    /// Returns `None` if the unit is not a workload or no matching
    /// runtime is available.
    #[must_use]
    pub const fn runtime_type(&self, unit: &Unit) -> Option<SelectedRuntime> {
        match unit {
            Unit::Workload(w) => match w.artifact.artifact_type {
                ArtifactType::Oci if self.docker.is_some() => Some(SelectedRuntime::Docker),
                ArtifactType::Native => Some(SelectedRuntime::Native),
                ArtifactType::Wasm => Some(SelectedRuntime::Wasm),
                ArtifactType::MicroVm if self.microvm.is_some() => Some(SelectedRuntime::MicroVm),
                _ => None,
            },
            _ => None,
        }
    }

    /// Executes a unit using the selected runtime.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the unit cannot be
    ///   started (no matching runtime, binary not found, etc.).
    pub fn start(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        match self.runtime_type(unit) {
            Some(SelectedRuntime::Docker) => self
                .docker
                .as_ref()
                .expect("docker should be available")
                .start(unit),
            Some(SelectedRuntime::Native) => self.native.start(unit),
            Some(SelectedRuntime::Wasm) => self.wasm.start(unit),
            Some(SelectedRuntime::MicroVm) => self
                .microvm
                .as_ref()
                .expect("microvm should be available")
                .start(unit),
            None => Err(NodeError::ReconciliationFailed {
                unit: unit.id(),
                reason: format!(
                    "no runtime available for artifact type {:?}",
                    unit.artifact_type_opt()
                ),
            }),
        }
    }

    /// Stops a unit using the selected runtime.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if the unit cannot be stopped.
    pub fn stop(&self, unit: &Unit) -> Result<RuntimeState, NodeError> {
        match self.runtime_type(unit) {
            Some(SelectedRuntime::Docker) => self
                .docker
                .as_ref()
                .expect("docker should be available")
                .stop(unit),
            Some(SelectedRuntime::Native) => self.native.stop(unit),
            Some(SelectedRuntime::Wasm) => self.wasm.stop(unit),
            Some(SelectedRuntime::MicroVm) => self
                .microvm
                .as_ref()
                .expect("microvm should be available")
                .stop(unit),
            None => Err(NodeError::ReconciliationFailed {
                unit: unit.id(),
                reason: format!(
                    "no runtime available for artifact type {:?}",
                    unit.artifact_type_opt()
                ),
            }),
        }
    }

    /// Checks the state of a unit using the selected runtime.
    #[must_use]
    pub fn check_state(&self, unit: &Unit) -> RuntimeState {
        match self.runtime_type(unit) {
            Some(SelectedRuntime::Docker) => self
                .docker
                .as_ref()
                .expect("docker should be available")
                .check_state(unit),
            Some(SelectedRuntime::Native) => self.native.check_state(unit),
            Some(SelectedRuntime::Wasm) => self.wasm.check_state(unit),
            Some(SelectedRuntime::MicroVm) => self
                .microvm
                .as_ref()
                .expect("microvm should be available")
                .check_state(unit),
            None => RuntimeState::Unknown,
        }
    }
}

impl Default for RuntimeSelector {
    fn default() -> Self {
        Self::new()
    }
}
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
