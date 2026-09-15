# WAL & Persistence

The Write-Ahead Log (WAL) is the foundation of taba's durability
guarantee (INV-C4): **every mutation is persisted to WAL before
its effects become visible to queries.**

## Frame format (DL-014)

```
┌──────────┬──────────┬───────────────────────┬──────────┐
│ len: u32 │ crc: u32 │ WalEntry (protobuf)   │ pad 0-7  │
└──────────┴──────────┴───────────────────────┴──────────┘
```

- `len`: payload length in bytes (little-endian u32)
- `crc`: CRC32C of the payload (corruption detection, FM-07)
- Payload: prost-encoded `WalEntry`
- Padding: zero bytes to 8-byte alignment

## Segment naming

```
wal-{sequence_start:016}.log
```

Lexicographic sort = temporal order. Default segment size: 64 MB.
New segment on threshold or explicit rotation.

## Entry types (DL-008)

| Type | When | Discardable when |
|------|------|------------------|
| `Merged` | Unit verified and merged into graph | Erasure-coded to cluster (durable beyond this node) |
| `Pending` | Unit verified but references not satisfied | Promoted (refs arrived) or expired (1-hour timeout) |
| `Promoted` | Previously pending unit activated | Immediately after corresponding Merged entry |

## Compaction

Compaction = create new segment with non-discardable entries,
fsync, then delete old segments.

**Crash safety**: new segment is written and fsynced **before** old
segments are deleted. If the process crashes during compaction,
old segments are still intact and can be replayed on recovery.

Compaction eligibility (deterministic, all nodes agree — INV-G1):
1. Ephemeral data (highest priority)
2. Decision trails
3. Terminated bounded tasks
4. Superseded policies
5. Terminated services
6. Expired persistent data

Governance units are **exempt** from compaction (INV-G3). They are
actively replicated (full copies on N nodes), not just erasure-coded.

## Auto-compaction (INV-R6)

| Trigger | Threshold | Action |
|---------|-----------|--------|
| Memory pressure | 80% of limit | Auto-compaction begins |
| Memory limit | 100% of limit | Node enters Degraded mode |
| Operator | Manual | Compaction on demand |

## WAL interface

```rust
pub trait WalManager {
    async fn append(&self, entry: WalEntry) -> Result<WalPosition, NodeError>;
    async fn replay(&self, from: WalPosition, callback: &mut dyn FnMut(WalEntry) -> Result<(), NodeError>) -> Result<WalPosition, NodeError>;
    async fn compact(&self, before: WalPosition) -> Result<u64, NodeError>;
    fn size_bytes(&self) -> u64;
    fn latest_position(&self) -> WalPosition;
}
```

The `DiskWalManager` is the production implementation. The
`InMemoryWal` (in taba-test-harness) is used for tests.

## Decision trails (INV-O1)

Decision trails use the same frame format but a separate file
sequence (`trail-{sequence_start:016}.log`), managed by taba-observe
via taba-node. This keeps the graph WAL focused on graph mutations.
