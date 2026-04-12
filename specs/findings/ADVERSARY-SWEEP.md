# Adversary Sweep — Specification Review

**Status**: COMPLETE
**Date**: 2026-04-12
**Scope**: All specification documents, ADRs, feature files, assumptions, invariants
**Phase**: Pre-architect (specs only, no code)

## Categories Exercised

- [x] Correctness
- [x] Security
- [x] Resilience
- [x] Consistency
- [x] Scalability
- [x] Operational
- [x] Spec-Gap (BDD coverage, terminology)

## Summary

| Severity | Count |
|----------|-------|
| Critical | 5     |
| High     | 25    |
| Medium   | 23    |
| Low      | 4     |
| **Total**| **57**|

## Chunk Files

- `correctness-consistency.md` — CRDT merge, solver determinism, policy conflicts, taint propagation (18 findings)
- `security.md` — Capability bypass, replay attacks, key revocation, gossip poisoning (16 findings)
- `resilience-scalability.md` — Cascading failures, erasure storms, graph growth, partition interactions (21 findings)
- `operational-spec-gaps.md` — Day-2 ops, BDD coverage, upgrade procedures (20 findings — some overlap with above, cross-referenced)

## Critical Findings (must resolve before architect phase)

1. **F-001**: Author scope isolation (A1) has no enforcement mechanism — CRDT depends on this
2. **F-104**: Shamir root key ceremony completely unspecified — root of trust undefined
3. **F-200**: Cascading erasure reconstruction storms — can destroy cluster
4. **F-300**: No upgrade/rollback procedures for solver version skew
5. **F-301**: "Degraded mode" mentioned but never defined

## Key Themes

1. **Load-bearing assumption A1 has no enforcement** — appears in F-001, F-006, F-007, F-210
2. **Solver determinism unresolved** — appears in F-014, F-111, F-205, F-317
3. **Key/signature lifecycle gaps** — appears in F-013, F-100, F-101, F-106, F-218
4. **Partition + policy conflict interaction** — appears in F-007, F-202, F-212, F-318
5. **Operational procedures entirely absent** — appears in F-300, F-301, F-305, F-311
