# Error Taxonomy

Defines error types for every crate in the taba workspace. Each error enum uses
`thiserror` with typed variants. Every variant has a meaningful name and
sufficient context for diagnosis. Errors propagate with `?`. User-facing errors
(surfaced through taba-cli) are actionable.

## Conventions

- Error enums are named `{Crate}Error` (e.g., `CommonError`, `CoreError`).
- Variants carry structured context (IDs, descriptions) not just strings.
- `#[source]` chains preserve the full causal chain.
- **Retryable** means the caller may retry the same operation after a delay.
- **Fatal** means the operation cannot succeed without external intervention
  (human action, configuration change, or code fix).

---

## taba-common: `CommonError`

Foundation types and validation. Wrapped by most other crate errors.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `InvalidTimestamp { millis, counter, reason }` | Timestamp fields fail validation (e.g., not_before > not_after in ValidityWindow). | Fatal | -- |
| `InvalidPpm { value, reason }` | Ppm/SignedPpm overflow or illegal operation. | Fatal | -- |
| `ConfigParse { path, source: toml::de::Error }` | TOML config file cannot be parsed. | Fatal | -- |
| `ConfigValidation { field, reason }` | Config value out of valid range (e.g., resilience_pct > 100, shamir_threshold > shamir_total_shares). | Fatal | -- |
| `IdParse { input, source }` | UUID parse failure for any identity newtype. | Fatal | -- |
| `SerializationError { context, source: bincode::Error }` | Wire serialization/deserialization failed. | Fatal | -- |

---

## taba-security: `SecurityError`

Cryptographic operations, signature verification, capability enforcement, and
trust domain management.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `SignatureInvalid { unit_id, author_id, reason }` | Cryptographic signature verification failed. Unit rejected at merge gate (INV-S3). | Fatal | FM-04, FM-05 |
| `SignatureContextMismatch { unit_id, expected_cluster, actual_cluster }` | Unit signed for a different cluster/trust domain (cross-cluster replay). | Fatal | FM-04 |
| `AuthorKeyRevoked { author_id, revoked_at, unit_created_at }` | Author's key was revoked before the unit's creation timestamp (INV-S3c). | Fatal | FM-05 |
| `AuthorScopeExceeded { author_id, unit_type, trust_domain }` | Author attempted to create a unit outside their (type scope x trust domain scope) (INV-S5). | Fatal | FM-05 |
| `ScopeDuplicate { author_a, author_b, scope }` | Two distinct authors have identical (unit_type_scope, trust_domain_scope) tuples (INV-S8). | Fatal | -- |
| `CapabilityDenied { unit_id, capability, reason }` | Unit tried to access a capability it did not declare or that policy denied (INV-S1). | Fatal | -- |
| `SecurityConflictUnresolved { units, capability }` | Incompatible capability declarations with no policy resolution. Composition refused (INV-S2). | Fatal | FM-10 |
| `TaintWidening { data_unit_id, parent_classification, child_classification }` | Child data unit attempts to widen (weaken) classification without policy (INV-S7). | Fatal | -- |
| `DeclassificationRequiresMultiParty { policy_id, signer_count }` | Declassification policy has fewer than 2 distinct signers (INV-S9). | Fatal | -- |
| `TrustDomainCreationRequiresMultiParty { signers_present }` | Trust domain creation has fewer than 2 distinct author signatures (INV-S10, INV-S6). | Fatal | -- |
| `ValidityWindowExpired { unit_id, window }` | Unit's validity window has passed. | Fatal | -- |
| `CeremonyError { ceremony_id, reason }` | Shamir key ceremony failure (insufficient shares, wrong threshold, etc.). | Fatal | -- |
| `KeyZeroizeFailed { context }` | Zeroize of key material could not be confirmed. Defensive panic candidate. | Fatal | -- |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `SignatureInvalid` | `Error: Unit {unit_id} has an invalid signature from author {author_id}. The unit was rejected. Verify the signing key and re-sign the unit.` |
| `AuthorKeyRevoked` | `Error: Author {author_id}'s key was revoked at {revoked_at}. Units created after revocation are not accepted. Re-author with a valid key.` |
| `AuthorScopeExceeded` | `Error: Author {author_id} is not authorized to create {unit_type} units in trust domain {trust_domain}. Check role assignments.` |
| `SecurityConflictUnresolved` | `Error: Capability conflict on '{capability}' between units {units}. Create a policy unit to resolve this conflict before composition can proceed.` |
| `TrustDomainCreationRequiresMultiParty` | `Error: Trust domain creation requires at least 2 distinct author signatures. Only {signers_present} provided.` |

