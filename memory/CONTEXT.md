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

M1 through M6 complete. Eleven crates implemented with 814 tests:
- taba-common: identity newtypes, Ppm arithmetic, clocks, config
- taba-core: unit model, capabilities, validation, contracts
- taba-security: Ed25519 signing/verification, scope enforcement,
  capability enforcement, taint computation, delegation, solo
  bootstrap, Shamir secret sharing (GF(2^8)), TPM/software
  attestation, node enrollment ceremony, SLSA provenance
- taba-graph: δ-state CRDT composition graph, merge, policy chains
- taba-solver: deterministic placement, conflict detection, cycles
- taba-observe: decision trails, structured events, health aggregation
- taba-node: disk-backed WAL, reconciler, SimulatedRuntime +
  DockerRuntime, health, mode manager, capability discoverer
- taba-gossip: SWIM membership, signed messages, 2-witness failure
  detection, capability advertisement, cross-domain stubs
- taba-erasure: Reed-Solomon GF(2^8), shard distribution,
  reconstruction with priority queue + circuit breaker
- taba-cli: clap-based CLI (init, apply, unit, status, compose,
  audit, push), TOML unit parser, local key management
- taba-test-harness: builders, InMemoryUnitStore, proptest strategies

Next: M7 (Migration) — taba-k8s tool that reads K8s manifests and
generates taba unit declarations.

## Open questions

See `memory/OPEN_QUESTIONS.md` for tracked questions.
