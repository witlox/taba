# Open Questions

Questions surfaced during design that need resolution during implementation.

## OQ-001: CRDT graph data structure selection
**Status**: Open
**Phase**: Architect
Which CRDT variant for the composition graph? Options: state-based (CvRDT) or
operation-based (CmRDT). State-based is simpler but higher bandwidth. Op-based
is more efficient but requires causal delivery. The graph structure (DAG of signed
units) may suggest a custom CRDT with domain-specific merge semantics.

## OQ-002: Erasure coding algorithm
**Status**: Open
**Phase**: Architect
Reed-Solomon is the obvious choice but has computational overhead. Alternatives:
fountain codes (rateless, good for varying redundancy), LT codes. Need to evaluate
reconstruction latency vs coding overhead for typical graph shard sizes.

## OQ-003: WAL format and compaction
**Status**: Partially resolved (DL-008)
**Phase**: Architect
The WAL needs to support: graph mutations, placement decisions, membership changes.
Format options: custom binary, protobuf-encoded entries, or an existing WAL crate.
Compaction strategy: when can old WAL entries be safely discarded?
**Partial resolution**: WAL entry types defined (Merged, Pending, Promoted) per
INV-C4. Causal buffering with pending queue for out-of-order arrivals. Format
and compaction strategy still open for architect phase.

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
**Status**: Open
**Phase**: Analyst/Architect
TOML for human authoring is decided. But what's the schema? Needs to be as simple
as a Dockerfile for simple cases, with richness available but not mandatory. The
authoring experience is a key adoption driver — get this wrong and the project fails
regardless of technical merit.

## OQ-007: Graph size limits per node
**Status**: Open
**Phase**: Architect
How large can the active graph get before performance degrades? This determines
when compaction is critical and whether sharding the graph (beyond erasure coding)
is needed. Need benchmarks with realistic graph sizes.

## OQ-008: Gossip protocol parameters
**Status**: Open
**Phase**: Architect
SWIM protocol has tunable parameters: protocol period, suspicion timeout, number
of indirect probes. These affect failure detection latency and false positive rate.
Need to determine sensible defaults and whether these should be auto-tuned based
on fleet size.