---

## taba-core: `CoreError`

Unit lifecycle, composition, and aggregate management.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `UnitNotFound { unit_id }` | Referenced unit does not exist in the local graph. | Retryable (may arrive via gossip) | -- |
| `UnitAlreadyExists { unit_id }` | Duplicate insertion of the same unit ID. CRDT merge is idempotent, so this is a no-op warning, not a hard error. | Fatal (no-op) | -- |
| `InvalidUnitState { unit_id, current, expected }` | Unit state transition is illegal (e.g., Terminated -> Running). | Fatal | -- |
| `CompositionFailed { reason, unresolved_capabilities }` | Composition could not satisfy all capability needs (INV-K1). | Fatal | -- |
| `CapabilityTypeMismatch { need, provide, reason }` | Typed capability matching failed (INV-K2). | Fatal | -- |
| `CyclicRecoveryDependency { cycle }` | Units form a circular recovery dependency chain (INV-K5). | Fatal | -- |
| `PlacementConstraintViolation { unit_id, constraint, reason }` | Unit tolerance declarations (latency, failure mode, resources) cannot be met (INV-K3). | Retryable (nodes may join) | FM-06 |
| `ScalingParameterInvalid { unit_id, reason }` | Scaling declaration is malformed or contradictory (INV-K4). | Fatal | -- |
| `HierarchyDepthExceeded { data_unit_id, depth, max }` | Data unit exceeds maximum hierarchy depth (16 levels). | Fatal | -- |
| `RedundantChild { child_id, parent_id }` | Child data unit has identical constraints to parent (INV-D3). | Fatal | -- |
| `ProvenanceChainBroken { data_unit_id, missing_ref }` | Provenance reference cannot be resolved (INV-D1). Pending until ref arrives. | Retryable (causal buffering) | -- |
| `RetentionExpired { data_unit_id, expired_at }` | Data unit retention period has elapsed (INV-D2). Eligible for compaction. | Fatal (expected) | -- |
| `PolicyConflict { conflict_tuple, existing_policy, new_policy }` | Two non-revoked policies resolve the same conflict (INV-C7). New must supersede. | Fatal | FM-10 |
| `OrphanedPolicy { policy_id, references }` | Policy references a non-existent conflict. Eligible for archival (INV-C5). | Fatal | -- |
| `Security { source: SecurityError }` | Propagated from taba-security. | Inherits | -- |
| `Common { source: CommonError }` | Propagated from taba-common. | Inherits | -- |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `CompositionFailed` | `Error: Composition failed. Unresolved capabilities: {list}. Ensure all required capability providers are declared and deployed.` |
| `CyclicRecoveryDependency` | `Error: Circular recovery dependency detected: {cycle}. Add a policy unit declaring restart priority to break the cycle.` |
| `PlacementConstraintViolation` | `Error: Cannot place unit {unit_id}: constraint '{constraint}' unsatisfiable. {reason}. Consider relaxing tolerances or adding nodes.` |
| `PolicyConflict` | `Error: Duplicate policy for conflict {conflict_tuple}. New policy must explicitly supersede {existing_policy} via the 'supersedes' field.` |

---

## taba-graph: `GraphError`

CRDT graph operations, merge, query, and WAL persistence.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `MergeRejected { unit_id, reason }` | Unit failed pre-merge validation (signature, scope, etc.). Wraps SecurityError. | Fatal | FM-04 |
| `CrdtViolation { operation, reason }` | Merge operation would violate commutativity/associativity/idempotency (INV-C2). Internal bug. | Fatal | FM-06 |
| `WalWriteFailed { path, source: io::Error }` | WAL write failed (disk full, I/O error). Node must stop accepting mutations (INV-C4). | Retryable (after disk recovery) | FM-07 |
| `WalCorrupted { path, offset, reason }` | WAL integrity check failed. Node must recover from peers. | Fatal (requires reconstruction) | FM-07 |
| `WalReplayFailed { entry_index, reason }` | WAL entry cannot be replayed during recovery. | Fatal | FM-07 |
| `PendingRefTimeout { unit_id, missing_refs, waited }` | Pending unit's references never arrived within timeout. | Retryable (extended wait or manual) | -- |
| `MemoryLimitExceeded { current_bytes, limit_bytes }` | Active graph exceeds configured memory limit (INV-R6). Node enters degraded mode. | Retryable (after compaction) | FM-08 |
| `CompactionFailed { reason }` | Auto-compaction could not free sufficient memory. | Fatal | FM-08 |
| `InsertionOrderDependence { unit_ids, reason }` | Detected composition result differing by insertion order (INV-C6). Internal bug. | Fatal | FM-06 |
| `QueryError { query, reason }` | Graph query failed (malformed query, internal state inconsistency). | Fatal | -- |
| `Security { source: SecurityError }` | Propagated from taba-security (signature verification at merge gate). | Inherits | -- |
| `Core { source: CoreError }` | Propagated from taba-core (unit validation). | Inherits | -- |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `WalWriteFailed` | `Error: Cannot write to WAL at {path}: {source}. Node is pausing mutations. Check disk space and filesystem health.` |
| `WalCorrupted` | `Error: WAL corrupted at {path} offset {offset}. Node will recover graph state from peers. Manual intervention may be required.` |
| `MemoryLimitExceeded` | `Warning: Graph memory usage ({current_bytes} bytes) exceeds limit ({limit_bytes} bytes). Node entering degraded mode. Compaction in progress.` |

