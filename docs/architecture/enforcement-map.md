# Enforcement Map

Maps every invariant to the crate, trait method, runtime check, timing,
and violation response that enforces it.

Covers: INV-S1–S10, INV-S8a, INV-C1–C7, INV-K1–K5, INV-D1–D5,
INV-R1–R6, INV-E1–E3, INV-N1–N5, INV-A1–A2, INV-O1–O3, INV-T1–T3,
INV-W1–W4a, INV-G1–G5, INV-X1–X6. (59 invariants total.)

## Legend

- **Crate**: which taba crate owns the enforcement logic.
- **Trait::method**: the trait and method where the check executes.
- **Check**: what the code does at runtime.
- **When**: the lifecycle event that triggers the check.
- **On Violation**: the error variant raised and recovery action taken.

---

## Security Invariants (INV-S1 through INV-S10)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-S1**: Zero-default capabilities | taba-security | `CapabilityGuard::check_access(unit, capability)` | Verify the unit's declared capabilities include the requested capability AND an approved policy exists granting it. Empty capability set = no access. | On every capability access request (runtime, before workload I/O) | `SecurityError::CapabilityDenied`. Access blocked. No fallback. |
| **INV-S2**: Security conflicts fail closed | taba-solver | `Solver::detect_conflicts(composition)` | Scan all capability declarations in a composition for incompatible pairs. If any pair is incompatible and no policy resolves it, refuse composition. | On composition (solver evaluation) | `SolverError::ConflictDetected` / `SecurityError::SecurityConflictUnresolved`. Composition refused. Requires policy unit creation. |
| **INV-S3**: Signed units, verified before merge | taba-security, taba-graph | `SignatureVerifier::verify(unit)` called by `Graph::merge(unit)` | (a) Verify Ed25519 signature over (unit \|\| trust_domain_id \|\| cluster_id \|\| validity_window). (b) Confirm author had valid scope at creation time. (c) Confirm author's key was not revoked before unit's creation timestamp. Synchronous -- blocks merge until complete. | On merge (before unit enters graph state) | `SecurityError::SignatureInvalid`, `SecurityError::AuthorKeyRevoked`, `SecurityError::ValidityWindowExpired`. Unit rejected. Not added to graph or WAL. |
| **INV-S4**: Taint propagation | taba-core, taba-security | `TaintResolver::compute_taint(data_unit)` | Traverse provenance graph from the queried data unit back through all inputs. Compute union (most restrictive) of all input classifications. Compare against the unit's declared classification. | On query (not merge). Taint is eventually consistent across nodes. | `SecurityError::TaintWidening` if classification weaker than computed taint without policy. Query returns the computed (restrictive) classification regardless. |
| **INV-S5**: Author scope enforcement | taba-security | `ScopeValidator::validate_author_scope(author, unit)` | Check that author's (unit_type_scope, trust_domain_scope) includes the unit's type and trust domain. | On unit creation and on merge (pre-merge gate, part of INV-S3 verification) | `SecurityError::AuthorScopeExceeded`. Unit rejected. |
| **INV-S6**: Multi-party trust domain creation | taba-security | `MultiPartyValidator::validate_signers(governance_unit, required_count)` | Count distinct author signatures on the trust domain creation governance unit. Require >= 2. | On trust domain creation (before merge of governance unit) | `SecurityError::TrustDomainCreationRequiresMultiParty`. Creation refused. |
| **INV-S7**: Data hierarchy narrowing/widening | taba-core, taba-security | `ClassificationValidator::validate_hierarchy(child, parent)` | Compare child classification against parent using lattice (public < internal < confidential < PII). Narrowing (child more restrictive) always allowed. Widening (child less restrictive) requires an explicit declassification policy. | On data unit insertion (child added to parent) | `SecurityError::TaintWidening`. Child rejected unless policy exists. |
| **INV-S8**: Unique author scopes | taba-security | `ScopeValidator::check_uniqueness(author, scope, existing_assignments)` | Before persisting a role assignment governance unit, scan all existing role assignments. Reject if any other author already holds the identical (unit_type_scope, trust_domain_scope) tuple. | On role assignment creation (before merge of governance unit) | `SecurityError::ScopeDuplicate`. Role assignment rejected. |
| **INV-S9**: Multi-party declassification | taba-security | `MultiPartyValidator::validate_declassification(policy)` | Count distinct signers on the declassification policy. Require >= 2: one with policy scope, one with data-steward scope. | On declassification policy creation (before merge) | `SecurityError::DeclassificationRequiresMultiParty`. Policy rejected. |
| **INV-S10**: Multi-party trust domain (threshold) | taba-security | `MultiPartyValidator::validate_signers(governance_unit, required_count)` | Same mechanism as INV-S6. Verify the policy unit lists required signers and all required cryptographic signatures are present. | On trust domain creation (before merge) | `SecurityError::TrustDomainCreationRequiresMultiParty`. Creation refused. |

