# ADR-002: CRDT composition graph with no consensus protocol

## Status

Accepted

## Context

The composition graph (the system's desired state) must be replicated across all
nodes for resilience and to enable local solver computation. Traditional approaches
use consensus protocols (Raft, Paxos) to ensure strong consistency of replicated
state. Kubernetes uses etcd (Raft) for this purpose.

However, the taba unit and author model provides a property that makes consensus
unnecessary: all mutations to the graph are authored by entities with non-overlapping
scopes. Two authors cannot legitimately produce conflicting state for the same scope,
and if they do, the solver detects it as a conflict requiring explicit policy
resolution. The political consensus happened at authoring time, not at replication time.

## Decision

The composition graph is a CRDT (Conflict-free Replicated Data Type), replicated
peer-to-peer with no consensus protocol, no leaders, and no external metadata store.

Every unit in the graph is signed by its author. Nodes validate signatures before
merging. The graph is erasure-coded across nodes for resilience.

The graph supports: append (new units), supersede (updated policies), hierarchical
containment (data sub-units), and provenance chains.

Desired state is the CRDT graph. Actual state is what's running locally. Each node
reconciles independently.

## Consequences

### Positive
- No single point of failure (no leader, no etcd)
- No serialization bottleneck (no API server)
- Two architectural layers instead of four (K8s: etcd → API server → scheduler → kubelet)
- Scale-invariant: same protocol for 1 node or 1000
- Partition-tolerant by design

### Negative
- Eventually consistent — brief windows where nodes see different graph state
- Custom CRDT implementation required (no off-the-shelf graph CRDT fits exactly)
- Debugging distributed state is harder without a single source of truth
- Graph compaction and garbage collection become the operator's responsibility

### Risks
- If the authoring model has a scope overlap bug, the CRDT cannot resolve it
- CRDT merge semantics must be formally verified (bugs here are system-wide)
- Erasure coding adds complexity to node join/leave operations

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Raft consensus | Well-understood, strong consistency | Leader bottleneck, etcd-like SPOF | Contradicts no-masters design |
| External store (etcd, consul) | Mature, proven | External dependency, operational burden | Violates self-contained design |
| Full replication (no erasure) | Simpler | Higher bandwidth, wasteful | Doesn't scale to large clusters |

## References

- `docs/vision/SYSTEM_VISION.md` § The CRDT composition graph
- `docs/vision/SYSTEM_VISION.md` § Peer-to-peer architecture
- `memory/OPEN_QUESTIONS.md` OQ-001 (CRDT variant selection)
