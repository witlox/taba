# Context for Claude Code Sessions

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

Starting: Phase 1 (Analyst) — domain extraction from design conversation.

## Open questions

See `memory/OPEN_QUESTIONS.md` for tracked questions.
