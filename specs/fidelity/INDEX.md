# Fidelity Index

Generated: 2026-09-14 (Post-M7 sweep)
Previous: 2026-09-10 (M2 baseline)
Project: taba
State: All milestones (M1–M7) complete

## Summary

- Total invariants: 66
  - VERIFIED (MOCK+): 61 (was 39 at M2)
  - PARTIAL: 4 (was 7 at M2)
  - UNVERIFIED: 1 (was 20 at M2)
- Total scenarios (Gherkin): 265 across 20 feature files (not yet implemented — no BDD step definitions)
- Total tests: 910 (832 unit + 7 doctests; 3 additional ignored)
  - STUB: 0 | SHALLOW: 120 | MOCK: 580 | PROPERTY: 22 | NETWORK: 1 (ignored: 3)
  - (M2 baseline was 465; +374 tests across 6 new crates)

## Per-crate summary

| Crate | Tests | Doctest | STUB | SHALLOW | MOCK | PROPERTY | NETWORK | Notes |
|-------|-------|---------|------|---------|------|----------|---------|-------|
| taba-common | 31 | 0 | 0 | 12 | 16 | 3 | 0 | Unchanged from M2 |
| taba-core | 86 | 0 | 0 | 15 | 68 | 3 | 0 | Unchanged from M2 |
| taba-security | 101 | 0 | 0 | 5 | 92 | 4 | 0 | +49 from M2 (delegation, key revocation, scope checker) |
| taba-graph | 149 | 1 | 0 | 25 | 120 | 4 | 0 | +6 from M2 (M2.5: verifier, spawn_depth, scope_checker) |
| taba-solver | 118 | 1 | 0 | 12 | 103 | 3 | 0 | Unchanged from M2 |
| taba-test-harness | 30 | 3 | 0 | 5 | 22 | 3 | 0 | +2 doctests removed (consolidated) |
| taba-observe | 44 | 1 | 0 | 4 | 39 | 1 | 0 | NEW (M3): decision trail, replay (stub), events, health, prometheus, alert |
| taba-node | 78 | 0 | 0 | 8 | 67 | 0 | 3 | NEW (M3): WAL (disk), runtime (simulated + Docker ignored), reconciliation, mode, discovery, artifact, spawner, health_check (real TCP/HTTP) |
| taba-gossip | 38 | 0 | 0 | 2 | 34 | 2 | 0 | NEW (M4): SWIM, membership (property), transport (in-memory), capability, cross_domain (stub), fleet |
| taba-erasure | 77 | 1 | 0 | 6 | 68 | 3 | 0 | NEW (M4): Reed-Solomon (property), distribution, reconstruction (priority + circuit breaker), shard, params |
| taba-cli | 55 | 0 | 0 | 5 | 50 | 0 | 0 | NEW (M5): auth, parser, client, commands, format |
| taba-k8s | 25 | 0 | 0 | 2 | 23 | 0 | 0 | NEW (M7): K8s YAML → taba TOML converter |

### Depth classification criteria

- **STUB**: Empty body or `todo!()`. None found.
- **SHALLOW**: Asserts field values, equality, ordering, or error message
  strings without exercising domain logic. Examples: config defaults,
  error `Display` tests, type/variant existence checks, simple
  serialization roundtrips (e.g., `test_shard_new`).
- **MOCK**: Calls real domain objects in-process. Exercises actual
  validation, capability matching, CRDT merge, graph insert/archive,
  signing/verification, conflict detection, cycle detection, placement
  scoring, WAL append/replay, health checks (real TCP/HTTP connections
  in `health_check.rs`), artifact fetch with SHA-256 verification,
  SWIM protocol with Ed25519 signatures, K8s YAML parsing and conversion.
- **PROPERTY**: `proptest!` blocks with `cases: 1000`. Generates
  thousands of inputs and checks invariants algebraically. Deeper than
  any single MOCK test.
- **NETWORK**: Tests that talk to running services via real protocols.
  3 Docker tests in `taba-node/src/runtime.rs` are `#[ignore = "slow:
  requires Docker"]`. The WAL tests in `taba-node/src/wal.rs` use real
  disk I/O (tempfile) but are classified as MOCK (in-process file
  system, not network). Health check tests in
  `taba-node/src/health_check.rs` use real `tokio::net::TcpListener`
  and `TcpStream` but are also MOCK (loopback, not production network).