---

## Consistency Invariants (INV-C1 through INV-C7)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-C1**: Graph is single source of desired state | taba-graph, taba-core | `Graph::query(predicate)` (architectural -- no other state store exists) | Structural enforcement: no alternative desired-state store is implemented. All desired-state reads go through Graph::query. Solver reads only from graph. | Always (architectural constraint) | N/A -- violation is a design bug, not a runtime error. Code review and architecture enforcement. |
| **INV-C2**: CRDT merge properties | taba-graph | `CrdtMerge::merge(local_state, remote_state)` | Property-based tests verify commutativity (merge(A,B) == merge(B,A)), associativity, and idempotency. Runtime: merge is implemented as set union with LWW timestamps -- algebraically guarantees properties. | On every merge (gossip reconciliation, peer sync) | `GraphError::CrdtViolation`. Internal bug. Merge aborted. Alert raised. Node should not apply partial merge. |
| **INV-C3**: Solver determinism (fixed-point, no float) | taba-solver | `Solver::compute_placement(graph_state, membership)` | All arithmetic uses `Ppm`/`SignedPpm` (u64/i64 fixed-point). Tiebreaker: lexicographically lowest NodeId. Compile-time lint: `#[deny(clippy::float_arithmetic)]` on solver crate. Runtime: hash solver output and compare across nodes via gossip. | On placement computation. Cross-node verification via gossip protocol. | `SolverError::DeterminismViolation`, `SolverError::FloatingPointDetected`. Placement paused. Critical alert. |
| **INV-C4**: WAL-before-effect | taba-graph | `Wal::append(entry)` called before `Graph::apply(mutation)` | WAL entry (Merged/Pending/Promoted) is fsync'd to disk before the in-memory graph state is updated. If WAL write fails, mutation is not applied. | On every mutation (insert, promote, merge) | `GraphError::WalWriteFailed`. Mutation rejected. Node stops accepting new mutations until WAL is writable. |
| **INV-C5**: Policy references existing conflicts | taba-core | `PolicyValidator::check_references(policy)` | On query: verify that the conflict tuple referenced by the policy still exists in the graph. Orphaned policies (referencing non-existent conflicts) are flagged for archival. | On query (not merge). Validity checked lazily. | `CoreError::OrphanedPolicy`. Policy ignored in solver decisions. Eligible for archival. |
| **INV-C6**: Insertion-order independence | taba-solver, taba-graph | `Solver::compute_composition(units)` | Solver re-evaluates all affected compositions on any unit addition. Capability lists sorted lexicographically before matching (INV-K2). Property tests verify same output regardless of insertion order. | On composition (after any unit insert/merge) | `GraphError::InsertionOrderDependence`. Internal bug. Critical alert. |
| **INV-C7**: Single non-revoked policy per conflict | taba-core | `PolicyValidator::check_uniqueness(policy, conflict_tuple)` | Before accepting a new policy for a conflict tuple, check if a non-revoked policy already exists. If yes, the new policy must have a `supersedes` field referencing the existing one. Solver uses latest non-revoked version in the chain. | On policy creation/merge | `CoreError::PolicyConflict`. New policy rejected unless it explicitly supersedes existing. |

