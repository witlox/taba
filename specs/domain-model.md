# Domain Model

## Design Principle: Progressive Disclosure

taba follows a progressive disclosure model across all subsystems. The system
works at minimum complexity and gains sophistication only as the operator
declares more. This is a load-bearing design decision — every subsystem must
have a zero-friction entry point that scales up without architectural change.

| Subsystem | Simple (default) | Evolves to |
|-----------|-----------------|------------|
| Ceremony | Solo key (`taba init`) | Shamir with password-protected shares |
| Environment | Single trust domain + env tags | Separate trust domains per environment |
| Health | Process alive? (OS-level) | HTTP probe → custom check endpoint |
| Observability | Process monitoring + graph queries | OpenTelemetry export + alerting hooks |
| Runtime | Native binary on dev box | Rootless container → full container → K8s pod |
| Governance | Auto-promote everything | Human gates, multi-party sign-off |

## Core Entities

### Unit
The fundamental primitive. A self-describing, signed, typed entity carrying
capability declarations, behavioral contracts, and security requirements.

**Types**: Workload | Data | Policy | Governance
**Subtypes**: Service (indefinite) | BoundedTask (lifecycle-limited)
**States**: Declared → Composed → Placed → Running → Draining → Terminated
**Identity**: UnitId (globally unique, immutable after creation)
**Signed by**: AuthorId (cryptographic signature, verified on graph merge)
**Validity window**: optional. If omitted, unit is valid indefinitely (until
key revocation or explicit termination). If set, unit has a bounded lifecycle
(logical clock range and/or wall-time deadline).

### Workload Unit
A compute process. Runtime-agnostic (container, microVM, Wasm, native process —
isolation mechanism is matched by the solver to node runtime capabilities).

**Declares**: needs (capabilities required, with optional purpose qualifier),
provides (typed interfaces exposed, with optional purpose qualifier),
tolerates (latency/failure budgets), trusts (identity-based access),
scaling (min/max instances, triggers), failure semantics (OOM behavior,
restart policy), recovery relationships (dependency ordering on failure —
cycles fail closed per INV-K5),
state recovery (stateless | replay-from-offset | require-quorum),
placement_on_failure (replace | leave-dead, default varies by environment —
see Node Environment)

**Artifact**: every workload unit declares its packaging:
- `artifact.type`: oci | native | wasm | k8s-manifest
- `artifact.ref`: content reference (OCI image tag, binary URL, file path, etc.)
- `artifact.digest`: content hash (SHA256) for integrity and dedup
- `artifact.requires`: additional runtime requirements (e.g., `["windows", "dotnet-4.8"]`)

The solver matches `artifact.type` to node `runtime:*` capabilities. A workload
declaring `artifact.type = "oci"` matches nodes with `runtime:oci` or
`runtime:oci-rootless`. The node handles fetching and executing — taba does not
prescribe the runtime mechanism.

**Version**: git-native. The version field is a git ref (commit SHA or tag).
Provenance links versions: "v1.2 was built from commit abc123 which descends
from v1.1 at def456." The composition graph IS the version history.

**Workload subtypes**:
- Service: long-running, indefinite lifetime. No validity window. Terminated
  only explicitly or by key revocation.
- BoundedTask: lifecycle-limited. Terminates on completion, failure, or
  deadline (logical clock range or wall time). A service can spawn bounded
  tasks at runtime with delegated authority.

**Spawning**: a running service can spawn bounded tasks. The spawned task:
- Is a full unit in the graph (signed, provenance-tracked, solver-placed)
- Inherits the parent's trust context (trust domain, author scope)
- Has a bounded lifecycle (auto-terminates on completion/failure/deadline)
- Links to the parent via provenance ("spawned by web-api v1.3")
- Max spawn depth: 4 (service → task → sub-task → cleanup). Deeper nesting
  requires explicit governance override. Enforced at graph merge.

**Bounded task termination triggers**:
- Completion: task finishes successfully → auto-terminates
- Failure: task crashes or exceeds retry limit → failure semantics apply
- Deadline: logical clock range exceeded or wall-time deadline passed →
  auto-terminates regardless of state

