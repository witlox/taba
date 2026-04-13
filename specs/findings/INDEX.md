# Findings Index

**Sweep**: Adversary specification review (pre-architect)
**Date**: 2026-04-12
**Status**: Complete — see ADVERSARY-SWEEP.md
**Resolution**: All critical and high findings addressed (2026-04-12). See DECISIONS_LOG.md DL-004 through DL-011.

## Critical (5) — RESOLVED

| ID | Title | Resolution |
|----|-------|-----------|
| F-001 | A1 scope isolation has no enforcement mechanism | INV-S8 added. Scope uniqueness enforced at role assignment. |
| F-104 | Shamir root key ceremony unspecified | DL-005. 3-tier evolving ceremony (sovra-inspired). Domain model updated. |
| F-200 | Cascading erasure reconstruction storm | INV-R1 updated with backpressure. FM-13 added. |
| F-300 | No upgrade/rollback procedures for solver version skew | FM-12 added. Version-gated solver upgrades. |
| F-301 | "Degraded mode" mentioned but never defined | Domain model: Operational Modes (Normal, Degraded, Recovery) defined. |

## High — RESOLVED

| ID | Title | Resolution |
|----|-------|-----------|
| F-002 | Policy unit orphaning race condition | INV-C5 updated: validity checked at query time, not merge time. |
| F-003 | Circular recovery dependencies | INV-K5 added: cycles fail closed, require policy. |
| F-004 | Taint propagation inconsistency | DL-007. INV-S4 updated: query-time computation. |
| F-005 | WAL-before-effect vs CRDT causal ordering | DL-008. INV-C4 updated: causal buffering with pending queue. |
| F-006 | Author scope escalation via policy embedding | INV-S8 added: units are single-authored, scope enforced per type. |
| F-007 | Conflicting policies — no resolution order | DL-006. INV-C7 added: policy uniqueness with supersession. |
| F-100 | Signature verification race window | INV-S3 updated: synchronous gate before merge. |
| F-101 | Key revocation — no cascade mechanism | INV-S3 updated: temporal validity. FM-05 updated. |
| F-102 | Taint declassification bypass | DL-007. INV-S9 added: multi-party declassification. |
| F-103 | Trust domain creation escalation | INV-S10 added: minimum 2-author threshold. DL-005 for ceremony. |
| F-105 | Gossip membership poisoning | DL-009. INV-R3 updated: signed messages, 2-witness confirmation. |
| F-106 | Signature replay attack | INV-S3 updated: signatures bind context (trust_domain, cluster, validity). |
| F-107 | Data unit purpose/consent enforcement | DL-010. INV-K2 updated: purpose as capability qualifier. |
| F-109 | Erasure shard — no post-decode verification | INV-R1 updated: re-verify all signatures after reconstruction. |
| F-111 | Solver determinism unproven assumption | DL-004. A2 resolved: fixed-point ppm. |
| F-112 | Partition — dual role-carrying units | INV-R2 updated: role-carrying units disabled on minority side. |
| F-116 | Composition order dependency | INV-C6 added: composition is order-independent. |
| F-201 | Unbounded graph growth | DL-011. INV-R6 added: memory limit, auto-compaction, sharding Phase 3+. |
| F-202 | CRDT merge pathological policy conflicts | INV-C7 added: one policy per conflict. Supersession resolves. |
| F-203 | Gossip false positive cascade | INV-R5 added: suspected nodes stay in pool. Witness confirmation. |
| F-204 | Network partition + erasure double fault | INV-R4 added: reconstruction threshold with degraded mode. |
| F-205 | Solver floating-point divergence | DL-004. A2 resolved: fixed-point ppm. |
| F-206 | WAL corruption + node failure | INV-R1 updated: post-reconstruction signature re-verification. |
| F-207 | Solver starvation from pathological compositions | Deferred to architect phase (solver timeout + complexity budget). |
| F-208 | Bootstrap cold-start seed failure | DL-005. Root key ceremony pre-graph bootstrap. INV-R6 governance replication. |
| F-210 | Policy scope overlap → role escalation | INV-S8 added: scope uniqueness enforced at assignment. |
| F-211 | Erasure re-coding storm during rolling updates | FM-12 added: version-gated upgrades. FM-13 backpressure. |
| F-214 | Governance authority loss | INV-R6 updated: governance units actively replicated, not just erasure-coded. |
| F-218 | Key compromise window | INV-S3 updated: temporal validity check. FM-05 updated. |
| F-302 | Graph compaction triggers | INV-R6 added: auto-compaction at 80%. |
| F-303 | No backpressure | FM-13 added. INV-R1 updated with backpressure. |
| F-304 | Circular policy dependencies | INV-K5 covers cycles. Deferred: max depth limit to architect phase. |
| F-305 | Trust domain bootstrap ceremony | DL-005. Root key ceremony is pre-graph bootstrap. |
| F-306 | WAL recovery after mid-recoding crash | DL-008. WAL entry types defined. Recovery logic deferred to architect. |
| F-309 | Forged unit propagation | INV-S3 updated: synchronous verification gate. FM-04 updated. |
| F-311 | No monitoring/observability spec | Deferred to architect phase (metrics spec). |
| F-317 | No cross-platform determinism tests | DL-004. FM-11 added. Testing strategy update deferred to architect. |
| F-318 | Partition + conflicting policies no BDD | INV-C7 + FM-03 updated. BDD scenario deferred to architect. |
| F-332 | No operator error recovery BDD | Deferred to architect phase (feature file creation). |
| F-336 | Missing FM: solver determinism regression | FM-11 added. |

