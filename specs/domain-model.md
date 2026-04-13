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
**States**: Declared → Composed → Placed → Running → Draining → Terminated
**Identity**: UnitId (globally unique, immutable after creation)
**Signed by**: AuthorId (cryptographic signature, verified on graph merge)

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