### Data Unit
A dataset carrying constraints. Hierarchical: parent contains children.
Granularity is demand-driven — children exist only where constraints diverge
from parent.

**Declares**: schema (typed), classification (PII, proprietary, public, etc.
— ordered in lattice: public ⊂ internal ⊂ confidential ⊂ PII),
provenance (which unit produced it, from what inputs, when),
retention (duration, legal basis), consent scope (what it may be used for,
expressed as purpose qualifier on provided capabilities),
storage requirements (encryption, replication, jurisdiction)

**Retention modes**:
- Persistent (default): governed by retention policy (wall-time, compliance-driven).
  Tombstoned on retention expiry.
- Ephemeral: auto-removed when the producing bounded task terminates.
  No tombstone by default; governance can mandate tombstone for audit trail.
- Local-only: never enters the composition graph. Node-local scratch data.
  No CRDT overhead, no provenance, no audit. Governance can restrict
  local-only for classified data above `public` (requires explicit policy,
  same pattern as declassification).

**Inheritance rules**: children inherit parent constraints by default.
Children can narrow (add restrictions) freely. Children can widen (remove
restrictions) only with explicit policy. This is the taint propagation model.
Direction: narrowing = adding restrictions (always allowed), widening = removing
restrictions (requires policy per INV-S7). Max hierarchy depth: 16 levels.

### Policy Unit
Resolves capability conflicts between other units. Required whenever the
solver detects incompatible declarations. Authored by policy-scoped roles.

**Declares**: which conflict it resolves (references the conflicting units),
the resolution (allow, deny, conditional), scope (which trust domain),
rationale (human-readable justification), supersedes (optional: ID of the
policy this replaces, creating a versioned lineage chain)

**Invariant**: every security decision that isn't trivially resolved by
compatible declarations has a policy unit. No implicit resolution.

**Uniqueness**: only one non-revoked policy per conflict tuple (set of unit IDs +
capability name). A second policy for the same conflict must explicitly supersede
the first (INV-C7). Solver uses the latest non-revoked version in the chain.

**Collision handling**: because policy scopes may overlap (INV-S8a), two
authors can independently create policies for the same conflict:
- Same decision: dedup by lexicographically lowest PolicyId (transparent).
- Different decisions: fail closed. Requires explicit supersession by one
  author, or a governance unit resolving the meta-conflict.

**Declassification**: policies that remove data classification (taint) require
multi-party signing — minimum 2 distinct authors with policy and data-steward
scopes respectively (INV-S9).

**Promotion policies**: a subtype of policy that gates workload placement
by environment. A promotion policy declares: unit reference, approved
environment tag (e.g., `env:test`, `env:prod`), and rationale. The solver
uses promotion policies to determine which environments a workload version
may be placed in. Governance declares which environment transitions
auto-promote (e.g., dev→test via CI) and which require human approval
(e.g., test→prod).

### Governance Unit
Trust domain definitions, role scope assignments, certification attestations,
and fleet-wide operational commands.

**Subtypes**:
- TrustDomain: boundary definition, who participates, expiry
- RoleAssignment: author → (unit type scope, trust domain scope)
- Certification: attestation that a composition meets a standard
- OperationalCommand: fleet-wide administrative instruction (e.g.,
  `refresh-capabilities`, `enter-degraded`). Signed by an author with
  governance scope, propagated via gossip, auditable in the graph.
- PromotionGate: declares which environment transitions auto-promote and
  which require human approval within a trust domain

### Composition Graph
CRDT (Conflict-free Replicated Data Type). The single source of truth for
desired state. Distributed across all nodes via erasure coding.

**Contains**: all units and their relationships
**Operations**: insert, compose, query, supersede, archive
**Merge**: commutative, associative, idempotent (CRDT properties). Signature
verification is synchronous gate before merge. Units with unsatisfied references
enter pending queue (causal buffering per INV-C4).
**Integrity**: every entry signed, verified before merge. Unit identity:
(UnitId, Author, CreationTimestamp) — graph is a set, no duplicates.
**Sharding**: Phase 1-2: full graph per node, memory-bounded (INV-R6).
Phase 3+: trust domain sharding with cross-domain forwarding protocol.

