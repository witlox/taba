# Runtime Matching

taba supports four workload runtime types. The solver matches each
workload's `ArtifactType` to the node's `RuntimeCapability` (hard
constraint), then ranks by resource availability (soft constraint).

## Supported runtimes

| Runtime | ArtifactType | TOML key | Example |
|---------|-------------|----------|---------|
| Container | `Oci` | `image` | `image = "myapp:v1.0"` |
| Native binary | `Native` | `binary` | `binary = "/bin/daemon"` |
| WebAssembly | `Wasm` | `wasm` | `wasm = "module.wasm"` |
| MicroVM | `MicroVm` | `microvm` | `microvm = "vmlinux-5.10"` |
| K8s manifest | `K8sManifest` | `k8s` | `k8s = "pod-spec.yaml"` |

### Container (OCI)

Docker or Podman with root daemon. The node must advertise
`RuntimeCapability::Oci` or `RuntimeCapability::OciRootless`.

```toml
[unit]
name = "web-api"
image = "registry.example.com/web-api:v1.0"
digest = "sha256:abc123..."
```

### Native binary

Standalone executables (MSI, RPM, DEB, scripts). The node must
advertise `RuntimeCapability::Native`. `NativeRuntime` spawns the
binary via `std::process::Command` and tracks the PID.

```toml
[unit]
name = "batch-job"
binary = "/usr/local/bin/batch-processor"
```

### WebAssembly

Wasm modules (wasmtime/wasmer compatible). The node must advertise
`RuntimeCapability::Wasm`. `WasmRuntime` tracks lifecycle state;
full wasmtime execution is deferred.

```toml
[unit]
name = "edge-function"
wasm = "/opt/edge-fn.wasm"
```

### MicroVM

Firecracker, cloud-hypervisor, or QEMU. The node must advertise
`RuntimeCapability::MicroVm` (requires a VM monitor binary).
`MicroVmRuntime` auto-detects the monitor and generates the
appropriate VM configuration (INV-N6).

```toml
[unit]
name = "secure-worker"
microvm = "vmlinux-5.10"
kernel = "/opt/vmlinux"
rootfs = "/opt/rootfs.ext4"
digest = "sha256:abc123..."
```

## How the solver matches

1. **Capability filter** (hard constraint): `DefaultCapabilityFilter`
   checks if the node's `RuntimeCapability` list matches the
   workload's `ArtifactType`. Nodes without a matching capability
   are excluded.

2. **Placement scorer** (soft constraint): `DefaultPlacementScorer`
   ranks eligible nodes by resource availability, environment match,
   author affinity, and health status. Exact runtime matches get a
   bonus.

3. **RuntimeSelector**: At reconcile time, `RuntimeSelector`
   dispatches to the correct `RuntimeExecutor` (DockerRuntime,
   NativeRuntime, WasmRuntime, or MicroVmRuntime) based on the
   unit's `ArtifactType`.

## Node capability advertisement

Nodes advertise their capabilities via the `NodeCapabilitySet`:

| Capability | Description | Detection |
|------------|-------------|-----------|
| `Oci` | Docker/Podman with root daemon | `Docker::connect_with_defaults()` |
| `OciRootless` | Rootless Docker/Podman | Socket probe |
| `K8s` | Kubernetes API access | `kubectl` available |
| `Wasm` | WebAssembly runtime | Always available |
| `Native` | Native binary execution | Always available |
| `MicroVm` | VM execution (Firecracker/QEMU) | `which firecracker`, `which cloud-hypervisor`, `which qemu-system-*` |
