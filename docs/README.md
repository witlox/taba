<img src="logo-docs.png" alt="taba" width="63" height="63">

# taba

taba (束, Japanese for "sheaf") is a next-generation infrastructure primitive. It replaces container + orchestrator (Docker + Kubernetes) with self-describing, capability-aware workload units composed through a distributed solver. The control plane emerges from unit composition — it is not a separate system.

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

# Author a workload unit
cat > hello.taba.toml << 'EOF'
[unit]
name = "hello-web"
image = "nginx:alpine"
EOF

# Apply it to the graph
cargo run --bin taba -- apply hello.taba.toml

# Check status
cargo run --bin taba -- status

# Run the solver
cargo run --bin taba -- compose

# List units
cargo run --bin taba -- unit list

# Audit decision trails
cargo run --bin taba -- audit trails
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
name = "api-server"
image = "api:v2.1.0"

[needs]
postgres = { type = "storage", purpose = "primary" }

[provides]
http-api = { type = "network", purpose = "serving" }

[scaling]
min = 2
max = 10
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
- **Scoped authority**: authors are parameterized by (unit type scope × trust domain scope)

## K8s migration

```sh
# Convert K8s manifests to taba unit declarations
cargo run --bin taba-k8s -- convert deployment.yaml --output-dir ./units

# Apply the generated units
cargo run --bin taba -- apply units/api-server.taba.toml
```

Supported K8s resources: Deployment, StatefulSet, DaemonSet, Pod, Service, ConfigMap, Secret, NetworkPolicy, Role, ClusterRole, RoleBinding, ClusterRoleBinding.

## Testing

| Tier | What | When | Command |
|------|------|------|---------|
| 1 (fast) | Unit tests + BDD @smoke + integration | Between every edit | `just` or `just test` |
| 2 (slow) | Tier 1 + slow-marked tests + full BDD | Pre-PR | `just test-slow` |
| 3 (full) | Tier 2 + slow integration tests | Pre-merge / nightly | `just test-full` |

960 tests across 13 crates + taba-e2e + taba-acceptance. Property tests (proptest, 10k+ cases) for CRDT merge laws, solver determinism, Shamir secret sharing, and capability matching.

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

## License

Apache-2.0. See [LICENSE](https://github.com/witlox/taba/blob/main/LICENSE).