### Node
A machine participating in the taba cluster. Peer — no distinction between
"control plane" and "worker." Can run in userspace (no root required) or
as a system service.

**States**: Joining → Attesting → Active → Suspected → Draining → Left | Failed
**Identity**: Ed25519 key pair (generated at join, or from TPM attestation)
**Responsibilities**: holds graph shards, runs solver locally, reconciles
local state, participates in gossip (signed messages per INV-R3), stores WAL

**Installation modes**:
- Userspace: `taba init` in home directory. No root, no sudo. Narrower
  capability set (no privileged ports, no system package installs). Suitable
  for dev boxes and low-friction onboarding.
- System: installed as a system service. Full capability set. Typical for
  test/prod nodes.

#### Node Capabilities (static)

Auto-discovered at startup, cached locally. Re-probed on `taba refresh`
(single node) or fleet-wide via `taba fleet refresh-capabilities`
(governance operational command propagated via gossip).

| Layer | Examples | Discovery |
|-------|----------|-----------|
| Hardware | `arch:x86_64`, `arch:aarch64`, `memory:16gb`, `gpu:cuda`, `tpm:present` | Auto-detected |
| OS | `os:linux`, `os:windows`, `os:macos` | Auto-detected |
| Privilege | `privilege:root`, `privilege:user` | Auto-detected |
| Runtime | `runtime:oci`, `runtime:oci-rootless`, `runtime:k8s`, `runtime:wasm`, `runtime:native` | Auto-discovered (probe for Docker/Podman/containerd/wasmtime) |
| Network | `ports:privileged` (can bind <1024), `ports:unprivileged` | Derived from privilege |
| Storage | `storage:local`, `storage:encrypted`, `storage:nfs` | Declared or probed |
| Environment | `env:dev`, `env:test`, `env:prod` | Declared by operator |
| Author affinity | `author:alice` | Derived from `taba init` identity (dev nodes) |
| Custom tags | `rack:east-3`, `oracle-licensed:true` | Freeform key:value, operator-declared in config |

Capabilities are advertised via gossip (change rarely). The solver uses
capabilities as hard constraints: CAN this workload run here?

Runtime auto-discovery probes:
- Docker/Podman socket → `runtime:oci` (or `runtime:oci-rootless` if rootless)
- K8s API available → `runtime:k8s` (whether node is inside or outside the cluster)
- Wasmtime/Wasmer binary → `runtime:wasm`
- OS package manager → `runtime:native`

#### Node Resources (dynamic)

Reported locally, updated periodically or on significant change. Advertised
via gossip as resource snapshots.

| Resource | Examples |
|----------|----------|
| Memory | `memory.total: 16gb`, `memory.available: 8gb` |
| CPU | `cpu.cores: 8`, `cpu.load: 0.3` |
| Disk | `disk.available: 200gb` |
| GPU | `gpu.available: 2` |

The solver uses resources as soft constraints: SHOULD this workload run here?
Capability filtering (hard) happens first, then resource ranking (soft/best-fit).

#### Node Environment

Nodes carry environment tags (`env:dev`, `env:test`, `env:prod`) that control
workload placement via promotion policies.

**Dev nodes**: carry `author:X` affinity. Workloads in `env:dev` are placed
only on the authoring developer's dev nodes. One author can have multiple
dev nodes; the solver picks by capability/resource fit.

**Placement-on-failure defaults** (overridable per unit):
- `env:dev` → leave-dead (developer knows their laptop is closed)
- `env:test`, `env:prod` → auto-replace (treat as standard node failure)

### Author
An authenticated identity with scoped authority to create units.

**Parameterized by**: unit type scope (which types), trust domain scope (where)
**Identity**: Ed25519 key pair, signed into role assignments
**No implicit authority**: zero access by default, all scopes explicit
**Scope uniqueness (refined)**: for state-producing unit types (workload, data),
no two distinct authors may have identical scope tuples (INV-S8). For
decision-making types (policy, governance), overlapping scopes are permitted
because conflict resolution is structural (supersession chain per INV-C7,
collision dedup). This enables role succession without single points of failure.

