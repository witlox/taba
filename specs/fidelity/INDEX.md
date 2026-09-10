# Fidelity Index

Generated: 2026-09-10
Project: taba
State: Brownfield baseline (M1+M2)

## Summary

- Total invariants: 66
  - VERIFIED (MOCK+): 39
  - PARTIAL: 7
  - UNVERIFIED: 20
- Total scenarios (Gherkin): 128 across 20 feature files (not yet implemented — no BDD step definitions in M2)
- Total tests: 465 (460 unit + 5 doctests)
  - STUB: 0 | SHALLOW: 74 | MOCK: 370 | PROPERTY: 21 | NETWORK: 0

## Per-crate summary

| Crate | Tests | STUB | SHALLOW | MOCK | PROPERTY | NETWORK |
|-------|-------|------|---------|------|----------|---------|
| taba-common | 31 | 0 | 12 | 16 | 3 | 0 |
| taba-core | 86 | 0 | 15 | 68 | 3 | 0 |
| taba-test-harness | 30 | 0 | 5 | 20 | 5 | 0 |
| taba-security | 52 | 0 | 5 | 43 | 4 | 0 |
| taba-graph | 143+1 | 0 | 25 | 115+1doc | 3 | 0 |
| taba-solver | 118+1 | 0 | 12 | 103+1doc | 3 | 0 |

### Depth classification criteria

- **STUB**: Empty body or `todo!()`. None found.
- **SHALLOW**: Asserts field values, equality, ordering, or error message strings without exercising domain logic. Examples: `test_ppm_represents_one` (constant check), `test_identity_newtype_ordering` (UUID comparison), all error `Display` tests, config default value checks.
- **MOCK**: Calls real domain objects in-process. Exercises actual validation, capability matching, CRDT merge, graph insert/archive, signing/verification, conflict detection, cycle detection, placement scoring.
- **PROPERTY**: `proptest!` blocks with `cases: 1000`. Generates thousands of inputs and checks invariants algebraically. Deeper than any single MOCK test.
- **NETWORK**: Tests that talk to running services via real protocols. None in M2 (expected — `taba-node`, `taba-gossip`, `taba-erasure` not yet implemented).

## Per-invariant