---

## taba-solver: `SolverError`

Deterministic placement, capability matching, and conflict detection.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `DeterminismViolation { input_hash, node_a_result, node_b_result }` | Two evaluations of the same input produced different output (INV-C3). Critical bug. | Fatal | FM-06, FM-11 |
| `FloatingPointDetected { operation }` | Floating-point arithmetic detected in solver path. Compile-time lint preferred, runtime backstop (INV-C3). | Fatal | FM-11 |
| `NoViableNode { unit_id, constraints }` | No node in the membership satisfies placement constraints (INV-K3). | Retryable (nodes may join) | -- |
| `CapabilityUnsatisfied { unit_id, need, available_provides }` | No provider matches a required capability (INV-K1, INV-K2). | Fatal | -- |
| `ConflictDetected { units, capability, conflict_type }` | Incompatible declarations found. Requires policy resolution (INV-S2). | Fatal (awaits policy) | FM-10 |
| `VersionSkew { local_version, peer_versions }` | Not all nodes report the same solver version. Placement paused (FM-12). | Retryable (after upgrade completes) | FM-12 |
| `ScalingComputationFailed { unit_id, reason }` | Unit-declared scaling parameters are internally contradictory (INV-K4). | Fatal | -- |
| `CyclicDependency { cycle }` | Recovery dependency cycle detected (INV-K5). | Fatal | -- |
| `Graph { source: GraphError }` | Propagated from taba-graph. | Inherits | -- |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `NoViableNode` | `Error: No node can satisfy placement constraints for unit {unit_id}: {constraints}. Add nodes with the required capabilities or relax unit tolerances.` |
| `ConflictDetected` | `Error: Capability conflict on '{capability}' between units {units}. Create a policy unit to resolve this conflict.` |
| `VersionSkew` | `Warning: Solver version mismatch across cluster. Placement paused until all nodes are at the same version. Local: {local_version}, peers: {peer_versions}.` |

---

## taba-erasure: `ErasureError`

Erasure coding, shard management, and reconstruction.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `InsufficientShards { available, required_k, total_n }` | Not enough shards to reconstruct. System enters degraded mode (INV-R4). | Retryable (shards may arrive) | FM-02 |
| `ShardCorrupted { shard_id, node_id, reason }` | Shard integrity check failed. Must fetch replacement from peers. | Retryable | FM-01, FM-02 |
| `ReconstructionFailed { unit_ids, reason }` | Shard reconstruction could not complete. | Fatal (if below threshold) | FM-02, FM-13 |
| `ReconstructionBackpressure { queue_depth, threshold }` | Reconstruction queue exceeded circuit breaker threshold (INV-R1). New reconstructions paused. | Retryable (after queue drains) | FM-13 |
| `EncodingFailed { unit_id, reason }` | Erasure encoding of a unit/shard failed. | Fatal | -- |
| `ThresholdExceeded { failures, max_tolerable }` | Actual failures exceed floor(N - k). Degraded mode (INV-R4). | Fatal (operator intervention) | FM-02 |
| `SignatureVerificationPostReconstruct { unit_id, reason }` | Reconstructed unit fails signature re-verification (INV-R1). Data integrity issue. | Fatal | FM-04 |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `InsufficientShards` | `Error: Only {available} of {required_k} required shards available (of {total_n} total). System entering degraded mode. Restore failed nodes to recover.` |
| `ReconstructionBackpressure` | `Warning: Reconstruction queue depth ({queue_depth}) exceeds threshold ({threshold}). Pausing new reconstructions to prevent cascade. Monitor node health.` |
| `ThresholdExceeded` | `Critical: {failures} node failures exceed erasure tolerance ({max_tolerable}). Data loss risk. Immediate operator intervention required.` |