### Trust Domain
A boundary for authorization scope. Itself a governance unit, created
through composition requiring multi-party agreement (or self-signed in Tier 0).

**Contains**: units, authors, policies within its boundary
**Creation** (progressive):
- Tier 0 (solo): self-signed by a single author via `taba init`. No ceremony.
  Suitable for solo developers, dev environments, and evaluation.
- Tier 1+ (multi-party): requires multi-party signing (minimum 2 distinct
  authors per INV-S10). The root trust domain is created via Shamir key
  ceremony (pre-graph bootstrap).
**Scoping**: authors are scoped to trust domains, not global. No implicit role
inheritance across domain boundaries — cross-domain roles require explicit policy.
**Environment scoping**: a trust domain can optionally contain environment
tags as governance policy, declaring promotion gates and auto-promote rules.
Solo dev: everything auto-promotes. Regulated: explicit gates per environment.

**Cross-domain capability advertisement**: a trust domain can publish
capabilities it is willing to share with other domains via a governance unit
(CrossDomainCapability). Bridge nodes gossip these advertisements across
domain boundaries. External domains discover available capabilities through
bridge nodes or operator-configured seed nodes.

### Cross-Trust-Domain Forwarding

When the graph is sharded by trust domain (Phase 3+), cross-domain
interactions require a forwarding protocol. This is not a separate subsystem —
it emerges from bridge nodes, bilateral policy, and the existing gossip/solver
infrastructure.

#### Bridge Nodes

A bridge node is any node that participates in multiple trust domains. It
holds graph shards for each domain and can serve cross-domain forwarding
queries.

**Emergent bridges** (default): any node admitted to domains A and B is
automatically a bridge. No special designation needed. Bridges emerge
naturally as teams share infrastructure.

**Explicit bridges** (governance override): when control matters, governance
units in each domain can designate specific nodes as authorized bridges.
Non-designated nodes in both domains hold both graphs locally but do NOT
serve forwarding queries. This constrains the blast radius of bridge
compromise.

**Bridge as composition need**: when no bridge exists between domains A and B,
the solver surfaces this as an unresolved capability: "cross-domain
composition requires a bridge between A and B." This is the same mechanism
as any unresolved need — it's infrastructure-as-composition.

**Bridge redundancy**: the system surfaces alerts when a single bridge is the
only link between domains. Operator decides whether to admit additional
nodes. No automatic bridge creation.

**Bridge security**: a compromised bridge has visibility into all domains it
participates in. This is standard node compromise (FM-04) with wider blast
radius — a reason to be intentional about which nodes bridge domains.
Governance can require stronger attestation for bridge nodes (e.g., mandatory
TPM) but this is policy, not a structural requirement.

#### Cross-Domain Composition

Workload W in domain A needs a capability that only exists in domain B.

1. Solver in domain A detects unresolved need for capability C
2. Solver checks cross-domain capability advertisements (gossiped by bridges)
3. Domain B advertises capability C via CrossDomainCapability governance unit
4. Solver creates a cross-domain composition request

**Bilateral policy** (mutual consent): cross-domain composition requires
policy in BOTH domains:
- Domain A: "I authorize workload W to consume capability C from domain B"
- Domain B: "I authorize domain A to access capability C under conditions X"

Neither domain can unilaterally access the other. Same fail-closed principle
(INV-S2) across boundaries. No implicit cross-domain access.

#### Forwarding Query Protocol

```
1. Solver on node-1 (domain A) needs unit info from domain B
2. Gossip: "who is a bridge for domain B?"
3. node-3 (bridge) responds
4. node-1 sends signed forwarding query to node-3
5. node-3 executes query against local domain B graph
6. node-3 returns signed result (proof of query execution)
7. node-1 verifies node-3's signature and domain B membership
8. Solver uses result for composition/placement
```

**Query results are read-only views**: not merged into domain A's graph.
Domain A references domain B's units by ID but does not hold the full unit.

