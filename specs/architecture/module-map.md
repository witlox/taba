# Module Map

Canonical reference for what each crate owns, exports, and explicitly does not
own. Every boundary is load-bearing: crossing a boundary without going through
the documented API surface is a defect.

---

## taba-common

### Responsibilities
Foundation types and infrastructure shared by every other crate. Owns identity
types, configuration parsing, protobuf codegen, and tracing initialization.
No domain logic.

### Public API surface
**Types**: `NodeId`, `UnitId`, `AuthorId`, `TrustDomainId`, `ClusterId`,
`PolicyId`, `DelegationTokenId`, `Timestamp`, `ValidityWindow`,
`Ppm` (fixed-point ppm wrapper, u64)

**Logical clock**: `LogicalClock` (u64, monotonically increasing),
`DualClockEvent` (logical_clock, wall_time, timezone),
`clock_advance(local, remote) -> LogicalClock` (max(local, remote) + 1)

**Clock capability**: `ClockQuality` (Ntp | Ptp | Gps | Unsync),
`Timezone` (IANA string)

**Config**: `TabaConfig` (TOML-backed), `NodeConfig`, `ErasureConfig`,
`GossipConfig`, `SolverConfig`, `ObserveConfig`, `ArtifactConfig`

**Proto**: generated prost structs for all wire types, tonic service stubs

**Tracing**: `init_tracing()`, span helpers

**Error foundation**: `CommonError` enum (serialization, config, I/O)

### Internal modules
- `types` -- identity newtypes, `Ppm` arithmetic
- `config` -- TOML deserialization, validation, defaults
- `proto` -- prost/tonic build output
- `tracing` -- subscriber setup, span macros

### Does NOT own
- Domain semantics of any type (units, capabilities, policies)
- Crypto (no signing, no key material)
- Any network I/O

### Traces to
- A2 (ppm type lives here, arithmetic helpers)
- A7 (config is TOML, human-readable)
- INV-T1 (logical clock monotonic, sync on communication)
- INV-T2 (dual clock model: logical for ordering, wall for retention)

---

## taba-core

### Responsibilities
The unit type system. Owns the definition of what a unit is, what capabilities
are, how contracts are structured, and validation of well-formedness. This is
the domain model in code. Provides the capability matching algorithm.

### Public API surface
**Unit types**: `Unit`, `WorkloadUnit`, `DataUnit`, `PolicyUnit`,
`GovernanceUnit`, `GovernanceSubtype` (TrustDomain | RoleAssignment |
Certification | OperationalCommand | PromotionGate | CrossDomainCapability)

**Unit lifecycle**: `UnitState` (Declared, Composed, Placed, Running,
Draining, Terminated)

**Workload subtypes**: `WorkloadKind` (Service | BoundedTask),
`BoundedTaskTermination` (Completed | Failed | DeadlineExceeded),
`ValidityWindow` (optional: lc_range, wall_time_deadline)

**Artifact model**: `Artifact` (type_, ref_, digest, requires),
`ArtifactType` (Oci | Native | Wasm | K8sManifest)

**Delegation tokens**: `DelegationToken` (service_id, node_id, trust_domain,
valid_lc_range, max_spawns, author_sig), `DelegationTokenId`

**Node capabilities**: `NodeCapabilitySet` (hardware, os, privilege, runtimes,
network, storage, environment, author_affinity, custom_tags),
`RuntimeCapability` (Oci | OciRootless | K8s | Wasm | Native),
`EnvironmentTag` (Dev | Test | Prod | Custom(String)),
`PrivilegeLevel` (Root | User)

**Promotion**: `PromotionPolicy` (unit_ref, version, environment, rationale),
`PromotionGate` (transitions: Vec<PromotionTransition>),
`PromotionTransition` (from_env, to_env, mode: Auto | HumanApproval)

**Tombstone**: `Tombstone` (unit_id, author_id, unit_type, created_at_lc,
terminated_at_lc, termination_reason, references, original_digest)

**Health checks**: `HealthCheck` (type_: OsProcess | Http | Tcp | Command,
endpoint, interval, timeout)

**Capabilities**: `Capability` (type, name, purpose?), `CapabilityList`
(sorted, deterministic), `NeedsDeclare`, `ProvidesDeclare`

**Contracts**: `Tolerates`, `Trusts`, `ScalingParams`, `FailureSemantics`,
`RecoveryRelationship`, `StateRecovery`, `PlacementOnFailure` (Replace |
LeaveDead, default from environment)

**Data model**: `DataClassification` (Public < Internal < Confidential < PII),
`Provenance`, `Retention`, `ConsentScope`, `DataHierarchy`

**Policy model**: `ConflictTuple` (set of UnitIds + capability name),
`Resolution` (Allow | Deny | Conditional), `SupersessionChain`

