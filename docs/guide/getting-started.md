# Getting Started

## Prerequisites

- **Rust** stable (1.85+). Install via [rustup](https://rustup.rs).
- **just** (optional, command runner): `cargo install just --locked`
- **cargo-nextest** (optional, faster tests): `cargo install cargo-nextest --locked`
- **cargo-deny** (optional, dependency auditing): `cargo install cargo-deny --locked`
- **mdbook** (optional, local docs): `cargo install mdbook --locked`

## Installation

### From source

```sh
git clone https://github.com/witlox/taba.git
cd taba
cargo build --workspace --release

# Binaries are in target/release/
# - taba      (CLI)
# - taba-k8s  (K8s converter)
```

### Verify the build

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

## Your first unit

### 1. Initialize a local node

```sh
taba init
```

This generates:
- An Ed25519 keypair (private key stored with `0600` permissions)
- A trust domain
- A root governance unit
- A role assignment (you are the admin)

State is stored in `~/.taba/` by default. Use `--state-dir` to override.

### 2. Author a workload unit

Create `hello.taba.toml`:

```toml
[unit]
name = "hello-web"
image = "nginx:alpine"
```

This is the simplest possible unit — Level 0 in the progressive disclosure
schema. Everything else defaults: kind = service, scaling min=1 max=1,
health = OS-level process monitoring, placement on failure = from environment.

### 3. Apply it to the graph

```sh
taba apply hello.taba.toml
```

The unit is:
1. Parsed from TOML
2. Identity filled in (author, trust domain, cluster ID from your keypair)
3. Signed with Ed25519 (INV-S3)
4. Structurally validated (`DefaultValidator`)
5. Scope-checked (INV-S8: no duplicate author scopes)
6. Signature verified by the graph's verifier
7. Inserted into the composition graph
8. Persisted to `~/.taba/graph.json`

### 4. Check status

```sh
taba status
```

Shows graph stats (active units, pending, archived, memory), operational
mode, node ID, trust domain, and cluster ID.

### 5. Run the solver

```sh
taba compose
```

The solver takes a snapshot of the graph, runs deterministic placement
(fixed-point Ppm arithmetic, no floating-point), and shows the results:
placements, unplaceable units, and detected conflicts.

### 6. List units

```sh
taba unit list
taba unit inspect <unit-id>
```

### 7. Archive a unit

```sh
taba unit archive <unit-id>
```

Archived units are removed from the active set but retained for
provenance integrity. Governance units cannot be archived (INV-G3).

### 8. Audit

```sh
taba audit trails          # decision trails
taba audit provenance <id> # provenance chain for a data unit
```

## Next steps

- [Unit Authoring](unit-authoring.md) — full TOML schema, all 6 levels
- [Composition & Placement](composition.md) — how the solver works
- [K8s Migration](k8s-migration.md) — convert existing K8s manifests
- [Configuration Reference](../admin/configuration.md) — all config options
