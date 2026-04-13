# Cross-Context Interactions

## Context boundaries

taba has the following bounded contexts:

### 1. Unit Management (taba-core)
Owns: unit types, capability declarations, validation, type system.
Produces: validated units ready for graph insertion.
Consumes: nothing — this is the foundation.

### 2. Composition Graph (taba-graph)
Owns: CRDT graph state, merge semantics, signature verification, provenance.
Produces: graph state updates, query results, provenance chains.
Consumes: validated units from Unit Management.

### 3. Solver (taba-solver)
Owns: composition resolution, conflict detection, placement computation.
Produces: placement decisions, conflict reports, scaling decisions.
Consumes: graph state from Composition Graph, node membership from Gossip.

### 4. Security (taba-security)
Owns: signing, verification, capability enforcement, taint computation,
Shamir ceremony management, key revocation.
Produces: signed units, verification results, capability check results,
taint classifications (computed at query time via provenance traversal).
Consumes: units from Unit Management, graph state for taint tracking.
Cross-cuts: every other context calls into Security for verification.
Ceremony: manages root key lifecycle (Tier 1/2/3 evolving across phases).

### 5. Node Operations (taba-node)
Owns: local reconciliation, WAL, actual state, health reporting.
Produces: actual state reports, drift detection, health status.
Consumes: placement decisions from Solver, graph shards.

### 6. Distribution (taba-gossip + taba-erasure)
Owns: membership, failure detection, shard distribution, reconstruction.
Produces: membership view, shard assignments.
Consumes: node join/leave events.

## Interaction contracts

### Unit Management → Composition Graph
- **Insert unit**: validated unit submitted for graph insertion.
  Graph verifies signature synchronously (calls Security) before accepting.
  Signature binds context (trust_domain_id, cluster_id, validity_window).
  Units with unsatisfied references enter pending queue (causal buffering).
  Failure: rejection with typed error (InvalidSignature | ScopeViolation |
  KeyRevoked | ContextMismatch).

### Composition Graph → Solver
- **Graph updated**: solver re-evaluates affected compositions.
  Solver reads graph state (immutable snapshot, not live reference).
  Failure: solver reports conflicts or unsatisfied capabilities.

### Solver → Node Operations
- **Placement decision**: solver assigns unit to node.
  Node reconciliation loop picks up new placements from graph.
  Failure: node cannot start workload (resource unavailable, runtime error).
  Node reports failure back to graph as health status.

### Distribution → Composition Graph
- **Membership change**: node joins or leaves.
  Graph shards redistributed (erasure re-coding).
  Solver re-evaluates placements for affected units.
  Failure: insufficient nodes for erasure threshold.

### Security ↔ Everything
- **Signing**: Unit Management calls Security to sign new units.
  Signatures bind context: Sign(key, hash(unit || trust_domain_id || cluster_id || validity_window)).
- **Verification**: Graph calls Security to verify on merge (synchronous gate).
  Checks: signature valid, author scope valid at creation time, key not revoked
  before creation timestamp.
- **Capability check**: Node Operations calls Security at runtime.
- **Taint computation**: Solver calls Security to compute inherited constraints
  at query time by traversing provenance graph. Multi-input: union of all input
  classifications. Declassification requires multi-party signing (INV-S9).
- **Key revocation**: Security publishes revocation events via priority gossip.
  Graph rejects units signed after revocation timestamp.
- **Gossip authentication**: Distribution calls Security to sign/verify gossip messages.
  Failure: verification failure → reject. Capability denied → logged.
  Gossip signature invalid → message dropped, node flagged for investigation.

### 7. Artifact Distribution (taba-artifact)
Owns: artifact fetching, peer cache, content addressing, P2P distribution.
Produces: cached artifacts available for node execution, fetch status events.
Consumes: artifact references from workload units, peer discovery from gossip.

### 8. Observability (taba-observe)
Owns: decision trail recording, solver replay, health check orchestration,
structured event emission, integration export (OpenTelemetry, Prometheus).
Produces: decision trail events, health status, structured logs, metric
endpoints.
Consumes: solver run outputs, node resource reports, workload health checks.
Cross-cuts: every context emits structured events that Observability collects.

## Interaction contracts (new)

### Solver → Artifact Distribution
- **Placement decided**: solver assigns unit to node. Node's reconciliation
  loop triggers artifact fetch. Fetch order: peer cache → external source.
  Artifact digest verified after fetch (INV-A1).
  Failure: fetch failure → retry with backoff → report to graph → solver
  re-places if persistent.

### Node Operations → Artifact Distribution
- **Artifact needed**: node requests artifact by digest. Artifact Distribution
  checks peer cache (local, then peers via gossip-discovered inventory).
  Falls back to external source (registry URL, HTTP endpoint) if no peer
  has it.
  Failure: all sources exhausted → report unavailable to graph.

### Artifact Distribution ↔ Distribution (Gossip)
- **Peer cache inventory**: nodes advertise cached artifact digests via gossip
  (lightweight bloom filter or digest list, not full artifacts). Other nodes
  query peers for specific digests before hitting external sources.

### Observability → Solver
- **Decision trail**: after every solver run, Observability records inputs
  (graph snapshot ID, node membership snapshot) and outputs (placements,
  conflicts, solver version) as a decision trail entry in the graph (INV-O1).

### Observability → Node Operations
- **Health check orchestration**: Observability drives health check execution
  based on workload unit declarations. Default: OS-level process monitoring.
  Progressive: HTTP probe, TCP check, custom command. Results reported to
  graph as health status.

### Node Operations → Observability
- **Resource snapshots**: nodes periodically report resource state (memory,
  CPU, disk, GPU availability) to Observability. Advertised via gossip for
  solver resource ranking (INV-N3).

## Failure at context boundaries

| Boundary | Failure mode | Response |
|----------|-------------|----------|
| Unit → Graph | Signature invalid | Reject unit, typed error |
| Unit → Graph | Author scope insufficient | Reject unit, typed error |
| Graph → Solver | Graph snapshot stale | Solver operates on available snapshot, reconciles on next update |
| Solver → Node | Node cannot start workload | Health status updated in graph, solver re-places |
| Solver → Node | Artifact fetch failed | Retry with backoff, re-place if persistent (FM-15) |
| Solver → Node | Capability mismatch at runtime | Mark capability suspect, re-place, alert operator (FM-17) |
| Distribution → Graph | Not enough nodes for erasure | Degraded mode, reduced redundancy, operator alert |
| Security → Graph | Key revoked mid-merge | Reject units signed after revocation timestamp, existing pre-revocation units remain valid (INV-S3) |
| Security → Graph | Forged unit detected | Reject, quarantine sending node for investigation |
| Distribution → Graph | Gossip message unsigned/invalid | Drop message, flag sender for investigation |
| Solver → Solver | Cyclic recovery dependencies | Report unresolvable conflict, require policy (INV-K5) |
| Solver → Solver | Conflicting policies for same conflict | Use supersession chain, latest non-revoked wins (INV-C7) |
| Solver → Solver | Conflicting promotion policies | Same decision: dedup by lowest PolicyId. Different: fail closed (FM-14) |
| Artifact → Node | Digest mismatch after fetch | Reject artifact, report to graph, retry from different source |
| Governance → Fleet | Operational command propagation | Signed, gossiped. Nodes that miss it catch up on next gossip round. |