**Validation**: `validate_unit() -> Result<ValidatedUnit, ValidationError>`

**Matching**: `match_capabilities(needs, provides) -> MatchResult`,
`CapabilityMatcher` trait

**Errors**: `CoreError` (InvalidUnit | MalformedCapability |
MatchAmbiguous | HierarchyDepthExceeded)

### Internal modules
- `unit` -- type definitions, state machine, workload subtypes (Service/BoundedTask)
- `capability` -- capability tuples, sorted lists, matching
- `contract` -- tolerance, trust, scaling, failure, recovery, placement-on-failure
- `data` -- classification lattice, provenance, retention (persistent/ephemeral/
  local-only), hierarchy (max depth 16)
- `policy` -- conflict tuples, resolution, supersession, promotion policies
- `governance` -- trust domain, role assignment, certification, operational command,
  promotion gate, cross-domain capability advertisement subtypes
- `validation` -- well-formedness checks, cross-field consistency, spawn depth
- `artifact` -- artifact type, ref, digest, requires
- `delegation` -- delegation token type, scope, lifetime
- `tombstone` -- tombstone type, reference preservation
- `node_capability` -- capability set, runtime capabilities, environment tags,
  privilege level, custom tags
- `health` -- health check declaration types

### Does NOT own
- Signing or verification (that is taba-security)
- Graph storage or merge (that is taba-graph)
- Placement or composition resolution (that is taba-solver)
- Serialization format decisions beyond derive macros (wire format is proto in taba-common)
- Delegation token validation (that is taba-security)

### Traces to
- Domain model (all entity definitions)
- INV-K1, INV-K2 (capability matching)
- INV-K3, INV-K4 (tolerance/scaling declarations)
- INV-S7 (classification lattice ordering, hierarchy narrowing/widening)
- INV-D3 (hierarchy depth limit)
- INV-D4, INV-D5 (ephemeral/local-only data lifecycle)
- INV-E1, INV-E2, INV-E3 (environment progression)
- INV-N2, INV-N4 (capability matching, custom tags)
- INV-W1, INV-W2, INV-W3 (service vs bounded task lifecycle, spawn depth)
- INV-W4 (delegation token structure)
- DL-010 (purpose as optional qualifier)

---

## taba-security

### Responsibilities
All cryptographic operations and security policy enforcement. Owns Ed25519
signing/verification, capability enforcement (declared vs allowed), taint
computation via provenance traversal, Shamir root key ceremony (Tier 0
solo through Tier 3), delegation token validation, and key revocation
lifecycle (causal model).

Cross-cutting: every other crate calls into taba-security for verification,
signing, or enforcement.

### Public API surface
**Signing**: `Signer` trait, `sign_unit(key, unit, context) -> SignedUnit`,
`SigningContext` (trust_domain_id, cluster_id, logical_clock, validity_window?)

**Verification**: `Verifier` trait,
`verify_unit(signed_unit, context) -> Result<(), VerificationError>`,
`verify_signature(sig, pubkey, payload) -> Result<(), CryptoError>`

**Delegation**: `DelegationValidator` trait,
`validate_delegation(token, spawned_unit) -> Result<(), DelegationError>`,
`create_delegation_token(author_key, service_id, node_id, trust_domain, lc_range, max_spawns) -> DelegationToken`,
`revoke_delegation(token_id) -> Result<(), DelegationError>`

**Scope enforcement**: `ScopeChecker` trait,
`check_author_scope(author, unit_type, trust_domain) -> Result<(), ScopeError>`,
`validate_scope_uniqueness(assignments, unit_type) -> Result<(), ScopeConflict>`
(INV-S8: unique for workload/data, INV-S8a: overlapping OK for policy/governance)

**Capability enforcement**: `CapabilityEnforcer` trait,
`enforce(unit, granted_capabilities) -> Result<(), EnforcementError>`

**Taint**: `TaintComputer` trait,
`compute_taint(unit_id, graph) -> DataClassification` (query-time traversal),
`check_declassification(policy) -> Result<(), DeclassError>` (requires
multi-party per INV-S9)

**Ceremony**: `CeremonyManager` trait, `CeremonyState` (Started |
CollectingShares | Complete), `CeremonyTier` (Tier0 | Tier1 | Tier2 | Tier3),
`solo_init() -> Result<(KeyPair, TrustDomain, GovernanceUnit), CeremonyError>`
(Tier 0: single command, no Shamir)

**Revocation** (causal model per INV-S3):
`is_key_revoked(author_id, local_graph) -> bool` (checks if revocation
governance unit is merged into local graph — not clock comparison),
`RevocationGovernanceUnit`,
`check_with_grace(author_id, creation_lc, revocation_lc, grace_window) -> bool`
(optional fallback)