---

## Composition Invariants (INV-K1 through INV-K5)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-K1**: All capability needs satisfied | taba-solver | `Solver::match_capabilities(composition)` | For every `needs` declaration in the composition, find a matching `provides` with compatible type. Unmatched needs cause composition failure. | On composition (solver evaluation) | `SolverError::CapabilityUnsatisfied`, `CoreError::CompositionFailed`. Composition refused. |
| **INV-K2**: Typed capability matching | taba-solver | `CapabilityMatcher::matches(need, provide)` | Compare (type, name, purpose?) tuples. Type must be compatible (exact or subtype). Name must match. If purpose is declared on either side, it must match (purpose mismatch triggers conflict requiring policy). Capabilities sorted lexicographically before matching. | On composition (during capability matching) | `CoreError::CapabilityTypeMismatch`. If purpose mismatch: `SolverError::ConflictDetected`. |
| **INV-K3**: Placement respects tolerances | taba-solver | `Solver::score_placement(unit, node)` | Evaluate unit tolerance declarations (latency budget, failure mode, resource requirements) against each candidate node's reported capabilities. Node must satisfy all constraints. | On placement (after composition) | `CoreError::PlacementConstraintViolation`, `SolverError::NoViableNode`. Unit not placed. Retryable if nodes join. |
| **INV-K4**: Scaling from unit-declared parameters | taba-solver | `Solver::compute_scaling(unit)` | Scaling (min/max instances, triggers) read exclusively from unit declaration. Solver does not invent or infer scaling logic. Validate that min <= max, triggers are well-formed. | On placement and periodic re-evaluation | `CoreError::ScalingParameterInvalid`. Unit flagged. No scaling applied until fixed. |
| **INV-K5**: Cyclic recovery dependencies fail closed | taba-solver | `Solver::detect_cycles(recovery_graph)` | Build directed graph of recovery dependencies. Run cycle detection (Tarjan's or DFS). If cycle found, require explicit policy declaring restart priority. Tiebreaker without policy: lexicographically lowest UnitId gets priority. | On composition (after recovery dependency resolution) | `CoreError::CyclicRecoveryDependency`, `SolverError::CyclicDependency`. Composition refused until policy breaks cycle. |

---

## Data Invariants (INV-D1 through INV-D3)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-D1**: Unbroken provenance chain | taba-core | `ProvenanceValidator::validate_chain(data_unit)` | Every data unit produced by a workload must reference its input data units and producing workload. On query, traverse provenance links and verify all referenced units exist. On merge, units with unsatisfied references enter pending queue (causal buffering per INV-C4). | On query (full validation). On merge (reference check, causal buffering). | `CoreError::ProvenanceChainBroken`. On merge: unit enters pending state. On query: provenance gap flagged. |
| **INV-D2**: Retention enforcement | taba-core | `RetentionEnforcer::check_expiry(data_unit)` | Compare data unit's retention duration against current time. Expired units are marked eligible for compaction. Compaction is not optional. | Periodic (compaction sweep) and on query | `CoreError::RetentionExpired`. Unit eligible for compaction. Compactor removes from active graph. |
| **INV-D3**: No redundant children | taba-core | `HierarchyValidator::check_redundancy(child, parent)` | Compare child constraints against parent constraints. If identical, child is redundant and should not exist. Also enforce max hierarchy depth (16 levels). | On data unit insertion (child creation) | `CoreError::RedundantChild`, `CoreError::HierarchyDepthExceeded`. Child rejected. |

---

## Resilience Invariants (INV-R1 through INV-R6)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-R1**: Node failure does not corrupt graph | taba-erasure | `ErasureCodec::reconstruct(shards)`, `ReconstructionScheduler::schedule(priority_queue)` | Reconstruct graph shards from surviving erasure-coded shards. After reconstruction, re-verify all unit signatures (via `SignatureVerifier::verify`). Backpressure: throttled rate, prioritized by shard criticality (governance > policy > data constraints > workload). Circuit breaker when queue depth exceeds threshold. | On node failure detection (from gossip) | `ErasureError::ReconstructionFailed`, `ErasureError::ReconstructionBackpressure`, `ErasureError::SignatureVerificationPostReconstruct`. If below threshold: degraded mode. |
| **INV-R2**: Partition consistency via CRDT | taba-graph, taba-gossip | `CrdtMerge::merge(local, remote)`, `PartitionResolver::resolve_duplicates(placements)` | Both partition sides maintain graph via CRDT. On heal: merge produces correct state (INV-C2). Duplicate placements resolved by deterministic tiebreaker (lowest NodeId wins, INV-C3). Loser drains. Role-carrying units disabled on minority side. | On partition heal (gossip reconnection and merge) | Duplicate placement: loser receives drain signal. Role units on minority side: `NodeError::DegradedMode` for affected operations. |
| **INV-R3**: Gossip convergence with signed messages | taba-gossip | `GossipProtocol::verify_message(msg)`, `FailureDetector::declare_failure(node_id, witnesses)` | All gossip messages verified with sending node's Ed25519 key. Membership state changes (node declared failed) require corroboration from >= 2 independent witnesses (configurable via `ClusterConfig::witness_count`). | On every gossip message receipt. On failure declaration. | `GossipError::MessageSignatureInvalid` (message dropped). `GossipError::InsufficientWitnesses` (failure declaration deferred until witnesses met). |
| **INV-R4**: Shard reconstructability threshold | taba-erasure | `ErasureCodec::check_threshold(available_shards, k, n)` | Compute k = ceil(N * (1 - R/100)). If actual failures > floor(N - k), enter degraded mode and surface operator alert. | On node failure detection and periodically during health checks | `ErasureError::ThresholdExceeded`, `ErasureError::InsufficientShards`. System enters degraded mode. Operator alert. |
| **INV-R5**: Suspected nodes stay in placement pool | taba-gossip, taba-solver | `Solver::score_placement(unit, node)` with `NodeHealth::Unknown` handling | Suspected nodes remain in pool with health='unknown'. Solver deprioritizes them (lower score) but does not remove. Only SWIM multi-probe consensus confirms failure and triggers removal. | On placement (solver considers suspected nodes as last resort). On gossip protocol probe cycle. | No error -- behavioral: solver avoids suspected nodes when alternatives exist. `GossipError::NodeUnreachable` tracks probe failures. |
| **INV-R6**: Graph memory limit with auto-compaction | taba-graph, taba-node | `MemoryMonitor::check_usage(graph)`, `Compactor::compact(graph, target_bytes)` | Monitor active graph memory. At 80% of `ClusterConfig::graph_memory_limit_bytes`, trigger auto-compaction. At 100%, node enters degraded mode: refuses new placements. Governance units are fully replicated (not just erasure-coded). | Periodic (memory monitor interval). On every graph mutation (lightweight size check). | `GraphError::MemoryLimitExceeded`, `NodeError::DegradedMode`. At 80%: compaction runs. At 100%: new placements refused until compaction completes. |

---

## Environment & Promotion Invariants (INV-E1 through INV-E3)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-E1**: Promotion policy gates placement by env | taba-solver | `PromotionEvaluator::evaluate(unit, promotions, gates)`, `CapabilityFilter::filter(unit, nodes, promotions)` | Workload can only be placed on nodes whose env tag matches a promotion policy. Exception: env:dev uses author affinity (no promotion needed). | On placement (solver evaluation) | `SolverError::NoCapableNode` with detail "no promotion policy for env:X". Unit not placed. |
| **INV-E2**: Promotions are cumulative | taba-solver | `PromotionEvaluator::evaluate()` | Promotion to env:prod does NOT remove from env:test. Environments are independent targets. Solver evaluates each environment independently. | On placement (solver evaluation) | Structural — solver evaluates each env separately. No violation error. |
| **INV-E3**: No PromotionGate = all auto | taba-solver | `PromotionEvaluator::evaluate()` | If no PromotionGate governance unit exists, all transitions default to auto-promote. Zero config = full auto. | On promotion evaluation | Structural — default behavior when governance unit absent. |

---

## Node Capability Invariants (INV-N1 through INV-N5)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-N1**: Auto-discovery on startup, cached | taba-node | `CapabilityDiscoverer::discover()`, `CapabilityDiscoverer::refresh()` | Node probes system for capabilities at startup. Cache is authoritative until refresh. Fleet refresh via governance OperationalCommand. | On node startup. On `taba refresh`. On fleet RefreshCapabilities command. | `NodeError::CapabilityDiscoveryFailed`. Node starts with empty capability set (no workloads placed). |
| **INV-N2**: Capabilities are hard constraints | taba-solver | `CapabilityFilter::filter(unit, nodes, promotions)` | Binary match: artifact.type must match node runtime capability. No fallback. | On placement (solver capability filter) | `SolverError::NoCapableNode`. Unit not placed. |
| **INV-N3**: Resources are soft constraints | taba-solver | `ResourceRanker::rank(unit, nodes, resources)` | Among capability-matched nodes, rank by resource availability. Ppm arithmetic, versioned snapshots. | On placement (after capability filter) | No error — ranking always produces an ordering. Worst-fit node still valid if capabilities match. |
| **INV-N4**: Custom tags match like capabilities | taba-solver | `CapabilityFilter::filter()` | Freeform key:value tags checked identically to auto-discovered capabilities in solver matching. | On placement (capability filter) | Same as INV-N2 — `SolverError::NoCapableNode` if required tag not found. |
| **INV-N5**: Placement-on-failure defaults by env | taba-solver, taba-node | `Solver::handle_node_failure(unit, env)`, `Reconciler::reconcile()` | env:dev defaults to leave-dead. Other envs default to auto-replace. Per-unit `placement_on_failure` overrides. | On node failure detection (gossip → solver re-evaluation) | Behavioral — not an error. Dev workloads left, prod workloads re-placed. |

---

## Artifact Distribution Invariants (INV-A1 through INV-A2)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-A1**: SHA256 digest verification | taba-node | `ArtifactFetcher::fetch(ref, digest)` | After fetching artifact (from peer or external), compute SHA256 and compare to expected digest. Mismatch = reject, report to graph. | On artifact fetch (before workload execution) | `NodeError::ArtifactDigestMismatch`. Artifact rejected. Retry from different source. Report to graph. |
| **INV-A2**: Peer cache first | taba-node | `ArtifactFetcher::fetch()` | Check peer cache before external source. Optimization, not security boundary — INV-A1 digest verification is the integrity guarantee. | On artifact fetch | Behavioral — if peer has artifact, skip external download. |

---

## Observability Invariants (INV-O1 through INV-O3)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-O1**: Every solver run produces decision trail | taba-observe | `DecisionTrailRecorder::record(snapshot, membership, result, version)` | Node calls recorder after each solver invocation. Trail persisted to graph via WAL. | After every solver run | `ObserveError::TrailNotFound` if recording fails. Alert. Solver continues (observability failure does not block placement). |
| **INV-O2**: Trail retention since last compaction | taba-observe, taba-graph | `DecisionTrailQuery::query_by_range()`, `Compactor::compact()` | Default retention = since last compaction. Unit `decision_retention` or governance policy can extend. Compaction respects retention. | On compaction sweep and trail query | `ObserveError::TrailCompacted` if querying beyond retention. |
| **INV-O3**: Progressive health checks | taba-node | `HealthCheckOrchestrator::run_checks()` | Default: OS-level process monitoring. If HealthCheck declared: execute that instead. Node never skips monitoring. | Periodic (configurable interval per workload) | Health failure reported to graph as `HealthStatus`. Triggers failure semantics (restart, re-place). |

---

## Logical Clock Invariants (INV-T1 through INV-T3)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-T1**: Monotonic logical clock, sync on communication | taba-common | `LogicalClock::tick()`, `LogicalClock::sync(remote)` | Every system action increments. On inter-node communication: `local = max(local, remote) + 1`. | On every action and every gossip/merge message | Structural — u64 overflow at 1.8×10^19 events. No practical violation. |
| **INV-T2**: Dual clock model | taba-common | `DualClockEvent` triple recorded on every event | Logical clock for ordering, wall clock for retention/compliance. Both recorded. System chooses by operation type. | On every event | Structural — type system enforces recording both clocks. |
| **INV-T3**: Causal revocation | taba-security | `KeyManager::is_revoked_in_local_graph(author)` | Check if revocation governance unit exists in local graph. No clock comparison. Optional grace window fallback. | On unit merge (pre-merge gate) | `SecurityError::KeyRevoked`. Unit rejected. |

---

## Workload Lifecycle Invariants (INV-W1 through INV-W4a)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-W1**: Services valid indefinitely | taba-core | `UnitHeader::validity` = None | No validity window → no automatic expiry. Terminated only explicitly or by drained placement. Key revocation does NOT invalidate existing services. | Structural (type: validity is Option) | N/A — services never auto-expire. |
| **INV-W2**: Bounded tasks auto-terminate | taba-node | `Reconciler::reconcile()`, deadline check in reconciliation loop | Check three triggers: completion (exit 0), failure (retries exhausted), deadline (LC range or wall time). Any trigger → terminate. | On task exit, on periodic reconciliation (deadline check) | Task transitions to Terminated. Ephemeral data eligible for removal (INV-D4). |
| **INV-W3**: Spawn depth enforced at merge | taba-graph, taba-core | `Graph::merge()` pre-check, `SpawnContext::spawn_depth` validation | On merge: traverse spawn provenance chain, compute depth. Reject if > max (default 4). | On graph merge of spawned task unit | `GraphError::SpawnDepthExceeded`. Unit rejected. |
| **INV-W4**: Delegation token signing | taba-security | `DelegationValidator::validate(token, spawned_unit_lc)` | Verify: (a) token signature valid, (b) LC range covers spawned task, (c) spawn count ≤ max, (d) token not revoked, (e) parent active. | On graph merge of delegation-signed unit | `SecurityError::DelegationExpired`, `DelegationSpawnLimitExceeded`, `DelegationTokenForged`. Unit rejected. |
| **INV-W4a**: No governance via delegation | taba-security | `DelegationValidator::check_governance_block(token, unit_type)` | Spawned tasks cannot create policy, governance, or declassification units. Hard block at merge. | On graph merge of delegation-signed unit with policy/governance type | `SecurityError::DelegationGovernanceBlocked`. Unit rejected. |

---

## Data Lifecycle Invariants (INV-D4 through INV-D5)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-D4**: Ephemeral data reference check | taba-graph | `Compactor::remove_ephemeral(data_unit)` | On producing task termination: check downstream references. Has refs → tombstone. No refs → full remove. Governance can mandate tombstone for all. | On bounded task termination | Behavioral — no error. Reference check determines treatment. |
| **INV-D5**: Local-only requires policy for classified data | taba-core, taba-security | `RetentionValidator::validate_local_only(data_unit)` | If retention = LocalOnly and classification > Public, require explicit policy authorization. | On unit creation / validation | `CoreError::LocalOnlyRequiresPolicy`. Unit rejected without policy. |

---

## Compaction Invariants (INV-G1 through INV-G5)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-G1**: Compaction eligibility deterministic | taba-graph | `Compactor::compute_eligible(graph)` | Given same graph state, all nodes agree on eligible units. Eligibility derived from graph state only (terminated status, retention expiry, supersession). | On compaction sweep | Structural — same algorithm on same input = same output. |
| **INV-G2**: Tombstones preserve provenance | taba-graph | `Compactor::tombstone(unit)` | Tombstone retains UnitId, AuthorId, type, LC range, termination reason, references, original digest. | On compaction (tombstone creation) | Structural — Tombstone struct includes required fields. |
| **INV-G3**: Governance never compacted | taba-graph | `Compactor::is_exempt(unit)` | Skip governance units, active policies, root ceremony chain during compaction sweep. | On compaction sweep | Structural — exemption check before tombstoning. |
| **INV-G4**: Eviction ≠ compaction | taba-node | `MemoryManager::evict(unit)` | Eviction drops content locally without tombstone. Unit remains live. Content recoverable from peers. | On local memory pressure (node-specific) | Behavioral — eviction is transparent to graph state. |
| **INV-G5**: Compaction priority order | taba-graph | `Compactor::compact(graph, target_bytes)` | Priority: ephemeral → trails → terminated tasks → superseded policies → terminated services → expired data. Mirrors reconstruction priority inverse. | On compaction sweep | Structural — priority ordering in compaction algorithm. |

---

## Cross-Trust-Domain Invariants (INV-X1 through INV-X6)

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-X1**: Bilateral policy for cross-domain | taba-gossip, taba-solver | `CrossDomainGossip::validate_bilateral(query)`, `CapabilityFilter::filter()` | Before executing forwarding query, verify bilateral policy in both domains. Missing either side = fail closed. | On cross-domain forwarding query | `GossipError::BilateralPolicyMissing`. Query rejected. Composition blocked. |
| **INV-X2**: Read-only cross-domain views | taba-gossip | `CrossDomainGossip::forward_query()` returns `ForwardingResult` | Query results NOT merged into querying domain's graph. Reference by UnitId only. | On forwarding query response | Structural — ForwardingResult is a read-only type, not a mergeable unit. |
| **INV-X3**: Fail-open cache default | taba-gossip | `CrossDomainGossip::query_with_cache(cache_policy)` | Default: serve stale if bridge down. Governance override: fail closed for freshness. | On cross-domain query with bridge unavailable | Behavioral — cache served (default) or `GossipError::BridgeUnavailable` (strict mode). |
| **INV-X4**: Emergent bridge default | taba-gossip | `CrossDomainGossip::discover_bridges(target_domain)` | Any node in multiple domains is a bridge. Governance can restrict to designated only. | On bridge discovery | Behavioral — gossip queries all multi-domain nodes (or designated only). |
| **INV-X5**: Cross-domain capability via governance | taba-gossip | `CrossDomainGossip::propagate_advertisement(capability_def)` | Bridge nodes relay CrossDomainCapability governance units across boundaries. | On governance unit merge (bridge detects cross-domain advertisement) | Behavioral — bridge gossips the advertisement to other domains. |
| **INV-X6**: No bridge = unresolved capability | taba-solver, taba-gossip | `CapabilityFilter::filter()`, `CrossDomainGossip::discover_bridges()` | If no bridge to target domain, solver surfaces as unresolved. No auto-creation. | On cross-domain composition attempt | `SolverError::NoCapableNode` with detail "no bridge to domain X". Alert raised. |

---

## INV-S8a: Overlapping Scopes for Decision-Making Types

| Invariant | Crate | Trait::method | Check | When | On Violation |
|-----------|-------|---------------|-------|------|-------------|
| **INV-S8a**: Overlapping policy/governance scopes | taba-security | `ScopeValidator::check_uniqueness(author, scope, existing, unit_type)` | For policy/governance types: skip uniqueness check (overlapping permitted). For workload/data: enforce INV-S8 (strict uniqueness). | On role assignment creation | Structural — check branches on unit_type. |