---

## taba-gossip: `GossipError`

SWIM-based membership protocol, peer communication, and failure detection.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `MessageSignatureInvalid { from_node, reason }` | Gossip message failed signature verification (INV-R3). Dropped. | Fatal (for this message) | FM-04 |
| `InsufficientWitnesses { node_id, witnesses, required }` | Node failure declaration lacks required corroboration (INV-R3). | Retryable (more witnesses needed) | FM-09 |
| `NodeUnreachable { node_id, attempts, last_error }` | Peer not responding to direct or indirect probes. | Retryable (SWIM protocol continues) | FM-01 |
| `FalsePositiveDetected { node_id }` | A previously declared-dead node has rejoined. Graph/placement recovery needed. | Retryable (auto-recovery) | FM-09 |
| `BootstrapFailed { seed_nodes, reason }` | Cannot contact any seed node during initial join. | Retryable (after network check) | -- |
| `VersionAnnounceMismatch { node_id, solver_version }` | Peer announces a different solver version. Flagged for version skew handling (FM-12). | Retryable (during upgrade) | FM-12 |
| `PartitionDetected { reachable_nodes, unreachable_nodes }` | Gossip topology suggests network partition (FM-03). Informational. | Retryable (heal expected) | FM-03 |
| `Security { source: SecurityError }` | Propagated from taba-security (message signature verification). | Inherits | -- |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `BootstrapFailed` | `Error: Cannot reach any seed node ({seed_nodes}). Check network connectivity and seed node configuration.` |
| `PartitionDetected` | `Warning: Possible network partition. {reachable_nodes} reachable, {unreachable_nodes} unreachable. CRDT merge will reconcile on heal.` |
| `InsufficientWitnesses` | `Info: Node {node_id} suspected but only {witnesses}/{required} witnesses confirm. Awaiting corroboration before declaring failure.` |

---

## taba-node: `NodeError`

Node lifecycle, local state management, operational mode transitions.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `AlreadyInState { node_id, state }` | Node state transition to current state. No-op. | Fatal (no-op) | -- |
| `IllegalStateTransition { node_id, from, to }` | Invalid node state machine transition (e.g., Failed -> Active without Joining). | Fatal | -- |
| `DegradedMode { reason }` | Node entered degraded mode. New placements refused (INV-R6, INV-R4). | Retryable (after recovery) | FM-02, FM-08 |
| `RecoveryInProgress { progress_pct }` | Operation rejected because node is in recovery mode. | Retryable (after recovery) | -- |
| `AttestationFailed { node_id, reason }` | TPM or identity attestation failed during join. | Fatal | -- |
| `LocalStateDiverged { expected_hash, actual_hash }` | Local actual state does not match desired state from solver. Reconciliation needed. | Retryable (reconciliation) | FM-04 |
| `Graph { source: GraphError }` | Propagated from taba-graph. | Inherits | -- |
| `Gossip { source: GossipError }` | Propagated from taba-gossip. | Inherits | -- |
| `Erasure { source: ErasureError }` | Propagated from taba-erasure. | Inherits | -- |
| `Solver { source: SolverError }` | Propagated from taba-solver. | Inherits | -- |

**User-facing messages (CLI-visible):**

| Variant | User message |
|---------|-------------|
| `DegradedMode` | `Warning: Node entering degraded mode: {reason}. New placements suspended. Resolve the underlying issue and the node will recover automatically.` |
| `AttestationFailed` | `Error: Node attestation failed: {reason}. Node cannot join the cluster. Verify TPM configuration or identity key.` |

---

## taba-cli: `CliError`

Top-level user-facing errors. Wraps all lower-level errors and translates them
into actionable messages.

| Variant | Description | Retryable | FM |
|---------|-------------|-----------|-----|
| `Node { source: NodeError }` | Any error propagated from node operations. | Inherits | -- |
| `Security { source: SecurityError }` | Security errors surfaced directly (e.g., ceremony commands). | Inherits | -- |
| `Core { source: CoreError }` | Core errors surfaced directly (e.g., unit creation commands). | Inherits | -- |
| `InvalidArgument { flag, reason }` | CLI argument validation failed. | Fatal | -- |
| `ConnectionFailed { addr, source: io::Error }` | Cannot connect to the local or remote node. | Retryable | -- |
| `Timeout { operation, duration }` | Operation timed out. | Retryable | -- |
| `OutputFormatError { format, reason }` | Cannot serialize output to requested format (JSON, table, etc.). | Fatal | -- |