**Caching**: query results are cached in the querying domain. Default: fail
open (serve stale cache if bridge unavailable). Governance can require strict
freshness for sensitive cross-domain data — cached result rejected if bridge
unreachable, query blocks until bridge returns (fail closed for freshness).

**Bridge unavailable**: if no bridge exists or all bridges are down, cross-
domain references enter pending state (causal buffering, same as INV-C4).
Existing compositions with cached results continue operating (fail open).
New cross-domain compositions cannot start.

#### Inter-Domain Discovery

**Via bridge nodes** (auto): bridge nodes naturally gossip cross-domain
capability advertisements. A node in domain A learns about domain B's
capabilities through a shared bridge node.

**Via configuration** (manual): operators can configure known domains and
seed nodes. Useful for bootstrapping before a bridge exists, or for
connecting domains that don't share any nodes yet.

**Progressive disclosure**:

| Complexity | Cross-domain model |
|-----------|-------------------|
| Solo dev / small team | Single trust domain. No cross-domain. |
| Multi-team in one org | Shared nodes are natural bridges. Auto-discovery. |
| Multi-org | Explicit bridge governance. Bilateral policy. |
| Regulated cross-org | All above + certification + strict cache freshness. |

### Root Key (Progressive Ceremony)
The root of all authority. Ceremony tier determines the key management model.

**Ceremony tiers** (progressive disclosure):
- Tier 0 (solo): `taba init` generates a single Ed25519 keypair. No Shamir,
  no shares, no ceremony. The key is both the node identity and the author
  identity. Produces a self-signed trust domain + root governance unit in
  one command. The developer is immediately operational.
- Tier 1 (small team): Shamir-split into shares. Default 5 shares, threshold 3.
  Basic ceremony — start → add shares → complete with witness.
- Tier 2 (regulated): password-protected — each share encrypted with Argon2id-derived key.
- Tier 3 (high security): offline two-factor — seed code + password, shares
  never unencrypted on server.

**Bootstrap**: the ceremony (or `taba init` for Tier 0) is the pre-graph
bootstrap. It creates the first trust domain + root governance unit which
seeds the composition graph.
**Upgrade path**: a Tier 0 trust domain can be upgraded to Tier 1+ by creating
a new trust domain with ceremony and migrating units. The original Tier 0
domain remains valid (no destructive migration).
**Break-glass**: at any tier, the root key can re-assign roles. This is the
escape hatch when all authors with a given scope have left.
**Audit**: ceremony events recorded as governance units in the graph.
**Memory safety**: all key material zeroed after use (zeroize crate).

### Logical Clock
Monotonically increasing counter for causal ordering across the cluster.
Every system action increments the local counter. On inter-node communication
(gossip, graph merge), nodes sync: `local = max(local, remote) + 1`.

**Dual clock model**:
- Logical clock: authoritative for ordering, causality, key revocation,
  signature validity. If event A caused event B, A's logical clock < B's.
  No NTP dependency, no clock skew, no timezone conversion.
- Wall clock: authoritative for duration-based operations (retention policies,
  compliance deadlines). Informational for human-readable audit trails.

Every event records the triple: `(logical_clock, wall_time, timezone)`.
The system chooses which clock based on operation type.

**Key revocation**: uses logical clock. "Key revoked at LC 50000" means
units signed by that author with LC > 50000 are rejected. The gossip
convergence window (not clock skew) is the real exposure. Governance can
configure the revocation grace period as policy.

**Clock capability**: nodes report clock quality as a capability:
- `clock:ntp` (seconds accuracy), `clock:ptp` (microseconds),
  `clock:gps` (nanoseconds), `clock:unsync` (no sync)
- `tz:Europe/Amsterdam` (timezone for display)
- Governance can require minimum clock quality for environments
  (e.g., prod requires `clock:ntp` or better for retention compliance)

**Local drift detection**: each node records `(logical_clock, wall_clock)`
pairs. Comparing with peers via gossip detects outlier wall clocks without
depending on them for correctness.

### Operational Modes
System-wide state affecting which operations are permitted.

