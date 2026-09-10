# Taba — Rust Coding Standards

Extends `~/.config/opencode/guidelines/rust.md` with project-specific
conventions. The global standard (fmt, clippy, unwrap, thiserror,
test naming, coverage, edition, slow tests, proptest) applies in full;
this file adds taba-specific detail on top.

## Workspace

- Crates under `crates/` (see `specs/architecture/module-map.md`)
- Shared dependencies pinned at workspace level
- `taba-common` is the leaf crate — imports only stdlib + serde + uuid
- `taba-proto` is generated code — do not hand-edit
- Crate prefix: `taba-`
- Binary names: `taba` (node daemon), `taba-cli` (CLI tool)
- Proto package: `taba.v1`

## Unsafe code policy

`unsafe_code = "deny"` at workspace level. No crates currently require
unsafe. If future crates need it (e.g., FFI bindings), add explicit
per-crate `#[allow(unsafe_code)]` with a `// SAFETY:` comment on every
unsafe block.

## Clippy configuration

```toml
[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"

[workspace.lints.rust]
unsafe_code = "deny"
```

## Cargo-deny (deny.toml)

- Advisory DB: unmaintained = error, yanked = warn
- Allowed licenses: Apache-2.0, MIT, BSD-2/3-Clause, ISC, Unicode-DFS-2016, MPL-2.0, Zlib, OpenSSL, BlueOak-1.0.0
- Sources: only crates.io (no unknown git)
- Multiple versions: warn

## Dependency policy

- Minimize dependencies. Every new crate must be justified.
- Pin major versions. Use `>=x.y, <x+1` ranges.
- Prefer crates with stable APIs (1.0+). Document risk for pre-1.0 deps.
- Run `cargo deny check` before merging.
- No `git` dependencies in release builds.
- Approved foundational crates: tokio, tonic, prost, serde, clap,
  thiserror, tracing, proptest, bytes, dashmap, parking_lot.
- New dependencies require review against this list.

## Types and naming

- Newtypes for domain concepts: `struct NodeId(Uuid)`, not bare `Uuid`.
- Derive `Debug, Clone, Serialize, Deserialize` on all public types
  where sensible.
- Use `#[non_exhaustive]` on public enums that may grow.
- Use builder pattern for complex construction (more than 3 required fields).
- All type names match `specs/ubiquitous-language.md` exactly. New
  domain terms: check spec first, escalate if not found. No
  abbreviations in public APIs (write `WorkloadUnit`, not `WlUnit`).

## Core patterns

### Units & composition

- All unit types implement `Debug, Clone, Serialize, Deserialize`
- Units are immutable after signing — mutations produce new versions
- Unit identity: content-addressed hash of (type + payload + author + timestamp)
- Capability tokens are unforgeable, non-transferable, scoped, and time-bounded

### CRDT graph

- CRDT operations must be commutative, associative, and idempotent
- No consensus required for normal operations — Raft only where unavoidable
- Graph merges are deterministic: same inputs on any node = same result
- Conflict detection via vector clocks; resolution via policy (fail-closed for security)

### Solver

- Deterministic: same input = same output on any node
- Solver must be pure (no side effects, no I/O, no randomness)
- Constraint satisfaction expressed as typed constraints, not ad-hoc checks
- Backtracking bounded to prevent starvation

### Gossip & membership

- SWIM-like protocol for membership
- Gossip payloads must be bounded (no unbounded state replication)
- Membership changes are eventually consistent

### Data & lineage

- Data units carry provenance metadata (origin, transforms, consumers)
- Lineage is structural — not a separate logging system
- Retention policies enforced by the graph, not by external tooling

## Error handling

- `thiserror` for all error types (see `specs/architecture/error-taxonomy.md`)
- Every error is categorized: Retriable, Permanent, Security
- Wrap with context: `.map_err(|e| TabaError::from(e).with_context("unit compose"))`
- No `anyhow` in library crates; `anyhow` only in binary crates
- User-facing errors must be actionable: say what went wrong AND what to do

## Async

- `tokio` multi-threaded runtime for I/O and gossip
- CPU-bound solver operations on `tokio::task::spawn_blocking`
- No blocking I/O on async threads
- No `block_on` inside async context
- `#[tokio::test]` for async tests
- Cancellation safety: document whether each async fn is cancellation-safe

## Protobuf

- `tonic` for gRPC server/client (if inter-node communication uses gRPC)
- `prost` for protobuf codegen
- Proto definitions in `proto/`
- All messages carry: `author_id`, timestamp, trace ID

## BDD

- `cucumber` crate for Gherkin scenario execution
- Feature files in `specs/features/`
- Step definitions in `tests/acceptance/`
- One step definition file per feature file

## Property testing

- `proptest` for invariant-critical code (CRDT merges, solver,
  capability validation)
- Minimum 10k+ cases for core invariants
- `criterion` benchmarks for solver and CRDT hot paths

## Security-critical code

- All cryptographic operations go through `taba-security` crate
- No custom crypto implementations. Use audited libraries
- Signature verification before any graph merge operation
- Capability checks before any resource access
- Log security-relevant events via `tracing` at appropriate levels

## Performance

- Allocation-conscious in hot paths (solver, graph merge, gossip)
- Use `bytes::Bytes` for zero-copy network buffers
- Profile before optimizing. No premature optimization
- Benchmark critical paths with `criterion`
