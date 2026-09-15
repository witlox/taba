# System Overview

taba is a next-generation infrastructure primitive that replaces
the container + orchestrator model (Docker + Kubernetes) with
self-describing, capability-aware workload units composed through
a distributed solver.

## The problem

Every step in the infrastructure abstraction trajectory — VMs,
containers, Kubernetes — increased control plane complexity
monotonically. The diagnosis:

- **The container is too dumb**: an opaque black box that carries
  no information about what it needs, what it provides, or what
  it tolerates.
- **The control plane is too smart**: an unbounded state
  reconciliation engine that must infer everything the container
  didn't say.

The separation between workload description and orchestration is
drawn in the wrong place.

## The taba approach

taba draws it differently. Units describe themselves — what they
need, what they provide, what they tolerate, who they trust. A
deterministic solver composes them. The result is the desired
state. There is no separate desired state store.

## Five pillars

1. **Self-describing typed units** — workload, data, policy,
   governance. Each carries capability declarations, behavioral
   contracts, and security requirements.
2. **Emergent control plane** — the control plane is the union of
   deployed units' operational semantics. One unit = trivial
   control plane. A thousand = the union of their contracts.
3. **Security as first class** — zero-access default, capability-based,
   fail-closed on conflicts. Every unit is signed by its author.
   Taint propagation is structural.
4. **Data as first-class unit** — datasets carry schema,
   classification, provenance, retention, and consent. Lineage
   falls out of the composition graph.
5. **Peer-to-peer** — no masters, no leaders, no external metadata
   store. CRDT graph, erasure-coded, gossip membership.

## Architecture diagram

```
taba-common          types, config, protobuf
    |
taba-core            unit type system, capabilities, contracts
    |
taba-security        signing, verification, taint, Shamir ceremony
   / \
taba-graph  taba-solver    CRDT graph + deterministic solver (parallel)
   / \         |
taba-erasure  taba-gossip  erasure coding + membership (parallel)
       \       /
       taba-node            per-node daemon, WAL, reconciliation
           |
        taba-cli            command-line interface
           |
        taba-k8s            K8s manifest converter
```

14 crates. Acyclic dependency graph. Single-node works before
multi-node (progressive complexity).

## Crates

| Crate | Responsibility | Tests |
|-------|---------------|-------|
| taba-common | Identity newtypes, Ppm arithmetic, clocks, config | 31 |
| taba-core | Unit model, capabilities, validation, contracts | 86 |
| taba-security | Ed25519, scope, taint, delegation, ceremony, SLSA | 106 |
| taba-graph | δ-state CRDT composition graph, merge, policy chains | 150 |
| taba-solver | Deterministic placement, conflict detection, cycles | 119 |
| taba-observe | Decision trails, structured events, health, Prometheus | 45 |
| taba-node | WAL, reconciler, runtime, health, mode, discovery | 80 |
| taba-gossip | SWIM membership, signed messages, failure detection | 38 |
| taba-erasure | Reed-Solomon GF(2^8), shard distribution, reconstruction | 78 |
| taba-cli | clap CLI (init, apply, unit, status, compose, audit) | 55 |
| taba-k8s | K8s manifest converter | 31 |
| taba-acceptance | BDD acceptance tests (cucumber-rs) | — |
| taba-test-harness | Builders, InMemoryUnitStore, proptest strategies | 33 |
| taba-integration | End-to-end integration tests | 15 |

**Total: 867 tests.**

## Key invariants

| ID | Description |
|----|-------------|
| INV-C1 | Graph is the single source of desired state |
| INV-C2 | CRDT merge: commutative, associative, idempotent |
| INV-C3 | Solver determinism: fixed-point, no float |
| INV-C4 | WAL-before-effect |
| INV-C6 | Composition is order-independent |
| INV-C7 | One non-revoked policy per conflict tuple |
| INV-S1 | Zero-default capabilities |
| INV-S2 | Security conflicts fail closed |
| INV-S3 | Signed units, causal revocation |
| INV-S4 | Taint propagation at query time |
| INV-S5 | Author scope enforcement |
| INV-S7 | Data hierarchy: narrow freely, widen with policy |
| INV-S8 | Unique author scopes (state-producing) |
| INV-S9 | Multi-party declassification |
| INV-S10 | Multi-party trust domain creation |
| INV-W3 | Spawn depth ≤ 4 |
| INV-R3 | 2-witness failure confirmation |
| INV-R6 | Graph memory limit, auto-compaction at 80% |
| INV-G3 | Governance units exempt from compaction |
| INV-O1 | Every solver run produces a decision trail |

See `specs/invariants.md` for the complete list (66 invariants).