**Normal**: all operations allowed.
**Degraded**: entered when erasure threshold exceeded (INV-R4), memory limit
exceeded (INV-R6), or operator-triggered. Authoring/composition/placement frozen.
Drain and evacuation only. Operator intervention required.
**Recovery**: gradual re-coding underway after degraded trigger resolved.
Placement throttled. Auto-transitions to Normal when recovery complete.

### Artifact Distribution
Workload artifacts (OCI images, binaries, Wasm modules, packages) must reach
the nodes where they are placed. taba supports both push and pull modes with
a peer-to-peer cache for efficiency.

**Pull mode** (default): node told "run OCI image X" → pulls from registry.
If another peer already has the artifact, pull from peer instead.
Content-addressed by SHA256 digest for dedup and integrity.

**Push mode** (air-gapped/dev): developer builds locally, taba distributes
the artifact to target nodes via the existing P2P infrastructure. Necessary
when nodes cannot reach external registries.

**Hybrid**: dev box builds and pushes to local peer cache. Test/prod nodes
pull from peer cache first, fall back to external registry.

**Content addressing**: all artifacts are identified by SHA256 digest.
Same digest = same content = no redundant transfer. A fleet of 100 nodes
pulling the same image results in one external download + P2P distribution.

### Observability
Observability follows the progressive disclosure model: structural observability
is built in (falls out of the graph), integration observability plugs into
external systems.

**Structural (in-graph)**:
- Decision trail: every solver run records inputs + outputs as a queryable
  graph event. "Why was web-api placed on node-3?" is a graph query.
- Promotion audit: full chain from git commit → CI → promotion policy → placement.
- Capability change log: what changed on which node, when, why.
- Drift detection events: actual vs desired divergence, timestamped.
- Solver replay: "show me the solver's view at time T" for any past state.
  Retention: since last compaction by default, overridable per unit for
  longer retention.

**Integration (external systems)**:
- OpenTelemetry export: workload-level metrics and traces via standard protocols.
- Prometheus endpoint: per-node resource and health metrics.
- Log forwarding: structured events to external sink (stdout, file, syslog).
- Alerting hooks: webhook on degraded mode, policy conflict, promotion failure.

**Health checks** (progressive):
- Default: OS-level process monitoring (is the process alive?). Zero config.
- Declared: workload unit optionally declares a health check endpoint
  (HTTP path, TCP port, or command). Node probes at declared interval.
- Custom: workload provides a health check command. Node executes and
  reports result.

Workload-level metrics (request rate, latency, errors) are NOT managed by
taba — workloads export via standard protocols (OpenTelemetry, Prometheus
scrape). taba surfaces node-level and graph-level observability only.

### Environment Progression
Workloads progress from dev → test → prod via the git-native promotion model.
This is not a separate subsystem — it emerges from the composition of
environment capabilities, promotion policies, and git-native versioning.

**Flow**:
1. Developer commits code, `taba apply` on dev box → unit placed on `env:dev`
   nodes matching `author:X`
2. Git merge to main → CI authors a promotion policy for `env:test` → solver
   places on test nodes
3. Tests pass, git tag → promotion policy for `env:prod` (auto or human-gated
   per governance) → solver places on prod nodes

**Selection among parallel developers**: git merge is the selection mechanism.
Three devs on three branches produce three unit versions on their own dev
nodes. Merge to main selects the winner. taba does not invent a separate
selection mechanism.

**Governance controls**: a PromotionGate governance unit in the trust domain
declares which transitions auto-promote and which require human approval.
Solo dev default: all auto. Regulated default: test→prod requires sign-off.

### Graph Compaction
Reclaims space in the active graph while preserving provenance integrity.
Two distinct operations:

**Compaction** (graph-level, deterministic eligibility):
All nodes agree on WHAT is eligible for compaction based on graph state
(task terminated, retention expired, policy superseded). Produces a
CRDT-compatible tombstone that merges across all nodes. Tombstones are
monotonic: once tombstoned, always tombstoned. Timing varies per node;
result converges via CRDT.

