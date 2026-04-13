# Invariants

## Security Invariants (must NEVER be violated)

**INV-S1**: A unit can only access capabilities it explicitly declared AND that
policy approved. No implicit access. Zero default.

**INV-S2**: Security conflicts fail closed. If the solver detects incompatible
capability declarations, composition is refused until explicit policy resolves it.

**INV-S3**: Every unit in the graph is signed by an author with valid scope.
Unsigned or wrongly-signed units are rejected on merge. Signature verification
is synchronous and blocks merge — no unit enters graph state before verification
completes. Signatures bind context: Sign(key, hash(unit || trust_domain_id ||
cluster_id || validity_window)). A unit is valid iff: (a) signature is
cryptographically valid, (b) author had valid scope at creation time, and
(c) author's key was not revoked before the unit's creation timestamp.

**INV-S4**: Taint propagation: if input data unit has classification C, output
data unit inherits C unless explicit policy declassifies. Taint is computed at
query time by traversing the provenance graph — not cached at merge time. This
makes taint eventually consistent across nodes. Multi-input workloads inherit
the union (most restrictive) of all input classifications.

**INV-S5**: Authors cannot create units outside their (type scope × trust domain scope).

**INV-S6**: Trust domain creation requires multi-party policy resolution (no
single author can unilaterally create a trust domain).

**INV-S7**: Data unit children can narrow parent constraints freely but can
widen only with explicit policy. Direction: narrowing = adding restrictions
(child more restrictive than parent, always allowed). Widening = removing
restrictions (child less restrictive than parent, requires policy).
Classification lattice: public ⊂ internal ⊂ confidential ⊂ PII.

**INV-S8**: Scope uniqueness is type-dependent. For state-producing unit
types (workload, data), no two distinct authors can have identical
(unit_type_scope, trust_domain_scope) tuples. Role assignment governance
units must validate non-overlap before persisting. This is the enforcement
mechanism for A1.

**INV-S8a**: For decision-making unit types (policy, governance), overlapping
scopes are permitted. Conflict resolution is structural: policy collisions
are handled by supersession chain (INV-C7) and deterministic dedup (same
decision → lowest PolicyId is canonical; different decisions → fail closed).
This enables role succession and eliminates single points of failure for
policy authoring.

**INV-S9**: Declassification policies (taint removal) require multi-party
signing: minimum 2 distinct authors — one with policy scope, one with
data-steward scope. Single-author declassification is rejected.

**INV-S10**: Trust domain creation requires cryptographic signatures from at
least 2 distinct authors (threshold). The policy unit explicitly lists required
signers and the solver verifies all required signatures are present.

## Consistency Invariants (must ALWAYS hold)

**INV-C1**: The composition graph is the single source of desired state.
There is no other desired-state store.

**INV-C2**: CRDT merge is commutative, associative, and idempotent.
merge(A, B) == merge(B, A). merge(merge(A, B), C) == merge(A, merge(B, C)).
merge(A, A) == A.

**INV-C3**: The solver is deterministic. Given identical graph state and
identical node membership, any node produces identical placement decisions.
All solver arithmetic uses fixed-point at ppm scale (10^6, u64/i64). No
floating-point in scoring or placement. Partition tiebreaker: lexicographically
lowest NodeId wins; loser drains immediately using declared on_shutdown.

**INV-C4**: WAL-before-effect. Each mutation is WAL'd atomically before its
effects become visible to local queries. Mutations form a partial (causal) order,
not a total order. WAL records three entry types: Merged(unit) — verified and in
graph; Pending(unit, missing_refs) — verified but references not yet satisfied;
Promoted(unit_id) — pending unit activated after refs arrived.

**INV-C5**: Every policy unit references the specific conflict it resolves.
Orphaned policies (referencing non-existent conflicts) are detected at query
time and eligible for archival. Policy validity is checked on query, not merge.

**INV-C6**: Composition result is independent of unit insertion order. The solver
re-evaluates all affected compositions on any unit addition. Given the same set
of units, composition produces the same result regardless of the order they
were added.

**INV-C7**: Only one non-revoked policy unit may resolve any given conflict
tuple (set of unit IDs + capability name). A second policy for the same conflict
must explicitly supersede the first (versioned lineage). The solver uses the
latest non-revoked version. Supersession creates an immutable chain.

## Composition Invariants

**INV-K1**: A composition is valid only if all capability needs are satisfied
by corresponding provides, with no unresolved security conflicts.

**INV-K2**: Capability matching is typed. "needs postgres" matches
"provides postgres-compatible" but not "provides redis." Capabilities are
tuples: (type, name, purpose?). Purpose is an optional qualifier — if declared,
it must match (purpose mismatch triggers conflict requiring policy). Capability
lists are sorted lexicographically by (type, name, purpose) before matching to
ensure determinism regardless of declaration order.

**INV-K5**: Cyclic recovery dependencies fail closed. If units form a circular
recovery dependency chain, the solver reports an unresolvable conflict requiring
explicit policy declaring restart priority. Tiebreaker: lexicographically
lowest UnitId gets priority if no policy exists.

**INV-K3**: Placement respects all unit tolerance declarations (latency,
failure mode, resource requirements).

