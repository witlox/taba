# Adversary Architecture Review — Findings

Gate 1 review of architecture specs before implementation.

## Resolved (9)

| ID | Severity | Title | Resolution |
|----|----------|-------|-----------|
| A003 | Critical | Wal trait circular dependency | Moved Wal trait definition to taba-core. Graph calls it, node implements it. DAG preserved. |
| A005 | Critical | Verifier::verify() missing logical_clock + validity_window params | Added both params to signature. Documented scope check as separate gate. |
| A009 | Critical | Shared snapshot types in wrong crates | GraphSnapshot, MembershipSnapshot, Wal defined in taba-core. Dependency graph updated. |
| A025 | High | CrossDomainGossip traits not defined | Created full gossip interface: CrossDomainGossip, CapabilityAdvertiser, FleetCommandService traits + ForwardingResult type. |
| A002 | High | DualClockEvent vs Timestamp alias confusion | Removed alias. Use DualClockEvent consistently. |
| A006 | High | GraphSnapshot immutability risk | Documented Arc-based immutable snapshot contract. |
| A013 | High | EventEmitter::emit() swallows errors | Returns Result now. Non-blocking with queue backpressure. |
| A020 | Medium | ProvenanceLink uses bare u64 timestamp | Changed to DualClockEvent. |
| A024 | Medium | MemoryMonitor, Compactor traits not defined | Added both traits to interfaces/graph.rs with CompactionAction enum. |

## Dismissed with rationale (8)

| ID | Title | Rationale |
|----|-------|-----------|
| A001 | GraphSnapshot cross-crate | Resolved by A009 (shared types in taba-core). Dependency graph design note already covered this. |
| A004 | SignatureContextMismatch variant missing | Already exists in error-taxonomy.md. Interface placeholders don't list all variants — implementation uses taxonomy. |
| A012 | Duplicate NodeId types in security.rs | Placeholder types (`pub struct NodeId(/* opaque */)`) are architecture spec convention. Implementation uses common::NodeId. |
| A010 | Async ceremony vs sync KeyManager | Deliberate. INV-S3 requires sync verification. Keys must be local. Ceremony is async because it's interactive. Two separate concerns. |
| A011 | PlacementScorer missing promotions | Promotion is intentionally a pre-filter (CapabilityFilter handles env tags). Scoring ranks survivors. Two-phase is cleaner than one method doing both. |
| A015 | SpawnContext missing parent reference | SpawnContext.spawned_by IS the parent UnitId. Validator looks up parent state in graph via that ID. |
| A018 | LogicalClock u64 overflow | At 1M events/sec = 584 million years. Not practical. |
| A021 | Verifier missing scope check | Deliberate separation. Verifier checks signature + revocation. ScopeValidator checks scope. Two gates, independently testable. Both required before merge. |

## Deferred to implementation (8)

| ID | Title | Phase |
|----|-------|-------|
| A007 | CapabilityFilter param for custom tags | M2. Unit.needs contains requirements; NodeCapabilitySet has tags. Filter can match from given params. |
| A008 | Incremental ConflictDetector | M2 optimization. Full scan works for M1. |
| A014 | ResourceRanker versioning contract | Phase 2. Resource ranking deferred per architectural decision (capability-only in Phase 1). |
| A016 | PromotionGate dedup logic | M2. Collision detection is at solver eval time (INV-C5 pattern: query-time, not merge-time). |
| A017 | HealthCheckOrchestrator → graph reporting | Standard reconciliation loop handles this. No special path needed. |
| A019 | RoleAssignment uniqueness at merge | ScopeValidator called in pre-merge gate chain. Enforcement map documents this. |
| A022 | Promotion collision at merge vs eval | Consistent with INV-C5 (policy validity on query). Collisions surfaced at solver eval. |
| A023 | Missing state transition events | Existing events catalog covers most transitions. Fill gaps incrementally during M2-M4. |
