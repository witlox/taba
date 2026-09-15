<table>
  <tr>
    <td>
      <h1>taba (束)</h1>
      <p><strong>Self-describing, capability-aware workload units composed through a distributed solver.</strong></p>
      <p>
        <a href="https://github.com/witlox/taba/actions/workflows/ci.yml"><img src="https://github.com/witlox/taba/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
        <a href="https://codecov.io/gh/witlox/taba"><img src="https://codecov.io/gh/witlox/taba/branch/main/graph/badge.svg?token=J0KZQKQKQK" alt="Coverage"></a>
        <a href="https://github.com/witlox/taba/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue.svg" alt="License"></a>
        <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust"></a>
        <a href="https://witlox.github.io/taba/"><img src="https://img.shields.io/badge/docs-latest-brightgreen.svg" alt="Docs"></a>
      </p>
    </td>
    <td align="right" valign="middle">
      <img src="logo-readme.png" alt="taba" width="128" height="127">
    </td>
  </tr>
</table>

taba replaces the container + orchestrator model (Docker + Kubernetes) with typed, signed workload units that carry their own contracts. The control plane isn't a separate system — it emerges from the composition of deployed units. Complexity scales linearly with what you actually run.

## Why

Every step in the infrastructure abstraction trajectory — VMs, containers, Kubernetes — increased control plane complexity monotonically. The diagnosis: **the container is too dumb** (opaque black box) and **the control plane is too smart** (unbounded state reconciliation engine). The separation between workload description and orchestration is drawn in the wrong place.

taba draws it differently. Units describe themselves — what they need, what they provide, what they tolerate, who they trust. A deterministic solver composes them. The result is the desired state. There is no separate desired state store.

## Core design

### Five pillars

1. **Self-describing typed units** — workload, data, policy, governance. Each carries capability declarations, behavioral contracts, and security requirements.
2. **Emergent control plane** — the control plane is the union of deployed units' operational semantics. One unit = trivial control plane. A thousand = the union of their contracts.
3. **Security as first class** — zero-access default, capability-based, fail-closed on conflicts. Every unit is signed by its author. Taint propagation is structural.
4. **Data as first-class unit** — datasets carry schema, classification, provenance, retention, and consent. Lineage falls out of the composition graph.
5. **Peer-to-peer** — no masters, no leaders, no external metadata store. CRDT graph, erasure-coded, gossip membership.

### Load-bearing decisions

| Decision | Rationale |
|----------|-----------|
| No masters | All nodes are peers. Same binary, same protocol, 1 node or 10,000. |
| CRDT graph | No consensus for normal operations. Eventually consistent, partition-tolerant. |
| Fail closed | Security conflicts are never implicitly resolved. |
| Deterministic solver | Same graph + same nodes = same placement on any node. Fixed-point arithmetic (ppm). |
| Signed units | Every unit is signed with context binding (trust domain, cluster, validity window). |
| Erasure coding | Not replication. k-of-n with fleet-adaptive parameters. |
| Gossip (SWIM) | Authenticated messages, 2-witness failure confirmation. |

## Project status

taba is **feature-complete** (M1–M7). All 7 milestones implemented, 867 tests passing.

| Milestone | Crates | Capability | Status |
|-----------|--------|------------|--------|
| M1: Types compile | common, core, test-harness | Unit declarations parse and validate | ✅ 147 tests |
| M2: Single-node compose | + graph, solver, security | Compose units on one node, signed | ✅ 313 tests |
| M3: Persistent | + observe, node | Survives restart (WAL, reconciler) | ✅ 130 tests |
| M4: Multi-node | + gossip, erasure | SWIM membership, Reed-Solomon | ✅ 115 tests |
| M5: Usable | + cli | Human-operable (init, apply, status) | ✅ 55 tests |
| M6: Hardened | + security advanced | Shamir, attestation, SLSA, enrollment | ✅ 49 tests |
| M7: Migration | + k8s | K8s manifest converter | ✅ 31 tests |

