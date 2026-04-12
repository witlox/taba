# Coding Guidelines

## Language and toolchain

- Rust stable (latest), edition 2021
- `rustfmt` with project config (see `rustfmt.toml`)
- `clippy` with `-D warnings` (deny all warnings)
- `cargo-deny` for dependency auditing

## Dependency policy

- Minimize dependencies. Every new crate must be justified.
- Pin major versions. Use `>=x.y, <x+1` ranges.
- Prefer crates with stable APIs (1.0+). Document risk for pre-1.0 deps.
- Run `cargo deny check` before merging.
- No `git` dependencies in release builds.
- Approved foundational crates: tokio, tonic, prost, serde, clap, thiserror,
  tracing, proptest, bytes, dashmap, parking_lot.
- New dependencies require review against this list.

## Error handling

- Use `thiserror` for library error types. Typed enums, not strings.
- Every error variant has a meaningful name and context.
- No `.unwrap()` in production code. Use `.expect("reason")` only in
  truly-unreachable cases with a comment explaining why.
- Propagate errors with `?`. Don't swallow errors silently.
- User-facing errors must be actionable: say what went wrong AND what to do.

## Types and naming

- Newtypes for domain concepts: `struct NodeId(Uuid)`, not bare `Uuid`.
- Derive `Debug, Clone, Serialize, Deserialize` on all public types where sensible.
- Use `#[non_exhaustive]` on public enums that may grow.
- Use builder pattern for complex construction (more than 3 required fields).

## Async

- Runtime: tokio multi-threaded.
- All I/O is async. CPU-bound work uses `tokio::task::spawn_blocking`.
- No `block_on` inside async context.
- Cancellation safety: document whether each async fn is cancellation-safe.

## Testing

- Unit tests: `#[cfg(test)] mod tests` in the same file.
- Integration tests: `tests/` directory per crate.
- BDD: feature files in `specs/features/`, step definitions in test code.
- Property-based: `proptest` for invariant-bearing code. Minimum 10,000 cases.
- Mocks: use trait objects or the test harness crate. No mocking frameworks.

## Documentation

- Every public item has a doc comment.
- Doc comments explain WHY, not just WHAT.
- Include examples in doc comments for non-obvious APIs.
- Module-level docs explain the module's role in the system.

## Security-critical code

- All cryptographic operations go through `taba-security` crate.
- No custom crypto implementations. Use audited libraries.
- Signature verification before any graph merge operation.
- Capability checks before any resource access.
- Log security-relevant events via `tracing` at appropriate levels.

## Performance

- Allocation-conscious in hot paths (solver, graph merge, gossip).
- Use `bytes::Bytes` for zero-copy network buffers.
- Profile before optimizing. No premature optimization.
- Benchmark critical paths with `criterion`.

## Git conventions

- Conventional commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`
- One logical change per commit.
- Feature branches off `main`.
- Squash merge to `main`.
