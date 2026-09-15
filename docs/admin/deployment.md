# Deployment

## Single-node (dev/test)

```sh
# Initialize
taba init

# Apply units
taba apply workload.taba.toml

# Compose
taba compose

# Status
taba status
```

State is persisted to `~/.taba/` (or `--state-dir`):
- `keypair` — Ed25519 private key (0600 permissions)
- `config.json` — trust domain, cluster ID, node ID
- `graph.json` — active units (JSON array)

### Restart

On restart, `taba` loads the keypair, config, and graph from the
state directory. The graph is reconstructed in memory from the
persisted JSON.

## Multi-node (M4+)

Multi-node operation requires taba-gossip (SWIM membership) and
taba-erasure (Reed-Solomon coding). These are implemented as
library crates but not yet wired into a multi-node daemon.

The planned multi-node flow:
1. Each node runs `taba init` independently
2. Nodes discover each other via seed nodes (gossip)
3. Units are replicated via CRDT merge (commutative, associative, idempotent)
4. Graph shards are erasure-coded across N nodes (k-of-n)
5. Solver runs locally on each node (same input = same output)

## Docker

The taba CLI and node daemon can run in Docker:

```dockerfile
FROM rust:1.85-slim AS builder
WORKDIR /taba
COPY . .
RUN cargo build --workspace --release

FROM debian:bookworm-slim
COPY --from=builder /taba/target/release/taba /usr/local/bin/
COPY --from=builder /taba/target/release/taba-k8s /usr/local/bin/
ENTRYPOINT ["taba"]
```

## Configuration

See [Configuration Reference](configuration.md) for all options.
