# ADR-005: Erasure coding (not replication) for graph resilience

## Status

Accepted

## Context

The CRDT composition graph must survive node failures. Traditional approaches use
full replication (3 copies of everything, as in etcd). This is wasteful and doesn't
scale — with 100 nodes, you still only have 3 copies. Erasure coding provides
configurable resilience with lower storage overhead.

## Decision

The composition graph is erasure-coded across active nodes. The coding parameters
(n total shards, k required for reconstruction) are a function of fleet size and
desired resilience level. Users configure the resilience *property* they want
("tolerate loss of 20% of nodes") and the system computes the coding parameters.

Graph state is segmented per trust domain or composition scope, so losing nodes
only requires reconstruction of the segments they held, and different segments
can have different redundancy levels.

## Consequences

### Positive
- Storage efficient: k-of-n coding is much less overhead than 3x replication
- Scalable: redundancy adapts to fleet size automatically
- Per-segment coding allows differentiated resilience (GxP data gets higher redundancy)
- Reconstruction is distributed — no single node bears the full load

### Negative
- Erasure coding has CPU overhead (encoding/decoding)
- Reconstruction after node failure takes time proportional to data volume
- More complex than simple replication — harder to reason about
- Node join/leave requires re-coding of affected segments

### Risks
- If too many nodes fail simultaneously (more than n-k), data is lost
- Reconstruction during a cascading failure could make things worse
- The coding parameters need to adapt dynamically — getting this wrong is dangerous

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Full 3x replication | Simple, proven | Wasteful, fixed redundancy | Doesn't scale, over-provisions small clusters |
| Chain replication | Efficient reads | Complex failure handling | Requires ordering, contradicts peer model |
| No redundancy (WAL only) | Simplest | Single node failure = data loss | Unacceptable for production |

## References

- `docs/vision/SYSTEM_VISION.md` § Peer-to-peer architecture
- `memory/OPEN_QUESTIONS.md` OQ-002 (algorithm selection)