## Medium (23)

| ID | Title | File |
|----|-------|------|
| F-008 | Data unit hierarchy constraint direction ambiguous | correctness-consistency.md |
| F-009 | Solver tiebreaker logic unspecified | correctness-consistency.md |
| F-010 | Erasure coding parameters undefined | correctness-consistency.md |
| F-011 | Gossip false positives + partition recovery interaction | correctness-consistency.md |
| F-012 | Composition graph unbounded growth — no guardrails | correctness-consistency.md |
| F-013 | Key revocation — unit validity temporal ambiguity | correctness-consistency.md |
| F-014 | Solver determinism — floating point unresolved | correctness-consistency.md |
| F-015 | Trust domain creation quorum undefined | correctness-consistency.md |
| F-108 | Policy unit self-signature bypasses multi-party | security.md |
| F-110 | Role expiry — clock-based enforcement vulnerable | security.md |
| F-113 | Gossip membership — no Merkle tree verification | security.md |
| F-114 | Provenance chain — no cryptographic integrity binding | security.md |
| F-115 | Capability type system underspecified | security.md |
| F-209 | Byzantine node poisons gossip | resilience-scalability.md |
| F-212 | Partition creates divergent policy resolutions | resilience-scalability.md |
| F-213 | Memory exhaustion during partition heal merge | resilience-scalability.md |
| F-215 | Multi-writer data unit silent data loss | resilience-scalability.md |
| F-216 | Composition deadlock in cyclic dependencies | resilience-scalability.md |
| F-217 | Data retention vs consent withdrawal — zombie state | resilience-scalability.md |
| F-219 | Taint propagation doesn't track historical classification | resilience-scalability.md |
| F-220 | Solver thundering herd after graph broadcast | resilience-scalability.md |
| F-307 | Data unit hierarchy — no maximum depth | operational-spec-gaps.md |
| F-308 | Expired roles — unit lifecycle unclear | operational-spec-gaps.md |
| F-310 | Taint propagation — multi-input under-specified | operational-spec-gaps.md |
| F-312 | Retention policy conflict escalation incomplete | operational-spec-gaps.md |
| F-313 | Key revocation mid-transaction — partial authoring | operational-spec-gaps.md |
| F-314 | Latency tolerance — no enforcement mechanism | operational-spec-gaps.md |
| F-315 | Role inheritance across trust domains unspecified | operational-spec-gaps.md |
| F-316 | Wire format version compatibility unspecified | operational-spec-gaps.md |
| F-319 | Orphaned workloads — no escalation SLO | operational-spec-gaps.md |
| F-331 | No BDD scenario for unit deletion/archival | operational-spec-gaps.md |
| F-333 | No BDD scenario for compliance audit | operational-spec-gaps.md |

## Low (4)

