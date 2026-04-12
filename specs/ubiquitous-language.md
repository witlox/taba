# Ubiquitous Language

Every term defined once. Anti-definitions clarify what terms do NOT mean.

| Term | Definition | NOT this |
|------|-----------|----------|
| **Unit** | Self-describing, signed, typed entity carrying contracts | Not a container. Not a pod. Not a process. It's the envelope + contracts, not the runtime. |
| **Workload Unit** | A unit representing a compute process | Not limited to containers — can be microVM, Wasm, native process |
| **Data Unit** | A unit representing a dataset with constraints | Not a volume or PVC. Carries classification, lineage, retention, consent. |
| **Policy Unit** | A unit resolving a capability conflict | Not RBAC rules. Not network policy. Resolves a specific conflict between specific units. |
| **Governance Unit** | A unit defining trust domains, roles, or certifications | Not configuration. Structural authority declarations. |
| **Capability** | A typed resource or service a unit needs or provides | Not a Linux capability. "postgres-compatible store" not "CAP_NET_ADMIN". |
| **Composition** | The result of the solver matching units' capabilities | Not deployment. Not scheduling. The logical resolution of who-needs-what. |
| **Composition Graph** | CRDT containing all units and their relationships | Not etcd. Not a database. The distributed, peer-replicated desired state. |
| **Solver** | Deterministic function resolving compositions and placements | Not a scheduler. Not a controller. A pure function: graph + nodes → placements. |
| **Conflict** | Incompatible capability declarations between units | Not an error. A normal condition requiring explicit policy resolution. |
| **Fail Closed** | Default to denial when a security decision is ambiguous | Not "crash." The solver refuses the composition until policy resolves it. |
| **Trust Domain** | Authorization boundary scoping author permissions | Not a namespace. Not a tenant. A governance unit with multi-party creation. |
| **Author** | Authenticated identity with scoped unit-creation authority | Not a user account. Parameterized by (unit type scope × trust domain scope). |
| **Taint Propagation** | Automatic inheritance of data constraints through composition | If input is PII, output is PII unless explicit policy declassifies. |
| **Desired State** | The composition graph | Not a separate config store. The graph IS the desired state. |
| **Actual State** | What is physically running on each node | Local to each node. Compared against desired state for drift detection. |
| **Drift** | Divergence between desired and actual state | Detected locally. Corrected locally. No central reconciliation loop. |
| **Reconciliation** | Local process on each node converging actual to desired | Not a global controller. Each node reconciles itself independently. |
| **Placement** | Assignment of a unit to a node by the solver | Deterministic: same graph + same nodes = same placement on any node. |
| **Erasure Coding** | Redundancy scheme for graph shards across nodes | Not replication. k-of-n coding where parameters adapt to fleet size. |
| **Gossip** | SWIM-based protocol for membership and failure detection | Not pub/sub. Not messaging. Membership protocol only. |
| **WAL** | Write-ahead log for local persistence of graph state | Per-node. Survives restart. Not a distributed log. |
| **Node Attestation** | Cryptographic proof of node integrity on join | TPM when available. Optional for dev. Required for production. |
| **Root Key** | Shamir-shared key bootstrapping the entire trust model | Not a node key. Not an author key. The root of all authority. |
| **Provenance** | Chain of units that produced a data unit | Structural — falls out of the composition graph, not a separate system. |
| **Lineage** | The full derivation history of a data unit | The composition graph IS the lineage graph. |
| **Supersession** | A policy unit explicitly replacing an earlier policy for the same conflict | Not deletion. Creates a versioned lineage chain. Solver uses latest non-revoked version. |
| **Purpose** | Optional qualifier on a capability declaring intended use | Not classification. A capability constraint. "analytics" is a purpose, "PII" is a classification. |
| **Ceremony** | Multi-party protocol for high-trust operations (root key, trust domain creation) | Not authentication. A structured ritual with witnesses, audit trail, and quorum. |
| **Pending** | A unit verified but with unsatisfied graph references, awaiting causal delivery | Not invalid. Buffered in WAL, promoted to active when references arrive. |
| **Degraded Mode** | Operational state where authoring/composition/placement are frozen | Not failure. System is alive but restricted. Drain and evacuation only. Operator must intervene. |
| **Reconstruction Backpressure** | Throttling of erasure shard reconstruction to prevent cascading failures | Not a failure mode. A protective mechanism with priority queue and circuit breaker. |
| **Witness** | Independent node corroborating a gossip membership state change | Not a quorum. Two witnesses required before declaring a node failed. |
| **Fixed-Point (ppm)** | Solver arithmetic at 10^6 scale factor in u64/i64 | Not floating-point. Deterministic across all platforms. Division rounds toward zero. |