**Key management**: `KeyPair` (Ed25519), `PublicKey`, `SecretKey` (zeroize on drop)

**Errors**: `SecurityError` (InvalidSignature | ScopeViolation | KeyRevoked |
ContextMismatch | TaintViolation | CeremonyFailed | DeclassificationDenied |
DelegationExpired | DelegationScopeViolation | DelegationSpawnLimitExceeded |
DelegationGovernanceBlocked | DelegationTokenForged)

### Internal modules
- `crypto` -- Ed25519 operations, key generation, zeroize
- `signing` -- unit signing with bound context
- `verification` -- signature verification, causal revocation check (key
  revoked in local graph? per INV-S3)
- `scope` -- author scope checking, type-dependent uniqueness (INV-S8/S8a)
- `delegation` -- delegation token creation, validation, revocation,
  governance authority block (INV-W4a)
- `enforcement` -- runtime capability enforcement
- `taint` -- provenance graph traversal, classification union for multi-input,
  declassification multi-party check
- `ceremony` -- Shamir split/reconstruct, ceremony state machine, witness
  tracking, tier-specific logic
- `revocation` -- revocation event creation, propagation via priority gossip
  (interface only; gossip transport is taba-gossip)

### Does NOT own
- The graph data structure (traverses it for taint, does not store it)
- Gossip transport (publishes revocation events; transport is taba-gossip)
- Policy semantics (checks signatures on policies, does not interpret them)
- TPM attestation (Phase 5, feature-gated, not in initial scope)

### Traces to
- INV-S1 (zero-default capability enforcement)
- INV-S2 (fail-closed on conflicts -- enforcement side)
- INV-S3 (signature binding, causal revocation model, grace window)
- INV-S4 (taint: query-time traversal, union of inputs)
- INV-S5 (author scope enforcement)
- INV-S8, INV-S8a (scope uniqueness: strict for workload/data, relaxed for policy/governance)
- INV-S9 (multi-party declassification)
- INV-S10 (multi-party trust domain creation signing)
- INV-W4, INV-W4a (delegation tokens: validation, governance authority block)
- DL-005 (ceremony tiers: Tier 0 solo through Tier 3)
- DL-007 (query-time taint)
- FM-04 (compromised node), FM-05 (compromised author)
- FM-18 (role succession, break-glass via root key)

---

## taba-graph

### Responsibilities
The CRDT composition graph. Owns the distributed data structure that IS the
desired state. Manages merge semantics (commutative, associative, idempotent),
signature verification as a synchronous gate before merge, causal buffering
(pending queue for units with unsatisfied references), provenance chain
tracking, hierarchical data unit containment, and WAL integration for local
persistence.

### Public API surface
**Graph**: `CompositionGraph` trait, `GraphSnapshot` (immutable, for solver)

**Operations**: `insert_unit(signed_unit) -> Result<InsertResult, GraphError>`,
`query_units(filter) -> Vec<Unit>`, `query_provenance(unit_id) -> ProvenanceChain`,
`supersede_policy(old_id, new_policy) -> Result<(), GraphError>`,
`archive_subgraph(filter) -> Result<ArchivedSubgraph, GraphError>`

**Insert result**: `InsertResult` (Merged | Pending { missing_refs })

**Merge**: `merge(local, remote) -> CompositionGraph` (CRDT merge),
`MergeOutcome` (units added, conflicts surfaced)

**Causal buffering**: `PendingQueue`, `promote(unit_id) -> Result<(), GraphError>`

**WAL**: `WalEntry` (Merged(unit) | Pending(unit, missing_refs) |
Promoted(unit_id)), `Wal` trait, `WalWriter`, `WalReader`

**Provenance**: `ProvenanceChain`, `ProvenanceLink` (producer, inputs, timestamp, policies)

**Compaction**: `CompactionPolicy`, `compact(policy) -> CompactionResult`

**Errors**: `GraphError` (SignatureInvalid | ScopeViolation | KeyRevoked |
ContextMismatch | WalFailed | CompactionFailed | HierarchyViolation)

### Internal modules
- `crdt` -- graph CRDT implementation, merge algorithm, idempotency checks
- `merge` -- merge entry point, signature verification gate (calls taba-security),
  causal reference resolution
- `pending` -- pending queue, reference tracking, promotion logic
- `provenance` -- chain construction, query
- `hierarchy` -- data unit parent-child relationships, constraint inheritance
- `wal` -- WAL entry types, write-ahead semantics, replay on startup
- `compaction` -- retention-driven cleanup, auto-compaction at 80% memory,
  archival of cold subgraphs
- `snapshot` -- immutable graph snapshot for solver consumption
- `query` -- unit filtering, policy validity check at query time (INV-C5)

