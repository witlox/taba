# Event Catalog

This document defines every event type in taba, its producers and consumers,
ordering guarantees, delivery semantics, propagation mechanism, and mapping
to WAL entries. It is the authoritative reference for all cross-context
communication.

## Design principles

1. Events are typed enums. Every variant carries the minimum payload needed
   for the consumer to act.
2. Local events flow through `tokio::broadcast` or `tokio::mpsc` channels
   within a single node. They are never serialized to wire format.
3. Gossip events flow through the signed gossip protocol (INV-R3). They are
   serialized to protobuf and signed with the sending node's identity key.
4. WAL entries are the durable record. An event that mutates graph state
   must be WAL'd before the mutation is visible (INV-C4).
5. No event carries mutable references or live pointers. All payloads are
   owned, cloneable, serializable values.

---

## Event types

### GraphEvent

Events produced by the composition graph after state changes.

```rust
/// Events emitted by taba-graph when graph state changes.
/// Consumed locally (never sent over gossip -- graph state
/// propagates via CRDT merge, not event replay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphEvent {
    /// A unit passed signature verification and was merged into the graph.
    /// WAL: Merged(unit).
    UnitMerged {
        unit_id: UnitId,
        unit_type: UnitType,
        author: AuthorId,
        trust_domain: TrustDomainId,
        timestamp: Timestamp,
        /// IDs of compositions affected by this insertion.
        affected_compositions: Vec<UnitId>,
    },

    /// A unit was verified but has unsatisfied references.
    /// Buffered in the pending queue until references arrive (INV-C4).
    /// WAL: Pending(unit, missing_refs).
    UnitPending {
        unit_id: UnitId,
        missing_refs: Vec<UnitId>,
        timestamp: Timestamp,
    },

    /// A previously pending unit's references are now satisfied.
    /// WAL: Promoted(unit_id).
    UnitPromoted {
        unit_id: UnitId,
        timestamp: Timestamp,
    },

    /// A unit was rejected at the merge gate.
    /// Not WAL'd (rejected units do not enter durable state).
    UnitRejected {
        unit_id: UnitId,
        reason: MergeRejection,
        timestamp: Timestamp,
    },

    /// A policy superseded a prior policy for the same conflict (INV-C7).
    /// WAL: Merged(new_policy) -- the supersession is structural.
    PolicySuperseded {
        new_policy_id: UnitId,
        old_policy_id: UnitId,
        conflict_tuple: ConflictTuple,
        timestamp: Timestamp,
    },

    /// A subgraph was archived (compaction, retention expiry).
    /// WAL: dedicated compaction record.
    SubgraphArchived {
        root_unit_ids: Vec<UnitId>,
        reason: ArchivalReason,
        timestamp: Timestamp,
    },

    /// Graph memory usage crossed the 80% compaction threshold (INV-R6).
    CompactionTriggered {
        current_bytes: u64,
        limit_bytes: u64,
        timestamp: Timestamp,
    },
}
```

**Producer**: `taba-graph` (composition graph module).
**Consumers**: `taba-solver` (reacts to UnitMerged, UnitPromoted, PolicySuperseded),
`taba-node` (reacts to CompactionTriggered, SubgraphArchived for local cleanup),
`taba-security` (reacts to UnitRejected for audit logging).
**Channel**: `tokio::broadcast` (multiple consumers per event).
**Ordering**: causal. Events for a single unit are ordered by `Timestamp`. Events
across unrelated units have no ordering guarantee.
**Delivery**: at-most-once within a node (broadcast drops if a receiver is lagging).
Acceptable because graph state is the source of truth, not the event stream. A
consumer that misses an event will see the state on its next graph query.

---

### SolverEvent

Events produced by the solver after evaluating compositions and placements.

```rust
/// Events emitted by taba-solver after composition or placement computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SolverEvent {
    /// A composition was successfully resolved.
    CompositionResolved {
        composition_id: UnitId,
        participating_units: Vec<UnitId>,
        timestamp: Timestamp,
    },

    /// The solver detected an unresolvable conflict requiring policy.
    ConflictDetected {
        conflicting_units: Vec<UnitId>,
        capability: CapabilityRef,
        reason: ConflictReason,
        timestamp: Timestamp,
    },

    /// A placement decision was computed.
    /// Deterministic: any node with the same graph + membership
    /// computes the same result (INV-C3).
    PlacementDecision {
        unit_id: UnitId,
        target_node: NodeId,
        score: Ppm,
        timestamp: Timestamp,
    },

    /// A previously placed unit must be moved (node failure, rebalance).
    PlacementRevoked {
        unit_id: UnitId,
        previous_node: NodeId,
        reason: RevocationReason,
        timestamp: Timestamp,
    },

    /// Scaling decision computed from unit-declared parameters (INV-K4).
    ScalingDecision {
        unit_id: UnitId,
        current_instances: u32,
        target_instances: u32,
        trigger: ScalingTrigger,
        timestamp: Timestamp,
    },

    /// Cyclic recovery dependency detected (INV-K5).
    CyclicDependency {
        cycle: Vec<UnitId>,
        timestamp: Timestamp,
    },
}
```