| ID | Title | File |
|----|-------|------|
| F-016 | Capability matching — set ordering could break determinism | correctness-consistency.md |
| F-017 | CRDT merge idempotency for signed units not proven | correctness-consistency.md |
| F-018 | Provenance chain completeness not enforced during merge | correctness-consistency.md |
| F-334 | Ubiquitous language — composition vs placement sometimes conflated | operational-spec-gaps.md |

## Analyst Adversary Pass (2026-04-13) — OPEN

Adversarial review of analyst session additions: environment progression,
logical clock, workload lifecycle, compaction, cross-domain forwarding,
progressive disclosure.

### Critical (3) — RESOLVED

| ID | Title | Resolution |
|----|-------|-----------|
| F-A300 | Lamport clock cannot verify causal revocation ordering | Replaced clock-comparison with causal revocation model (graph merge order). Grace window as fallback. INV-S3, INV-T3 updated. |
| F-A301 | Ephemeral data removal breaks provenance chain | Reference check before removal: downstream refs → tombstone, no refs → full remove. INV-D4 updated. |
| F-A302 | Spawned task signature authority undefined | Delegation token model added. Author pre-signs bounded token; node signs spawned tasks via token. INV-W4/W4a updated, domain-model updated. |

### High (5)

| ID | Title | File |
|----|-------|------|
| F-A303 | Compaction determinism breaks under wall clock skew | analyst-adversary-pass.md |
| F-A304 | Tier 0 single key is all authority — no recovery | analyst-adversary-pass.md |
| F-A305 | Stale cross-domain cache violates bilateral authorization | analyst-adversary-pass.md |
| F-A306 | Solver resource ranking non-deterministic across nodes | analyst-adversary-pass.md |
| F-A307 | Policy supersession breaks environment independence | analyst-adversary-pass.md |

### High (security-focused)

| ID | Title | File |
|----|-------|------|
| F-A308 | Bridge node has unscoped read access to both domains | analyst-adversary-pass.md |

### Medium (5)

| ID | Title | File |
|----|-------|------|
| F-A309 | Local-only data classification bypass | analyst-adversary-pass.md |
| F-A310 | Git-native versioning doesn't cover all workload sources | analyst-adversary-pass.md |
| F-A311 | Environment tags are unverified soft convention | analyst-adversary-pass.md |
| F-A312 | Spawned task declassification authority ambiguous | RESOLVED: INV-W4a prohibits governance authority via delegation tokens. Spawned tasks cannot initiate declassification. |
| F-A313 | Cross-domain forwarding bridge bottleneck | analyst-adversary-pass.md |

### Medium (operational)

| ID | Title | File |
|----|-------|------|
| F-A314 | Fleet refresh governance command has no rate limit | analyst-adversary-pass.md |

### Low (1)

| ID | Title | File |
|----|-------|------|
| F-A315 | INV-W1 language conflicts with INV-S3 on key revocation | RESOLVED: INV-W1 clarified — key revocation doesn't invalidate existing services. |

## Recurring Themes

1. **A1 enforcement gap** — F-001, F-006, F-007, F-210 (load-bearing assumption without mechanism)
2. **Solver determinism unresolved** — F-014, F-111, F-205, F-300, F-317, F-336, F-A306 (resource ranking adds non-determinism)
3. **Key/signature lifecycle** — F-013, F-100, F-101, F-106, F-218, F-313, F-A300, F-A302, F-A315 (Lamport clock + spawning + revocation language)
4. **Partition + policy interaction** — F-007, F-112, F-202, F-212, F-318 (conflicting policies during split)
5. **Operational procedures absent** — F-300, F-301, F-305, F-311, F-A314 (fleet refresh rate limit)
6. **Cascading failure amplification** — F-200, F-203, F-211, F-303 (no backpressure or circuit breakers)
7. **Dual clock model tensions** — F-A300, F-A303 (logical vs wall clock creates edge cases in revocation and compaction)
8. **Progressive disclosure security tradeoffs** — F-A304, F-A309, F-A311 (simpler defaults = weaker security properties)
9. **Cross-domain trust boundary gaps** — F-A305, F-A308, F-A313 (cache staleness, bridge read access, bottleneck)
10. **Spawn model under-specification** — F-A302, F-A312 (signature delegation, declassification authority)