### Bootstrap special case
The initial governance unit from the Shamir ceremony (CeremonyCompleted) is
inserted without normal signature verification — it is trusted by the ceremony
protocol itself. This is the only unit that bypasses the verification gate.
The ceremony produces a signed root governance unit; the graph accepts it via
a dedicated `insert_bootstrap(unit)` method that requires the ceremony's
public key as a parameter (not the normal author key flow).

### Governance unit replication
Governance units are actively replicated (full copies on N nodes), not just
erasure-coded. taba-graph decides which units are governance (by type) and
signals this to taba-erasure via the shard distribution interface.

### Does NOT own
- Signature computation (delegates to taba-security; owns the gate that calls it)
- Composition resolution or placement (that is taba-solver)
- Erasure coding of shards (that is taba-erasure)
- Gossip transport of graph deltas (that is taba-gossip)
- Solver determinism (graph provides snapshots; solver owns determinism)

### Traces to
- INV-C1 (graph is single source of desired state)
- INV-C2 (merge: commutative, associative, idempotent)
- INV-C4 (WAL-before-effect, causal buffering, entry types)
- INV-C5 (orphaned policy detection at query time)
- INV-D1 (provenance chain unbroken, causal buffering for pending refs)
- INV-D2 (retention enforcement via compaction)
- INV-D3 (hierarchy depth enforcement)
- INV-R6 (memory limit, auto-compaction, governance replication)
- INV-S3 (synchronous signature verification gate before merge)
- DL-008 (WAL entry types: Merged, Pending, Promoted)
- DL-011 (memory-bounded Phase 1-2, sharding Phase 3+)

---

## taba-solver

### Responsibilities
Composition resolution, conflict detection, deterministic placement, policy
application, scaling computation, and cycle detection. The solver is a pure
deterministic function: given an immutable graph snapshot and node membership,
it produces identical results on any node.

### Public API surface
**Solver**: `Solver` trait,
`resolve(snapshot, membership) -> SolverResult`

**Composition**: `compose(snapshot) -> CompositionResult`,
`CompositionResult` (compositions, unresolved conflicts)

**Placement**: `place(compositions, membership, capabilities, resources) -> PlacementPlan`,
`PlacementPlan`, `PlacementDecision` (unit_id, node_id, rationale)

**Capability filter** (hard constraints per INV-N2):
`filter_by_capabilities(unit, nodes) -> Vec<NodeId>` — binary match on
runtime, arch, OS, privilege, custom tags. Artifact type → runtime capability.

**Resource ranking** (soft constraints per INV-N3):
`rank_by_resources(nodes, resources) -> Vec<(NodeId, Ppm)>` — best-fit
scoring using versioned resource snapshots (determinism per F-A306).

**Environment filter**: `filter_by_environment(unit, promotions, nodes) -> Vec<NodeId>`
— checks PromotionPolicy authorization for each environment tag.
env:dev uses author affinity instead of promotion (INV-E1).

**Promotion evaluation**: `evaluate_promotion(unit, policies, gates) -> PromotionResult`
— checks PromotionGate governance, handles auto vs human-approval.
Collision detection: same-decision → dedup by lowest PolicyId; different
decisions → fail closed (INV-S8a).

**Conflict**: `ConflictReport`, `Conflict` (security | ambiguity | cycle),
`detect_conflicts(snapshot) -> Vec<Conflict>`

**Policy application**: `apply_policies(conflicts, policies) -> ResolutionResult`,
`SupersessionResolver` (walks chain, uses latest non-revoked per INV-C7)

**Scaling**: `compute_scaling(unit, metrics) -> ScalingDecision`

**Cycle detection**: `detect_cycles(units) -> Vec<CyclicDependency>`

**Tiebreaker**: `partition_tiebreak(node_a, node_b) -> NodeId` (lexicographic lowest)

**Errors**: `SolverError` (UnresolvableConflict | CyclicDependency |
InsufficientResources | PrecisionOverflow)

### Internal modules
- `composition` -- capability needs/provides matching, composition construction
  (order-independent per INV-C6)
- `conflict` -- security conflict detection, ambiguity detection, promotion
  policy collision detection (same-decision dedup, different-decision fail closed)
- `placement` -- deterministic scoring (fixed-point ppm per DL-004),
  resource fitting, tolerance matching
- `capability_filter` -- hard constraint filtering: runtime matching
  (artifact.type → node runtime capability), environment tags,
  privilege requirements, custom tag matching
- `resource_rank` -- soft constraint ranking: memory, CPU, disk, GPU
  availability. Uses versioned resource snapshots for determinism.
- `promotion` -- promotion policy evaluation, PromotionGate checking,
  auto vs human-approval, environment transition validation
- `policy` -- policy lookup, supersession chain resolution, validity check
- `scaling` -- declared-parameter-driven scaling (INV-K4)
- `cycle` -- recovery dependency cycle detection, fail-closed (INV-K5),
  spawn depth validation (INV-W3)