**Producer**: `taba-solver`.
**Consumers**: `taba-node` (reacts to PlacementDecision, PlacementRevoked, ScalingDecision
for local reconciliation), `taba-graph` (records placement as graph annotation).
**Channel**: `tokio::mpsc` (solver to node is point-to-point per node).
**Ordering**: total within a single solver evaluation pass. Across passes, causal
(a pass triggered by UnitMerged happens-after that merge).
**Delivery**: at-least-once. PlacementDecision is written to the graph as a placement
annotation. If the local channel drops the event, the node's reconciliation loop
will discover the placement on its next graph read.

---

### NodeEvent

Events produced by node operations reflecting local actual state.

```rust
/// Events emitted by taba-node about local workload and health state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeEvent {
    /// A workload unit was successfully started on this node.
    WorkloadStarted {
        unit_id: UnitId,
        node_id: NodeId,
        timestamp: Timestamp,
    },

    /// A workload unit failed to start or crashed.
    WorkloadFailed {
        unit_id: UnitId,
        node_id: NodeId,
        error: WorkloadError,
        timestamp: Timestamp,
    },

    /// A workload unit is draining (graceful shutdown in progress).
    WorkloadDraining {
        unit_id: UnitId,
        node_id: NodeId,
        reason: DrainReason,
        timestamp: Timestamp,
    },

    /// A workload unit has terminated.
    WorkloadTerminated {
        unit_id: UnitId,
        node_id: NodeId,
        exit_status: ExitStatus,
        timestamp: Timestamp,
    },

    /// Drift detected between desired and actual state.
    DriftDetected {
        unit_id: UnitId,
        node_id: NodeId,
        expected: UnitState,
        actual: UnitState,
        timestamp: Timestamp,
    },

    /// Node health status update.
    HealthReport {
        node_id: NodeId,
        cpu_usage_ppm: Ppm,
        memory_usage_ppm: Ppm,
        wal_healthy: bool,
        timestamp: Timestamp,
    },
}
```

**Producer**: `taba-node` (reconciliation loop).
**Consumers**: `taba-graph` (WorkloadFailed and HealthReport update graph annotations
that the solver reads), `taba-solver` (indirect, via graph state updates).
**Channel**: `tokio::mpsc` (node to graph is point-to-point).
**Ordering**: causal per unit. Events for a single unit_id on a single node are
strictly ordered. Events across units have no ordering guarantee.
**Delivery**: at-least-once. Health reports are idempotent (latest wins). Workload
state changes are also visible to the reconciliation loop on its next tick, so a
dropped event self-heals.

---

### GossipMessage

Messages exchanged between nodes over the gossip protocol. These are not local
events -- they are wire-format messages signed per INV-R3.

