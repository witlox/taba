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
| **Progressive Disclosure** | Design principle: every subsystem has a zero-friction entry point that scales up without architectural change | Not feature flags. Not configuration complexity. The same mechanisms serve simple and complex deployments. |
| **Artifact** | A workload's packaged executable (OCI image, binary, Wasm module, installer) | Not the unit itself. The unit declares what artifact to run; the artifact is the thing that runs. |
| **Artifact Digest** | SHA256 content hash identifying an artifact for integrity and dedup | Not a version. Not a tag. Content-addressed: same bytes = same digest regardless of name. |
| **Promotion** | Policy-gated progression of a workload version to a new environment | Not re-deployment. Not cloning. The same unit in the graph, with a new policy allowing placement in the target environment. |
| **Promotion Policy** | A policy unit subtype that gates workload placement by environment tag | Not a deploy script. A signed, auditable declaration: "unit X version Y is approved for env:Z." |
| **Promotion Gate** | A governance unit declaring which environment transitions auto-promote and which require human approval | Not a pipeline. Governance policy within a trust domain. Solo dev: all auto. Regulated: gates where needed. |
| **Environment Tag** | A node capability declaring its environment role (`env:dev`, `env:test`, `env:prod`) | Not a namespace. Not a cluster. A capability like any other — the solver matches it. |
| **Author Affinity** | A dev node capability binding it to a specific author's workloads | Not tenancy. In dev, Alice's workloads run on Alice's nodes. In prod, no affinity — any promoted workload runs anywhere. |
| **Node Capability** | A static, auto-discovered property of a node (arch, OS, runtime, privilege) | Not a resource. Capabilities are what a node CAN do. Resources are what a node HAS available. |
| **Node Resource** | A dynamic, periodically-reported property of a node (free memory, CPU load) | Not a capability. Resources change constantly. The solver uses them for ranking, not filtering. |
| **Runtime Capability** | A node's ability to execute a specific artifact type (OCI, native, Wasm, K8s) | Not a workload property. The node advertises runtimes; the workload declares artifact type; the solver matches. |
| **Auto-Discovery** | Node startup process that probes the system for capabilities (Docker, Podman, K8s, TPM, etc.) | Not configuration. The node figures out what it can do. Operator only declares what taba can't detect. |
| **Fleet Refresh** | Governance operational command triggering all nodes to re-probe capabilities | Not a restart. Nodes re-run auto-discovery and advertise updated capabilities via gossip. |
| **Ceremony Tier** | The trust level of a key management operation, from Tier 0 (solo key) to Tier 3 (offline two-factor) | Not a security level. A ceremony tier determines key management complexity, not authorization scope. |
| **Decision Trail** | Queryable record of solver inputs + outputs for every placement decision | Not a log. A structural graph event. Enables solver replay: "show me why this placement happened at time T." |
| **Solver Replay** | Reconstructing the solver's decision at a past point in time from historical graph state | Not debugging. An operational tool. Deterministic solver means replay produces the exact same result. |
| **Placement-on-Failure** | Per-unit policy for what happens when the hosting node fails (replace or leave-dead) | Not a restart policy. Controls whether the solver re-places the workload to another node. Default: leave-dead for dev, auto-replace for prod. |
| **Peer Cache** | P2P artifact distribution across nodes, avoiding redundant external downloads | Not a registry. Not a CDN. Nodes share artifacts directly. Content-addressed for dedup. |
| **Operational Command** | A governance unit subtype representing a fleet-wide administrative instruction | Not a unit mutation. An instruction (refresh capabilities, enter degraded) propagated via gossip. |
| **Health Check** | Progressive workload health monitoring: OS-level (default) → HTTP probe → custom command | Not a liveness probe (K8s concept). Declared by the workload unit, executed by the node. Zero config default: is the process alive? |
| **Logical Clock** | Monotonically increasing counter for causal ordering, incremented on every system action, synced on node communication | Not wall time. Not a timestamp. A Lamport-style counter that provides causal order without clock synchronization. |
| **Dual Clock** | Every event records (logical_clock, wall_time, timezone). Logical clock is authoritative for ordering; wall clock for duration/retention | Not redundancy. Two clocks for two purposes. Correctness uses logical. Compliance uses wall. |
| **Service** | A workload unit with indefinite lifetime. Runs until explicitly terminated or key revoked | Not a bounded task. No validity window. The default workload type. |
| **Bounded Task** | A workload unit with a lifecycle limit. Auto-terminates on completion, failure, or deadline | Not a service. Has a validity window (logical clock range and/or wall-time deadline). Spawned by services or authored directly. |
| **Spawn** | A running service creating a bounded task at runtime with delegated authority | Not fork. The spawned task is a full graph unit with provenance linking to the parent. Max depth: 4. |
| **Spawn Depth** | The number of ancestor spawn links from a bounded task to the root service | Not hierarchy. A chain: service → task → sub-task → cleanup = depth 4 max. |
| **Tombstone** | Minimal record replacing a compacted unit in the graph. Preserves identity and references, removes content | Not deletion. The provenance graph shape survives. Full content retrievable from archive (if archived). |
| **Compaction** | Graph-level operation replacing eligible units with tombstones. Deterministic eligibility, CRDT-compatible | Not eviction. Permanent lifecycle event. All nodes converge on the same result. |
| **Eviction** | Node-local operation dropping full unit content to relieve memory pressure. Recoverable from peers | Not compaction. Temporary cache operation. Unit remains live in the graph. Content reconstructable via erasure coding. |
| **Ephemeral Data** | A data unit with retention: ephemeral. Auto-removed when producing bounded task terminates | Not temporary files. Still a graph unit (unless local-only). Provenance-tracked during its lifetime. |
| **Local-Only Data** | Scratch data that never enters the composition graph. Node-local, no CRDT, no provenance | Not ephemeral. Truly invisible to the graph. Requires policy if classified above public. |
| **Archival** | Moving full unit content to cold storage (S3, local path) before tombstoning | Not backup. Selective, governance-controlled. Pluggable backend. Retrievable by digest. |
| **Clock Capability** | Node-reported quality of its wall clock (ntp, ptp, gps, unsync) plus timezone | Not a requirement. An informational capability. Governance can mandate minimum quality per environment. |
| **Revocation Grace Period** | Optional policy-configurable logical clock delta for additional slow-propagation protection | Not a delay. Fallback for the causal revocation model. Default: none (pure causal). |
| **Causal Revocation** | Revocation takes effect when the revocation governance unit is merged into a node's local graph. Units accepted before merge are grandfathered | Not clock-comparison. Uses graph merge order, not logical clock comparison. The security window is gossip propagation time. |
| **Delegation Token** | A pre-signed, bounded authorization for a node to sign spawned tasks on behalf of an author | Not an author key. Scoped to one service, one node, one LC range, one spawn count limit. Revocable independently of key revocation. |
| **Bridge Node** | A node participating in multiple trust domains, serving cross-domain forwarding queries | Not a gateway. Not a proxy. A regular node that happens to be admitted to multiple domains. Emergent by default, governance-restricted when needed. |
| **Bilateral Policy** | Mutual authorization for cross-domain composition: both consuming and providing domains must have explicit policy | Not one-sided access. Neither domain can unilaterally reach into the other. Fail closed if either policy is missing. |
| **Cross-Domain Capability Advertisement** | A governance unit publishing capabilities a trust domain is willing to share | Not automatic exposure. An explicit declaration: "we offer payment-api to other domains under these conditions." |
| **Forwarding Query** | A signed request from one domain's solver to a bridge node for cross-domain unit information | Not a graph merge. Read-only view. Result cached in querying domain, never merged into its graph. |
| **Fail Open (cache)** | Serving stale cached cross-domain query results when bridge is unavailable | Not a security decision. The authorization (bilateral policy) is already settled. This is a freshness decision. Governance can override to fail closed. |