- `tiebreak` -- deterministic tiebreaker (lexicographic lowest NodeId per INV-C3)
- `arithmetic` -- ppm fixed-point operations (u64/i64, division rounds toward zero)

### Does NOT own
- Graph state (reads immutable snapshots from taba-graph)
- Membership (reads from taba-gossip)
- Placement execution (taba-node reconciles actual state)
- Capability matching algorithm (defined in taba-core; solver uses it)
- Taint computation (delegates to taba-security)

### Traces to
- INV-C3 (deterministic solver, fixed-point, tiebreaker)
- INV-C6 (composition order-independent)
- INV-C7 (policy uniqueness, supersession chain)
- INV-K1 (valid composition = all needs satisfied, no conflicts)
- INV-K2 (typed capability matching with purpose)
- INV-K3 (placement respects tolerance declarations)
- INV-K4 (scaling from unit-declared parameters only)
- INV-K5 (cyclic recovery dependencies fail closed)
- INV-S2 (fail closed on security conflicts)
- DL-004 (fixed-point ppm arithmetic)
- DL-006 (policy supersession)
- FM-06 (solver bug), FM-11 (determinism regression), FM-12 (version skew)

---

## taba-erasure

### Responsibilities
Erasure coding for graph resilience. Owns Reed-Solomon encoding/decoding,
shard distribution across nodes, reconstruction on node failure with
backpressure (priority queue, circuit breaker), and re-coding when fleet size
changes.

### Public API surface
**Coding**: `ErasureCoder` trait,
`encode(data, params) -> Vec<Shard>`,
`decode(shards, params) -> Result<Vec<u8>, ErasureError>`

**Parameters**: `ErasureParams` (k, n, resilience_pct),
`compute_params(fleet_size, resilience_pct) -> ErasureParams`

**Distribution**: `ShardDistributor` trait,
`distribute(shards, membership) -> ShardAssignment`,
`reassign(assignment, membership_change) -> ShardAssignment`

**Reconstruction**: `Reconstructor` trait,
`reconstruct(shard_ids, available_shards) -> Result<Vec<u8>, ReconstructionError>`,
`ReconstructionQueue` (priority: governance > policy > data constraints > workload),
`BackpressureConfig` (rate limit, queue depth threshold, circuit breaker)

**Re-coding**: `recode(current_shards, new_params) -> Vec<Shard>`

**Shard**: `Shard` (shard_id, data, parity_index), `ShardAssignment`

**Errors**: `ErasureError` (InsufficientShards | ReconstructionFailed |
BackpressureTripped | CircuitBreakerOpen)

### Internal modules
- `reed_solomon` -- RS encoding/decoding implementation
- `params` -- parameter computation (k = ceil(N * (1 - R/100)))
- `distribution` -- shard-to-node mapping, rebalancing
- `reconstruction` -- decode + signature re-verification (calls taba-security),
  priority queue, backpressure, circuit breaker
- `recoding` -- fleet size change handling, rolling re-encode

### Does NOT own
- Graph structure (encodes opaque data blobs; taba-graph decides what to shard)
- Gossip transport (receives membership changes; does not participate in gossip)
- Shard persistence (taba-node stores shards on disk)
- Signature verification (delegates to taba-security after reconstruction)

### Traces to
- INV-R1 (reconstruction backpressure, priority, circuit breaker,
  post-reconstruction signature re-verification)
- INV-R4 (reconstructable if failures <= floor(N-k), degraded mode otherwise)
- A3 (erasure overhead acceptable for graph shard sizes)
- FM-02 (multiple node failures), FM-13 (reconstruction storm)

---

## taba-gossip

### Responsibilities
SWIM-based membership protocol. Owns failure detection with 2-witness
confirmation, node join/leave handling, membership view dissemination, and
signed gossip message transport. Also carries priority messages (key revocation
events, solver version announcements).

### Public API surface
**Membership**: `MembershipService` trait,
`members() -> MembershipView`,
`join(node_id, pubkey) -> Result<(), GossipError>`,
`leave(node_id) -> Result<(), GossipError>`

**View**: `MembershipView`, `MemberState` (Joining | Attesting | Active |
Suspected | Draining | Left | Failed), `NodeHealth` (healthy | suspected | unknown)

**Failure detection**: `FailureDetector` trait,
`probe(node_id) -> ProbeResult`,
`confirm_failure(node_id, witnesses) -> Result<FailureConfirmation, GossipError>`

**Messages**: `GossipMessage` (signed), `MessageType` (Ping | PingReq |
Ack | MembershipDelta | PriorityEvent)

**Priority events**: `PriorityEvent` (KeyRevocation | SolverVersionAnnounce |
DegradedModeAlert | OperationalCommand)