```rust
/// Signed messages exchanged between nodes via SWIM-like gossip.
/// Every message is signed: Sign(node_key, hash(payload || cluster_id)).
/// Unsigned or invalid messages are dropped and the sender is flagged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    // -- Membership (SWIM protocol) --

    /// Ping: "are you alive?"
    Ping {
        from: NodeId,
        sequence: u64,
    },

    /// Ack: "yes, I am alive."
    Ack {
        from: NodeId,
        sequence: u64,
    },

    /// Indirect ping request: "please ping target on my behalf."
    PingReq {
        from: NodeId,
        target: NodeId,
        sequence: u64,
    },

    /// Membership state dissemination (piggybacked on ping/ack).
    MembershipUpdate {
        from: NodeId,
        updates: Vec<MembershipEntry>,
        lamport_clock: u64,
    },

    // -- Graph state synchronization --

    /// CRDT delta for graph merge. Contains one or more signed units.
    /// The receiving node verifies each unit signature before merging.
    GraphDelta {
        from: NodeId,
        /// Serialized CRDT delta (units + tombstones).
        delta: Vec<u8>,
        /// Causal context for the delta.
        vector_clock: VectorClock,
    },

    /// Request missing graph state identified by vector clock comparison.
    GraphSyncRequest {
        from: NodeId,
        /// The requester's current vector clock.
        have: VectorClock,
    },

    /// Response with missing graph state.
    GraphSyncResponse {
        from: NodeId,
        delta: Vec<u8>,
        vector_clock: VectorClock,
    },

    // -- Security (priority gossip) --

    /// Key revocation notice. Propagated with priority (skips queue).
    /// All nodes must process before accepting new units from the revoked key.
    KeyRevocation {
        from: NodeId,
        revoked_author: AuthorId,
        revocation_timestamp: Timestamp,
        /// Signature from the revoking authority (not the revoked key).
        revocation_signature: Vec<u8>,
    },

    // -- Erasure coding --

    /// Shard assignment after membership change or reconstruction.
    ShardAssignment {
        from: NodeId,
        shard_id: ShardId,
        assigned_to: NodeId,
        coding_params: ErasureParams,
    },

    /// Shard reconstruction request (node failure recovery).
    ShardRecoveryRequest {
        from: NodeId,
        shard_id: ShardId,
        /// Priority class per INV-R1.
        priority: ShardPriority,
    },

    /// Shard reconstruction data (responding to recovery request).
    ShardRecoveryData {
        from: NodeId,
        shard_id: ShardId,
        fragment: Vec<u8>,
    },

    // -- Operational --

    /// Solver version announcement (FM-12).
    /// Placement pauses until all active nodes report the same version.
    SolverVersionAnnounce {
        from: NodeId,
        solver_version: Version,
    },

    /// Operational mode transition.
    ModeTransition {
        from: NodeId,
        new_mode: OperationalMode,
        reason: ModeTransitionReason,
        timestamp: Timestamp,
    },
}

/// Priority classes for shard reconstruction (INV-R1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ShardPriority {
    /// Governance units -- highest priority.
    Governance,
    /// Policy units.
    Policy,
    /// Data constraint units.
    DataConstraint,
    /// Workload units -- lowest priority.
    Workload,
}
```

**Producer**: every node (gossip is peer-to-peer).
**Consumer**: every node.
**Channel**: UDP for ping/ack/ping-req. TCP for large payloads (GraphDelta, ShardRecoveryData).
**Ordering**: membership updates use Lamport clocks. Graph deltas use vector clocks
for causal ordering. Key revocations have no ordering guarantee but are idempotent
(processing the same revocation twice is a no-op).
**Delivery**: at-least-once (gossip retransmits via piggybacking). Graph deltas are
crdt-merged (idempotent), so duplicate delivery is harmless.

---

### SecurityEvent

Events produced by the security subsystem. Some propagate locally, others
via priority gossip.

```rust
/// Events emitted by taba-security.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityEvent {
    /// Signature verification succeeded.
    /// Local only, not persisted.
    VerificationPassed {
        unit_id: UnitId,
        author: AuthorId,
        timestamp: Timestamp,
    },

    /// Signature verification failed.
    /// Local only, logged for audit.
    VerificationFailed {
        unit_id: UnitId,
        reason: VerificationFailure,
        timestamp: Timestamp,
    },

    /// A key was revoked. Propagated via priority gossip (KeyRevocation message).
    KeyRevoked {
        author: AuthorId,
        revocation_timestamp: Timestamp,
        revoked_by: AuthorId,
    },

    /// Capability check result (runtime, on node).
    CapabilityCheckResult {
        unit_id: UnitId,
        capability: CapabilityRef,
        allowed: bool,
        policy_id: Option<UnitId>,
        timestamp: Timestamp,
    },

    /// Taint classification computed for a data unit (query-time, INV-S4).
    TaintComputed {
        data_unit_id: UnitId,
        classification: DataClassification,
        inherited_from: Vec<UnitId>,
        timestamp: Timestamp,
    },

    /// Gossip message authentication failed.
    /// Sender flagged for investigation.
    GossipAuthFailure {
        sender: NodeId,
        reason: GossipAuthError,
        timestamp: Timestamp,
    },
}
```

**Producer**: `taba-security`.
**Consumers**: `taba-graph` (VerificationPassed/Failed gate merge), `taba-node`
(CapabilityCheckResult gates runtime access), `taba-gossip` (KeyRevoked triggers
priority gossip broadcast), audit log (all events).
**Channel**: local -- `tokio::broadcast`. KeyRevoked additionally triggers a
GossipMessage::KeyRevocation for cluster-wide propagation.
**Ordering**: none guaranteed across events. Verification events are synchronous
within the merge path (the merge blocks until verification completes).
**Delivery**: at-most-once locally (acceptable -- security decisions are synchronous
gates, not async reactions). KeyRevocation via gossip is at-least-once.

---

### CeremonyEvent

Events related to the Shamir key ceremony lifecycle.

