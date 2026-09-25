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

With `just` (cascading test tiers):

```
just            # fmt-check + lint + deny + Tier 1
just test       # Tier 1: nextest + doc tests + BDD @smoke
just test-slow  # Tier 2: + ignored tests + full BDD
just test-full  # Tier 3: + integration tests
just docs       # mdbook build
```

Binary and Docker e2e:

```
cargo test -p taba-e2e --test binary                    # 21 binary subprocess tests
cargo test -p taba-e2e --test docker -- --ignored       # 22 Docker container tests
cargo test -p taba-e2e --test wal -- --ignored          # 2 WAL crash recovery tests
cargo test -p taba-gossip --test udp -- --ignored       # 2 UDP transport tests
```

Environment variables:
- `TABA_BDD_FAST=1` — run only `@smoke` tagged BDD scenarios
- `TABA_BDD_ACCEPTANCE=1` — run all 268 BDD scenarios (all have real assertions)

## Development conventions

- All public types derive `Debug, Clone, Serialize, Deserialize` where sensible
- No `.unwrap()` in production code
- No `unsafe` without documented justification
- Every public item has a doc comment
- Conventional commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`

## Project state

**Phase**: Post-implementation validation. All 7 milestones (M1–M7)
complete. 13 crates + taba-e2e + taba-integration. 960 unit tests,
47 e2e tests, 268 BDD scenarios. Fidelity: 67 invariants all
VERIFIED (MOCK+), 0 PARTIAL, 0 UNVERIFIED. Adversary sweep: 3
Critical + 6 High all resolved, 21 GitHub issues filed for
Medium/Low/Info. Release v2026.6.56 (workspace: 2026.6.0; binaries + Docker image +
gh-pages docs).

| Stage | Status |
|-------|--------|
| Domain model, invariants, failure modes | Complete |
| Adversary spec review (57 findings) | All critical/high resolved |
| Architecture (module map, interfaces, data models) | Complete |
| Adversary architecture review (45 findings) | All critical/high resolved |
| BDD feature files (268 scenarios, 20 files) | Complete — all 268 scenarios have real assertions (0 no-ops) |
| Fidelity baseline | Established — 67 VERIFIED, 0 PARTIAL, 0 UNVERIFIED |
| Adversary implementation sweep (30 findings) | All 3 Critical + 6 High resolved, 21 issues filed |
| M1: Types compile (common, core, test-harness) | Complete — 165 tests |
| M2: Single-node compose (+ graph, solver, security) | Complete — 330 tests |
| M3: Persistent (+ node WAL, observe) | Complete — 132 tests |
| M4: Multi-node (+ gossip, erasure) | Complete — 119 tests |
| M5: Usable (+ cli) | Complete — 55 tests |
| M6: Hardened (+ security advanced) | Complete — 49 tests |
| M7: Migration (+ k8s tool) | Complete — 31 tests |
| Integration tests | Complete — 15 tests |
| Binary e2e (subprocess) | Complete — 22 tests |
| Docker e2e (containers) | Complete — 4 tests |
| WAL crash recovery e2e | Complete — 2 tests |
| UDP gossip e2e | Complete — 2 tests |
| Documentation (mdbook, gh-pages) | Complete |
| STRIDE security analysis | Complete — 19 threats, 0 Critical/High remaining |
| CI (4 workflows) | Complete — ci, nightly, docs, release |
| Branch protection | Active — 2 rulesets (no-delete+non-FF, linear+5 checks) |
| Release | Tagged v2026.6.56 (x86_64, aarch64, macOS, Docker) |

### Post-M7 accomplishments

| Item | Status |
|------|--------|
| NativeRuntime + WasmRuntime + MicroVmRuntime (INV-N6) | Complete — std::process, state tracking, firecracker/QEMU subprocess |
| RuntimeSelector dispatches by ArtifactType | Complete — LocalClient uses selector for all four runtime types |
| TOML parser accepts 'microvm' with kernel/rootfs | Complete — Level 2.5 in TOML schema |
| ADR-007: Runtime model | Complete — per-artifact-type dispatch via RuntimeSelector |
| All 268 BDD scenarios with real assertions | Complete — 0 no-ops (was 264 no-ops) |
| 47 e2e tests (22 Docker + 21 binary + 2 WAL + 2 UDP) | Complete — all four unit types exercised |
| Ed25519 signing wired into CLI (INV-S3) | Complete — LocalClient signs every unit, graph verifies |
| Scope checker + verifier wired into LocalClient | Complete — role assignment created on init, populated on load |
| 6 UNVERIFIED invariants implemented (K4, D3, D5, E2, N5, G4) | Complete — each with tests |
| 6 PARTIAL invariants resolved (S7, C5, K3, D2, E3, N3) | Complete — each with tests |
| Docker reconciliation (`taba reconcile` command) | Complete — starts containers for each placement |
| UdpTransport implemented and tested | Complete — tokio::net::UdpSocket + serde_json |
| WAL crash recovery e2e | Complete — append, drop, reopen, verify all entries + CRC |
| Binary e2e (all CLI commands) | Complete — init, apply, status, compose, unit, audit, push, k8s |
| K8s converter e2e | Complete — Deployment, StatefulSet, Secret, ConfigMap, NetworkPolicy, HPA |
| @smoke BDD with real assertions | Complete — table parsing, signing, graph insert, WAL check |
| GitHub branch protection | Complete — 2 rulesets, 5 required status checks |
| Versioning script (YYYY.ADRcount.commitNr) | Complete — scripts/set-version.sh |
| justfile with cascading test tiers | Complete — Tier 1/2/3 matching CI |
| Dockerfile for container image | Complete — slim Debian, taba + taba-k8s |
| Coverage with Codecov | Complete — separate job in ci.yml |
| README with binary download + accurate claims | Complete |
| All docs links verified | Complete |

**Next**: Replace remaining continuous reconciliation daemon
(taba-node). Expand multi-runtime e2e (MicroVm, Wasm, Native).
Update docs for runtime model (ADR-007).
(taba-node). Wire WAL (DiskWalManager) into CLI persistence path.
Wire UdpTransport into multi-node gossip between real processes.

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
- **K8s**: migration tool (M7) reads manifests, generates taba units

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
├── .github/workflows/    # ci.yml, nightly.yml, docs.yml, release.yml
├── specs/                # Domain specs, features, architecture
│   ├── domain-model.md
│   ├── ubiquitous-language.md
│   ├── invariants.md
│   ├── assumptions.md
│   ├── failure-modes.md
│   ├── toml-schema.md
│   ├── features/*.feature (268 scenarios, 20 files)
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
│   ├── findings/         # Adversary review findings (30 total)
│   ├── fidelity/         # INDEX.md (67 VERIFIED, 0 PARTIAL, 0 UNVERIFIED)
│   └── escalations/
├── docs/                 # mdbook documentation (deployed to gh-pages)
│   ├── SUMMARY.md        # Table of contents
│   ├── README.md         # Book intro (with logo)
│   ├── guide/            # Getting started, unit authoring, composition, status, k8s
│   ├── admin/            # Deployment, config, modes, WAL, health, keys, ceremony, enrollment, SLSA, attestation
│   ├── architecture/     # Overview, module map, dependency graph, build phases, testing, enforcement, errors, events
│   ├── security/         # Model, STRIDE analysis, capabilities, taint, delegation, zero-access
│   ├── operations/       # Troubleshooting, performance, findings
│   ├── api/              # CLI reference, TOML schema, k8s converter
│   └── decisions/        # ADR-001 through ADR-006 + index + template
├── memory/               # Session context, decisions log, open questions
├── crates/               # Rust workspace (13 crates)
│   ├── taba-common/      # Types, config, protobuf
│   ├── taba-core/        # Unit model, validation, data
│   ├── taba-security/    # Signing, verification, scope, taint, ceremony, delegation, attestation, provenance
│   ├── taba-graph/       # CRDT composition graph, WAL, compaction, query
│   ├── taba-solver/      # Deterministic placement, conflict detection, cycle detection, scoring, scaling
│   ├── taba-erasure/     # Reed-Solomon over GF(2^8), shard distribution, reconstruction
│   ├── taba-gossip/      # SWIM membership, transport (in-memory + UDP), capability, cross-domain
│   ├── taba-node/        # WAL (disk), runtime (simulated + Docker), reconciliation, mode, health, spawner, eviction, discovery
│   ├── taba-observe/     # Decision trails, events, health aggregator, Prometheus, alerts
│   ├── taba-cli/         # CLI binary (taba), parser, auth, client, commands, format
│   ├── taba-k8s/         # K8s converter binary (taba-k8s)
│   ├── taba-test-harness/ # Builders, InMemoryUnitStore, proptest strategies
│   └── taba-acceptance/  # Cucumber BDD (268 scenarios, smoke.rs + common.rs)
├── tests/
│   ├── integration/      # 15 integration tests (library API)
│   └── e2e/             # 47 e2e tests (binary, Docker, WAL, UDP)
├── scripts/
│   └── set-version.sh    # YYYY.ADRcount.commitNr versioning
├── proto/                # Protobuf definitions
├── Dockerfile            # Slim Debian with taba + taba-k8s
├── justfile              # Tier 1/2/3 test targets
├── deny.toml             # cargo-deny config
├── rust-toolchain.toml   # Stable + rustfmt + clippy
├── book.toml             # mdbook config
├── logo.png              # Original (770x648)
├── logo-readme.png       # README size (128x127)
├── logo-docs.png         # Docs size (64x63)
└── LICENSE               # Apache-2.0
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
  definitions in `crates/taba-acceptance/tests/steps/` (`smoke.rs`
  for real assertions, `common.rs` for no-ops)
- Property testing: `proptest` for invariant-critical code (CRDT
  merges, solver, capability validation), minimum 10k+ cases
- Benchmarks: `criterion` for solver and CRDT hot paths
- E2E: binary subprocess tests in `tests/e2e/binary.rs`, Docker
  tests in `tests/e2e/docker.rs` (marked `#[ignore = "slow:requires-docker"]`)

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
