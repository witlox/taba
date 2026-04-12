# taba (束)

**Self-describing, capability-aware workload units composed through a distributed solver.**

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

## Architecture

```
taba-common          types, config, protobuf
    |
taba-core            unit type system, capabilities, contracts
    |
taba-security        signing, verification, taint, Shamir ceremony
   / \
taba-graph  taba-solver    CRDT graph + deterministic solver (parallel build)
   / \         |
taba-erasure  taba-gossip  erasure coding + membership (parallel build)
      \       /
      taba-node            per-node daemon, WAL, reconciliation
          |
       taba-cli            command-line interface
```

10 crates. Acyclic dependency graph. Single-node works before multi-node (progressive complexity).

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

## Project status

taba is in the **pre-implementation** phase. Specifications are complete and adversary-reviewed.

| Phase | Status |
|-------|--------|
| Domain model, invariants, failure modes | Complete |
| Adversary spec review (57 findings) | All critical/high resolved |
| Architecture (module map, interfaces, data models) | Complete |
| Adversary architecture review (45 findings) | All critical/high resolved |
| BDD feature files (128 scenarios) | Complete |
| Rust implementation | Not started |

### Implementation phases

| Milestone | Crates | Capability |
|-----------|--------|------------|
| M1: Types compile | common, core | Unit declarations parse and validate |
| M2: Single-node compose | + graph, solver, security | Compose units on one node |
| M3: Persistent | + node (WAL) | Survives restart |
| M4: Multi-node | + gossip, erasure | Distributed operation |
| M5: Usable | + cli | Human-operable |
| M6: Hardened | + security advanced | Production-grade security |
| M7: Migration | + k8s tool | K8s users can onboard |

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

- **Language**: Rust (workspace), edition 2021
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
```

## License

Apache-2.0
