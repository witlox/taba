# Decisions Log

Decisions made during implementation. Each entry captures the decision, rationale,
alternatives considered, and what would trigger revisiting.

Format:
```
## DL-NNN: [Title]
**Date**: YYYY-MM-DD
**Phase**: Analyst | Architect | Implementer | Integrator
**Decision**: What was decided
**Rationale**: Why this option was chosen
**Alternatives**: What else was considered and why it was rejected
**Revisit if**: Conditions that would make us reconsider
```

---

## DL-001: Project name — taba
**Date**: 2026-04-11
**Phase**: Pre-analyst
**Decision**: Name the project "taba" (束, Japanese mathematical term for sheaf)
**Rationale**: Sheaf is the closest mathematical concept to the system's behavior
(local data gluing together consistently into a global whole). Japanese avoids
collision with existing "sheaf" software projects. Short, pronounceable, clean
namespace on GitHub and crates.io.
**Alternatives**: Nexus (too overloaded), tessera (taken), cuneus (Rust GPU engine),
voussoir (French), ashlar (taken), mycelium (taken in overlapping domain)
**Revisit if**: crates.io name is taken (check before first publish)

## DL-002: Language — Rust
**Date**: 2026-04-11
**Phase**: Pre-analyst
**Decision**: Implement in Rust as a cargo workspace
**Rationale**: Consistent with pact and lattice. Memory safety without GC matters
for infrastructure (latency in solver, memory predictability for erasure coding).
Go was seriously considered for ecosystem maturity and simpler dependency management.
**Alternatives**: Go (more stable ecosystem, simpler deps, but GC pauses concern
for solver/erasure hot paths). Pact/lattice integration would require FFI.
**Revisit if**: Rust dependency churn becomes unmanageable despite cargo-deny and
pinning policy. Measure actual pain over first 3 months.

## DL-003: License — Apache-2.0
**Date**: 2026-04-11
**Phase**: Pre-analyst
**Decision**: Apache-2.0 license
**Rationale**: Consistent with pact and sovra. Permissive, encourages adoption.
**Alternatives**: MPL-2.0 (file-level copyleft, protects against embrace-and-extend),
AGPL-3.0 (prevents cloud service without contributing back), BSL (delayed open source).
**Revisit if**: Cloud providers run taba-as-a-service without contributing back and
this becomes a sustainability problem.

## DL-004: Solver arithmetic — fixed-point ppm
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: All solver arithmetic uses fixed-point at ppm scale (10^6 factor,
u64/i64). No floating-point anywhere in solver scoring or placement paths.
**Rationale**: Floating-point is not deterministic across CPU architectures
(x86 vs ARM). Solver determinism (INV-C3) is load-bearing for no-masters
architecture. ppm gives 6 decimal digits — sufficient for resource ratios.
**Alternatives**: Integer-only (too restrictive for ratios), deterministic float
library like MPFR (heavy dependency, still cross-platform risk), basis points
(only 4 digits, insufficient).
**Revisit if**: ppm precision proves insufficient for some scoring scenario.
Escalate to ppb (10^9) before considering floats.

## DL-005: Shamir ceremony — evolving 3-tier (sovra-inspired)
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: Shamir root key ceremony evolves across phases. Tier 1 (basic
ceremony: start → add shares → complete with witness) in Phase 1. Tier 2
(password-protected, Argon2id-derived encryption per custodian) in Phase 3.
Tier 3 (offline two-factor: seed code + password, shares never unencrypted on
server) in Phase 5 hardening. Trait interface stable across all tiers.
**Rationale**: Adapted from sovra's production CRK management. Ed25519 keypair,
5-of-3 default. Post-reconstruction validation (derived pubkey must match).
Memory zeroing via `zeroize` crate. Ceremony state machine as Rust enum.
Bootstrap ceremony creates first trust domain + root governance unit (pre-graph).
Ceremony audit events are governance units in the composition graph.
**Alternatives**: Single-tier from start (delays Phase 1), external KMS
integration (adds dependency, defeats self-contained design).
**Revisit if**: Tier 1 is too insecure for early multi-node testing. Pull Tier 2
into Phase 3a if needed.

## DL-006: Policy conflict model — uniqueness with supersession
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: Only one non-revoked policy unit may resolve any given conflict
tuple (set of unit IDs + capability name). A second policy for the same conflict
must explicitly supersede the first. Supersession creates a versioned lineage
chain. Solver always uses the latest non-revoked version (INV-C7).
**Rationale**: Matches CRDT design (non-overlapping writes per A1). Prevents
conflicting policies during partition heals — on merge, the supersession chain
determines which policy is active. Deterministic resolution without consensus.
**Alternatives**: Policy priority by tiebreaker (arbitrary, doesn't prevent
conflicting resolution), meta-policy escalation (creates unbounded depth).
**Revisit if**: Supersession creates governance overhead for large teams. Consider
adding auto-supersession for same-author same-conflict policies.