```rust
/// Events emitted during Shamir ceremony operations.
/// Recorded as governance units in the graph after ceremony completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CeremonyEvent {
    /// A new ceremony was initiated.
    CeremonyStarted {
        ceremony_id: CeremonyId,
        initiator: AuthorId,
        total_shares: u8,
        threshold: u8,
        timestamp: Timestamp,
    },

    /// A share was contributed to an in-progress ceremony.
    ShareAdded {
        ceremony_id: CeremonyId,
        share_index: u8,
        contributor: AuthorId,
        timestamp: Timestamp,
    },

    /// Ceremony completed successfully. Root key reconstructed.
    /// The ceremony record becomes a governance unit in the graph.
    CeremonyCompleted {
        ceremony_id: CeremonyId,
        witness: AuthorId,
        trust_domain_created: TrustDomainId,
        timestamp: Timestamp,
    },

    /// Ceremony failed or was aborted.
    CeremonyAborted {
        ceremony_id: CeremonyId,
        reason: CeremonyFailure,
        timestamp: Timestamp,
    },
}
```

**Producer**: `taba-security` (ceremony manager).
**Consumers**: `taba-graph` (CeremonyCompleted creates the root governance unit
that seeds the graph), audit log.
**Channel**: `tokio::mpsc` (ceremony to graph, point-to-point).
**Ordering**: total within a single ceremony (started < share_added* < completed/aborted).
**Delivery**: at-least-once. CeremonyCompleted must durably persist -- it is the
bootstrap event for the entire trust chain.

---

### DistributionEvent

Events related to membership changes and erasure shard management.

```rust
/// Events emitted by taba-gossip and taba-erasure about cluster topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DistributionEvent {
    /// A new node has joined and completed attestation.
    NodeJoined {
        node_id: NodeId,
        listen_addr: String,
        timestamp: Timestamp,
    },

    /// A node is suspected of failure (SWIM suspicion phase).
    NodeSuspected {
        node_id: NodeId,
        suspected_by: NodeId,
        timestamp: Timestamp,
    },

    /// A node was confirmed failed (2+ witnesses, INV-R3).
    NodeFailed {
        node_id: NodeId,
        witnesses: Vec<NodeId>,
        timestamp: Timestamp,
    },

    /// A node has gracefully left the cluster.
    NodeLeft {
        node_id: NodeId,
        timestamp: Timestamp,
    },

    /// Erasure shard redistribution triggered by membership change.
    ShardRedistribution {
        trigger: MembershipTrigger,
        reassignments: Vec<(ShardId, NodeId)>,
        timestamp: Timestamp,
    },

    /// Shard reconstruction completed after node failure.
    ShardReconstructed {
        shard_id: ShardId,
        new_holder: NodeId,
        timestamp: Timestamp,
    },

    /// Reconstruction backpressure activated (INV-R1, FM-13).
    ReconstructionThrottled {
        queue_depth: u32,
        threshold: u32,
        timestamp: Timestamp,
    },
}
```

**Producer**: `taba-gossip` (membership events), `taba-erasure` (shard events).
**Consumers**: `taba-graph` (NodeFailed/NodeLeft triggers shard redistribution),
`taba-solver` (membership changes trigger placement re-evaluation),
`taba-node` (ShardRedistribution triggers local shard acceptance/release).
**Channel**: `tokio::broadcast` (multiple consumers).
**Ordering**: membership events use SWIM protocol ordering (suspect before confirm).
Shard events are causally ordered after the membership event that triggered them.
**Delivery**: at-least-once. Membership state is convergent (SWIM guarantee).
Shard redistribution is idempotent (assigning a shard to a node that already holds
it is a no-op).

---

### ObserveEvent (NEW)

Events produced by the observability subsystem.