**Capability advertisement**: `CapabilityAdvertisement` (node_id,
capability_set, resource_snapshot, logical_clock). Nodes advertise
capabilities (rarely, on change/refresh) and resources (periodically).

**Cross-domain**: `CrossDomainGossip` — bridge nodes relay capability
advertisements and forwarding query responses between trust domains.
`ForwardingQuery`, `ForwardingResult` (signed by bridge).

**Fleet operations**: `FleetCommand` (RefreshCapabilities | other).
Rate-limited: one command per type per gossip convergence window.
Deduplication by command type + logical clock.

**Errors**: `GossipError` (SignatureInvalid | InsufficientWitnesses |
NodeNotFound | TransportFailed | FleetCommandRateLimited)

### Internal modules
- `swim` -- SWIM protocol state machine, probe scheduling, indirect probes
- `membership` -- membership view, state transitions, convergence tracking
- `failure` -- suspicion tracking, witness collection, 2-witness confirmation
  threshold, suspected-node retention (INV-R5)
- `messages` -- message types, signing (calls taba-security), verification
- `transport` -- UDP/TCP gossip transport, message serialization
- `priority` -- priority event dissemination (key revocation, version announcements)
- `capability` -- capability/resource advertisement, periodic resource snapshots
- `cross_domain` -- bridge node detection, cross-domain capability advertisement
  relay, forwarding query routing, cache management (fail-open default)
- `fleet` -- operational command propagation, rate limiting, dedup (INV-A314)

### Does NOT own
- Graph state or shards (carries deltas; does not interpret them)
- Placement decisions (announces membership; solver uses it)
- Key material (signs messages via taba-security)
- Shard redistribution logic (that is taba-erasure)

### Traces to
- INV-R3 (gossip convergence, signed messages, 2-witness confirmation)
- INV-R5 (suspected nodes stay in pool with health='unknown')
- INV-N1 (capability advertisement via gossip)
- INV-X4, INV-X5 (bridge node discovery, cross-domain capability advertisement)
- DL-009 (signed gossip, witness confirmation)
- A4 (SWIM scales to target cluster sizes)
- A17 (bridge nodes sufficient for cross-domain discovery)
- FM-04 (gossip poisoning resistance), FM-09 (false positive handling)
- FM-12 (solver version announcements via priority gossip)
- FM-25 (bridge failure isolates domains)
- F-A314 (fleet refresh rate limiting)

---

## taba-node

### Responsibilities
Per-node daemon. Owns local reconciliation (desired vs actual state), WAL
management and persistence, shard storage, health reporting, and operational
mode management (Normal, Degraded, Recovery). This is the main runtime
binary that ties all other crates together.

### Public API surface
**Daemon**: `NodeDaemon` (entry point),
`start(config) -> Result<(), NodeError>`,
`shutdown() -> Result<(), NodeError>`

**Reconciliation**: `Reconciler` trait,
`reconcile(desired, actual) -> Vec<ReconciliationAction>`,
`ReconciliationAction` (Start | Stop | Drain | Migrate)

**WAL management**: `WalManager` (wraps taba-graph `Wal`),
`replay() -> Result<GraphState, WalError>`,
`checkpoint() -> Result<(), WalError>`

**Shard storage**: `ShardStore` trait,
`store_shard(shard) -> Result<(), StorageError>`,
`retrieve_shard(shard_id) -> Result<Shard, StorageError>`

**Health**: `HealthReporter` trait, `report() -> NodeHealthStatus`,
`OperationalMode` (Normal | Degraded | Recovery)

**Auto-discovery**: `CapabilityDiscoverer` trait,
`discover() -> NodeCapabilitySet` (probes Docker, Podman, K8s, Wasm, TPM,
GPU, etc.), `refresh() -> NodeCapabilitySet` (re-probe)

**Artifact management**: `ArtifactFetcher` trait,
`fetch(artifact_ref, digest) -> Result<ArtifactPath, ArtifactError>`,
fetch order: peer cache → external source (INV-A2).
`PeerCache` (content-addressed by SHA256, P2P distribution).
`push(artifact_path) -> Result<(), ArtifactError>` (for air-gapped/dev).
Digest verification mandatory after fetch (INV-A1).

**Task spawning**: `TaskSpawner` trait,
`spawn(delegation_token, task_unit) -> Result<SpawnResult, SpawnError>`,
validates delegation token (calls taba-security), signs task via token,
inserts into graph. Tracks spawn count against token limit.

**Health check orchestration**: `HealthCheckOrchestrator` trait,
`register_check(unit_id, health_check) -> ()`,
`run_checks() -> Vec<HealthCheckResult>`.
Progressive: OS-level process monitoring (default) → HTTP/TCP probe →
custom command (declared by workload unit per INV-O3).