**Eviction** (node-level, local pressure):
Node under memory pressure drops full unit content locally. NOT a tombstone —
the unit is still live in the graph. Content reconstructable from peers
(erasure coding) or archive. This is a cache operation, not a lifecycle event.

**Tombstone**: minimal record replacing a compacted unit:
- UnitId, AuthorId, unit type (preserved identity)
- Created-at and terminated-at logical clock values
- Termination reason (completed | expired | failed | superseded)
- References: what the unit consumed/produced (preserves provenance graph)
- Original digest (SHA256, for verification if full unit retrieved from archive)

Tombstones preserve the **shape** of the provenance graph without the content.
INV-D1 (unbroken provenance chain) is maintained.

**Compaction priority** (least valuable first):

| Priority | What | Trigger | Treatment |
|----------|------|---------|-----------|
| 1 | Ephemeral data (retention: ephemeral) | Producing task terminates | Remove entirely (no tombstone, unless governance mandates) |
| 2 | Decision trails past retention | Retention period expires (INV-O2) | Remove |
| 3 | Terminated bounded tasks | Task completed/failed/expired | Tombstone |
| 4 | Superseded policies | Successor stable | Tombstone |
| 5 | Terminated services | No active data dependents | Tombstone |
| 6 | Expired data units | Retention period (wall time) exceeded | Tombstone |

**Never compacted**:
- Governance units (trust domains, role assignments) — authority structure
- Active policies (non-superseded, resolving active conflicts)
- Root ceremony chain — bootstrap trust anchor, ever
- Live data's provenance references — producing workload gets tombstoned
  (not deleted), preserving the reference chain

**Compaction priority mirrors reconstruction priority (inverse)**:
Most important things are last to compact and first to reconstruct after failure.
`governance > policy > data constraints > workload` (INV-R1).

**Compaction triggers**:
1. Memory pressure: auto at 80% of limit (INV-R6), compacts in priority order
2. Retention expiry: periodic scan (wall-time-based)
3. Task completion: ephemeral data + bounded tasks auto-eligible immediately
4. Operator command: `taba compact` or fleet-wide governance command
5. Background periodic: configurable interval

### Archival
Optional cold storage for full unit content before tombstoning. Operator-
configured, pluggable backend.

**Archive interface** (trait):
- `write(unit_id, digest, content) → Result`
- `read(digest) → Result<content>`
- Built-in backends: local path, S3-compatible object store
- No archive configured: tombstones only, original content gone

**Governance control**: trust domains can require archival for specific unit
types or classification levels. "Retain full records for 7 years" →
archival mandatory before compaction for covered units.

**Retrieval**: "show me the full details of tombstoned workload W" →
retrieve from archive by digest, verify integrity against tombstone's
`original_digest`.

## Aggregate Boundaries

- **Unit + its declarations** = one aggregate (atomic creation/update)
- **Composition** (set of composed units + policies resolving their conflicts) = one aggregate
- **Trust Domain** (boundary + role assignments + governance units) = one aggregate
- **Node** (membership + shards + local state) = one aggregate

## Key Relationships

- Unit **composed-with** Unit (via solver, producing composition)
- Workload Unit **consumes** Data Unit (capability match: needs → provides)
- Workload Unit **produces** Data Unit (output, creates provenance link)
- Workload Unit **versioned-from** Workload Unit (git-native provenance)
- Policy Unit **resolves** conflict between Units
- Promotion Policy **gates** Workload Unit to environment
- Author **creates** Unit (within scope)
- Unit **belongs-to** Trust Domain
- Node **holds** Graph Shards
- Node **advertises** Capabilities (static, auto-discovered)
- Node **reports** Resources (dynamic, periodic)
- Node **caches** Artifacts (P2P distribution)
- Data Unit **contains** Data Unit (hierarchical)
- Solver Run **produces** Decision Trail (queryable graph event)
- Service **spawns** BoundedTask (delegated authority, provenance-linked)
- BoundedTask **produces** Ephemeral Data (auto-removed on task completion)
- Compaction **tombstones** Unit (preserves identity + references, removes content)
- Archival **preserves** Unit content (cold storage, retrievable by digest)
