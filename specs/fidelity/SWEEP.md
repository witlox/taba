# Fidelity Sweep Plan

Generated: 2026-09-10
Project: taba
State: Brownfield baseline (M1+M2)
Auditor: auditor role (zai-org/GLM-5.2)

## Scope

Full baseline audit across all 6 workspace crates, covering all 66
invariants from `specs/invariants.md` and all test modules.

## Crates audited

| Crate | Path | Modules audited | Tests (unit) | Doctests | Status |
|-------|------|-----------------|-------------|----------|--------|
| taba-common | `crates/taba-common/src/` | types.rs, config.rs, error.rs | 31 | 0 | COMPLETE |
| taba-core | `crates/taba-core/src/` | unit.rs, capability.rs, contract.rs, artifact.rs, health.rs, delegation.rs, data.rs, tombstone.rs, node_capability.rs, validation.rs, store.rs | 86 | 0 | COMPLETE |
| taba-test-harness | `crates/taba-test-harness/src/` | builder.rs, store.rs, arbitrary.rs | 30 | 3 | COMPLETE |
| taba-security | `crates/taba-security/src/` | crypto.rs, signing.rs, verification.rs, scope.rs, enforcement.rs, taint.rs, delegation.rs, ceremony.rs, error.rs | 52 | 0 | COMPLETE |
| taba-graph | `crates/taba-graph/src/` | crdt.rs, entry.rs, graph.rs, query.rs, merge.rs, snapshot.rs, wal.rs, compaction.rs, memory.rs, error.rs | 143 | 1 | COMPLETE |
| taba-solver | `crates/taba-solver/src/` | solver.rs, conflict.rs, scorer.rs, cycle.rs, filter.rs, resource.rs, promotion.rs, membership.rs, placement.rs, error.rs | 118 | 1 | COMPLETE |

## Audit chunks

Each chunk was read in full — every line of production code and every
line of test code. Assertions were individually evaluated for
falsifiability (would the assertion fail if the code were wrong?).

### Chunk 1: taba-common (types, config, error)
- **Audited**: `types.rs` (582 lines), `config.rs` (251 lines), `error.rs` (88 lines)
- **Key findings**: Ppm arithmetic uses u128 intermediates (no float, correct for INV-C3). LogicalClock sync uses max+1 (correct for INV-T1). Serialization roundtrips are real serde but SHALLOW for simple newtypes.
- **Property tests**: 3 (clock sync monotonic, ppm commutative, serialization roundtrip) — 1000 cases each.

### Chunk 2: taba-core (unit, capability, contract, artifact, health, delegation, data, tombstone, node_capability, validation, store)
- **Audited**: 11 modules, ~2600 lines total
- **Key findings**: DefaultValidator enforces INV-S5 (author scope), INV-W2 (bounded task validity), INV-K2 (sorted capabilities), self-references in recovery. CapabilityMatcher implements typed matching per INV-K2 with `postgres-compatible` support. Classification lattice (INV-S4) uses `max()` for union. All serialization roundtrips pass.
- **Property tests**: 3 (workload serialization, capability sorting deterministic, capability matching deterministic) — 1000 cases each.

### Chunk 3: taba-test-harness (builder, store, arbitrary)
- **Audited**: 3 modules, ~1600 lines total
- **Key findings**: WorkloadUnitBuilder produces units that pass DefaultValidator (verified by test). DataUnitBuilder and PolicyUnitBuilder likewise. InMemoryUnitStore implements UnitStore trait correctly (insert/get/archive/list). Arbitrary strategies generate valid units (verified by proptest with 1000 cases).
- **Property tests**: 5 (arbitrary capability, arbitrary workload unit valid, arbitrary unit id, arbitrary logical clock, arbitrary classification) — 1000 cases each.

### Chunk 4: taba-security (crypto, signing, verification, scope, enforcement, taint, delegation, ceremony, error)
- **Audited**: 9 modules, ~3000 lines total
- **Key findings**: Real Ed25519 sign/verify via ed25519-dalek. SigningKey does not implement Clone/Serialize (correct — private key protection). SignatureContext binds trust_domain + cluster + validity_window (INV-S3). DefaultVerifier checks signature + revocation (causal model, INV-T3). ScopeChecker enforces INV-S5, INV-S8/S8a. CapabilityEnforcer enforces INV-S1 (zero default, fail closed). TaintComputer traverses provenance graph (INV-S4). DelegationValidator enforces INV-W4/W4a. CeremonyManager is correctly stubbed (returns CeremonyError, M6 scope).
- **Property tests**: 4 (key id deterministic, sign-verify raw roundtrip, sign-verify unit roundtrip, signature different for different contexts) — 1000 cases each.
- **Gap**: The graph (DefaultGraph) uses structural validation only (placeholder crypto with zero bytes), NOT real Ed25519 verification. The security crate's verification is tested in isolation but not integrated with the graph in M2.

### Chunk 5: taba-graph (crdt, entry, graph, query, merge, snapshot, wal, compaction, memory, error)
- **Audited**: 10 modules, ~5000 lines total
- **Key findings**: CompositionGraphData is a δ-state CRDT with BTreeMap entries (ordered, deterministic). merge_delta is commutative/associative/idempotent (verified by 3 proptest tests, 1000 cases each). Graph implements insert with reference computation and causal buffering (pending queue). WAL records Merged/Pending/Promoted entries (in-memory for M2). MemoryMonitor enforces INV-R6 (80% compaction, 100% degraded). Compactor implements INV-G1 (deterministic eligibility), INV-G2 (tombstones preserve references), INV-G3 (governance exempt), INV-G5 (priority order). Supersede enforces INV-C7 (must reference old policy).
- **Property tests**: 3 (CRDT commutative, CRDT idempotent, CRDT associative) — 1000 cases each.
- **1 doctest**: GraphSnapshot::is_current

### Chunk 6: taba-solver (solver, conflict, scorer, cycle, filter, resource, promotion, membership, placement, error)
- **Audited**: 10 modules, ~3500 lines total
- **Key findings**: DefaultSolver is deterministic (BTreeMap iteration, sorted output). ConflictDetector detects missing capabilities, purpose mismatches, cross-domain without policy (INV-S2). CycleDetector uses DFS with normalization to lexicographically smallest UnitId (INV-C3, INV-K5). DefaultCapabilityFilter enforces INV-N2 (hard constraints), INV-E1 (environment matching). DefaultPlacementScorer uses Ppm arithmetic (no float, INV-C3), applies suspected penalty (INV-R5), tiebreaker by lowest NodeId.
- **Property tests**: 3 (solve deterministic, solve order independent, rank nodes deterministic) — 1000 cases each.
- **1 doctest**: MembershipSnapshot::single_node

## Summary

All 6 crates audited in full. 460 unit tests + 5 doctests = 465 total
test artifacts. All tests pass. 21 property tests across 5 crates,
each running 1000 cases. Zero STUB tests. Zero NETWORK tests (expected
for M2 — no network services implemented yet).

Status: **COMPLETE**