**Mode transitions**: `ModeManager`,
`enter_degraded(reason) -> ()`,
`enter_recovery() -> ()`,
`enter_normal() -> ()`

**Errors**: `NodeError` (WalCorrupt | DiskFull | ReconciliationFailed |
ShardStorageFailed | ModeTransitionInvalid | ArtifactFetchFailed |
ArtifactDigestMismatch | DelegationInvalid | SpawnFailed |
CapabilityDiscoveryFailed)

### Internal modules
- `daemon` -- startup, shutdown, tokio runtime, signal handling
- `init` -- `taba init` flow: key generation, Tier 0 bootstrap, node setup
  (userspace or system)
- `reconciliation` -- desired-vs-actual comparison, action planning, execution
- `wal_manager` -- WAL lifecycle, replay on startup, periodic checkpoint
- `shard_store` -- on-disk shard persistence, retrieval
- `health` -- health status computation, peer health observation
- `health_check` -- health check orchestration (OS-level, HTTP, TCP, command)
- `mode` -- operational mode state machine, transition rules, degraded-mode
  freeze (no new placements), recovery throttling
- `runtime` -- workload execution (process, container, Wasm dispatch)
- `discovery` -- auto-discovery probes (Docker, Podman, K8s, Wasm, TPM, GPU),
  capability set construction, refresh on command
- `artifact` -- artifact fetching (peer cache → external), digest verification,
  P2P peer cache, push mode for air-gapped
- `spawner` -- delegation token management, task spawning, spawn count tracking

### Does NOT own
- Graph CRDT logic (uses taba-graph)
- Solver logic (invokes taba-solver with snapshot + membership)
- Gossip protocol (participates via taba-gossip)
- Erasure coding (delegates to taba-erasure)
- Signing/verification (delegates to taba-security)
- Delegation token validation (delegates to taba-security; owns lifecycle)
- Decision trail recording (delegates to taba-observe)

### Traces to
- INV-C4 (WAL-before-effect, local persistence)
- INV-R6 (memory limit enforcement, auto-compaction trigger at 80%)
- INV-N1 (auto-discovery, cached capabilities, refresh)
- INV-A1, INV-A2 (artifact digest verification, fetch order)
- INV-W4 (delegation token lifecycle for spawning)
- INV-O3 (progressive health checks)
- INV-N5 (placement-on-failure defaults by environment)
- Domain model: Operational Modes, Node states, Environment Progression
- FM-01 (single node failure), FM-07 (WAL corruption), FM-08 (graph growth)
- FM-15 (artifact unavailable), FM-17 (stale capability discovery)
- FM-22 (bounded task deadline exceeded)

---

## taba-observe

### Responsibilities
Cross-cutting observability. Owns decision trail recording, solver replay,
structured event emission, health check result aggregation, and integration
export (OpenTelemetry, Prometheus, alerting webhooks). Provides queryable
audit infrastructure for the "why did this happen?" question.

### Public API surface
**Decision trails**: `DecisionTrailRecorder` trait,
`record_solver_run(inputs, outputs) -> DecisionTrailId`,
`query_trail(unit_id, time_range?) -> Vec<DecisionTrail>`,
`replay_solver(trail_id) -> SolverResult` (deterministic replay per INV-C3)

**Decision trail type**: `DecisionTrail` (trail_id, graph_snapshot_id,
node_membership_snapshot, solver_version, placements, conflicts,
logical_clock, wall_time)

**Structured events**: `EventEmitter` trait,
`emit(event) -> ()`. All significant system actions emit structured events
(JSON-formatted, timestamped, typed).

**Health aggregation**: `HealthAggregator` trait,
`report_health(unit_id, result) -> ()`,
`query_health(unit_id) -> HealthStatus`