Post-M7 validation: fidelity sweep #2, adversary implementation sweep (30 findings, 3 Critical resolved), OQ-005 (K8s scope) resolved, OQ-007 (benchmarks) re-evaluated.

## Quick start

```sh
# Install Rust (stable, 1.85+)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/witlox/taba.git
cd taba
cargo build --workspace

# Initialize a local node
cargo run --bin taba -- init

# Author and apply a workload unit
echo '[unit]
name = "hello-web"
image = "nginx:alpine"' > hello.taba.toml
cargo run --bin taba -- apply hello.taba.toml

# Check status and run the solver
cargo run --bin taba -- status
cargo run --bin taba -- compose
cargo run --bin taba -- unit list
```

## Architecture

```
taba-common          types, config, protobuf
    |
taba-core            unit type system, capabilities, contracts
    |
taba-security        signing, verification, taint, Shamir ceremony
   / \
taba-graph  taba-solver    CRDT graph + deterministic solver (parallel)
   / \         |
taba-erasure  taba-gossip  erasure coding + membership (parallel)
       \       /
       taba-node            per-node daemon, WAL, reconciliation
           |
        taba-cli            command-line interface
           |
        taba-k8s            K8s manifest converter
```

14 crates. Acyclic dependency graph. Single-node works before multi-node (progressive complexity).

## Unit model

Everything in taba is a **typed, self-describing unit**:

```toml
# Example: a workload unit declaration
[unit]
type = "workload"
trust_domain = "acme-prod"

[needs]
postgres-store = { type = "data-store", purpose = "analytics" }

[provides]
aggregation-api = { type = "http-api" }

[tolerates]
max_latency_ms = 10
failure_mode = "restart"

[scaling]
min_instances = 2
max_instances = 10
```

Four unit types:
- **Workload** — compute process (container, microVM, Wasm, native)
- **Data** — dataset with classification, provenance, retention, consent
- **Policy** — resolves a specific capability conflict between units
- **Governance** — trust domain definitions, role assignments, certifications

## Security model

- **Zero-access default**: units access nothing unless explicitly declared and policy-approved
- **Capability-based**: typed capabilities with optional purpose qualifiers
- **Fail closed**: ambiguous security decisions are denied, not guessed
- **Taint propagation**: PII in = PII out, unless multi-party policy declassifies
- **Signed everything**: units, gossip messages, ceremony events
- **Scoped authority**: authors are parameterized by (unit type scope x trust domain scope)

## K8s migration

```sh
# Convert K8s manifests to taba unit declarations
cargo run --bin taba-k8s -- convert deployment.yaml --output-dir ./units

# Apply the generated units
cargo run --bin taba -- apply units/api-server.taba.toml
```

Supported: Deployment, StatefulSet, DaemonSet, Pod, Service, ConfigMap, Secret, NetworkPolicy, Role, ClusterRole, RoleBinding, ClusterRoleBinding.

## Ecosystem

taba is the fourth project in the witlox infrastructure ecosystem:

| Project | Language | Purpose |
|---------|----------|---------|
| [pact](https://github.com/witlox/pact) | Rust | HPC configuration management |
| [lattice](https://github.com/witlox/lattice) | Rust | HPC workload scheduling |
| [sovra](https://github.com/witlox/sovra) | Go | Federated key management |
| **taba** | Rust | Next-gen infrastructure composition |

Integration between projects is opt-in via `hpc-core` crates. Each project owns its space.

## Technology

- **Language**: Rust (workspace), edition 2024
- **Config**: TOML (human), protobuf (wire) via prost + tonic
- **Async**: tokio multi-threaded
- **CLI**: clap
- **Errors**: thiserror, typed enums
- **Testing**: proptest (properties), cucumber-rs (BDD), criterion (benchmarks)
- **License**: Apache-2.0

## Building

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

## Documentation

Full documentation at **[witlox.github.io/taba](https://witlox.github.io/taba/)** — or build locally:

```sh
mdbook serve --open    # http://localhost:3000
```

## License

Apache-2.0. See [LICENSE](LICENSE).
