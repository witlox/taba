# taba — AGENTS.md

Extends `~/.config/opencode/AGENTS.md` with project-specific context.
The global workflow router (mode detection, intent dispatch, diamond
protocol, six role agents, CI tiers, documentation requirements) applies
in full. This file adds taba-specific detail on top.

## What is taba?

taba (束, Japanese for "sheaf") is a next-generation infrastructure
primitive. It replaces container + orchestrator (Docker + K8s) with
self-describing, capability-aware workload units composed through a
distributed solver. The control plane emerges from unit composition —
it is not a separate system.

Fourth project in the witlox ecosystem alongside pact (Rust, HPC
config), lattice (Rust, HPC scheduling), sovra (Go, federated key
management). Integration is opt-in via hpc-core crates. Each project
owns its space.

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

## Project state

**Phase**: Implementation. M1 (types compile) and M2 (single-node
compose) complete. Specs, architecture, and adversary reviews were
completed pre-implementation.

| Stage | Status |
|-------|--------|
| Domain model, invariants, failure modes | Complete |
| Adversary spec review (57 findings) | All critical/high resolved |
| Architecture (module map, interfaces, data models) | Complete |
| Adversary architecture review (45 findings) | All critical/high resolved |
| BDD feature files (128 scenarios, 20 files) | Complete |
| Fidelity baseline | Not established (first auditor sweep after M3) |
| M1: Types compile (common, core, test-harness) | Complete — 147 tests |
| M2: Single-node compose (+ graph, solver, security) | Complete — 313 tests |
| M3: Persistent (+ node WAL) | Not started |

**Next**: All milestones (M1–M7) complete. The system is
feature-complete: types, single-node compose, persistence,
multi-node, CLI, hardened security, and K8s migration.

## Build phases

See `specs/architecture/build-phases.md` for the full dependency DAG
and per-phase implementation details.

| Milestone | Crates | Capability |
|-----------|--------|------------|
| M1: Types compile | common, core, test-harness | Unit declarations parse and validate |
| M2: Single-node compose | + graph, solver, security | Compose units on one node |
| M3: Persistent | + node (WAL) | Survives restart |
| M4: Multi-node | + gossip, erasure | Distributed operation |
| M5: Usable | + cli | Human-operable |
| M6: Hardened | + security advanced | Production-grade security |
| M7: Migration | + k8s tool | K8s users can onboard |

## Integration points

- **pact**: opt-in via hpc-core for HPC node management
- **lattice**: opt-in for HPC workload scheduling
- **sovra**: opt-in for federated key management and cross-org trust
- **K8s**: migration tool (later phase) reads manifests, generates taba units

## Pre-commit discipline

Before claiming "all tests pass" or committing code:

1. Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`
2. Run the relevant `cargo test` commands and **show the output**
3. Never commit based solely on subagent reports — verify in the main context
4. When adding steps/functions to shared namespaces (BDD steps, trait impls), grep for name conflicts first

Use `/verify` to run the full checklist. See `~/.config/opencode/commands/verify.md`.

## Repository structure

```
taba/
├── AGENTS.md             # This file (project workflow router)
├── .opencode/
│   └── guidelines/
│       └── rust.md       # Taba-specific Rust coding (extends global)
├── specs/                # Domain specs, features, architecture
│   ├── domain-model.md
│   ├── ubiquitous-language.md
│   ├── invariants.md
│   ├── assumptions.md
│   ├── failure-modes.md
│   ├── toml-schema.md
│   ├── features/*.feature
│   ├── cross-context/
│   ├── architecture/
│   │   ├── module-map.md
│   │   ├── dependency-graph.md
│   │   ├── build-phases.md
│   │   ├── testing-strategy.md
│   │   ├── enforcement-map.md
│   │   ├── error-taxonomy.md
│   │   ├── interfaces/
│   │   ├── data-models/
│   │   └── events/
│   ├── findings/         # Adversary review findings
│   ├── fidelity/         # (established when auditor runs)
│   └── escalations/
├── docs/                 # Vision, ADRs
│   ├── vision/SYSTEM_VISION.md
│   └── decisions/ADR-00*.md
├── memory/               # Session context, decisions log, open questions
├── crates/               # Rust workspace (generated during implementation)
├── proto/                # Protobuf definitions (generated during implementation)
└── tests/                # Integration and e2e tests
```

## Project-specific role context

When spawning role subagents, include the relevant section below in
the prompt. The global role files (`~/.config/opencode/agents/`) define
behavioral rules and output artifacts; these sections add taba-specific
domain context.

### Architect — taba design principles

- Thin crate interfaces: narrow typed traits, not god-objects
- Solver must be deterministic: same graph + same nodes = same placement
- CRDT merge must be commutative, associative, idempotent — verify algebraically
- Security checks at composition boundary, not deep in implementation
- WAL before effect: all state mutations through write-ahead log
- Single-node must work before multi-node (progressive complexity)

### Adversary — taba attack surfaces

- **CRDT merge**: malformed operations causing inconsistent state?
  Merge ordering exploits? Conflict resolution bypass?
- **Solver determinism**: same input producing different output across
  nodes? Floating point, hash order, or randomness creep? Starvation
  from pathological input?
- **Capability system**: capability forgery? Scope escalation? Stale
  capabilities after revocation? Cross-unit capability leakage?
- **Unit signing**: author impersonation? Replay of signed units?
  Signature verification bypass on gossip-received units?
- **Gossip membership**: poisoned membership lists? Sybil attack via
  rapid join/leave? Stale membership causing incorrect placement?
- **Erasure coding**: reconstruction failure under concurrent node
  loss? Corrupted shard accepted without detection? Reconstruction
  storms?
- **WAL integrity**: partial writes? Replay correctness after crash?
  WAL growth unbounded?
- **Data lineage**: lineage chain tampered? Provenance metadata
  stripped? Lineage cycles?
- **Policy enforcement**: fail-open paths in security policy
  evaluation? Policy bypass via composition ordering?
- **Peer-to-peer**: network partition behavior? Split-brain on
  security decisions? Node lying about its state?

### Integrator — taba integration points

- Author composes units → solver validates constraints → placement → reconciliation loop
- Security capability request → policy evaluation → capability grant → runtime enforcement at node
- Data unit declaration → lineage attachment → consumption → provenance chain verifiable
- Unit publish → gossip propagation → CRDT merge on all peers → solver convergence
- Node join → gossip membership → graph sync → solver includes new capacity
- Node failure → gossip detection → erasure reconstruction → solver re-placement
- Policy conflict → fail-closed → resolution workflow → policy update → re-evaluate
- External integration (pact/lattice/sovra) → opt-in boundary → capability scoping
- WAL write → crash → recovery → CRDT state restored → gossip resync
- Retention policy → data lineage check → erasure-coded shard cleanup

### Implementer — taba constraints

- Async via tokio where appropriate; blocking threads for CPU-bound solver work
- Error handling: thiserror for typed errors, anyhow avoided in library code
- Serialization: protobuf for wire format, serde for internal persistence
- CRDT operations must be commutative, associative, idempotent
- Solver must be pure (no side effects, no I/O, no randomness)
- Domain language from `specs/ubiquitous-language.md` — no abbreviations
  in public APIs (write `WorkloadUnit`, not `WlUnit`)
- BDD: `cucumber` crate, feature files in `specs/features/`, step
  definitions in `tests/acceptance/`
- Property testing: `proptest` for invariant-critical code (CRDT
  merges, solver, capability validation), minimum 10k+ cases
- Benchmarks: `criterion` for solver and CRDT hot paths

### Analyst — source material

The system was designed through extended conversations covering:
architecture (self-describing typed units, emergent control plane,
peer-to-peer), consistency (CRDT graph, deterministic solver), security
(zero-access, capability-based, fail-closed), data (lineage and
provenance as structural properties), resilience (erasure coding, WAL,
gossip membership), and integration (opt-in with pact, lattice, sovra
via hpc-core crates).

The system vision is in `docs/vision/SYSTEM_VISION.md`.
Design decisions are in `docs/decisions/`.

## Workflow commands

Project state detection and verification use the global opencode
commands. The key ones for this project:

| Command | When |
|---------|------|
| `/status` | First message of every new session. Establishes project state. |
| `/verify` | Before every commit. Runs fmt + clippy + build + tests. |
| `/spec-check` | After completing a feature or design phase. Validates specs align with code. |
| `/e2e` | After integration phase, before declaring integration complete. |

Escalations go to `specs/escalations/` and must resolve before the
escalating phase completes.
