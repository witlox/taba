# Open Questions

Questions surfaced during design that need resolution during implementation.

## OQ-001: CRDT graph data structure selection
**Status**: Resolved (DL-012)
**Phase**: Architect
**Resolution**: δ-state CRDT (delta-state). The composition graph is a Grow-Only
Set with tombstones (2P-Set variant) where deltas are themselves valid partial
states. Add-set: signed units (grow-only). Remove-set: tombstones (monotonic).
Policy chains: versioned register per ConflictTuple converging to highest-version
non-revoked policy. `GraphDelta` IS a partial state, not an operation log —
merging deltas is idempotent (like CvRDT) while shipping only changes (like CmRDT).
Pending queue is node-local, not replicated. All graph operations are monotonic:
inserts grow the add-set, compaction adds to the remove-set (tombstones),
archival is a monotonic flag. This satisfies INV-C2 by construction.

**Why not pure CmRDT**: causal delivery adds transport complexity; the pending
queue already handles out-of-order. No benefit from requiring it at transport.
**Why not pure CvRDT**: shipping full graph state is prohibitive at scale.
Deltas are already the design.

## OQ-002: Erasure coding algorithm
**Status**: Resolved (DL-013)
**Phase**: Architect
**Resolution**: Reed-Solomon over GF(2^8). Use `reed-solomon-erasure` crate
(SIMD-accelerated, production-proven). GF(2^8) limits to 256 shards maximum,
which is sufficient — at 10k nodes, shards are distributed to a representative
subset (~128 max), not one-per-node. Re-coding on fleet change is already
designed (`ShardManager::recode`). Deterministic: any k of n shards suffice,
guaranteed (no probabilistic decoding failures). Shard sizes (KBs to low MBs
per A3) make encoding/decoding overhead negligible.

**Default parameters**:
- Default resilience_pct: 33 (tolerate ~1/3 node failures)
- 9-node cluster: k=6, m=3
- 100-node cluster: k=67, m=33
- Max practical n ≈ 128 (beyond this, distribute to subset)

**Why not fountain codes**: rateless coding is unnecessary given the fixed
k-of-n adaptation formula (INV-R4). Rust ecosystem less mature. Probabilistic
decoding adds complexity for no benefit at these shard sizes.

## OQ-003: WAL format and compaction
**Status**: Resolved (DL-014)
**Phase**: Architect
**Resolution**: Length-prefixed protobuf frames with CRC32 integrity.

**Frame format**:
```
┌──────────┬──────────┬───────────────────────┬──────────┐
│ len: u32 │ crc: u32 │ WalEntry (protobuf)   │ pad 0-7  │
└──────────┴──────────┴───────────────────────┴──────────┘
```
- 4 bytes: payload length (little-endian u32)
- 4 bytes: CRC32C of the payload (corruption detection, FM-07)
- N bytes: prost-encoded WalEntry
- 0-7 bytes: zero-padding to 8-byte alignment

**Segment naming**: `wal-{sequence_start:016}.log` — lexicographic sort = temporal order.
**Default segment size**: 64 MB. New segment on threshold or explicit rotation.

**Compaction strategy**: WAL entries are discardable when:
1. Merged entries: unit successfully erasure-coded to cluster (durable beyond this node)
2. Pending entries: promoted (refs arrived) or expired (1-hour configurable timeout)
3. Promoted entries: immediately after corresponding Merged entry written

Compaction = create new segment with non-discardable entries, atomic rename, delete old.

**Decision trails** (INV-O1) use the same WAL frame format but a separate file
sequence (`trail-{sequence_start:016}.log`) managed by taba-observe via taba-node.
This keeps graph WAL focused on graph mutations.

**Why protobuf**: already the wire format (prost), zero new serialization code.
**Why not sled/rocksdb**: adds heavyweight dependency for a simple append-only log.

## OQ-004: Deterministic solver — floating point
**Status**: Resolved (DL-004)
**Phase**: Implementer
**Resolution**: Fixed-point arithmetic at ppm scale (10^6 factor). All solver
calculations in u64/i64. No floating-point anywhere in solver paths. Division
rounds toward zero (Rust default). Property tests must verify cross-platform
determinism. See A2, INV-C3.

## OQ-005: K8s manifest coverage
**Status**: Open
**Phase**: Later (Phase 5b)
Which K8s resource types does the migration tool need to handle? At minimum:
Deployment, StatefulSet, DaemonSet, Service, ConfigMap, Secret, PVC,
NetworkPolicy, RBAC. But CRDs are unbounded. Scope decision needed.

## OQ-006: Unit declaration format
**Status**: Resolved (DL-015)
**Phase**: Architect
**Resolution**: Progressive-disclosure TOML schema with 6 complexity levels.
One file = one unit. Full spec in `specs/toml-schema.md`.

**Key decisions**:
- `image = "..."` implies type=workload, artifact_type=oci. Shorthand keys:
  `binary`, `wasm`, `k8s` for other artifact types.
- Names scoped to trust domain (not globally unique).
- Duration strings: "50ms", "30s", "2h", "7y".
- Capability shorthand: `postgres = { type = "storage" }`.
- Defaults: kind=service, scaling min=1/max=1, health=os-level, 
  placement_on_failure from environment (INV-N5).
- One file per unit (no multi-unit files).

**Why one file per unit**: simplicity, clear ownership, git-friendly (one file
= one author = one review). Multi-unit bundles can be a directory convention.

## OQ-007: Graph size limits per node
**Status**: Partially addressed
**Phase**: Implementer (needs benchmarks after M2)
How large can the active graph get before performance degrades? This determines
when compaction is critical and whether sharding the graph (beyond erasure coding)
is needed. INV-R6 enforces configurable memory limit with auto-compaction at 80%.
Need benchmarks with realistic graph sizes after M2 milestone.

## OQ-008: Gossip protocol parameters
**Status**: Resolved (DL-016)
**Phase**: Architect
**Resolution**: SWIM defaults with Lifeguard-style auto-tuning for cluster size.

**Default parameters**:
| Parameter              | Default | Notes                                    |
|------------------------|---------|------------------------------------------|
| gossip_interval        | 500ms   | One random peer probed per interval      |
| suspicion_timeout      | 5s      | Base timeout before witness confirmation |
| witness_count          | 2       | Independent witnesses for failure (INV-R3)|
| indirect_probe_count   | 3       | Peers asked for indirect probes          |
| retransmit_multiplier  | 4       | Piggyback rounds = mult × log2(N)       |
| max_piggyback_entries  | 8       | Bounds gossip message size               |
| suspicion_multiplier   | 4       | Auto-scales timeout with cluster size    |

**Auto-tuning formula**:
```
effective_suspicion_timeout = max(
    suspicion_timeout,
    suspicion_multiplier × ceil(log2(N)) × gossip_interval
)
```

**Key revocation priority**: retransmitted for `2 × retransmit_multiplier × log2(N)`
rounds (double normal) for rapid convergence.

**Bandwidth at 10k nodes**: ~1 KB/s per node, ~10 MB/s cluster-wide. Acceptable.

All parameters are operator-configurable in ClusterConfig, propagated via gossip.