```rust
/// Events emitted by taba-observe.
/// These are local events consumed by the graph (for persistence)
/// and integration exporters (for external systems).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObserveEvent {
    /// A decision trail was recorded for a solver run (INV-O1).
    DecisionTrailRecorded {
        trail_id: u64,
        graph_snapshot_id: String,
        placements_count: u32,
        conflicts_count: u32,
        timestamp: DualClockEvent,
    },

    /// Health check result for a workload (INV-O3).
    HealthCheckCompleted {
        unit_id: UnitId,
        node_id: NodeId,
        healthy: bool,
        check_type: String,
        timestamp: DualClockEvent,
    },

    /// Capability change detected on a node after re-probe (INV-N1).
    CapabilityChanged {
        node_id: NodeId,
        added: Vec<String>,
        removed: Vec<String>,
        trigger: String, // "startup", "manual_refresh", "fleet_refresh"
        timestamp: DualClockEvent,
    },

    /// Drift detected between desired and actual state.
    DriftDetected {
        unit_id: UnitId,
        node_id: NodeId,
        expected: String,
        actual: String,
        timestamp: DualClockEvent,
    },

    /// Promotion policy applied to a workload version.
    PromotionApplied {
        unit_id: UnitId,
        version: String,
        environment: String,
        author: AuthorId,
        timestamp: DualClockEvent,
    },

    /// Bounded task spawned by a service.
    TaskSpawned {
        parent_service: UnitId,
        child_task: UnitId,
        delegation_token_id: u64,
        spawn_depth: u8,
        timestamp: DualClockEvent,
    },

    /// Bounded task terminated.
    TaskTerminated {
        unit_id: UnitId,
        reason: String, // "completed", "failed", "deadline_exceeded"
        timestamp: DualClockEvent,
    },

    /// Compaction completed.
    CompactionCompleted {
        units_tombstoned: u32,
        units_removed: u32,
        bytes_freed: u64,
        timestamp: DualClockEvent,
    },

    /// Ephemeral data removed or tombstoned (INV-D4).
    EphemeralDataHandled {
        data_unit_id: UnitId,
        action: String, // "removed" or "tombstoned"
        had_references: bool,
        timestamp: DualClockEvent,
    },

    /// Cross-domain forwarding query executed.
    CrossDomainQueryExecuted {
        querying_domain: TrustDomainId,
        target_domain: TrustDomainId,
        bridge_node: NodeId,
        cached: bool,
        timestamp: DualClockEvent,
    },

    /// Alert dispatched via webhook.
    AlertDispatched {
        event_type: String,
        target_url: String,
        success: bool,
        timestamp: DualClockEvent,
    },
}
```

**Producer**: `taba-observe` (aggregates from all contexts).
**Consumers**: `taba-graph` (DecisionTrailRecorded persisted as graph event),
integration exporters (Prometheus, OpenTelemetry, webhook).
**Channel**: `tokio::broadcast` (multiple consumers).
**Ordering**: none guaranteed across events. Decision trails are causally
ordered by logical clock.
**Delivery**: at-most-once. Observability events are informational — dropped
events do not affect system correctness.

---

### GossipMessage additions (NEW)

New gossip message variants for the analyst-session concepts:

```rust
// Added to GossipMessage enum:

    /// Node capability advertisement (INV-N1).
    /// Sent on startup, refresh, and periodically on significant change.
    CapabilityAdvertisement {
        from: NodeId,
        capabilities: NodeCapabilitySet,
        logical_clock: u64,
    },

    /// Node resource snapshot (INV-N3).
    /// Sent periodically (configurable interval).
    ResourceSnapshot {
        from: NodeId,
        memory_available_bytes: u64,
        cpu_load_ppm: u64,
        disk_available_bytes: u64,
        gpu_available: u32,
        logical_clock: u64,
    },

    /// Cross-domain capability advertisement relay (INV-X5).
    /// Sent by bridge nodes to relay advertisements across domain boundaries.
    CrossDomainAdvertisement {
        from: NodeId, // bridge node
        source_domain: TrustDomainId,
        capability: CapabilityRef,
        conditions: String,
    },

    /// Cross-domain forwarding query (INV-X2).
    ForwardingQuery {
        from: NodeId,
        target_domain: TrustDomainId,
        query_type: String,
        query_payload: Vec<u8>,
        request_id: u64,
    },

    /// Cross-domain forwarding response.
    ForwardingResponse {
        from: NodeId, // bridge node
        request_id: u64,
        result: Vec<u8>, // serialized read-only view
    },

    /// Fleet-wide operational command (governance).
    FleetCommand {
        from: NodeId,
        command_type: String, // "refresh_capabilities", etc.
        governance_unit_id: UnitId,
        logical_clock: u64,
    },
```

---

## Ordering guarantees summary

| Event type | Ordering | Mechanism |
|---|---|---|
| GraphEvent | Causal per unit | `Timestamp` (wall + logical counter) |
| SolverEvent | Total within evaluation pass | Sequential solver execution |
| NodeEvent | Causal per (unit, node) pair | Reconciliation loop sequence |
| GossipMessage (membership) | Causal | Lamport clock on MembershipUpdate |
| GossipMessage (graph delta) | Causal | Vector clock on GraphDelta |
| GossipMessage (key revocation) | None (idempotent) | Priority broadcast |
| SecurityEvent | None (synchronous gates) | Inline in calling path |
| CeremonyEvent | Total within ceremony | Sequential ceremony state machine |
| DistributionEvent | Causal (suspect < confirm < redistribute) | SWIM protocol + sequencing |

**There is no total ordering across contexts.** taba is a peer-to-peer system
with no global clock and no consensus protocol for normal operations. Ordering
is causal where needed and convergent everywhere else.

