# taba — Contributing

## Development setup

### Prerequisites

- **Rust** stable (1.85+). Install via [rustup](https://rustup.rs/).
- **cargo-nextest** (optional, faster test runner): `cargo install cargo-nextest --locked`
- **cargo-deny** (optional, dependency auditing): `cargo install cargo-deny --locked`
- **just** (optional, command runner): see [justfile](justfile)

### Getting started

```sh
git clone https://github.com/witlox/taba.git
cd taba
cargo build --workspace
cargo test --workspace
```

## Coding standards

### Rust

All Rust code follows the project coding standards in
[`.opencode/guidelines/rust.md`](.opencode/guidelines/rust.md), which
extends the global standard at `~/.config/opencode/guidelines/rust.md`.

Key rules:

- `unsafe_code = "deny"` at workspace level — no `unsafe` without documented justification
- No `.unwrap()` in production code — use `expect()` with a reason in tests only
- Every public item has a doc comment (`///`) explaining WHY, not WHAT
- Conventional commits: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`
- Edition 2024, `rustfmt.toml` with `max_width = 100`
- Clippy pedantic + nursery at warn level, `-D warnings` in CI

### Formatting and linting

```sh
cargo fmt --all              # apply
cargo fmt --all -- --check   # verify (CI gate)
cargo clippy --workspace --all-targets -- -D warnings
```

## Testing

### Three-tier system

The project uses a cascading three-tier test system (see
[`.opencode/guidelines/ci.md`](.opencode/guidelines/ci.md)):

| Tier | What | When | Command |
|------|------|------|---------|
| 1 (fast) | Unit tests + BDD @smoke | Between every edit, pre-commit | `just` or `just test` |
| 2 (slow) | Tier 1 + slow-marked tests + full BDD | Pre-PR, nightly | `just test-slow` |
| 3 (full) | Tier 2 + e2e against real services | Pre-merge, nightly | `just test-full` |

### Slow tests

Mark slow tests with `#[ignore = "slow: <reason — what makes this expensive>"]`.
Tier 1 skips them; Tier 2 runs them via `--run-ignored=only`.

### Property tests

Use `proptest` for invariant-critical code (CRDT merges, solver,
capability validation, Shamir secret sharing). Minimum 10,000 cases
for core invariants.

### BDD acceptance tests

Feature files are in `specs/features/` (Gherkin format). Step
definitions are in `tests/acceptance/` using `cucumber-rs`. Tag
scenarios: `@smoke` (Tier 1), `@unit` (in-process), `@integration`
(multi-component).

## Architecture

See [`specs/architecture/module-map.md`](specs/architecture/module-map.md)
for the canonical crate boundaries. Every boundary is load-bearing:
crossing a boundary without going through the documented API surface
is a defect.

### Adding a new crate

1. Create `crates/<name>/` with `Cargo.toml` and `src/lib.rs`
2. Add to `[workspace].members` in root `Cargo.toml`
3. Apply `[lints] workspace = true`
4. Add doc comment to `lib.rs` explaining the crate's role
5. Update `specs/architecture/module-map.md` with responsibilities,
   public API surface, internal modules, does NOT own, traces to
6. Update `specs/architecture/dependency-graph.md` if the crate
   changes the build DAG

## Pull request process

1. Run `just` (fmt-check + clippy + Tier 1 tests) — must pass
2. Run `just test-slow` if your change touches security, graph, or
   solver code
3. Conventional commit messages: `feat:`, `fix:`, `refactor:`,
   `test:`, `docs:`
4. One logical change per commit
5. Squash merge to `main`
6. If your change changes setup, build, or test commands, update
   this file and `README.md`

## Pre-commit checklist

- [ ] `cargo fmt --all -- --check` clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 warnings
- [ ] `cargo test --workspace` — all tests pass
- [ ] No `unwrap()` in non-test code
- [ ] No `unsafe` without documented justification
- [ ] Every public item has a doc comment
- [ ] Domain language matches `specs/ubiquitous-language.md`
- [ ] Conventional commit message

## License

Apache-2.0. See [LICENSE](LICENSE).
