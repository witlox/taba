# Dependency Graph

Crate-level DAG for the taba workspace. Every edge is justified. The graph is
verified acyclic by construction: each crate depends only on crates appearing
above it in the topological order.

---

## Crate DAG

```
                    taba-common
                   /     |     \
                  /      |      \
                 v       v       v
           taba-core   (proto)  (config)
                |
                v
          taba-security
           /         \
          v           v
     taba-graph    taba-solver ----+
       /    \         |            |
      v      v        v            v
 taba-erasure  taba-gossip    taba-node
      \         /        \       / |
       \       /          \     /  |
        v     v            v   v   |
        taba-node  <-------+---+   |
             |                     |
             v                     |
          taba-cli                 |
                                   |
                                   |
  taba-test-harness  (dev-only, depends on all crates)
```

### Simplified linear view (topological order)

```
Level 0:  taba-common
Level 1:  taba-core
Level 2:  taba-security
Level 3:  taba-graph, taba-solver, taba-observe  (parallel)
Level 4:  taba-erasure, taba-gossip              (parallel)
Level 5:  taba-node
Level 6:  taba-cli

Dev-only: taba-test-harness (all levels)
```

Note: taba-observe is at Level 3 because it depends on taba-common, taba-core,
and taba-security (for trail types and solver replay). It does NOT depend on
taba-graph directly — decision trails are persisted via taba-node which
provides the graph integration.

### Precise dependency edges

```
taba-common     -> (none)
taba-core       -> taba-common
taba-security   -> taba-common, taba-core
taba-graph      -> taba-common, taba-core, taba-security
taba-solver     -> taba-common, taba-core, taba-security
taba-observe    -> taba-common, taba-core, taba-security
taba-erasure    -> taba-common, taba-security
taba-gossip     -> taba-common, taba-security
taba-node       -> taba-common, taba-core, taba-security, taba-graph,
                   taba-solver, taba-observe, taba-erasure, taba-gossip
taba-cli        -> taba-common, taba-core, taba-observe, taba-node
taba-test-harness -> taba-common, taba-core, taba-security, taba-graph,
                     taba-solver, taba-observe (dev-dependency only)
```

---

## Acyclicity verification

The topological order (Level 0-6) proves acyclicity: every dependency edge
points from a higher level to a lower level. No edge points upward or
laterally within the same level where a mutual dependency would exist.

Specifically:
- Level 3 crates (taba-graph, taba-solver) both depend on Level 2 and below,
  but NOT on each other. The solver reads graph snapshots via a trait defined
  in taba-graph, but this is a type dependency (GraphSnapshot), not a crate
  cycle. Both crates depend on taba-security independently.
- Level 4 crates (taba-erasure, taba-gossip) depend on Level 0 and Level 2,
  but NOT on each other or on Level 3.
- taba-node (Level 5) is the integration point that depends on Levels 0-4.
- taba-test-harness has no reverse dependencies (dev-only).

---

## Build order (parallel opportunities)

```
Step 1:  taba-common                          (solo)
Step 2:  taba-core                            (solo, needs common)
Step 3:  taba-security, taba-test-harness*    (parallel; security needs core;
                                               harness needs core)
Step 4:  taba-graph, taba-solver,             (parallel; all need security)
         taba-observe, taba-erasure, taba-gossip
Step 5:  taba-node                            (needs everything from Step 4)
Step 6:  taba-cli                             (needs node, observe)
```

*taba-test-harness can build incrementally starting at Step 3, adding
features as each crate it depends on becomes available.

Maximum parallelism at Step 4: four crates can compile simultaneously.

---

## Dependency justification

### taba-core -> taba-common
Core uses identity types (UnitId, AuthorId, NodeId, TrustDomainId), Ppm
arithmetic, Timestamp, and protobuf-generated types defined in common.

### taba-security -> taba-common
Security uses identity types for key management and signing context
(trust_domain_id, cluster_id in SigningContext).

### taba-security -> taba-core
Security must understand unit structure to sign units and enforce capabilities.
Needs Unit, Capability, DataClassification for taint computation,
GovernanceUnit for ceremony handling.

### taba-graph -> taba-common
Graph uses identity types, timestamps, config, and Wal-related types.

### taba-graph -> taba-core
Graph stores units -- needs Unit, PolicyUnit (for supersession), DataUnit
(for hierarchy), ProvenanceLink. Validates well-formedness on insert.

### taba-graph -> taba-security
Graph calls Verifier trait synchronously before merge (INV-S3). Checks
signature validity, author scope, and key revocation status. This is the
signature verification gate.

### taba-solver -> taba-common
Solver uses NodeId for placement, Ppm for fixed-point arithmetic, config
for solver parameters.

### taba-solver -> taba-core
Solver uses Capability, CapabilityMatcher, Unit types, ConflictTuple,
PolicyUnit, ScalingParams, RecoveryRelationship, Tolerates for composition
resolution and placement.

### taba-solver -> taba-security
Solver calls TaintComputer to compute inherited classifications at query
time (INV-S4). Calls ScopeChecker when evaluating policy validity.

### taba-observe -> taba-common
Observe uses identity types (UnitId, NodeId), LogicalClock, DualClockEvent,
Ppm for metric values, config for observe parameters.

### taba-observe -> taba-core
Observe needs unit types (for decision trail recording), Artifact, Capability,
PromotionPolicy, HealthCheck, Tombstone types for structured events.