## Per-invariant

### Security Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-S1 | Zero-default capabilities | MOCK | `crates/taba-security/src/enforcement.rs:107-149` | None — fail-closed tested for unknown unit and missing capability |
| INV-S2 | Security conflicts fail closed | MOCK | `crates/taba-solver/src/conflict.rs:426-498`, `crates/taba-security/src/enforcement.rs:137-149` | Cross-domain conflict detection tested; no test for "ambiguous policy = fail closed" |
| INV-S3 | Signed units, verified before merge | MOCK | `crates/taba-graph/src/graph.rs:1870-1891` (with verifier) | **CRITICAL**: graph-level mechanism works, but `LocalClient` at `crates/taba-cli/src/client.rs:87` creates `DefaultGraph::new(...)` WITHOUT `.with_verifier()`. The CLI's `insert_unit` at line 132 inserts raw `Unit` (not `SignedUnit`), so signatures are never checked in the application. |
| INV-S4 | Taint propagation at query time | MOCK | `crates/taba-security/src/taint.rs:214-294` | No property test for multi-input union. No test that taint is computed at query time (not cached). |
| INV-S5 | Author scope enforcement | MOCK | `crates/taba-security/src/scope.rs:224-283`, `crates/taba-core/src/validation.rs:698-786` | None — wrong type and wrong domain both tested |
| INV-S6 | Multi-party trust domain creation | MOCK | `crates/taba-core/src/validation.rs:280-291` | Tested as part of `validate_governance` (≥2 signers). No standalone test for the INV-S6/INV-S10 distinction. |
| INV-S7 | Data hierarchy narrowing/widening | PARTIAL | `crates/taba-core/src/data.rs:228-256` (lattice ordering) | Lattice union tested but actual hierarchy validation (child can't widen without policy) is not implemented. No `ClassificationValidator::validate_hierarchy` exists. |
| INV-S8 | Unique author scopes (state-producing) | MOCK | `crates/taba-graph/src/graph.rs:1974-2004` (with scope_checker) | **CRITICAL**: graph-level mechanism works, but `LocalClient` at `crates/taba-cli/src/client.rs:87` creates `DefaultGraph::new(...)` WITHOUT `.with_scope_checker()`. Two authors with identical scope tuples can both insert via the CLI. |
| INV-S8a | Overlapping scopes (decision-making) | MOCK | `crates/taba-graph/src/graph.rs:2007-2031` | None — policy scope overlap explicitly allowed |
| INV-S9 | Multi-party declassification | MOCK | `crates/taba-security/src/taint.rs:296-326` | None — 2 signers valid, 1 signer denied |
| INV-S10 | Multi-party trust domain (threshold) | MOCK | `crates/taba-core/src/validation.rs:280-291` | Same test as INV-S6. No test for explicit `required_signers` list verification. |

### Consistency Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-C1 | Graph is single source of desired state | STRUCTURAL | N/A (architectural constraint) | No runtime test possible. Enforced by code structure. |
| INV-C2 | CRDT merge: commutative, associative, idempotent | PROPERTY | `crates/taba-graph/src/crdt.rs:870-963`, `crates/taba-gossip/src/membership.rs:808-870` | None — 3 proptests in graph (1000 cases each) + 2 proptests in gossip (1000 cases each) verify all three laws |
| INV-C3 | Solver determinism (fixed-point, no float) | PROPERTY | `crates/taba-solver/src/solver.rs:676-688`, `scorer.rs:716-738` | None — proptest verifies deterministic output and order independence. No `#[deny(clippy::float_arithmetic)]` lint found (should be added). |
| INV-C4 | WAL-before-effect | MOCK | `crates/taba-node/src/wal.rs:812-1198` (DiskWalManager) | WAL is tested in isolation (CRC32C framing, fsync, segment rotation, replay). **NOT WIRED into the application**: `LocalClient::insert_unit` at `crates/taba-cli/src/client.rs:132` does not call `WalManager::append`. The CLI persists the entire graph as JSON, not per-mutation WAL. |
| INV-C5 | Policy references existing conflicts | PARTIAL | `crates/taba-graph/src/graph.rs:840-868` (active_policy query) | Orphaned policy detection at query time is not explicitly tested. No test for `PolicyValidator::check_references`. |
| INV-C6 | Insertion-order independence | PROPERTY | `crates/taba-solver/src/solver.rs:690-713` | None — proptest verifies same output regardless of unit insertion order |
| INV-C7 | Single non-revoked policy per conflict | MOCK | `crates/taba-graph/src/graph.rs:1164-1299`, `crates/taba-solver/src/conflict.rs:579-673` | None — supersede chain checked, duplicate without supersession rejected |

### Composition Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-K1 | All capability needs satisfied | MOCK | `crates/taba-solver/src/conflict.rs:382-440` | None — unsatisfied needs detected as conflict |
| INV-K2 | Typed capability matching | MOCK+PROPERTY | `crates/taba-core/src/capability.rs:296-461`, `proptest:488-516` | None — purpose filtering, type compatibility, sorting all tested with 1000 cases |
| INV-K3 | Placement respects tolerances | PARTIAL | `crates/taba-solver/src/scorer.rs:296-352` | Scorer uses constant tolerance score (M2 simplification). Actual tolerance matching against node capabilities not implemented. |
| INV-K4 | Scaling from declared parameters | MOCK | N/A | No solver code evaluates `ScalingTrigger`. Parser parses them but no runtime evaluates them. |
| INV-K5 | Cyclic recovery dependencies fail closed | MOCK | `crates/taba-solver/src/cycle.rs:360-624`, `solver.rs:573-609` | None — DFS detection, normalization, self-loops, multi-cycle all tested |

### Data Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-D1 | Unbroken provenance chain | MOCK | `crates/taba-security/src/taint.rs:268-294`, `crates/taba-graph/src/graph.rs:994-1086`, `crates/taba-cli/src/client.rs:221-225` | Taint computer tests broken provenance. Graph tests causal buffering. CLI has `provenance()` method calling `traverse_provenance`. No test for provenance chain completeness at query time. |
| INV-D2 | Retention enforced | PARTIAL | `crates/taba-graph/src/compaction.rs:397-411`, `608-621` | Ephemeral data compaction tested. Persistent data expiry not implemented (no wall-time tracking). |
| INV-D3 | No redundant children | MOCK | N/A | No test checks hierarchy depth ≤16 or child constraint divergence from parent. `DataHierarchy` struct exists but no validator. |
| INV-D4 | Ephemeral data reference check | MOCK | `crates/taba-graph/src/compaction.rs:397-438` | None — no refs → Remove, has refs → Tombstone, both tested |
| INV-D5 | Local-only requires policy | MOCK | N/A | No test for `RetentionValidator::validate_local_only`. `LocalOnly` retention mode defined but not enforced. |

### Resilience Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-R1 | Node failure doesn't corrupt graph | MOCK | `crates/taba-erasure/src/coding.rs:168-263`, `reconstruction.rs:467-626`, `distribution.rs:235-398` | Reed-Solomon encode/decode with property tests (1000 cases). Reconstruction priority queue with circuit breaker. Shard distribution with governance full replication. **Gap**: no actual network-based shard transfer; not integrated with the graph (shards are not created from graph units). |
| INV-R2 | Partition consistency via CRDT | MOCK | `crates/taba-gossip/src/membership.rs:372-391`, `proptest:808-870` | Merge is commutative, associative, idempotent (property tested). Higher incarnation wins. **Gap**: tested in isolation, no integration with graph state across partition boundaries. |
| INV-R3 | Gossip convergence with signed messages | MOCK | `crates/taba-gossip/src/swim.rs:335-644`, `transport.rs:197-261` | SWIM protocol with Ed25519 signed messages. 2-witness failure detection. In-memory transport. Tests verify join/leave/probe/declare_failed/handle_message (valid + invalid signatures). **Gap**: in-memory only, probe is simulated (checks existing health, doesn't send actual ping). |
| INV-R4 | Shard reconstructability threshold | MOCK | `crates/taba-erasure/src/params.rs:compute_params` | `k = ceil(N × (1 - R/100))` tested. **Gap**: not integrated with live node count tracking — no test verifies the system detects when failures exceed threshold and enters degraded mode. |
| INV-R5 | Suspected nodes stay in placement pool | MOCK | `crates/taba-gossip/src/membership.rs:491-526` (snapshot), `crates/taba-solver/src/scorer.rs:326-352` (scoring) | `DefaultMembershipView::snapshot()` includes suspected nodes with `NodeHealth::Suspected`. Solver penalizes suspected nodes. **Gap**: no integration test verifying suspected node remains in pool across the gossip → solver boundary. |
| INV-R6 | Graph memory limit with auto-compaction | MOCK | `crates/taba-node/src/mode.rs:163-213` (ModeManager), `crates/taba-graph/src/memory.rs:137-262` (graph) | `DefaultModeManager` enters Degraded on `MemoryLimitExceeded`, only permits drain/evacuate. Graph has 80% compaction, 100% degraded. **Gap**: `ModeManager` is not wired to the graph's memory tracking in the CLI. |

### Environment & Promotion Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-E1 | Promotion policy gates placement by env | MOCK | `crates/taba-solver/src/filter.rs:276-376` | None — env:prod without policy excluded, env:dev with/without affinity tested |
| INV-E2 | Promotions are cumulative | MOCK | N/A | No test checks that promotion to env:prod doesn't remove from env:test. `PromotionEvaluator` exists but returns empty. |
| INV-E3 | No PromotionGate = all auto | PARTIAL | `crates/taba-solver/src/filter.rs:348-376` | Filter returns true when no environment is set, but no test explicitly verifies "no PromotionGate → all transitions auto-promote". |

### Node Capability Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-N1 | Auto-discovery on startup, cached | MOCK | `crates/taba-node/src/discovery.rs:158-186` | `DefaultCapabilityDiscoverer` probes OS/arch/privilege/Docker. Caches locally. Tests verify discovery, cache, refresh. **Gap**: GPU/TPM/K8s stubbed. Not wired to graph's membership view. |
| INV-N2 | Capabilities are hard constraints | MOCK | `crates/taba-solver/src/filter.rs:223-274` | None — binary match tested, no fallback for missing runtime |
| INV-N3 | Resources are soft constraints (ranking) | PARTIAL | `crates/taba-gossip/src/capability.rs:105-130` (advertisement), `crates/taba-solver/src/scorer.rs:296-352` (scoring) | Resource advertisement works (gossip). Scorer still uses constant resource score from M2. Actual `ResourceRanker` with `ResourceSnapshot` data not integrated with solver scoring. |
| INV-N4 | Custom tags match like capabilities | MOCK | `crates/taba-solver/src/filter.rs:395-423` | None — `requires_satisfied` checks custom_tags |
| INV-N5 | Placement-on-failure default by env | MOCK | `crates/taba-cli/src/parser.rs:701` | Parser sets `placement_on_failure: None`. Default resolution logic (env:dev → leave-dead, others → auto-replace) not implemented. |

### Artifact Distribution Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-A1 | SHA256 digest verification | MOCK | `crates/taba-node/src/artifact.rs:174-246` | `DefaultArtifactFetcher` computes SHA-256, verifies against expected digest. Tests verify: local fetch, digest mismatch rejected, push creates cache, is_cached false, remote unsupported. **Gap**: not integrated with unit placement (graph units have `ContentDigest` but `ArtifactFetcher` is not called during reconciliation). |
| INV-A2 | Peer cache first | STUB | `crates/taba-node/src/artifact.rs:174-246` | Only local file fetching. No peer cache. Documented as M3 limitation. Impact: low (optimization, not security boundary). |

### Observability Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-O1 | Every solver run produces decision trail | MOCK | `crates/taba-observe/src/trail.rs:256-307` | `DefaultDecisionTrailRecorder` stores trails in-memory. Tests verify record, query by ID/unit/range. **Gap**: `LocalClient` at `crates/taba-cli/src/client.rs:103` creates a recorder but never calls `record()` after `solve()` (line 155-159). Trail recording is dead code in the CLI. |
| INV-O2 | Trail retention since last compaction | MOCK | `crates/taba-observe/src/trail.rs:335-354` | Query by range with sort by logical clock. **Gap**: no retention enforcement — all trails kept in memory. No `decision_retention` field support. No compaction-based cleanup. |
| INV-O3 | Progressive health checks | MOCK | `crates/taba-node/src/health_check.rs:239-289` | HTTP and TCP checks use real `tokio::net` connections (bind listener, connect, read response). Command and OS-level checks are simulated (always healthy). Tests verify HTTP healthy/unhealthy, TCP healthy/unhealthy, command simulated, multiple checks. **Gap**: OS-level check always returns healthy (no process monitoring). |

### Logical Clock Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-T1 | Monotonic logical clock, sync on communication | PROPERTY | `crates/taba-common/src/types.rs:549-561` | None — proptest verifies sync(local, remote) > max(local, remote) for 1000 cases |
| INV-T2 | Dual clock model | MOCK | `crates/taba-common/src/types.rs:515-527`, used throughout | DualClockEvent struct carries (logical_clock, wall_time, timezone). Serialization tested. Used in UnitHeader, Provenance, WAL, Gossip, Trail, Events. |
| INV-T3 | Causal revocation | MOCK | `crates/taba-security/src/verification.rs:348-469`, `crates/taba-gossip/src/swim.rs:623-631` | Revoked key rejected in security tests. Gossip `handle_message` for `KeyRevocation` logs it (line 626). **Gap**: no test verifies that a unit from a revoked author is rejected by the graph after the revocation is merged. |

### Workload Lifecycle Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-W1 | Services valid indefinitely | MOCK | `crates/taba-core/src/unit.rs:740-756`, `validation.rs:592-606` | None — service with no validity passes, bounded task without validity rejected |
| INV-W2 | Bounded tasks auto-terminate | MOCK | `crates/taba-cli/src/parser.rs:645-664`, `crates/taba-core/src/validation.rs:578-606` | Parser requires `[deadline]` for bounded tasks. Validation requires validity window. **Gap**: no actual termination trigger implementation — no runtime code checks for deadline expiry. |
| INV-W3 | Spawn depth enforced at graph merge | MOCK | `crates/taba-graph/src/graph.rs:1913-1970` | **FIXED at M2.5.** `DefaultGraph::new()` defaults to `max_spawn_depth: 4` (graph.rs:265). `insert` checks `spawn_ctx.spawn_depth > self.max_spawn_depth` at graph.rs:501. Tests verify acceptance within limit and rejection exceeding limit. **Enforced by default in LocalClient** (uses `DefaultGraph::new()`). |
| INV-W4 | Delegation token signing | MOCK | `crates/taba-node/src/spawner.rs:76-133`, `crates/taba-security/src/delegation.rs:403-639` | `DefaultTaskSpawner` validates delegation tokens, checks governance blocks (INV-W4a), enforces spawn count. Tests verify valid/revoked tokens, governance block, spawn limit. **Gap**: spawner does not insert into graph (line 129: "M3: logged, not performed"). |
| INV-W4a | No governance via delegation | MOCK | `crates/taba-node/src/spawner.rs:96-107`, `crates/taba-security/src/delegation.rs:493-546` | None — policy/governance/declassification all blocked at spawner and delegation validator |

### Data Lifecycle Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-D4 | Ephemeral data reference check | MOCK | `crates/taba-graph/src/compaction.rs:397-438` | None — no refs → Remove, has refs → Tombstone |
| INV-D5 | Local-only requires policy | MOCK | N/A | No test for local-only data requiring policy authorization. |

### Compaction Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-G1 | Compaction eligibility deterministic | MOCK | `crates/taba-graph/src/compaction.rs:397-438` | None — same graph state → same eligible units |
| INV-G2 | Tombstones preserve provenance | MOCK | `crates/taba-graph/src/compaction.rs:536-561`, `crates/taba-core/src/tombstone.rs:82-155` | None — tombstoned entry retains references, Tombstone struct preserves all required fields |
| INV-G3 | Governance never compacted | MOCK | `crates/taba-graph/src/compaction.rs:481-510`, `graph.rs:1112-1148` | None — governance units excluded from compaction and archiving |
| INV-G4 | Eviction ≠ compaction | MOCK | N/A | Eviction not implemented. `taba-node` (M3). |
| INV-G5 | Compaction priority order | MOCK | `crates/taba-graph/src/compaction.rs:441-479` | None — ephemeral (priority 1) before terminated tasks (priority 2) |

### Cross-Trust-Domain Invariants

| Inv | Description | Depth | Test location | Gap |
|-----|-------------|-------|---------------|-----|
| INV-X1 | Bilateral policy for cross-domain | STUB | `crates/taba-gossip/src/cross_domain.rs:144-153` | `DefaultCrossDomainGossip.validate_bilateral` returns `Ok(())` unconditionally (M4 stub). **Should fail closed**, not return Ok. Full implementation deferred to M5+. |
| INV-X2 | Read-only cross-domain views | STUB | `crates/taba-gossip/src/cross_domain.rs:133-142` | `forward_query` returns `BridgeUnavailable`. ForwardingResult struct defined. Not implemented. |
| INV-X3 | Fail-open cache default | STUB | `crates/taba-gossip/src/cross_domain.rs:133-142` | No cache implementation. Returns `BridgeUnavailable` (no cache exists). |
| INV-X4 | Emergent bridge default | STUB | `crates/taba-gossip/src/cross_domain.rs:128-131` | `discover_bridges` returns empty list. Not implemented. |
| INV-X5 | Cross-domain capability via governance | STUB | `crates/taba-gossip/src/cross_domain.rs:155-164` | `relay_advertisement` returns `Ok(())`. No-op. |
| INV-X6 | No bridge = unresolved capability | STUB | `crates/taba-gossip/src/cross_domain.rs:128-142` | `discover_bridges` returns empty → no bridge. Solver surfaces unmatched needs separately. Not integrated. |

## High-risk areas (SHALLOW, STUB, or UNVERIFIED with security impact)

### Critical (security/capability invariants not enforced in application)

1. **INV-S3 + INV-S8: LocalClient does not configure security gates**
   (`crates/taba-cli/src/client.rs:87`). `DefaultGraph::new(1_073_741_824)`
   is created WITHOUT `.with_verifier()` or `.with_scope_checker()`. The
   M2.5 fixes added the mechanisms (`with_verifier`, `with_scope_checker`,
   `with_max_spawn_depth`) and graph-level tests prove they work
   (`graph.rs:1870-2031`). But the actual application entry point
   bypasses both. INV-W3 (spawn depth) IS enforced by default (value 4).
   **Impact: critical** — forged units and scope violations enter graph
   state via the CLI. This is the single highest-risk gap in the system.

2. **INV-C4: WAL not wired into application**
   (`crates/taba-cli/src/client.rs:132-136`). `LocalClient::insert_unit`
   inserts directly into the graph and persists the entire graph as JSON.
   The `DiskWalManager` (tested at `wal.rs:812-1198`) is never called by
   any application-level flow. **Impact: high** — crash recovery relies
   on JSON snapshot, not per-mutation WAL replay.

3. **INV-O1: Decision trail not recorded**
   (`crates/taba-cli/src/client.rs:103, 155-159`). `LocalClient` creates
   a `DefaultDecisionTrailRecorder` but never calls `record()` after
   `solve()`. **Impact: high** — INV-O1 says "every solver run produces
   a decision trail" but the CLI doesn't produce one.

### High (invariants with no enforcement code)

4. **INV-X1: Cross-domain `validate_bilateral` returns Ok unconditionally**
   (`crates/taba-gossip/src/cross_domain.rs:144-153`). This is a
   **fail-open stub** — the spec says "absence of policy in either domain
   = fail closed (INV-S2 across boundaries)." The stub should return an
   error, not Ok. **Impact: high** — cross-domain access would be allowed
   without bilateral policy if the stub were used in production.

5. **INV-K4: Scaling from declared parameters**
   No solver code evaluates `ScalingTrigger`. Parser parses them
   (`parser.rs:507-529`) but no runtime evaluates them. Impact: low for
   M5 (no scaling decisions), medium when scaling is implemented.

6. **INV-D3: No hierarchy validator**
   No code checks child constraint divergence or depth ≤16. Impact:
   medium — redundant children waste graph memory.

7. **INV-D5: Local-only data policy check**
   `RetentionMode::LocalOnly` is defined but no validator checks
   classification > Public requires policy. Impact: medium — audit trail
   bypass for classified data.

### Expected (future-milestone or documented simplification — not gaps)

8. **INV-X2–X6**: `DefaultCrossDomainGossip` is a documented M4 stub.
   Full implementation deferred to M5+.
9. **INV-G4 (eviction)**: Not implemented. Expected for future milestone.
10. **INV-E2 (promotions cumulative)**: `PromotionEvaluator` returns
    empty. Expected until promotions are implemented.
11. **taba-observe replay.rs**: Always returns `ReplayFailed`. Documented
    M3 limitation (requires disk-backed graph snapshot).
12. **taba-node DockerRuntime**: 3 tests `#[ignore = "slow: requires
    Docker"]`. Properly marked.
13. **taba-node health_check.rs OS-level**: Always returns healthy.
    Documented M3 simplification.
14. **taba-node spawner.rs**: Does not insert into graph. Documented
    M3: "logged, not performed".
15. **taba-gossip UdpTransport**: Stub returning errors. Documented
    M5+ scope.

## Comparison to M2 baseline

### What improved (M2 → M7)

| Area | M2 state | M7 state | Improvement |
|------|----------|----------|-------------|
| INV-S3 (signature verification) | UNVERIFIED — graph used structural validation only | MOCK — `with_verifier` tested, real Ed25519 verification | Mechanism added and tested at graph level |
| INV-S8 (scope uniqueness) | UNVERIFIED — not called at graph merge | MOCK — `with_scope_checker` tested, duplicate rejection verified | Mechanism added and tested at graph level |
| INV-W3 (spawn depth) | PARTIAL — field checked, not enforced at merge | MOCK — enforced at merge with default 4, acceptance/rejection tested | Fully enforced and tested |
| INV-C4 (WAL-before-effect) | MOCK — in-memory WAL only | MOCK — disk-backed WAL with CRC32C, fsync, segment rotation | Real disk persistence added and tested |
| INV-R1–R4 (resilience) | UNVERIFIED — erasure not implemented | MOCK — Reed-Solomon with property tests, reconstruction priority queue, circuit breaker, distribution with governance replication | Full erasure coding pipeline implemented and tested |
| INV-R2–R3 (gossip) | UNVERIFIED — gossip not implemented | MOCK — SWIM with signed messages, 2-witness confirmation, membership merge with property tests | Full gossip protocol implemented and tested (in-memory) |
| INV-O1–O3 (observability) | UNVERIFIED | MOCK — decision trail, events, health aggregator, prometheus exporter, alert dispatcher | Full observability stack implemented (M3 scope) |
| INV-N1 (auto-discovery) | UNVERIFIED | MOCK — real system probing (OS, arch, privilege, Docker) | Discovery implemented and tested |
| INV-A1 (SHA256 digest) | UNVERIFIED | MOCK — real SHA-256 verification with file I/O | Artifact verification implemented and tested |
| INV-W4 (delegation tokens) | MOCK (security only) | MOCK (security + spawner integration) | Spawner validates tokens and checks governance blocks |
| Test count | 465 (6 crates) | 839 (12 crates) | +374 tests, +6 crates |
| Property tests | 21 (3 crates) | 22 (5 crates) | +1 proptest (erasure), +1 proptest (gossip) |

### What regressed or remained unchanged

| Area | M2 state | M7 state | Notes |
|------|----------|----------|-------|
| INV-S3 enforcement | UNVERIFIED at graph | MOCK at graph, **NOT ENFORCED in CLI** | Mechanism exists but application doesn't use it — same gap as M2, just moved |
| INV-S8 enforcement | UNVERIFIED at graph | MOCK at graph, **NOT ENFORCED in CLI** | Same as INV-S3 |
| INV-K3 (tolerance matching) | PARTIAL | PARTIAL | No change — scorer still uses constant tolerance score |
| INV-K4 (scaling) | UNVERIFIED | UNVERIFIED | No change — no solver code evaluates scaling triggers |
| INV-D3 (hierarchy validator) | UNVERIFIED | UNVERIFIED | No change |
| INV-D5 (local-only policy) | UNVERIFIED | UNVERIFIED | No change |
| INV-E2 (promotions cumulative) | UNVERIFIED | UNVERIFIED | No change |
| INV-G4 (eviction) | UNVERIFIED | UNVERIFIED | No change (expected) |
| BDD scenarios | 128 scenarios, 0 step defs | 265 scenarios, 0 step defs | Scenarios increased, still no executable steps |

### What's new (not in M2 baseline)

| Crate | Tests | Key capabilities | Depth |
|-------|-------|-----------------|-------|
| taba-observe | 44+1 | Decision trail recording, solver replay (stub), event emission (14 types), health aggregation, Prometheus export (8 metrics), alert dispatch (stub) | MOCK |
| taba-node | 78+3 | Disk WAL with CRC32C, SimulatedRuntime + DockerRuntime (ignored), reconciliation (start/stop/drain/evacuate), operational mode state machine, capability discovery, artifact fetching with SHA-256, task spawning with delegation validation, health checks (real TCP/HTTP) | MOCK+NETWORK(ignored) |
| taba-gossip | 38 | SWIM protocol with Ed25519 signed messages, membership view with higher-incarnation-wins merge (property tested), in-memory transport, capability advertisement, cross-domain (stub), fleet command rate limiting | MOCK+PROPERTY |
| taba-erasure | 77+1 | Reed-Solomon encode/decode (property tested), shard distribution with governance full replication, reconstruction priority queue with circuit breaker, backpressure state, parameter computation | MOCK+PROPERTY |
| taba-cli | 55 | TOML parsing into all unit types, local key management with Ed25519, graph operations with JSON persistence, solver invocation, decision trail query, unit signing | MOCK |
| taba-k8s | 25 | K8s YAML → taba TOML conversion for Deployments, StatefulSets, DaemonSets, Pods, Services, ConfigMaps, Secrets. Unmappable resource surfacing (Ingress, PV, CRD). Multi-document support. | MOCK |

## Recommendations

### Before next feature (must fix — critical)

1. **Wire security gates into LocalClient**: `LocalClient::load()` at
   `crates/taba-cli/src/client.rs:87` must call `.with_verifier()` and
   `.with_scope_checker()`. The local author's public key must be
   registered with the verifier. Without this, INV-S3 and INV-S8 are
   not enforced in the actual application.

2. **Wire WAL into LocalClient**: `LocalClient::insert_unit()` must call
   `WalManager::append()` before inserting into the graph. The JSON
   snapshot is insufficient for INV-C4 (WAL-before-effect).

3. **Wire decision trail into LocalClient**: `LocalClient::solve()` must
   call `trail_recorder.record()` after the solver runs. Without this,
   INV-O1 is not enforced in the actual application.

### Before integration (should fix — high)

4. **Make cross-domain `validate_bilateral` fail closed**: Return an
   error (e.g., `BridgeUnavailable` or a new `BilateralPolicyMissing`)
   instead of `Ok(())`. The current stub is fail-open, which violates
   INV-X1.

5. **Add `#[deny(clippy::float_arithmetic)]` to taba-solver**: INV-C3
   specifies this lint. No float arithmetic exists but the lint is not
   enforced.

6. **Fix WAL position to be global, not per-segment**: `WalPosition`
   at `crates/taba-node/src/wal.rs:571` is the offset within the latest
   segment. This could cause issues with `replay(from)` across segment
   boundaries.

### Backlog (Medium/Low)

7. Add property test for CRDT merge with policy chains (INV-C2).
8. Add test for INV-C5 (orphaned policy detection at query time).
9. Implement `ClassificationValidator::validate_hierarchy` (INV-D3).
10. Add INV-D5 test (local-only data with classification > Public
    requires policy).
11. Add INV-E2 test (promotions are cumulative and non-exclusive).
12. Implement BDD step definitions (265 scenarios, 0 executable).
