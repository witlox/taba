# Session Context

## Project identity

- **Name**: taba (束, Japanese mathematical term for sheaf)
- **Repo**: witlox/taba
- **License**: Apache-2.0
- **Language**: Rust (workspace)
- **Author**: Pim Witlox

## Key design decisions (load-bearing — do not change without full review)

1. No masters — all nodes are peers
2. CRDT for graph replication — no consensus protocol for normal operations
3. Fail closed on security conflicts — no implicit resolution
4. Deterministic solver — same input = same output on any node
5. Units are signed by their author — graph integrity depends on this
6. Capability-based security — zero default access
7. Erasure coding (not replication) for graph resilience
8. WAL for local persistence
9. Gossip (SWIM-like) for membership
10. The control plane emerges from unit composition — not a separate system

## Ecosystem relationships

- taba is a fourth project alongside pact (Rust), lattice (Rust), sovra (Go)
- Integration with hpc-core crates is opt-in, not required
- taba does not replace the other projects — they each own their space

## Naming conventions

- Crate prefix: `taba-`
- Binary names: `taba` (node daemon), `taba-cli` (CLI tool)
- Config files: TOML
- Proto package: `taba.v1`

## Current phase

M1 through M7 complete. Fourteen crates implemented with 867 tests:
- taba-common: identity newtypes, Ppm arithmetic, clocks, config
- taba-core: unit model, capabilities, validation, contracts
- taba-security: Ed25519, scope, taint, delegation, Shamir, SLSA
- taba-graph: d-state CRDT, merge, policy chains
- taba-solver: deterministic placement, conflict detection, cycles
- taba-observe: decision trails, events, health, Prometheus, alerts
- taba-node: WAL, reconciler, runtime, health, mode, discovery
- taba-gossip: SWIM membership, signed messages, 2-witness
- taba-erasure: Reed-Solomon GF(2^8), shard distribution
- taba-cli: clap CLI (init, apply, unit, status, compose, audit)
- taba-k8s: K8s manifest converter
- taba-acceptance: BDD (cucumber-rs 0.23)
- taba-test-harness: builders, InMemoryUnitStore, proptest
- taba-integration: e2e tests (15)

Post-M7: fidelity sweep #2, adversary sweep (30 findings, 3
Critical resolved), OQ-005 resolved, OQ-007 re-evaluated, CI/CD
(3-tier), LICENSE, CONTRIBUTING.md, CHANGELOG.md, rust-toolchain.toml,
mdbook docs (gh-pages).

Next: Resolve 6 High adversary findings. BDD step definitions.
Tag v0.1.0.

## Open questions

See `memory/OPEN_QUESTIONS.md` for tracked questions.
