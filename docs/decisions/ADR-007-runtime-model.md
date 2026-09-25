# ADR-007: Runtime model — per-artifact-type dispatch

Date: 2026-09-25
Status: Accepted

## Context

taba supports four workload types: containers (OCI), microVMs
(Firecracker, cloud-hypervisor, QEMU), WebAssembly modules
(wasmtime), and native binaries. The `RuntimeExecutor` trait
abstracts how workloads are started, stopped, and monitored.

Previously, the CLI's `LocalClient` had a single `DockerRuntime`
field — all units were passed to Docker regardless of their
`ArtifactType`. Native, Wasm, and MicroVm workloads would fail
because Docker tried to pull their artifact references as Docker
images.

## Decision

Introduce a `RuntimeSelector` that dispatches to the correct
`RuntimeExecutor` based on the unit's `ArtifactType`:

| `ArtifactType` | `RuntimeExecutor` | Detection |
|---|---|---|
| `Oci` | `DockerRuntime` (bollard) | `Docker::connect_with_defaults()` |
| `Native` | `NativeRuntime` (std::process) | Always available |
| `Wasm` | `WasmRuntime` (state tracking) | Always available |
| `MicroVm` | `MicroVmRuntime` (subprocess) | `which firecracker`, `which cloud-hypervisor`, `which qemu-system-*` |

`RuntimeSelector::new()` detects all available runtimes on the
node. `RuntimeSelector::available()` returns a `Vec<RuntimeCapability>`
for node advertisement. `RuntimeSelector::start(&unit)` dispatches
to the correct executor.

`LocalClient` replaces `docker: Option<DockerRuntime>` with
`runtime: Option<RuntimeSelector>`. The `reconcile()` method
now uses the selector to start/stop units of any type.

## Consequences

- **Positive**: All four workload types can be applied, composed,
  and reconciled via the CLI. The solver can place workloads on
  nodes with matching capabilities.
- **Positive**: New runtime types can be added by implementing
  `RuntimeExecutor` and adding a case to `RuntimeSelector`.
- **Negative**: `NativeRuntime` and `WasmRuntime` are minimal
  implementations (state tracking, not full execution). Full
  Wasm execution requires adding the `wasmtime` crate.
- **Negative**: `MicroVmRuntime` requires a VM monitor binary
  (firecracker, cloud-hypervisor, or QEMU) to be installed.

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|---|---|---|---|
| Docker-only | Simple, well-tested | Can't run native/Wasm/MicroVm | Violates vision doc |
| One runtime per node | Simpler dispatch | Can't mix workload types | Violates INV-N2 |
| Plugin system | Extensible | Over-engineered for 4 types | Premature abstraction |
