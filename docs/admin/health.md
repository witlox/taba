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

taba supports two runtime backends:

| Executor | Use case | Requires |
|----------|----------|----------|
| `SimulatedRuntime` | Tests, dev | Nothing (in-memory state machine) |
| `DockerRuntime` | Real workloads | Docker daemon (via `bollard` crate) |

The `SimulatedRuntime` transitions units through
`Pending → Starting → Running → Draining → Stopped` (or `Failed`)
in memory, with no real process spawning. Fast tests, no Docker
dependency.

The `DockerRuntime` pulls images, creates containers, starts/stops
them, and inspects container state. Tests using Docker are marked
`#[ignore = "slow: requires Docker"]`.