**User-facing messages:**

All `CliError` variants produce actionable messages. The CLI `main()` function
matches on `CliError` and prints:
- The human-readable message (from the variant's user-facing table above)
- A suggestion for next steps
- The exit code (non-zero for fatal, specific codes for retryable)

---

## Error Propagation Map

Shows how errors wrap and propagate across crate boundaries.

```
taba-common::CommonError
  ├── wrapped by taba-core::CoreError::Common
  ├── wrapped by taba-security::SecurityError (implicit, via field types)
  └── wrapped by taba-graph::GraphError (implicit, via field types)

taba-security::SecurityError
  ├── wrapped by taba-core::CoreError::Security
  ├── wrapped by taba-graph::GraphError::Security
  ├── wrapped by taba-gossip::GossipError::Security
  └── wrapped by taba-cli::CliError::Security

taba-core::CoreError
  ├── wrapped by taba-graph::GraphError::Core
  └── wrapped by taba-cli::CliError::Core

taba-graph::GraphError
  ├── wrapped by taba-solver::SolverError::Graph
  └── wrapped by taba-node::NodeError::Graph

taba-solver::SolverError
  └── wrapped by taba-node::NodeError::Solver

taba-erasure::ErasureError
  └── wrapped by taba-node::NodeError::Erasure

taba-gossip::GossipError
  └── wrapped by taba-node::NodeError::Gossip

taba-node::NodeError
  └── wrapped by taba-cli::CliError::Node
```

```
CLI user
  │
  ▼
CliError ─────────────────────────────────────┐
  │ wraps                                     │ wraps
  ▼                                           ▼
NodeError                              SecurityError
  │ wraps (4 sources)                  CoreError
  ▼
GraphError ── SolverError ── ErasureError ── GossipError
  │ wraps
  ▼
SecurityError, CoreError
  │ wraps
  ▼
CommonError
```

---

## Failure Mode Coverage

Every failure mode (FM-01 through FM-13) is covered by at least one error variant.

| FM | Primary Error(s) | Crate(s) |
|----|-------------------|----------|
| FM-01: Single node failure | `GossipError::NodeUnreachable`, `ErasureError::ShardCorrupted` | taba-gossip, taba-erasure |
| FM-02: Multiple node failures | `ErasureError::InsufficientShards`, `ErasureError::ThresholdExceeded`, `NodeError::DegradedMode` | taba-erasure, taba-node |
| FM-03: Network partition | `GossipError::PartitionDetected` | taba-gossip |
| FM-04: Compromised node | `SecurityError::SignatureInvalid`, `GossipError::MessageSignatureInvalid`, `ErasureError::SignatureVerificationPostReconstruct` | taba-security, taba-gossip, taba-erasure |
| FM-05: Compromised author | `SecurityError::AuthorKeyRevoked`, `SecurityError::AuthorScopeExceeded` | taba-security |
| FM-06: Wrong solver placement | `SolverError::DeterminismViolation`, `GraphError::CrdtViolation`, `GraphError::InsertionOrderDependence` | taba-solver, taba-graph |
| FM-07: WAL corruption/disk full | `GraphError::WalWriteFailed`, `GraphError::WalCorrupted`, `GraphError::WalReplayFailed` | taba-graph |
| FM-08: Graph unbounded growth | `GraphError::MemoryLimitExceeded`, `GraphError::CompactionFailed`, `NodeError::DegradedMode` | taba-graph, taba-node |
| FM-09: Gossip false positive | `GossipError::FalsePositiveDetected`, `GossipError::InsufficientWitnesses` | taba-gossip |
| FM-10: Conflicting legal requirements | `SolverError::ConflictDetected`, `SecurityError::SecurityConflictUnresolved`, `CoreError::PolicyConflict` | taba-solver, taba-security, taba-core |
| FM-11: Solver determinism regression | `SolverError::DeterminismViolation`, `SolverError::FloatingPointDetected` | taba-solver |
| FM-12: Solver version skew | `SolverError::VersionSkew`, `GossipError::VersionAnnounceMismatch` | taba-solver, taba-gossip |
| FM-13: Cascading reconstruction storm | `ErasureError::ReconstructionBackpressure`, `ErasureError::ReconstructionFailed` | taba-erasure |
