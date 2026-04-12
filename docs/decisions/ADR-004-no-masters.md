# ADR-004: No master nodes — fully peer-to-peer architecture

## Status

Accepted

## Context

Kubernetes requires a separate control plane (etcd cluster + API server + scheduler
+ controller manager) that must be provisioned, maintained, and scaled independently
from worker nodes. This creates operational overhead, a class of "control plane down"
failures, and an architectural phase transition between "dev mode" (minikube) and
"production" (HA control plane).

## Decision

taba has no master nodes. Every node is a peer running the same software with the
same role. The control plane runs on the same nodes as workloads. There is no
external metadata store, no leader election for normal operations, and no
architectural distinction between "control plane node" and "worker node."

Node join uses attestation (TPM when available, relaxed for dev). Node failure is
detected via gossip (SWIM protocol). Graph resilience uses erasure coding across
all nodes.

The system is scale-invariant: one node to thousands, same architecture, same
protocols, different parameters (erasure coding ratio, gossip fanout).

## Consequences

### Positive
- No control plane failures (there is no separate control plane)
- No operational overhead of maintaining control plane infrastructure
- No architectural phase transition between dev and production
- Genuine linear scaling — add nodes, system gets more capacity and resilience
- Single binary to deploy (node daemon does everything)

### Negative
- Some operations are inherently global (DNS, certificate roots) and need bootstrap
- Debugging is harder without a central point to query
- Monitoring requires aggregating from all nodes
- Initial cluster bootstrap requires at least one node to seed the graph

### Risks
- Gossip protocol may not scale to very large clusters (>10,000 nodes) without
  hierarchical gossip or federation
- Erasure coding parameters need to adapt as fleet size changes
- Without a leader, "who decides" questions become "the math decides" — which
  requires the deterministic solver to be correct

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Small fixed control plane (3-5 nodes) | Familiar, proven | Separate infra, SPOF class | Contradicts design goal |
| Elected leader for coordination | Simpler consistency | Leader is bottleneck and SPOF | Contradicts no-masters design |
| Hybrid (embedded leader per region) | Scalable, still distributed | Complex, two modes | Unnecessary if CRDT works |

## References

- `docs/vision/SYSTEM_VISION.md` § Peer-to-peer architecture
- `docs/decisions/ADR-002-crdt-graph.md` (CRDT eliminates need for consensus)
- `memory/OPEN_QUESTIONS.md` OQ-008 (gossip parameters)
