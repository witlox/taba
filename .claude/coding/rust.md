# Taba — Rust Coding Standards

Extends `.claude/guidelines/rust.md` with project-specific conventions.

## Workspace

- Crates under `crates/` (see `specs/architecture/module-map.md`)
- Shared dependencies pinned at workspace level
- `taba-common` is the leaf crate — imports only stdlib + serde + uuid
- `taba-proto` is generated code — do not hand-edit

## Unsafe Code Policy

`unsafe_code = "deny"` at workspace level. No crates currently require unsafe.
If future crates need it (e.g., FFI bindings), add explicit per-crate
`#[allow(unsafe_code)]` with a `// SAFETY:` comment on every unsafe block.

## Core Patterns

### Units & Composition

- All unit types implement `Debug, Clone, Serialize, Deserialize`
- Units are immutable after signing — mutations produce new versions
- Unit identity: content-addressed hash of (type + payload + author + timestamp)
- Capability tokens are unforgeable, non-transferable, scoped, and time-bounded

### CRDT Graph

- CRDT operations must be commutative, associative, and idempotent
- No consensus required for normal operations — Raft only where unavoidable
- Graph merges are deterministic: same inputs on any node = same result
- Conflict detection via vector clocks; resolution via policy (fail-closed for security)

### Solver

- Deterministic: same input = same output on any node
- Solver must be pure (no side effects, no I/O, no randomness)
- Constraint satisfaction expressed as typed constraints, not ad-hoc checks
- Backtracking bounded to prevent starvation

### Gossip & Membership

- SWIM-like protocol for membership
- Gossip payloads must be bounded (no unbounded state replication)
- Membership changes are eventually consistent

### Data & Lineage

- Data units carry provenance metadata (origin, transforms, consumers)
- Lineage is structural — not a separate logging system
- Retention policies enforced by the graph, not by external tooling

## Error Handling

- `thiserror` for all error types (see `specs/architecture/error-taxonomy.md` when available)
- Every error is categorized: Retriable, Permanent, Security
- Wrap with context: `.map_err(|e| TabaError::from(e).with_context("unit compose"))`
- No `anyhow` in library crates; `anyhow` only in binary crates

## Async

- `tokio` multi-threaded runtime for I/O and gossip
- CPU-bound solver operations on `tokio::task::spawn_blocking`
- No blocking I/O on async threads
- `#[tokio::test]` for async tests

## Protobuf

- `tonic` for gRPC server/client (if inter-node communication uses gRPC)
- `prost` for protobuf codegen
- Proto definitions in `proto/` (or `specs/architecture/proto/`)
- All messages carry: `author_id`, timestamp, trace ID

## BDD

- `cucumber` crate for Gherkin scenario execution
- Feature files in `specs/features/`
- Step definitions in `tests/acceptance/`
- One step definition file per feature file

## Property Testing

- `proptest` for invariant-critical code (CRDT merges, solver, capability validation)
- Minimum 10k+ cases for core invariants
- `criterion` benchmarks for solver and CRDT hot paths

## Domain Language

- All type names match `specs/ubiquitous-language.md` exactly
- New domain terms: check spec first, escalate if not found
- No abbreviations in public APIs (write `WorkloadUnit`, not `WlUnit`)