---

## Delivery semantics summary

| Event type | Delivery | Why acceptable |
|---|---|---|
| GraphEvent | At-most-once (broadcast) | Graph state is source of truth; missed events self-heal on next query |
| SolverEvent | At-least-once | PlacementDecision persisted to graph; reconciliation loop catches drops |
| NodeEvent | At-least-once | Health/state is idempotent (latest wins); reconciliation loop retries |
| GossipMessage | At-least-once (retransmit) | CRDT merge is idempotent; membership converges |
| SecurityEvent | At-most-once (local) | Verification is synchronous gate; KeyRevocation is at-least-once via gossip |
| CeremonyEvent | At-least-once | Ceremony completion is critical bootstrap; must durably persist |
| DistributionEvent | At-least-once | Shard assignment is idempotent; membership converges |

**No event requires exactly-once delivery.** The system is designed so that all
state-mutating operations are either idempotent or gated by synchronous checks.
This is a deliberate consequence of the CRDT architecture.

---

## Event-to-WAL mapping

WAL entries record durable state transitions. Not every event produces a WAL
entry -- only events that change graph state or local node state.

| Event | WAL entry type | Notes |
|---|---|---|
| GraphEvent::UnitMerged | `Merged(unit)` | Full unit payload, written before merge is visible (INV-C4) |
| GraphEvent::UnitPending | `Pending(unit, missing_refs)` | Unit + list of unsatisfied references |
| GraphEvent::UnitPromoted | `Promoted(unit_id)` | Reference to existing Pending entry |
| GraphEvent::PolicySuperseded | `Merged(new_policy)` | New policy is a Merged entry; old policy remains (supersession is structural) |
| GraphEvent::SubgraphArchived | Compaction record | References archived unit IDs; WAL entries for those units may be reclaimed |
| SolverEvent::PlacementDecision | Graph annotation | Placement stored as graph metadata, WAL'd as part of graph mutation |
| NodeEvent::WorkloadStarted | Local state record | Per-node WAL for actual state tracking |
| NodeEvent::WorkloadFailed | Local state record | Per-node WAL for actual state tracking |
| GossipMessage::GraphDelta | `Merged(unit)` per unit in delta | Each unit in the delta is individually verified and WAL'd |
| GossipMessage::KeyRevocation | Revocation record | Durable; must survive restart to prevent accepting revoked-key units |
| CeremonyEvent::CeremonyCompleted | `Merged(governance_unit)` | The root governance unit that seeds the graph |
| DistributionEvent::NodeFailed | Membership state record | Local membership view persisted for restart recovery |

Events not listed (e.g., SecurityEvent::VerificationPassed, GraphEvent::UnitRejected)
are not WAL'd. They are transient observations, not state transitions.

---

## Propagation: gossip vs. local channels

```
                        ┌─────────────────────────────────────┐
                        │            Node boundary            │
                        │                                     │
                        │  ┌──────────┐    ┌──────────────┐   │
                        │  │  taba-   │    │  taba-solver  │   │
                        │  │  graph   │───>│  (reads graph │   │
                        │  │          │    │   snapshot)   │   │
                        │  └────┬─────┘    └──────┬───────┘   │
                        │       │                 │           │
                        │  GraphEvent        SolverEvent      │
                        │  (broadcast)       (mpsc)           │
                        │       │                 │           │
                        │       v                 v           │
                        │  ┌──────────┐    ┌──────────────┐   │
                        │  │  taba-   │    │  taba-node   │   │
                        │  │ security │    │ (reconcile)  │   │
                        │  └────┬─────┘    └──────┬───────┘   │
                        │       │                 │           │
                        │  SecurityEvent     NodeEvent        │
                        │  (broadcast)       (mpsc)           │
                        │       │                 │           │
                        └───────┼─────────────────┼───────────┘
                                │                 │
                    ════════════╪═════════════════╪════════ node boundary
                                │                 │
                                v                 v
                        ┌─────────────────────────────────────┐
                        │         taba-gossip (wire)          │
                        │                                     │
                        │   Signed GossipMessage over UDP/TCP │
                        │   - MembershipUpdate (piggybacked)  │
                        │   - GraphDelta (CRDT sync)          │
                        │   - KeyRevocation (priority)        │
                        │   - ShardRecovery (on demand)       │
                        │   - SolverVersionAnnounce           │
                        │   - ModeTransition                  │
                        │                                     │
                        └──────────────┬──────────────────────┘
                                       │
                                       │  signed, serialized (protobuf)
                                       │
                               ┌───────┴───────┐
                               │   network     │
                               │  (UDP / TCP)  │
                               └───────┬───────┘
                                       │
                                       v
                        ┌─────────────────────────────────────┐
                        │          other peer nodes           │
                        └─────────────────────────────────────┘
```

