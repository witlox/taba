# Configuration Reference

## Cluster configuration (`ClusterConfig`)

Stored in TOML, loaded on node startup, propagated via gossip.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cluster_id` | UUID | (generated) | Unique identifier for this cluster |
| `resilience_pct` | u8 | 33 | Erasure coding resilience percentage. k = ceil(N * (1 - R/100)) |
| `graph_memory_limit_bytes` | u64 | 1,073,741,824 | Maximum memory for active graph per node (INV-R6) |
| `shamir_total_shares` | u8 | 5 | Total shares in Shamir ceremony |
| `shamir_threshold` | u8 | 3 | Minimum shares for reconstruction |
| `max_data_hierarchy_depth` | u8 | 16 | Maximum depth for hierarchical data units |
| `gossip_interval` | Duration | 500ms | SWIM probe interval |
| `gossip_suspicion_timeout` | Duration | 5s | Base suspicion timeout |
| `witness_count` | u8 | 2 | Independent witnesses for failure (INV-R3) |
| `indirect_probe_count` | u8 | 3 | Peers asked for indirect probes |
| `retransmit_multiplier` | u8 | 4 | Piggyback rounds = mult × log₂(N) |
| `max_piggyback_entries` | u8 | 8 | Max membership changes per message |
| `suspicion_multiplier` | u8 | 4 | Effective timeout = mult × ceil(log₂(N)) × interval |
| `max_spawn_depth` | u8 | 4 | Maximum spawn depth for bounded tasks (INV-W3) |
| `revocation_grace_window` | Option<u64> | None | Grace window in logical clock delta (INV-S3) |
| `fleet_command_rate_limit_lc` | u64 | 1000 | Min LC delta between fleet commands of same type |

## Node configuration (`NodeConfig`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `wal_dir` | String | "./wal" | Directory for WAL segment files |
| `listen_addr` | String | "0.0.0.0:7946" | Bind address for peer communication |
| `seed_nodes` | Vec<String> | [] | Seed nodes for gossip bootstrap |
| `require_tpm` | bool | false | Whether TPM attestation is required (A5) |
| `environment` | Option<String> | None | Environment tag (e.g., "env:dev") |
| `custom_tags` | Vec<(String, String)> | [] | Additional custom tags |
| `archive_backend` | Option<ArchiveBackendConfig> | None | Archive storage configuration |

## Archive backend

```toml
[archive_backend]
# Local filesystem
backend = "local"
path = "/var/lib/taba/archive"

# Or S3-compatible
backend = "s3"
endpoint = "https://s3.amazonaws.com"
bucket = "taba-archive"
region = "us-east-1"
```

## Gossip auto-tuning

Effective suspicion timeout scales with cluster size:

```
effective = max(base_timeout, suspicion_mult × ceil(log2(N)) × interval)
```

At 10,000 nodes: ~1 KB/s per node, ~10 MB/s cluster-wide. Acceptable.

Key revocation messages use `2 × retransmit_multiplier × log₂(N)`
rounds (double normal) for rapid convergence.