### Security Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-S1 | Zero-default capabilities | MOCK | `crates/taba-security/src/enforcement.rs:107-149` | None — fail-closed tested for unknown unit and missing capability |
| INV-S2 | Security conflicts fail closed | MOCK | `crates/taba-solver/src/conflict.rs:426-498`, `crates/taba-security/src/enforcement.rs:137-149` | Cross-domain conflict detection tested; no test for "ambiguous policy = fail closed" |
| INV-S3 | Signed units, verified before merge | MOCK | `crates/taba-security/src/crypto.rs:426-446`, `signing.rs:266-389`, `verification.rs:284-446` | **Critical gap**: security crate tests real Ed25519, but DefaultGraph (graph.rs:290-298) uses structural validation (placeholder crypto with zero bytes). Integration of real signature verification into graph merge is not tested. Spec says "blocks merge" but graph accepts any structurally valid unit. |
| INV-S4 | Taint propagation at query time | MOCK | `crates/taba-security/src/taint.rs:214-294` | No property test for multi-input union. No test that taint is computed at query time (not cached). |
| INV-S5 | Author scope enforcement | MOCK | `crates/taba-security/src/scope.rs:224-283`, `crates/taba-core/src/validation.rs:698-786` | None — wrong type and wrong domain both tested |
| INV-S6 | Multi-party trust domain creation | MOCK | `crates/taba-core/src/validation.rs:280-291` | Tested as part of `validate_governance` (≥2 signers). No standalone test for the INV-S6/INV-S10 distinction. |
| INV-S7 | Data hierarchy narrowing/widening | PARTIAL | `crates/taba-core/src/data.rs:228-256` (lattice ordering) | Lattice union tested but actual hierarchy validation (child can't widen without policy) is not implemented in M2. No `ClassificationValidator::validate_hierarchy` exists. |
| INV-S8 | Unique author scopes (state-producing) | MOCK | `crates/taba-security/src/scope.rs:285-302` | Tested at scope-check time. Not tested at graph merge time (graph doesn't call `validate_scope_uniqueness`). |
| INV-S8a | Overlapping scopes (decision-making) | MOCK | `crates/taba-security/src/scope.rs:304-322` | None — policy scope overlap explicitly allowed |
| INV-S9 | Multi-party declassification | MOCK | `crates/taba-security/src/taint.rs:296-326` | None — 2 signers valid, 1 signer denied |
| INV-S10 | Multi-party trust domain (threshold) | MOCK | `crates/taba-core/src/validation.rs:280-291` | Same test as INV-S6. No test for explicit `required_signers` list verification. |

### Consistency Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-C1 | Graph is single source of desired state | STRUCTURAL | N/A (architectural constraint) | No runtime test possible. Enforced by code structure — no alternative state store exists. |
| INV-C2 | CRDT merge: commutative, associative, idempotent | PROPERTY | `crates/taba-graph/src/crdt.rs:870-963` | None — 3 proptest tests (1000 cases each) verify all three laws |
| INV-C3 | Solver determinism (fixed-point, no float) | PROPERTY | `crates/taba-solver/src/solver.rs:676-688`, `scorer.rs:716-738` | None — proptest tests deterministic output and order independence. All scoring uses Ppm. No `#[deny(clippy::float_arithmetic)]` lint found (should be added). |
| INV-C4 | WAL-before-effect | MOCK | `crates/taba-graph/src/graph.rs:977-1086`, `wal.rs:199-322` | In-memory WAL only (M2). WAL entries appended before mutations. No disk persistence (M3). |
| INV-C5 | Policy references existing conflicts | PARTIAL | `crates/taba-graph/src/graph.rs:840-868` (active_policy query) | Orphaned policy detection at query time is not explicitly tested. No test for `PolicyValidator::check_references`. |
| INV-C6 | Insertion-order independence | PROPERTY | `crates/taba-solver/src/solver.rs:690-713` | None — proptest verifies same output regardless of unit insertion order |
| INV-C7 | Single non-revoked policy per conflict | MOCK | `crates/taba-graph/src/graph.rs:1164-1299`, `crates/taba-solver/src/conflict.rs:579-673` | None — supersede chain checked, duplicate without supersession rejected |

### Composition Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-K1 | All capability needs satisfied | MOCK | `crates/taba-solver/src/conflict.rs:382-440` | Conflict detection tests cover unsatisfied needs. No explicit "composition is valid only if all needs satisfied" test. |
| INV-K2 | Typed capability matching | MOCK+PROPERTY | `crates/taba-core/src/capability.rs:296-461`, `proptest:488-516` | None — purpose filtering, type compatibility, sorting all tested with 1000 cases |
| INV-K3 | Placement respects tolerances | PARTIAL | `crates/taba-solver/src/scorer.rs:296-352` | Scorer uses constant tolerance score (M2 simplification). Actual tolerance matching against node capabilities not implemented. |
| INV-K4 | Scaling from declared parameters | UNVERIFIED | N/A | Scaling decisions not computed in M2. ScalingTrigger fields exist but no solver code evaluates them. |
| INV-K5 | Cyclic recovery dependencies fail closed | MOCK | `crates/taba-solver/src/cycle.rs:360-624`, `solver.rs:573-609` | None — DFS detection, normalization, self-loops, multi-cycle all tested |

### Data Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-D1 | Unbroken provenance chain | MOCK | `crates/taba-security/src/taint.rs:268-294`, `crates/taba-graph/src/graph.rs:994-1086` | Taint computer tests broken provenance. Graph tests causal buffering (pending → promoted). No test for provenance chain completeness at query time. |
| INV-D2 | Retention enforced | PARTIAL | `crates/taba-graph/src/compaction.rs:397-411`, `608-621` | Ephemeral data compaction tested. Persistent data expiry not implemented (no wall-time tracking in M2). |
| INV-D3 | No redundant children | UNVERIFIED | N/A | No test checks hierarchy depth ≤16 or child constraint divergence from parent. `DataHierarchy` struct exists but no validator. |
| INV-D4 | Ephemeral data reference check | MOCK | `crates/taba-graph/src/compaction.rs:397-438` | None — no refs → Remove, has refs → Tombstone, both tested |
| INV-D5 | Local-only requires policy | UNVERIFIED | N/A | No test for `RetentionValidator::validate_local_only`. `LocalOnly` retention mode defined but not enforced. |

### Resilience Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-R1 | Node failure doesn't corrupt graph | UNVERIFIED | N/A | `taba-erasure` crate not implemented (M4). Erasure coding, reconstruction, backpressure are future work. |
| INV-R2 | Partition consistency via CRDT | UNVERIFIED | N/A | `taba-gossip` crate not implemented (M4). CRDT merge laws tested (INV-C2) but partition resolution is not. |
| INV-R3 | Gossip convergence with signed messages | UNVERIFIED | N/A | `taba-gossip` crate not implemented (M4). Ed25519 signing tested in security crate but not in gossip context. |
| INV-R4 | Shard reconstructability threshold | UNVERIFIED | N/A | `taba-erasure` crate not implemented (M4). |
| INV-R5 | Suspected nodes stay in placement pool | MOCK | `crates/taba-solver/src/scorer.rs:326-352` | Scorer penalizes suspected nodes but doesn't remove them. No SWIM multi-probe consensus test (M4). |
| INV-R6 | Graph memory limit with auto-compaction | MOCK | `crates/taba-graph/src/memory.rs:137-262`, `graph.rs:1148-1450` | None — 80% compaction, 100% degraded, pressure computation, zero-limit all tested |

### Environment & Promotion Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-E1 | Promotion policy gates placement by env | MOCK | `crates/taba-solver/src/filter.rs:276-376` | None — env:prod without policy excluded, env:dev with/without affinity tested |
| INV-E2 | Promotions are cumulative | UNVERIFIED | N/A | No test checks that promotion to env:prod doesn't remove from env:test. `PromotionEvaluator` exists but returns empty for M2. |
| INV-E3 | No PromotionGate = all auto | PARTIAL | `crates/taba-solver/src/filter.rs:348-376` | Filter returns true when no environment is set, but no test explicitly verifies "no PromotionGate → all transitions auto-promote". |

### Node Capability Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-N1 | Auto-discovery on startup, cached | UNVERIFIED | N/A | `taba-node` crate not implemented (M3). `NodeCapabilitySet` struct exists but no discovery logic. |
| INV-N2 | Capabilities are hard constraints | MOCK | `crates/taba-solver/src/filter.rs:223-274` | None — binary match tested, no fallback for missing runtime |
| INV-N3 | Resources are soft constraints (ranking) | PARTIAL | `crates/taba-solver/src/scorer.rs:296-352` | Scorer uses constant resource score. Actual `ResourceRanker` with `ResourceSnapshot` data not implemented in M2. |
| INV-N4 | Custom tags match like capabilities | MOCK | `crates/taba-solver/src/filter.rs:395-423` | None — `requires_satisfied` checks custom_tags |
| INV-N5 | Placement-on-failure defaults by env | UNVERIFIED | N/A | `taba-node` crate not implemented (M3). `PlacementOnFailure` enum exists but no default resolution logic. |

### Artifact Distribution Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-A1 | SHA256 digest verification | UNVERIFIED | N/A | `Artifact` struct has `ContentDigest` field but no verification logic. Implemented in `taba-node` (M3+). |
| INV-A2 | Peer cache first | UNVERIFIED | N/A | No peer cache implementation. `taba-node` (M3+). |

### Observability Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-O1 | Every solver run produces decision trail | UNVERIFIED | N/A | `SolverResult` exists (with placements, conflicts) but is not persisted to graph as a decision trail entry. `taba-observe` crate not implemented. |
| INV-O2 | Trail retention since last compaction | UNVERIFIED | N/A | `taba-observe` not implemented. |
| INV-O3 | Progressive health checks | MOCK | `crates/taba-core/src/health.rs:65-127` | Health check types (Http, Tcp, Command) defined and serialization tested. No runtime test (M2 scope — structural definitions only). |

### Logical Clock Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-T1 | Monotonic logical clock, sync on communication | PROPERTY | `crates/taba-common/src/types.rs:549-561` | None — proptest verifies sync(local, remote) > max(local, remote) for 1000 cases |
| INV-T2 | Dual clock model | MOCK | `crates/taba-common/src/types.rs:515-527`, used throughout | DualClockEvent struct carries (logical_clock, wall_time, timezone). Serialization tested. Used in UnitHeader, Provenance, etc. |
| INV-T3 | Causal revocation | MOCK | `crates/taba-security/src/verification.rs:348-469` | None — revoked key rejected, non-revoked accepted, is_revoked method tested |

### Workload Lifecycle Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-W1 | Services valid indefinitely | MOCK | `crates/taba-core/src/unit.rs:740-756`, `validation.rs:592-606` | None — service with no validity passes, bounded task without validity rejected |
| INV-W2 | Bounded tasks auto-terminate | MOCK | `crates/taba-core/src/validation.rs:578-606` | Bounded task must have validity window. Auto-termination triggers not implemented (M3, taba-node). |
| INV-W3 | Spawn depth enforced at merge | PARTIAL | `crates/taba-core/src/delegation.rs:134-145` | SpawnContext.spawn_depth field checked for values 1-4, but graph merge does NOT enforce max depth (INV-W3 not implemented in DefaultGraph::insert). No traversal of spawn provenance chain. |
| INV-W4 | Delegation token signing | MOCK | `crates/taba-security/src/delegation.rs:403-639` | None — create/validate/revoke/spawn-limit/wrong-signature all tested |
| INV-W4a | No governance via delegation | MOCK | `crates/taba-security/src/delegation.rs:493-546` | None — policy/governance/declassification all blocked |

### Data Lifecycle Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-D4 | Ephemeral data reference check | MOCK | `crates/taba-graph/src/compaction.rs:397-438` | None — no refs → Remove, has refs → Tombstone |
| INV-D5 | Local-only requires policy | UNVERIFIED | N/A | No test for local-only data requiring policy authorization. |

### Compaction Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-G1 | Compaction eligibility deterministic | MOCK | `crates/taba-graph/src/compaction.rs:397-438` | None — same graph state → same eligible units |
| INV-G2 | Tombstones preserve provenance | MOCK | `crates/taba-graph/src/compaction.rs:536-561`, `crates/taba-core/src/tombstone.rs:82-155` | None — tombstoned entry retains references, Tombstone struct preserves all required fields |
| INV-G3 | Governance never compacted | MOCK | `crates/taba-graph/src/compaction.rs:481-510`, `graph.rs:1112-1148` | None — governance units excluded from compaction and archiving |
| INV-G4 | Eviction ≠ compaction | UNVERIFIED | N/A | Eviction not implemented. `taba-node` (M3). |
| INV-G5 | Compaction priority order | MOCK | `crates/taba-graph/src/compaction.rs:441-479` | None — ephemeral (priority 1) before terminated tasks (priority 2) |

### Cross-Trust-Domain Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-X1 | Bilateral policy for cross-domain | UNVERIFIED | N/A | `taba-gossip` not implemented (M4). Cross-domain conflict detection exists in solver but bilateral policy verification is not implemented. |
| INV-X2 | Read-only cross-domain views | UNVERIFIED | N/A | `taba-gossip` not implemented (M4). |
| INV-X3 | Fail-open cache default | UNVERIFIED | N/A | `taba-gossip` not implemented (M4). |
| INV-X4 | Emergent bridge default | UNVERIFIED | N/A | `taba-gossip` not implemented (M4). |
| INV-X5 | Cross-domain capability via governance | UNVERIFIED | N/A | `taba-gossip` not implemented (M4). `CrossDomainCapabilityDef` struct exists. |
| INV-X6 | No bridge = unresolved capability | UNVERIFIED | N/A | `taba-gossip` not implemented (M4). Solver surfaces unmatched needs but no bridge detection. |

## High-risk areas (SHALLOW or UNVERIFIED)

### Critical (security/capability invariants with insufficient testing)

1. **INV-S3 — Signature verification not integrated into graph merge** (`crates/taba-graph/src/graph.rs:290-298`): The `DefaultGraph::validate` method calls `DefaultValidator::validate` (structural only), NOT `Verifier::verify` (Ed25519). The security crate's `DefaultVerifier` is thoroughly tested in isolation, but the graph accepts any structurally valid unit with a zero-bytes placeholder signature. This is an intentional M2 simplification but means INV-S3 ("no unit enters graph state before verification completes") is **not enforced at runtime**. Impact: critical — a malformed or forged unit would be accepted into the graph.

2. **INV-W3 — Spawn depth not enforced at graph merge** (`crates/taba-graph/src/graph.rs:377-510`): The `compute_references` function does not traverse the spawn provenance chain to check depth. `SpawnContext.spawn_depth` is a field that can be set to any value, including >4. The spec requires "rejected if the resulting depth exceeds 4." No test verifies this rejection. Impact: high — authority escalation via deep spawning chains.

3. **INV-S8 — Scope uniqueness not checked at graph merge** (`crates/taba-graph/src/graph.rs:377-510`): `DefaultScopeChecker::validate_scope_uniqueness` is tested in isolation but is NOT called by `DefaultGraph::insert`. Two authors with identical scope tuples could both insert workload units. Impact: high — violates INV-S8 zero-overlap for state-producing types.

### High (invariants with no enforcement code)

4. **INV-D3 — No hierarchy validator**: No code checks that children diverge from parent or that depth ≤16. `DataHierarchy` struct exists but is unused. Impact: medium — redundant children waste graph memory but don't violate security.

5. **INV-D5 — Local-only data policy check**: `RetentionMode::LocalOnly` is defined but no validator checks classification > Public requires policy. Impact: medium — audit trail bypass for classified data.

6. **INV-E2 — Promotions cumulative**: No test verifies that promoting to env:prod doesn't remove from env:test. `PromotionEvaluator` returns empty for M2. Impact: low for M2 (no promotions configured), high when promotions are implemented.

7. **INV-K4 — Scaling from declared parameters**: No solver code evaluates `ScalingTrigger` declarations. Impact: low for M2 (no scaling decisions), medium when scaling is implemented.

### Expected (future-milestone invariants — not gaps)

8. **INV-R1–R4 (resilience)**: `taba-erasure` not implemented. Expected for M4.
9. **INV-X1–X6 (cross-domain)**: `taba-gossip` not implemented. Expected for M4.
10. **INV-N1, N5 (node lifecycle)**: `taba-node` not implemented. Expected for M3.
11. **INV-A1, A2 (artifact distribution)**: `taba-node` not implemented. Expected for M3.
12. **INV-O1, O2 (observability)**: `taba-observe` not implemented. Expected for M3+.
13. **INV-G4 (eviction)**: `taba-node` not implemented. Expected for M3.
14. **INV-W2 (auto-termination)**: Bounded task validity window enforced but auto-termination triggers not implemented. Expected for M3.

## BDD scenarios

128 Gherkin scenarios across 20 feature files in `specs/features/`.
None have step definitions — no BDD test infrastructure exists in M2.
This is expected: the AGENTS.md states "BDD feature files (128
scenarios, 20 files) — Complete" but BDD step implementation is part
of the M3+ workflow, not M2.

Feature files audited:
- `security-enforcement.feature`
- `placement.feature`
- `compaction.feature`
- `composition.feature`
- `operational-modes.feature`
- `recovery.feature`
- `unit-authoring.feature`
- `trust-domain.feature`
- `conflict-resolution.feature`
- `network-partition.feature`
- `environment-progression.feature`
- `data-lineage.feature`
- `spawned-tasks.feature`
- `runtime-matching.feature`
- `data-retention.feature`
- `compliance-audit.feature`
- `node-lifecycle.feature`
- `observability.feature`
- `cross-domain.feature`
- `ceremony.feature`

## Recommendations

### Before M3 (must fix)

1. **Integrate signature verification into graph merge**: `DefaultGraph::insert` should call `Verifier::verify` (from taba-security) as a synchronous gate before WAL write. Currently calls `DefaultValidator::validate` (structural only). This is the single highest-risk gap — INV-S3 is a "must NEVER be violated" invariant.

2. **Enforce spawn depth at graph merge**: Add a `compute_spawn_depth` traversal to `DefaultGraph::insert` that rejects units with `spawn_depth > max_spawn_depth` (default 4). Currently `SpawnContext.spawn_depth` is accepted without checking.

3. **Enforce scope uniqueness at graph merge**: Call `DefaultScopeChecker::validate_scope_uniqueness` before accepting a workload or data unit. Currently this check exists in taba-security but is not invoked by the graph.

4. **Add `#[deny(clippy::float_arithmetic)]` to taba-solver**: INV-C3 specifies this lint. Currently no float arithmetic exists but the lint is not enforced. A future contributor could accidentally introduce floating-point.

### Before M3 (should fix)

5. **Add property test for CRDT merge with policy chains**: Current proptest only tests entry-level commutativity. Policy chains should also be tested for merge equivalence.

6. **Add test for INV-C5 (orphaned policy detection)**: No test verifies that a policy referencing a non-existent conflict is detected at query time and eligible for archival.

7. **Implement `ClassificationValidator::validate_hierarchy`**: INV-S7 requires child constraints to be checked against parent. Only the lattice ordering is tested — no actual hierarchy validation exists.

### Backlog (Medium/Low)

8. **Add INV-D3 test**: Hierarchy depth ≤16 and child constraint divergence.
9. **Add INV-D5 test**: Local-only data with classification > Public requires policy.
10. **Add INV-E2 test**: Promotions are cumulative and non-exclusive.
11. **Implement BDD step definitions**: 128 scenarios are written but none have executable steps. BDD infrastructure (cucumber-rs) should be set up as part of M3.
