# Health & Reconciliation

## Health Reporter

The `HealthReporter` trait tracks per-node health:

| Metric | Description |
|--------|-------------|
| `mode` | Current `OperationalMode` (Normal, Degraded, Recovery) |
| `units_running` | Count of running workload units |
| `units_failed` | Count of failed workload units |
| `wal_size_bytes` | Current WAL size in bytes |
| `graph_memory_bytes` | Current graph memory usage |
| `graph_memory_limit_bytes` | Configured memory limit |
| `memory_pressure_pct` | Memory usage as percentage of limit (0-100) |

### Triggers

| Trigger | Threshold | Action |
|---------|-----------|--------|
| Memory pressure | ≥ 80% | Auto-compaction begins (INV-R6) |
| Memory limit | ≥ 100% | Degraded mode (reason: MemoryLimitExceeded) |
| WAL failure | I/O error | Degraded mode (reason: WalFailure) |

## Reconciliation

The `Reconciler` trait compares desired state (from solver
placements) with actual state (what's running on this node).

### Reconciliation actions

| Desired | Actual | Action |
|---------|--------|--------|
| Running | Stopped | Start the unit |
| Running | Failed | Restart (respecting failure semantics) |
| Running | Unknown | Start the unit |
| Stopped | Running | Drain then stop |
| Draining | (any) | Continue draining |
| — | — | No drift |

### Reconciliation is NOT permitted in Degraded mode

Returns `NodeError::DegradedModeRestriction`. Only drain and
evacuation are permitted in Degraded mode.

### Evacuation

```rust
pub trait Reconciler {
    async fn reconcile(&self, placements: &[LocalPlacement]) -> Result<Vec<Drift>, NodeError>;
    async fn detect_drift(&self, placements: &[LocalPlacement]) -> Vec<Drift>;
    async fn drain(&self, unit: &UnitId) -> Result<(), NodeError>;
    async fn evacuate(&self) -> Result<(), NodeError>;
}
```

`evacuate()` drains all running units. Used before
`gossip::MembershipProtocol::leave` to give the system time
to re-place units elsewhere.

## Runtime executors

taba supports five runtime backends, dispatched by
`RuntimeSelector` based on the unit's `ArtifactType`:

| Executor | ArtifactType | Use case | Requires |
|----------|-------------|----------|----------|
| `SimulatedRuntime` | — | Tests, dev | Nothing (in-memory state machine) |
| `DockerRuntime` | `Oci` | Containers | Docker daemon (via `bollard` crate) |
| `NativeRuntime` | `Native` | Standalone binaries | Nothing (`std::process`) |
| `WasmRuntime` | `Wasm` | WebAssembly modules | Nothing (state tracking) |
| `MicroVmRuntime` | `MicroVm` | MicroVMs | Firecracker, cloud-hypervisor, or QEMU |

The `SimulatedRuntime` transitions units through
`Pending → Starting → Running → Draining → Stopped` (or `Failed`)
in memory, with no real process spawning. Fast tests, no Docker
dependency.

The `DockerRuntime` pulls images, creates containers, starts/stops
them, and inspects container state. Tests using Docker are marked
`#[ignore = "slow: requires Docker"]`.

The `NativeRuntime` spawns binaries via `std::process::Command` and
monitors them with `kill(pid, 0)`. No external dependencies.

The `WasmRuntime` tracks lifecycle state in memory. Full wasmtime
execution is deferred — sufficient for testing placement and
lifecycle without requiring a real WASM module.

The `MicroVmRuntime` auto-detects the VM monitor (Firecracker,
cloud-hypervisor, or QEMU) at runtime. It generates the appropriate
VM configuration (JSON for Firecracker, CLI args for cloud-hypervisor
or QEMU) from the unit's `kernel_ref` and `rootfs_ref` (INV-N6).

The `RuntimeSelector` is created once at node startup. It detects
all available runtimes and dispatches `start()`, `stop()`,
`check_state()`, and `drain()` to the correct executor based on
the unit's `ArtifactType`.
