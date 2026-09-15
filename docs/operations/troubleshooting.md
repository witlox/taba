# Troubleshooting

## Common issues

### `taba init` fails

| Error | Cause | Fix |
|-------|-------|-----|
| `Io: Permission denied` | Cannot create `~/.taba/` | Check home directory permissions, or use `--state-dir` |
| `Security: KeyError` | Key generation failed | Ensure `rand` crate is available (Linux: `/dev/urandom`) |

### `taba apply` fails

| Error | Cause | Fix |
|-------|-------|-----|
| `TomlParse` | Invalid TOML syntax | Validate with `taba unit validate <file>` |
| `Core: MalformedUnit` | Missing required fields | Check the [unit authoring guide](../guide/unit-authoring.md) |
| `Graph: SignatureRejected` | Signature verification failed | Ensure the unit is signed by a known author |
| `Graph: ScopeViolation` | Author lacks scope | Check role assignments in governance units |
| `Graph: MemoryLimitExceeded` | Graph at capacity | Run `taba compose` to trigger compaction, or increase memory limit |

### `taba compose` produces no placements

| Cause | Fix |
|-------|-----|
| No units in graph | Run `taba unit list` to verify |
| All units unplaceable | Check `unplaceable` section in output for reasons |
| Solver version mismatch | All nodes must agree on solver version (FM-12) |

### `taba status` shows Degraded mode

| Reason | Action |
|--------|--------|
| `ErasureThresholdExceeded` | Add nodes or reduce resilience_pct |
| `MemoryLimitExceeded` | Run compaction or increase graph_memory_limit_bytes |
| `WalFailure` | Check disk space, verify WAL directory |
| `OperatorTriggered` | Run `taba node normal` to clear |

### WAL corruption

If the WAL is corrupted (CRC mismatch on replay):

1. The node stops replay at the corrupted position
2. Returns `NodeError::WalCorrupted { position, reason }`
3. Entries before the corruption are intact
4. Entries after may be lost
5. Recovery: delete the corrupted segment, restart the node

### Node not joining cluster

1. Check seed node addresses in `NodeConfig`
2. Verify network connectivity (gossip uses UDP port 7946)
3. Check attestation requirements (`require_tpm` in config)
4. Verify the node's keypair is valid (not revoked)

## Debug logging

```sh
RUST_LOG=debug taba status
RUST_LOG=trace cargo test -p taba-graph
```

## Getting help

- [Architecture overview](../architecture/overview.md)
- [Configuration reference](../admin/configuration.md)
- [Security model](../security/model.md)
- [Open issues](https://github.com/witlox/taba/issues)
