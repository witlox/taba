# System Vision: taba

## Origin

taba emerged from a first-principles analysis of the infrastructure abstraction
trajectory: VMs abstracted hardware, containers abstracted the OS, Kubernetes
abstracted the fleet. Each step traded isolation for density and speed, then rebuilt
isolation guarantees at a higher level. Each step also increased control plane
complexity monotonically.

The diagnosis: the container is too dumb (an opaque black box), and the control plane
compensates by being too smart (a generic state reconciliation engine drowning in
CRDs). The separation between workload description and orchestration is drawn in the
wrong place.

## Core design

### The unit model

Everything in taba is a **typed unit**. A unit is a self-describing, contract-carrying
entity that declares what it needs, what it provides, what it tolerates, and what it
trusts.

Unit types include:
- **Workload units**: compute processes (containers, microVMs, Wasm modules, native
  processes — the isolation mechanism is itself a declared capability)
- **Data units**: datasets carrying schema, classification, provenance, retention,
  and consent constraints. Data units are hierarchical — a dataset contains cohorts,
  which may contain individual records. Granularity is demand-driven: child units
  exist only where constraints diverge from the parent. Children inherit parent
  constraints by default, can narrow freely, and can widen only with explicit policy.
- **Policy units**: governance declarations that resolve capability conflicts between
  other units. Every security conflict that isn't trivially resolved by compatible
  declarations requires an explicit policy unit.
- **Governance units**: trust domain definitions, role scope assignments, certification
  attestations.

A workload unit declares:
- What it **needs** (capabilities: "I need a postgres-compatible store", not "give me a PVC")
- What it **provides** (typed interfaces: "I expose HTTP on port 443 with OpenAPI spec X")
- What it **tolerates** (latency budget, failure modes, consistency requirements)
- What it **trusts** (identity-based, not network-topology-based)
- **Scaling parameters** (min/max instances, scaling triggers)
- **Failure semantics** ("I may OOM under load, back off inputs" vs "if I crash, something is wrong")
- **Recovery relationships** ("if I fail, drain service B first, then restart me")
- **State recovery semantics** ("stateless" vs "replay from offset X" vs "require quorum")

### Composition

Units compose through the solver. When unit A declares "I need capability X" and
unit B declares "I provide capability X," the solver matches them. If the match is
unambiguous and no security constraints are violated, composition succeeds automatically.

If declarations conflict — two units have incompatible security requirements, or a
capability match is ambiguous — the solver **fails closed** and requires an explicit
policy unit to resolve the conflict. No implicit resolution of security conflicts, ever.

The composition produces a **composition graph** — a directed graph of units and their
relationships. This graph IS the desired state of the system. There is no separate
"desired state store."

### Emergent control plane

The control plane is not a separate system. It is the union of deployed units'
operational semantics. One unit deployed = trivial control plane. Ten units composed =
the control plane is the union of their declared needs and contracts.

Key property: **complexity scales linearly with what you actually deploy.** You don't
pay for capabilities you're not using. This directly addresses the K8s problem where
you pay for the full control plane complexity regardless of workload simplicity.

### The distributed solver

The solver is the only "active" component. It:
- Resolves compositions (matches capabilities to needs)
- Computes placements (deterministic function from graph state + available resources)
- Detects conflicts (capability mismatches, security violations)
- Refuses invalid compositions (fail closed)

The solver is **deterministic**: given the same graph state and the same node membership,
any node computes the same placement. This eliminates the need for consensus on placement
decisions.

Two classes of operations:
- **Commutative** (placement, scaling, health updates): resolve peer-to-peer via
  CRDT merge. Fast, leaderless, eventually consistent.
- **Non-commutative** (policy resolution, trust domain creation): handled by the
  authoring model — the role system ensures these don't have concurrent conflicting
  writers, so CRDT is still sufficient.

### The CRDT composition graph

The composition graph is a CRDT (Conflict-free Replicated Data Type), erasure-coded
across all active nodes. No masters, no external metadata stores (no etcd).

Every unit in the graph is **signed by its author**. Nodes validate signatures before
merging graph updates. Unsigned or wrongly-signed entries are rejected.

The graph supports:
- Append (new units, new compositions, new policies)
- Supersede (updated policy replaces old, versioned)
- Hierarchical containment (data units containing sub-units)
- Provenance chains (output data unit links to input data units and processing workload)