### taba-observe -> taba-security
Observe calls solver replay which needs SecurityError types. Also needs
AuthorId for audit trails. Does NOT perform signing/verification — delegates
to taba-security when needed.

### taba-erasure -> taba-common
Erasure uses NodeId for shard distribution, config for erasure parameters
(resilience percentage, fleet size).

### taba-erasure -> taba-security
Post-reconstruction signature re-verification (INV-R1). Calls Verifier
trait after decoding shards to ensure reconstructed units have valid
signatures.

### taba-gossip -> taba-common
Gossip uses NodeId, config for gossip parameters (probe interval, suspicion
timeout).

### taba-gossip -> taba-security
All gossip messages are signed (INV-R3, DL-009). Gossip calls Signer to
sign outgoing messages and Verifier to verify incoming messages. Priority
events (key revocation) originate from taba-security.

### taba-node -> taba-common
Node uses all identity types, config for node parameters, tracing.

### taba-node -> taba-core
Node creates and validates units locally, understands unit lifecycle states.

### taba-node -> taba-security
Node uses key management for node identity (Ed25519 keypair), participates
in ceremonies, enforces capabilities at runtime.

### taba-node -> taba-graph
Node owns the local graph instance, manages WAL, triggers compaction,
provides graph snapshots to solver.

### taba-node -> taba-solver
Node invokes solver with graph snapshot + membership to compute local
placements and reconcile.

### taba-node -> taba-erasure
Node stores and retrieves erasure-coded shards, triggers reconstruction
when notified of failures.

### taba-node -> taba-gossip
Node runs gossip protocol for membership, receives membership changes,
disseminates priority events.

### taba-cli -> taba-common
CLI uses identity types for display and input parsing, config for
connection parameters.

### taba-cli -> taba-core
CLI uses unit types for authoring commands, validation for input checking,
capability types for display.

### taba-cli -> taba-node
CLI connects to the node daemon via gRPC (tonic client) for all operations.
This is the primary integration point -- CLI is a thin client.

### taba-test-harness -> (multiple)
Dev-dependency only. Needs type definitions from common, core, security,
graph, and solver to build fakes, builders, and proptest strategies.

---

## Phase mapping

Which crates are needed for which project milestone (from guidelines/BUILD_ORDER.md):

| Milestone | Crates required | Capability |
|-----------|----------------|------------|
| **M1: Types compile** | taba-common, taba-core, taba-test-harness | Unit declarations parse and validate |
| **M2: Single-node compose** | + taba-security, taba-graph, taba-solver | Compose units on one node, signed, with conflict detection |
| **M3: Persistent** | + taba-node (WAL only) | Survives restart, local reconciliation |
| **M4: Multi-node** | + taba-gossip, taba-erasure | Distributed operation, failure tolerance |
| **M5: Usable** | + taba-cli | Human-operable system |
| **M6: Hardened** | taba-security (advanced features) | Shamir Tier 2/3, TPM, SLSA |
| **M7: Migration** | + k8s migration tool (new crate) | K8s users can onboard |

### Phase-crate matrix

```
              M1   M2   M3   M4   M5   M6   M7
common         x    x    x    x    x    x    x
core           x    x    x    x    x    x    x
security            x    x    x    x    X    x    (X = advanced features)
graph               x    x    x    x    x    x
solver              x    x    x    x    x    x
observe              x    x    x    x    x    x    (decision trails from M2)
erasure                       x    x    x    x
gossip                        x    x    x    x
node                     x    x    x    x    x
cli                                x    x    x
test-harness   x    x    x    x    x    x    x    (dev-only, grows with each phase)
```

---

## Design notes

### Why taba-solver does not depend on taba-graph

The solver reads `GraphSnapshot`, which is a type defined in taba-graph. This
creates a dependency edge from taba-solver to taba-graph. However, this can
be avoided by defining the snapshot trait/type in taba-core (as part of the
domain model) and having both taba-graph (implements it) and taba-solver
(consumes it) depend on taba-core. This keeps the DAG cleaner.

**Decision**: taba-solver depends on taba-core for the `GraphSnapshot` type.
taba-graph implements the snapshot. The solver never imports taba-graph
directly. This allows graph and solver to build in parallel at Level 3.

### Why taba-erasure does not depend on taba-graph

Erasure coding operates on opaque byte slices. It does not need to understand
graph structure. taba-graph serializes subgraphs to bytes before handing them
to taba-erasure for encoding. taba-node orchestrates this interaction.

### Why taba-gossip does not depend on taba-graph

Gossip is a transport and membership protocol. It carries graph deltas as
opaque payloads. The graph deserialization and merge happen in taba-node
after gossip delivers the message.

### Cross-crate trait pattern

Several traits are defined in one crate and implemented in another:
- `CompositionGraph` trait: defined in taba-graph, implemented there
- `Solver` trait: defined in taba-solver, implemented there
- `Signer`/`Verifier` traits: defined in taba-security, implemented there
- `Wal` trait: defined in taba-graph, implemented in taba-node (disk) and
  taba-test-harness (in-memory)
- `ShardStore` trait: defined in taba-node, implemented there and in
  taba-test-harness (in-memory)
- `MembershipService` trait: defined in taba-gossip, implemented there and
  in taba-test-harness (fake)

This pattern enables testing without full dependency chains: taba-test-harness
provides in-memory implementations of all traits, allowing any crate to be
tested in isolation.