**INV-K4**: Scaling decisions are computed from unit-declared parameters.
The solver does not invent scaling logic.

## Data Invariants

**INV-D1**: Provenance chain is unbroken. Every data unit produced by a
workload links back to its input data units and the producing workload.
Provenance is verified at query time, not merge time. References to units
not yet in the local graph are marked 'pending' until the referenced unit
arrives (causal buffering per INV-C4).

**INV-D2**: Data unit retention is enforced. Expired data units are eligible
for compaction. This is not optional.

**INV-D3**: Hierarchical data units: children exist only where constraints
diverge from parent. Identical-constraint children are redundant.

## Resilience Invariants

**INV-R1**: Node failure does not corrupt the graph. Erasure coding enables
reconstruction from surviving shards. After reconstruction, all unit signatures
are re-verified. Reconstruction has backpressure: throttled rate, prioritized
by shard criticality (governance > policy > data constraints > workload), with
circuit breaker when queue depth exceeds threshold.

**INV-R2**: Network partition: both sides maintain graph consistency via CRDT.
On heal, merge produces correct state. Duplicate placements resolved by
deterministic tiebreaker (lexicographically lowest NodeId wins per INV-C3).
Role-carrying units (policy/governance authors) cannot be duplicated across
partition sides — if quorum unreachable, unit disabled on minority side.

**INV-R3**: Gossip membership converges. In absence of actual failures,
all nodes eventually agree on membership. All gossip messages are signed
with the sending node's identity key. Membership state changes (node declared
failed) require corroboration from at least 2 independent witnesses.

**INV-R4**: Graph shards are reconstructable if actual failures ≤ floor(N - k).
If failures exceed this threshold, system enters degraded mode and surfaces
operator alert. Erasure parameters: k = ceil(N × (1 - R/100)) where R is
the configured resilience percentage.

**INV-R5**: Suspected nodes remain in the placement pool with health='unknown'.
Solver avoids suspected nodes when alternatives exist but does not remove them
until SWIM confirms failure via multi-probe consensus.

**INV-R6**: Active graph per node must remain ≤ configurable memory limit.
Auto-compaction triggers at 80% of limit. Node exceeding limit enters degraded
mode: refuses new placements until compaction completes. Governance units are
actively replicated (full copies on N nodes), not just erasure-coded.

## Environment & Promotion Invariants

**INV-E1**: A workload unit can only be placed on nodes whose environment tag
matches a promotion policy authorizing that unit version for that environment.
Exception: `env:dev` placement requires only that the unit author matches the
node's author affinity (no promotion policy needed for dev).

**INV-E2**: Promotion policies are cumulative and non-exclusive. A promotion
to `env:prod` does not remove the unit from `env:test`. Environments are
independent placement targets, not a pipeline with mutual exclusion.

**INV-E3**: PromotionGate governance units are authoritative for auto-promote
rules within a trust domain. If no PromotionGate exists, all transitions
default to auto-promote (progressive disclosure: zero config = full auto).

## Node Capability Invariants

**INV-N1**: Node capabilities are auto-discovered at startup and cached
locally. Cached capabilities are authoritative until re-probed via
`taba refresh` or fleet-wide `refresh-capabilities` operational command.

**INV-N2**: The solver treats capabilities as hard constraints (binary
match/no-match). A workload requiring `runtime:oci` cannot be placed on a
node without `runtime:oci` or `runtime:oci-rootless`. No fallback, no
approximation.

**INV-N3**: The solver treats resources as soft constraints (ranking). Among
nodes that satisfy all capability requirements, the solver ranks by resource
availability (best-fit). Resource ranking uses fixed-point arithmetic (ppm)
for determinism (per INV-C3).

**INV-N4**: Custom tags (freeform key:value) are treated identically to
auto-discovered capabilities for solver matching purposes. The solver does
not distinguish between auto-discovered and operator-declared capabilities.

**INV-N5**: Placement-on-failure default is environment-derived: `env:dev`
defaults to leave-dead, all other environments default to auto-replace.
Per-unit `placement_on_failure` declaration overrides the environment default.

## Artifact Distribution Invariants

**INV-A1**: Every artifact referenced by a workload unit in the graph must
be identified by a SHA256 digest. The executing node verifies the digest
after fetching, before execution. Digest mismatch = reject, report to graph.

**INV-A2**: Artifact fetching order: peer cache first, then external source
(registry, URL). If peer cache has a matching digest, no external download
occurs. This is an optimization, not a security boundary — digest
verification (INV-A1) is the integrity guarantee.

## Observability Invariants

**INV-O1**: Every solver run produces a decision trail entry in the graph:
inputs (graph snapshot ID, node membership snapshot), outputs (placements,
conflicts), and the solver version. Decision trails are queryable.

**INV-O2**: Decision trail retention defaults to since-last-compaction.
Units can declare longer retention via a `decision_retention` field.
Governance units can set trust-domain-wide retention policy.

**INV-O3**: Health check semantics are progressive. If a workload unit
declares no health check, the node uses OS-level process monitoring (is the
process alive?). If a health check is declared, the node uses it. The node
never skips health monitoring — default is always active.
