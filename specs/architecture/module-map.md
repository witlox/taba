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
`Timestamp`, `ValidityWindow`, `Ppm` (fixed-point ppm wrapper, u64)

**Config**: `TabaConfig` (TOML-backed), `NodeConfig`, `ErasureConfig`,
`GossipConfig`, `SolverConfig`

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

---

## taba-core

### Responsibilities
The unit type system. Owns the definition of what a unit is, what capabilities
are, how contracts are structured, and validation of well-formedness. This is
the domain model in code. Provides the capability matching algorithm.

### Public API surface
**Unit types**: `Unit`, `WorkloadUnit`, `DataUnit`, `PolicyUnit`,
`GovernanceUnit`, `GovernanceSubtype` (TrustDomain | RoleAssignment |
Certification)

**Unit lifecycle**: `UnitState` (Declared, Composed, Placed, Running,
Draining, Terminated)

**Capabilities**: `Capability` (type, name, purpose?), `CapabilityList`
(sorted, deterministic), `NeedsDeclare`, `ProvidesDeclare`

**Contracts**: `Tolerates`, `Trusts`, `ScalingParams`, `FailureSemantics`,
`RecoveryRelationship`, `StateRecovery`

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
- `unit` -- type definitions, state machine
- `capability` -- capability tuples, sorted lists, matching
- `contract` -- tolerance, trust, scaling, failure, recovery declarations
- `data` -- classification lattice, provenance, retention, hierarchy (max depth 16)
- `policy` -- conflict tuples, resolution, supersession
- `governance` -- trust domain, role assignment, certification subtypes
- `validation` -- well-formedness checks, cross-field consistency

### Does NOT own
- Signing or verification (that is taba-security)
- Graph storage or merge (that is taba-graph)
- Placement or composition resolution (that is taba-solver)
- Serialization format decisions beyond derive macros (wire format is proto in taba-common)

### Traces to
- Domain model (all entity definitions)
- INV-K1, INV-K2 (capability matching)
- INV-K3, INV-K4 (tolerance/scaling declarations)
- INV-S7 (classification lattice ordering, hierarchy narrowing/widening)
- INV-D3 (hierarchy depth limit)
- DL-010 (purpose as optional qualifier)

---

## taba-security

### Responsibilities
All cryptographic operations and security policy enforcement. Owns Ed25519
signing/verification, capability enforcement (declared vs allowed), taint
computation via provenance traversal, Shamir root key ceremony (Tier 1
initially), and key revocation lifecycle.

Cross-cutting: every other crate calls into taba-security for verification,
signing, or enforcement.

### Public API surface
**Signing**: `Signer` trait, `sign_unit(key, unit, context) -> SignedUnit`,
`SigningContext` (trust_domain_id, cluster_id, validity_window)

**Verification**: `Verifier` trait,
`verify_unit(signed_unit, context) -> Result<(), VerificationError>`,
`verify_signature(sig, pubkey, payload) -> Result<(), CryptoError>`

**Scope enforcement**: `ScopeChecker` trait,
`check_author_scope(author, unit_type, trust_domain) -> Result<(), ScopeError>`,
`validate_scope_uniqueness(assignments) -> Result<(), ScopeConflict>`

**Capability enforcement**: `CapabilityEnforcer` trait,
`enforce(unit, granted_capabilities) -> Result<(), EnforcementError>`

**Taint**: `TaintComputer` trait,
`compute_taint(unit_id, graph) -> DataClassification` (query-time traversal),
`check_declassification(policy) -> Result<(), DeclassError>` (requires
multi-party per INV-S9)

**Ceremony**: `ShamirCeremony` trait, `CeremonyState` (Started |
CollectingShares | Complete), `CeremonyTier` (Tier1 | Tier2 | Tier3)

**Revocation**: `RevocationEvent`, `is_key_revoked(author_id, timestamp) -> bool`

**Key management**: `KeyPair` (Ed25519), `PublicKey`, `SecretKey` (zeroize on drop)

**Errors**: `SecurityError` (InvalidSignature | ScopeViolation | KeyRevoked |
ContextMismatch | TaintViolation | CeremonyFailed | DeclassificationDenied)

### Internal modules
- `crypto` -- Ed25519 operations, key generation, zeroize
- `signing` -- unit signing with bound context
- `verification` -- signature verification, temporal validity (key not revoked
  before creation timestamp)
- `scope` -- author scope checking, uniqueness validation
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
- INV-S3 (signature binding: context, temporal validity, synchronous gate)
- INV-S4 (taint: query-time traversal, union of inputs)
- INV-S5 (author scope enforcement)
- INV-S8 (scope uniqueness at role assignment)
- INV-S9 (multi-party declassification)
- INV-S10 (multi-party trust domain creation signing)
- DL-005 (Shamir 3-tier ceremony)
- DL-007 (query-time taint)
- FM-04 (compromised node), FM-05 (compromised author)

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

