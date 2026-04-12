# CLAUDE.md — taba project instructions

## What is taba?

taba (束, Japanese for "sheaf") is a next-generation infrastructure primitive.
It replaces container + orchestrator (Docker + K8s) with self-describing,
capability-aware workload units composed through a distributed solver. The
control plane emerges from unit composition — it is not a separate system.

Fourth project in the witlox ecosystem alongside pact (Rust, HPC config),
lattice (Rust, HPC scheduling), sovra (Go, federated key management).
Integration is opt-in via hpc-core crates. Each project owns its space.

## Core design

**Five pillars:**
1. Self-describing typed units (workload, data, policy, governance)
2. Emergent control plane — complexity scales linearly with deployment
3. Security as first class — zero-access default, capability-based, fail-closed
4. Data as first-class unit — lineage and provenance are structural
5. Peer-to-peer — CRDT graph, erasure-coded, no masters, gossip membership

**Load-bearing decisions** (do not change without full impact analysis):
- No masters — all nodes are peers
- CRDT graph — no consensus for normal operations
- Fail closed on security conflicts
- Deterministic solver — same input = same output on any node
- Units are signed by their author
- Erasure coding (not replication) for graph resilience
- WAL for local persistence
- Gossip (SWIM-like) for membership

See `docs/vision/SYSTEM_VISION.md` for the full conceptual design.
See `docs/decisions/` for ADRs.

## Technology

- **Language**: Rust (workspace), consistent with pact and lattice
- **Config**: TOML (human), protobuf (wire) via prost + tonic
- **Async**: tokio multi-threaded
- **CLI**: clap
- **Errors**: thiserror, typed enums
- **Testing**: proptest (properties), cucumber-rs (BDD), criterion (benchmarks)
- **License**: Apache-2.0

## Build commands

```
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

## Development conventions

- All public types derive `Debug, Clone, Serialize, Deserialize` where sensible
- No `.unwrap()` in production code
- No `unsafe` without documented justification
- Every public item has a doc comment
- Conventional commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`

## Repository structure

```
taba/
├── CLAUDE.md              # This file (project context)
├── .claude/               # Workflow router, roles, commands
├── specs/                 # Domain specs, features, architecture
├── docs/                  # Vision, ADRs
├── guidelines/            # Coding, testing, build order
├── memory/                # Session context, decisions log, open questions
├── crates/                # Rust workspace (generated during implementation)
├── proto/                 # Protobuf definitions (generated during implementation)
└── tests/                 # Integration and e2e tests
```

## Integration points

- **pact**: opt-in via hpc-core for HPC node management
- **lattice**: opt-in for HPC workload scheduling
- **sovra**: opt-in for federated key management and cross-org trust
- **K8s**: migration tool (later phase) reads manifests, generates taba units
