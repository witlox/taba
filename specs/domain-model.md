# Domain Model

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
isolation mechanism is itself a declared capability).

**Declares**: needs (capabilities required, with optional purpose qualifier),
provides (typed interfaces exposed, with optional purpose qualifier),
tolerates (latency/failure budgets), trusts (identity-based access),
scaling (min/max instances, triggers), failure semantics (OOM behavior,
restart policy), recovery relationships (dependency ordering on failure —
cycles fail closed per INV-K5),
state recovery (stateless | replay-from-offset | require-quorum)

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

**Declassification**: policies that remove data classification (taint) require
multi-party signing — minimum 2 distinct authors with policy and data-steward
scopes respectively (INV-S9).

### Governance Unit
Trust domain definitions, role scope assignments, certification attestations.

**Subtypes**:
- TrustDomain: boundary definition, who participates, expiry
- RoleAssignment: author → (unit type scope, trust domain scope)
- Certification: attestation that a composition meets a standard

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
"control plane" and "worker."

**States**: Joining → Attesting → Active → Suspected → Draining → Left | Failed
**Identity**: Ed25519 key pair (generated at join, or from TPM attestation)
**Responsibilities**: holds graph shards, runs solver locally, reconciles
local state, participates in gossip (signed messages per INV-R3), stores WAL

### Author
An authenticated identity with scoped authority to create units.

**Parameterized by**: unit type scope (which types), trust domain scope (where)
**Identity**: Ed25519 key pair, signed into role assignments
**No implicit authority**: zero access by default, all scopes explicit
**Scope uniqueness**: no two distinct authors may have identical scope tuples
(INV-S8). Validated at role assignment time.

### Trust Domain
A boundary for authorization scope. Itself a governance unit, created
through composition requiring multi-party agreement.

**Contains**: units, authors, policies within its boundary
**Creation**: requires multi-party signing (minimum 2 distinct authors per
INV-S10). The root trust domain is created via Shamir key ceremony (pre-graph
bootstrap — the ceremony produces the first governance unit which seeds the graph).
**Scoping**: authors are scoped to trust domains, not global. No implicit role
inheritance across domain boundaries — cross-domain roles require explicit policy.

### Root Key (Shamir Ceremony)
The root of all authority. Ed25519 keypair, Shamir-split into shares.

**Default**: 5 shares, threshold 3.
**Ceremony tiers** (evolving across phases):
- Tier 1 (Phase 1): basic ceremony — start → add shares → complete with witness
- Tier 2 (Phase 3): password-protected — each share encrypted with Argon2id-derived key
- Tier 3 (Phase 5): offline two-factor — seed code + password, shares never unencrypted on server
**Bootstrap**: root key ceremony is the pre-graph bootstrap. It creates the first
trust domain + root governance unit which seeds the composition graph.
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

## Aggregate Boundaries

- **Unit + its declarations** = one aggregate (atomic creation/update)
- **Composition** (set of composed units + policies resolving their conflicts) = one aggregate
- **Trust Domain** (boundary + role assignments + governance units) = one aggregate
- **Node** (membership + shards + local state) = one aggregate

## Key Relationships

- Unit **composed-with** Unit (via solver, producing composition)
- Workload Unit **consumes** Data Unit (capability match: needs → provides)
- Workload Unit **produces** Data Unit (output, creates provenance link)
- Policy Unit **resolves** conflict between Units
- Author **creates** Unit (within scope)
- Unit **belongs-to** Trust Domain
- Node **holds** Graph Shards
- Data Unit **contains** Data Unit (hierarchical)