**Integration export**:
- `PrometheusExporter` trait — per-node metrics endpoint
- `OpenTelemetryExporter` trait — trace/metric/log export
- `AlertWebhook` trait — webhook on degraded mode, policy conflict,
  promotion failure. Best-effort (failure doesn't block system operations).

**Retention**: decision trail retention defaults to since-last-compaction.
Unit-level `decision_retention` field or governance-level trust-domain
retention policy can extend (INV-O2).

**Errors**: `ObserveError` (TrailNotFound | ReplayFailed | ExportFailed |
RetentionPolicyViolation)

### Internal modules
- `trail` -- decision trail recording, storage, query
- `replay` -- solver replay from historical trail (deterministic)
- `events` -- structured event types, emission, buffering
- `health` -- health check result aggregation, status computation
- `prometheus` -- Prometheus exposition format endpoint
- `otel` -- OpenTelemetry SDK integration
- `alert` -- webhook dispatch, retry, best-effort delivery

### Does NOT own
- Solver logic (replays solver via taba-solver; does not own scoring)
- Health check execution (taba-node orchestrates; taba-observe aggregates results)
- Graph storage (trails stored in graph via taba-graph)
- Workload-level metrics (workloads export via standard protocols, not taba)

### Traces to
- INV-O1 (every solver run produces decision trail)
- INV-O2 (retention defaults, unit/governance override)
- INV-O3 (progressive health checks — aggregation side)
- Domain model: Observability (structural + integration)
- Domain model: Decision Trail, Solver Replay

---

## taba-cli

### Responsibilities
Command-line interface for human operators. Owns unit authoring commands,
composition management, status inspection, policy management, trust domain
management, audit/lineage queries, and ceremony initiation.

### Public API surface
**Commands** (clap subcommands):
- `init` -- Tier 0 solo bootstrap (key + trust domain + governance in one command)
- `apply` -- sign and insert a unit TOML into the graph (dev workflow)
- `unit` -- create, inspect, validate, list, archive
- `compose` -- trigger composition, inspect result, list conflicts
- `status` -- node health, graph stats, placement map, operational mode
- `policy` -- create, supersede, list, inspect resolution chain
- `promote` -- create promotion policy for a unit version + environment
- `trust-domain` -- create (ceremony), inspect, list members
- `audit` -- lineage query, provenance chain, taint report, decision trail
- `ceremony` -- initiate Shamir ceremony, add share, complete
- `node` -- join, leave, drain, health
- `refresh` -- re-probe capabilities (single node)
- `fleet` -- fleet-wide commands (refresh-capabilities, etc.)
- `observe` -- query decision trails, solver replay, health status
- `push` -- push artifact to peer cache (air-gapped/dev)

**Output**: structured (JSON, TOML) and human-readable table formats

**Errors**: `CliError` (ConnectionFailed | InvalidInput | Unauthorized |
ServerError)

### Internal modules
- `commands` -- clap command definitions and dispatch
- `client` -- gRPC client to taba-node daemon (tonic)
- `format` -- output formatting (table, JSON, TOML)
- `auth` -- local author key management (keyring integration)

### Does NOT own
- Any domain logic (pure client; all logic lives in other crates)
- Node runtime (connects to taba-node daemon)
- Key storage beyond local author key (cluster keys managed by taba-security)

### Traces to
- A7 (unit declarations are human-authorable in TOML)
- All features/*.feature (CLI is the user-facing entry point for every scenario)

---

## taba-test-harness

### Responsibilities
Shared test utilities for the workspace. Owns test data builders, in-memory
fakes for graph and solver, proptest strategies for core types, and BDD step
definition helpers. No production code depends on this crate.

### Public API surface
**Builders**: `UnitBuilder`, `WorkloadBuilder`, `DataBuilder`, `PolicyBuilder`,
`GovernanceBuilder`, `NodeBuilder`, `AuthorBuilder`, `CapabilityBuilder`,
`BoundedTaskBuilder`, `DelegationTokenBuilder`, `ArtifactBuilder`,
`PromotionPolicyBuilder`, `TombstoneBuilder`, `CapabilitySetBuilder`,
`HealthCheckBuilder`, `DecisionTrailBuilder`

**Fakes**: `InMemoryGraph` (implements `CompositionGraph`),
`InMemorySolver` (implements `Solver`),
`InMemoryWal` (implements `Wal`),
`FakeMembership` (implements `MembershipService`),
`FakeSigner` / `FakeVerifier`,
`FakeArtifactFetcher`, `FakeCapabilityDiscoverer`,
`FakeDecisionTrailRecorder`, `FakePeerCache`

**Proptest strategies**: `arb_unit()`, `arb_capability()`, `arb_node_id()`,
`arb_signed_unit()`, `arb_graph_snapshot()`, `arb_placement()`,
`arb_delegation_token()`, `arb_bounded_task()`, `arb_artifact()`,
`arb_capability_set()`, `arb_tombstone()`, `arb_logical_clock()`

**BDD helpers**: step definition utilities for cucumber-rs,
`TestCluster` (multi-node in-memory setup)

**Assertions**: `assert_crdt_laws(merge_fn)` (commutativity, associativity,
idempotency), `assert_solver_determinism(solver, snapshot, membership)`

### Internal modules
- `builders` -- fluent builder pattern for all domain types
- `fakes` -- in-memory trait implementations
- `strategies` -- proptest Arbitrary implementations
- `bdd` -- cucumber-rs step helpers, test cluster setup
- `assertions` -- property-based law checkers

### Does NOT own
- Production code paths (dev-dependency only)
- Test execution (tests live in each crate; harness provides utilities)

### Traces to
- INV-C2 (CRDT law assertion helpers)
- INV-C3, DL-004 (solver determinism assertion)
- FM-06, FM-11 (property-based solver testing)
- Guidelines: TESTING_STRATEGY.md