**Desired state**: the CRDT graph
**Actual state**: what's physically running on each node
**Drift**: difference between desired and actual, detected and corrected locally by
each node. No central reconciliation loop.

### Peer-to-peer architecture

Every node is a peer. The control plane runs on the same nodes as workloads.

- **Gossip protocol** (SWIM-like) for membership and failure detection
- **Erasure coding** for graph resilience — redundancy is a function of fleet size
- **WAL** per node for local persistence across restarts
- **Node attestation** on join (TPM when available, optional for dev/small deployments)
- **Shamir shared root key** for the root of trust (5 shares, threshold 3)

Scale-invariant: one node, ten nodes, a hundred nodes — same architecture, same
protocols, different parameters.

### Security model

Zero-access default. Capability-based.

- Units can only access what they explicitly declare AND what policy approves
- No lateral movement by default
- Taint propagation: if input data is classified PII, output inherits PII unless
  explicit policy declassifies
- Every unit is signed by its author
- Authors have scoped authority (unit type scope × trust domain scope)
- Conflicts fail closed — require explicit policy
- Build provenance (SLSA-style attestation) as a declarable capability requirement
- Health checks by peer observation, not self-reported (Byzantine resistance)

### Role model

One role primitive, parameterized by:
1. **Unit type scope**: which types of units can this author create
2. **Trust domain scope**: in which trust domains can this author operate

This yields all necessary roles without RBAC explosion:
- Developer: workload unit author in their trust domain
- Security team: policy unit author across trust domains
- Data steward: data unit author (constraints) in their trust domain
- Regulatory/QA: certification unit author
- Operator: placement approval + all unit types in their trust domain

Trust domains are themselves units, created through the same composition model.
Creating a trust domain is a composition that requires agreement from multiple
parties — which surfaces as a capability conflict requiring explicit policy.
The management ceremony IS the policy authoring.

### Data lineage

Data as first-class unit means lineage is structural:
- When workload A consumes data unit X and produces data unit Y, the solver knows
  this (it resolved the capability match)
- Y's provenance is automatically: "produced by A, from X, at time T, under policies P"
- The composition graph IS the lineage graph
- Audit trail is the graph itself — every unit has an author, every author has a scope
- Retention is a declared property of data units — expiry triggers compaction
- Regulatory audit = graph traversal

### Recovery

- **Deployment** is constraint satisfaction (known hard, solvable)
- **Recovery** uses unit-declared failure semantics — units teach the solver how to heal them
- Recovery plans compose less cleanly than deployment plans (circular dependencies possible)
- Solver needs cycle detection and fallback strategies
- **Network partitions**: CRDT merge handles graph state; duplicate placements resolved
  by deterministic tiebreaker (lowest node ID wins, other side drains)
- Data unit consistency declarations determine partition behavior per-workload
  (single-writer vs multi-writer with conflict resolution)

### Graph lifecycle

The active graph is naturally bounded:
- Completed workloads age out
- Data units carry retention declarations
- Legal requirements (GDPR, pharma regulations) expressible as data unit constraints
- "Must keep" and "must destroy" both supported — conflicts surface as policy requirements
- Archival = moving subgraphs to cold storage while preserving provenance chain
- Active graph tracks active system complexity, not historical accumulation

## Relationship to existing ecosystem

taba is a fourth project alongside pact, lattice, and sovra:
- **pact** handles HPC config management — taba handles general infrastructure composition
- **lattice** handles HPC workload scheduling — taba handles general workload placement
- **sovra** handles federated key management — taba can use sovra for its root of trust
- Integration is opt-in, not required
- When combined: pact configures nodes, taba composes workloads, lattice handles
  HPC-specific scheduling, sovra manages cross-org trust

## Design choices requiring explicit documentation

These choices are load-bearing and must not be changed without full impact analysis:

1. No masters — all nodes are peers
2. CRDT for graph replication — no consensus protocol for normal operations
3. Fail closed on security conflicts — no implicit resolution
4. Deterministic solver — same input = same output on any node
5. Units are signed — graph integrity depends on this
6. Capability-based security — zero default access
7. Erasure coding — not replication — for graph resilience
8. WAL for local persistence
9. Gossip for membership
10. Rust as implementation language