**Local channels** carry typed Rust enums. Zero serialization cost. Bounded
by channel capacity (backpressure via `tokio::broadcast` lag detection or
`mpsc` bounded send).

**Gossip** carries signed protobuf messages. Serialization + signing cost on
send, deserialization + verification cost on receive. Verification failure
drops the message and flags the sender (SecurityEvent::GossipAuthFailure).

---

## Key event flows

### Flow 1: Unit insertion (happy path)

```
  Author                taba-core          taba-security        taba-graph           taba-solver         taba-node
    │                      │                    │                    │                    │                   │
    │  submit unit         │                    │                    │                    │                   │
    │─────────────────────>│                    │                    │                    │                   │
    │                      │  sign(unit)        │                    │                    │                   │
    │                      │───────────────────>│                    │                    │                   │
    │                      │  signed unit       │                    │                    │                   │
    │                      │<───────────────────│                    │                    │                   │
    │                      │  insert(signed)    │                    │                    │                   │
    │                      │───────────────────────────────────────>│                    │                   │
    │                      │                    │  verify(sig)       │                    │                   │
    │                      │                    │<───────────────────│                    │                   │
    │                      │                    │  ok                │                    │                   │
    │                      │                    │───────────────────>│                    │                   │
    │                      │                    │                    │                    │                   │
    │                      │                    │               [WAL: Merged(unit)]       │                   │
    │                      │                    │                    │                    │                   │
    │                      │                    │                    │  GraphEvent::      │                   │
    │                      │                    │                    │  UnitMerged        │                   │
    │                      │                    │                    │───────────────────>│                   │
    │                      │                    │                    │                    │                   │
    │                      │                    │                    │                    │ SolverEvent::     │
    │                      │                    │                    │                    │ PlacementDecision │
    │                      │                    │                    │                    │─────────────────>│
    │                      │                    │                    │                    │                   │
    │                      │                    │                    │                    │                  [reconcile]
```

### Flow 2: Key revocation propagation

```
  Revoking authority     taba-security       taba-gossip         all peer nodes
        │                     │                   │                    │
        │  revoke(author_id)  │                   │                    │
        │────────────────────>│                   │                    │
        │                     │                   │                    │
        │                     │ SecurityEvent::   │                    │
        │                     │ KeyRevoked        │                    │
        │                     │──────────────────>│                    │
        │                     │                   │                    │
        │                     │                   │ PRIORITY gossip:   │
        │                     │                   │ KeyRevocation      │
        │                     │                   │───────────────────>│
        │                     │                   │                    │
        │                     │                   │                    │ [WAL: revocation record]
        │                     │                   │                    │ [reject future units
        │                     │                   │                    │  from revoked key]
        │                     │                   │                    │
        │                     │                   │  (retransmit via   │
        │                     │                   │   piggybacking     │
        │                     │                   │   until all nodes  │
        │                     │                   │   acknowledge)     │
```

Key revocations bypass the normal gossip queue. They are transmitted
immediately and piggybacked on every subsequent gossip round until all
active nodes have acknowledged receipt.

### Flow 3: Node failure and shard reconstruction

```
  failed node    taba-gossip (peers)    taba-erasure         taba-graph          taba-solver
      │                │                     │                    │                   │
      X (dies)         │                     │                    │                   │
                       │                     │                    │                   │
                  [SWIM detect]              │                    │                   │
                       │                     │                    │                   │
                  DistributionEvent::        │                    │                   │
                  NodeSuspected              │                    │                   │
                       │                     │                    │                   │
                  [2+ witnesses confirm]     │                    │                   │
                       │                     │                    │                   │
                  DistributionEvent::        │                    │                   │
                  NodeFailed                 │                    │                   │
                       │                     │                    │                   │
                       │────────────────────>│                    │                   │
                       │                     │                    │                   │
                       │                ShardRecoveryRequest      │                   │
                       │                (priority queue:          │                   │
                       │                 Gov > Policy > Data      │                   │
                       │                 > Workload)              │                   │
                       │                     │                    │                   │
                       │                     │ [reconstruct]      │                   │
                       │                     │                    │                   │
                       │                DistributionEvent::       │                   │
                       │                ShardReconstructed        │                   │
                       │                     │───────────────────>│                   │
                       │                     │                    │──────────────────>│
                       │                     │                    │                   │
                       │                     │                    │            [re-evaluate
                       │                     │                    │             placements for
                       │                     │                    │             orphaned units]
```

### Flow 4: Operational mode transition

