# Build Order

## Dependency DAG

```
taba-common          (foundation: types, config, protobuf)
    ↓
taba-core            (unit type system, capabilities, contracts)
    ↓
taba-security        (signing, verification, capability enforcement)
    ↓
taba-graph           (CRDT composition graph, operations, merge)
    ↓
taba-solver          (composition resolution, placement, conflicts)
    ↓
taba-erasure         (erasure coding, shard management)
    ↓
taba-gossip          (membership, failure detection)
    ↓
taba-node            (per-node daemon, reconciliation, WAL)
    ↓
taba-cli             (CLI binary, user interface)

taba-test-harness    (parallel: shared test utilities, no prod dependencies)
```

## Implementation phases

### Phase 1: Foundation (single-node, in-memory)

Build the core abstractions and prove they work on a single node.

**1a. taba-common**
- Shared types: NodeId, UnitId, AuthorId, TrustDomainId
- Configuration structures (TOML parsing)
- Protobuf definitions and generated bindings
- Tracing setup

**1b. taba-core**
- Unit type definitions (workload, data, policy, governance)
- Capability declaration model
- Unit validation (well-formedness checks)
- Capability matching algorithm
- Contract types (needs, provides, tolerates, trusts)
- Unit serialization/deserialization

**1c. taba-security** (initial)
- Ed25519 signing and verification
- Unit signing (author signs unit declaration)
- Capability enforcement (check declared vs allowed)
- Taint propagation rules

**1d. taba-test-harness**
- Unit builders (test data factories)
- In-memory fakes for graph and solver
- Property test strategies for core types

### Phase 2: Graph and Solver (single-node, persistent)

Build the composition engine.

**2a. taba-graph**
- CRDT graph data structure
- Graph operations: insert unit, compose, query
- Merge semantics (commutative, associative, idempotent)
- Signature verification on merge
- Provenance chain tracking
- Hierarchical data unit support
- WAL integration for persistence

**2b. taba-solver**
- Composition resolution (match capabilities to needs)
- Conflict detection (security, ambiguity)
- Deterministic placement (given graph + nodes → placement)
- Policy application (resolve conflicts using policy units)
- Scaling computation (when to add/remove instances)

### Phase 3: Distribution

Make it work across multiple nodes.

**3a. taba-gossip**
- SWIM-based membership protocol
- Node join/leave handling
- Failure detection (suspicion, confirmation)
- Membership view dissemination

**3b. taba-erasure**
- Reed-Solomon (or similar) erasure coding
- Graph shard distribution across nodes
- Shard reconstruction on node failure
- Dynamic re-coding when fleet size changes

**3c. taba-node**
- Per-node daemon (the main binary)
- Local reconciliation loop (desired vs actual)
- WAL management
- Graph shard storage
- Gossip participation
- Health reporting

### Phase 4: User interface

Make it usable.

**4a. taba-cli**
- Unit authoring commands
- Composition commands
- Status and inspection
- Policy management
- Trust domain management
- Audit and lineage queries

### Phase 5: Hardening

**5a. taba-security** (advanced)
- TPM attestation (optional, feature-gated)
- Shamir secret sharing for root key
- Node enrollment ceremony
- Build provenance verification (SLSA)

**5b. K8s migration tool**
- Read K8s manifests (Deployment, Service, ConfigMap, Secret, etc.)
- Generate taba unit declarations
- Surface conflicts and unmappable constructs

## Parallelism opportunities

- taba-test-harness can be built alongside any phase
- taba-gossip and taba-erasure are independent of each other
- taba-cli can start as soon as taba-core exists (building up as features land)
- Property tests can be written as soon as types exist

## Milestones

| Milestone | Crates | Capability |
|-----------|--------|------------|
| M1: Types compile | common, core | Unit declarations parse and validate |
| M2: Single-node compose | + graph, solver, security | Compose units on one node |
| M3: Persistent | + node (WAL only) | Survives restart |
| M4: Multi-node | + gossip, erasure | Distributed operation |
| M5: Usable | + cli | Human-operable |
| M6: Hardened | + security advanced | Production-grade security |
| M7: Migration | + k8s tool | K8s users can onboard |