## DL-007: Taint computation — query-time traversal
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: Taint/classification is computed at query time by traversing the
provenance graph. Not cached at merge time. Multi-input workloads inherit the
union (most restrictive) of all input classifications. Declassification requires
multi-party signing (INV-S9): minimum 2 distinct authors (policy-scoped +
data-steward-scoped).
**Rationale**: Avoids stale cached taint during eventual consistency windows.
When a declassification policy arrives, the next query reflects it immediately
on that node. No need to invalidate caches or propagate taint updates.
**Alternatives**: Merge-time taint (faster queries but stale during convergence),
hybrid (cache with invalidation, added complexity).
**Revisit if**: Query-time traversal becomes too slow for deep provenance chains.
Consider memoization with invalidation on graph mutation.

## DL-008: WAL semantics — causal buffering with pending queue
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: WAL entries are: Merged(unit) — verified and in graph;
Pending(unit, missing_refs) — verified but references not yet satisfied;
Promoted(unit_id) — pending unit activated after refs arrived. Mutations form
a partial (causal) order, not a total order. Units with unsatisfied references
are buffered in pending queue (also WAL'd) and promoted when refs arrive.
**Rationale**: Standard causal delivery pattern from CRDT literature. Handles
out-of-order gossip delivery without blocking merge or violating reference
integrity. WAL still guarantees no state mutation before persistence.
**Alternatives**: Require total ordering (blocks on out-of-order, reduces
availability), ignore references at merge time (breaks provenance invariant).
**Revisit if**: Pending queue grows unbounded. Add TTL and alert for stale
pending entries.

## DL-009: Gossip authentication — signed messages with witness confirmation
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: All gossip messages are signed with the sending node's Ed25519
identity key. Membership state changes (node declared failed) require
corroboration from at least 2 independent witness nodes. Dev mode: symmetric
HMAC with shared cluster key distributed at join.
**Rationale**: Prevents gossip poisoning by compromised nodes. Ed25519 verify
is ~70μs — negligible relative to gossip interval (~1s). Witness requirement
uses SWIM's existing indirect probe model but adds authentication. No consensus
needed — just independent corroboration.
**Alternatives**: Unsigned gossip (vulnerable to poisoning), full consensus for
membership (defeats no-masters), Merkle tree verification (additional complexity,
can add later as hardening).
**Revisit if**: Signature verification becomes bottleneck at 10K+ nodes. Consider
batch verification or hierarchical gossip.

## DL-010: Purpose as capability qualifier
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: Capabilities are tuples: (type, name, purpose?). Purpose is an
optional qualifier. Data units declare purpose in provides, workloads declare
purpose in needs. Purpose mismatch triggers a conflict requiring policy
resolution. Purpose is NOT part of taint — it is a capability constraint.
**Rationale**: Enables consent/GDPR enforcement without a separate subsystem.
Minimal change to existing capability model (optional field). Keeps taint
model focused on classification, purpose model focused on access control.
**Alternatives**: Separate consent enforcement system (adds bounded context),
purpose as part of taint (conflates classification with authorization).
**Revisit if**: Purpose matching creates too many conflicts in practice. Consider
purpose hierarchies (analytics ⊂ business-intelligence ⊂ internal-use).

## DL-011: Graph sharding — hybrid trust domain sharding (Phase 3+)
**Date**: 2026-04-12
**Phase**: Pre-architect (adversary sweep resolution)
**Decision**: Phase 1-2: no sharding, memory-bounded active graph with hard
limits and auto-compaction (INV-R6). Phase 3+: trust domain sharding — each
node holds graphs for trust domains it participates in. Cross-domain compositions
use a forwarding protocol via gossip where the requesting side queries the
providing side's graph. Policy resolution requires both sides' policies present.
**Rationale**: Trust domain boundaries are natural sharding boundaries, aligned
with security model. Avoids premature optimization in Phase 1-2. Cross-domain
protocol is a Phase 3 concern because it requires multi-node gossip.
**Alternatives**: No sharding ever (O(n) at scale, A8 risk), full sharding from
start (premature, delays Phase 1).
**Revisit if**: Single trust domain exceeds node memory after compaction. Would
need intra-domain sharding or distributed solver.