```
  trigger              taba-node            taba-gossip          all peer nodes
    │                     │                     │                     │
    │ (erasure threshold  │                     │                     │
    │  exceeded /         │                     │                     │
    │  memory limit hit)  │                     │                     │
    │────────────────────>│                     │                     │
    │                     │                     │                     │
    │                     │ ModeTransition       │                     │
    │                     │ Normal -> Degraded   │                     │
    │                     │────────────────────>│                     │
    │                     │                     │                     │
    │                     │                     │ GossipMessage::     │
    │                     │                     │ ModeTransition      │
    │                     │                     │────────────────────>│
    │                     │                     │                     │
    │                     │                     │                     │ [freeze authoring,
    │                     │                     │                     │  composition,
    │                     │                     │                     │  placement.
    │                     │                     │                     │  drain only.]
    │                     │                     │                     │
    │                 ... issue resolved ...     │                     │
    │                     │                     │                     │
    │                     │ ModeTransition       │                     │
    │                     │ Degraded -> Recovery │                     │
    │                     │────────────────────>│────────────────────>│
    │                     │                     │                     │
    │                 ... re-coding completes ...│                     │
    │                     │                     │                     │
    │                     │ ModeTransition       │                     │
    │                     │ Recovery -> Normal   │                     │
    │                     │────────────────────>│────────────────────>│
```

---

## Supporting enums (referenced above)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnitType {
    Workload,
    Data,
    Policy,
    Governance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MergeRejection {
    InvalidSignature,
    ScopeViolation,
    KeyRevoked,
    ContextMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictTuple {
    pub unit_ids: Vec<UnitId>,
    pub capability_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ArchivalReason {
    RetentionExpired,
    CompactionThreshold,
    ManualArchival,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictReason {
    IncompatibleCapabilities,
    SecurityConflict,
    AmbiguousMatch,
    CyclicRecoveryDependency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RevocationReason {
    NodeFailed,
    ResourcesExhausted,
    PolicyChange,
    Rebalance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrainReason {
    PlacementRevoked,
    NodeDraining,
    ScaleDown,
    PolicyConflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnitState {
    Declared,
    Composed,
    Placed,
    Running,
    Draining,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationalMode {
    Normal,
    Degraded,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModeTransitionReason {
    ErasureThresholdExceeded,
    MemoryLimitExceeded,
    OperatorTriggered,
    RecoveryComplete,
    RecodingComplete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationFailure {
    InvalidSignature,
    ExpiredValidity,
    KeyRevoked { revocation_timestamp: Timestamp },
    ScopeViolation,
    ContextMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataClassification {
    Public,
    Internal,
    Confidential,
    Pii,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CeremonyFailure {
    InsufficientShares,
    InvalidShare,
    Timeout,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipEntry {
    pub node_id: NodeId,
    pub state: MembershipState,
    pub lamport_clock: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MembershipState {
    Alive,
    Suspected,
    Failed,
    Left,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MembershipTrigger {
    NodeJoined(NodeId),
    NodeFailed(NodeId),
    NodeLeft(NodeId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureParams {
    pub data_shards: u16,
    pub parity_shards: u16,
}
```

---

## Cross-reference to invariants and decisions

| Invariant / Decision | Relevant events |
|---|---|
| INV-S3 (units signed, verified before merge) | SecurityEvent::VerificationPassed/Failed, GraphEvent::UnitMerged/UnitRejected |
| INV-S4 (taint propagation at query time) | SecurityEvent::TaintComputed (computed on demand, not on merge) |
| INV-C3 (deterministic solver) | SolverEvent::PlacementDecision (same on every node) |
| INV-C4 (WAL-before-effect, entry types) | GraphEvent::UnitMerged/UnitPending/UnitPromoted map to Merged/Pending/Promoted |
| INV-C7 (single policy per conflict) | GraphEvent::PolicySuperseded |
| INV-R1 (reconstruction backpressure) | DistributionEvent::ReconstructionThrottled, ShardPriority enum |
| INV-R3 (signed gossip, 2 witnesses) | GossipMessage (all variants signed), DistributionEvent::NodeFailed.witnesses |
| INV-R4 (erasure threshold, degraded mode) | GossipMessage::ModeTransition, DistributionEvent |
| INV-K5 (cyclic recovery) | SolverEvent::CyclicDependency |
| DL-008 (WAL entry types) | Merged, Pending, Promoted mapped in WAL table above |
| DL-009 (signed gossip) | GossipMessage outer signing envelope |
| FM-12 (solver version skew) | GossipMessage::SolverVersionAnnounce |
| FM-13 (reconstruction storm) | DistributionEvent::ReconstructionThrottled |