**Placement**: `place(compositions, membership) -> PlacementPlan`,
`PlacementPlan`, `PlacementDecision` (unit_id, node_id, rationale)

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
- `conflict` -- security conflict detection, ambiguity detection
- `placement` -- deterministic scoring (fixed-point ppm per DL-004),
  resource fitting, tolerance matching
- `policy` -- policy lookup, supersession chain resolution, validity check
- `scaling` -- declared-parameter-driven scaling (INV-K4)
- `cycle` -- recovery dependency cycle detection, fail-closed (INV-K5)
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
DegradedModeAlert)

**Errors**: `GossipError` (SignatureInvalid | InsufficientWitnesses |
NodeNotFound | TransportFailed)

### Internal modules
- `swim` -- SWIM protocol state machine, probe scheduling, indirect probes
- `membership` -- membership view, state transitions, convergence tracking
- `failure` -- suspicion tracking, witness collection, 2-witness confirmation
  threshold, suspected-node retention (INV-R5)
- `messages` -- message types, signing (calls taba-security), verification
- `transport` -- UDP/TCP gossip transport, message serialization
- `priority` -- priority event dissemination (key revocation, version announcements)

### Does NOT own
- Graph state or shards (carries deltas; does not interpret them)
- Placement decisions (announces membership; solver uses it)
- Key material (signs messages via taba-security)
- Shard redistribution logic (that is taba-erasure)

### Traces to
- INV-R3 (gossip convergence, signed messages, 2-witness confirmation)
- INV-R5 (suspected nodes stay in pool with health='unknown')
- DL-009 (signed gossip, witness confirmation)
- A4 (SWIM scales to target cluster sizes)
- FM-04 (gossip poisoning resistance), FM-09 (false positive handling)
- FM-12 (solver version announcements via priority gossip)

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

**Mode transitions**: `ModeManager`,
`enter_degraded(reason) -> ()`,
`enter_recovery() -> ()`,
`enter_normal() -> ()`

**Errors**: `NodeError` (WalCorrupt | DiskFull | ReconciliationFailed |
ShardStorageFailed | ModeTransitionInvalid)

### Internal modules
- `daemon` -- startup, shutdown, tokio runtime, signal handling
- `reconciliation` -- desired-vs-actual comparison, action planning, execution
- `wal_manager` -- WAL lifecycle, replay on startup, periodic checkpoint
- `shard_store` -- on-disk shard persistence, retrieval
- `health` -- health status computation, peer health observation (not self-reported)
- `mode` -- operational mode state machine, transition rules, degraded-mode
  freeze (no new placements), recovery throttling
- `runtime` -- workload execution (process, container, Wasm dispatch)

### Does NOT own
- Graph CRDT logic (uses taba-graph)
- Solver logic (invokes taba-solver with snapshot + membership)
- Gossip protocol (participates via taba-gossip)
- Erasure coding (delegates to taba-erasure)
- Signing/verification (delegates to taba-security)

### Traces to
- INV-C4 (WAL-before-effect, local persistence)
- INV-R6 (memory limit enforcement, auto-compaction trigger at 80%)
- Domain model: Operational Modes (Normal, Degraded, Recovery)
- Domain model: Node states (Joining, Attesting, Active, Suspected, Draining, Left, Failed)
- FM-01 (single node failure), FM-07 (WAL corruption), FM-08 (graph growth)

---

## taba-cli

### Responsibilities
Command-line interface for human operators. Owns unit authoring commands,
composition management, status inspection, policy management, trust domain
management, audit/lineage queries, and ceremony initiation.

### Public API surface
**Commands** (clap subcommands):
- `unit` -- create, inspect, validate, list, archive
- `compose` -- trigger composition, inspect result, list conflicts
- `status` -- node health, graph stats, placement map, operational mode
- `policy` -- create, supersede, list, inspect resolution chain
- `trust-domain` -- create (ceremony), inspect, list members
- `audit` -- lineage query, provenance chain, taint report
- `ceremony` -- initiate Shamir ceremony, add share, complete
- `node` -- join, leave, drain, health

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
`GovernanceBuilder`, `NodeBuilder`, `AuthorBuilder`, `CapabilityBuilder`

**Fakes**: `InMemoryGraph` (implements `CompositionGraph`),
`InMemorySolver` (implements `Solver`),
`InMemoryWal` (implements `Wal`),
`FakeMembership` (implements `MembershipService`),
`FakeSigner` / `FakeVerifier`

**Proptest strategies**: `arb_unit()`, `arb_capability()`, `arb_node_id()`,
`arb_signed_unit()`, `arb_graph_snapshot()`, `arb_placement()`

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
