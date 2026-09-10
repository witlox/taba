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

M1 (Types compile) complete. Three crates implemented with 147 tests:
- taba-common: identity newtypes, Ppm arithmetic, clocks, config
- taba-core: unit model, capabilities, validation, contracts
- taba-test-harness: builders, InMemoryUnitStore, proptest strategies

Next: M2 (Single-node compose) — taba-security (signing, verification,
scope enforcement), taba-graph (CRDT graph, merge, WAL), taba-solver
(composition resolution, placement, conflict detection).

## Open questions

See `memory/OPEN_QUESTIONS.md` for tracked questions.
