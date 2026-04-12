@resilience @consistency
Feature: Network partition handling
  The system handles network splits via CRDT properties (INV-C2).
  Partition tiebreaker is lexicographically lowest NodeId (INV-C3).
  Role-carrying units are disabled on minority side (INV-R2).
  Erasure thresholds determine degraded mode entry (INV-R4).

  # --- Clean partition and heal ---

  Scenario: Partition and heal with CRDT merge produces no conflicts
    Given a 5-node cluster ["n-001", "n-002", "n-003", "n-004", "n-005"]
    And the composition graph contains 10 units with consistent state
    When a network partition splits into side-A ["n-001", "n-002", "n-003"] and side-B ["n-004", "n-005"]
    And no new units are authored during the 60 second partition
    And the partition heals
    Then CRDT merge on all 5 nodes produces identical graph state
    And merge is idempotent: merge(A, A) == A (INV-C2)
    And no duplicate placements exist after convergence

  Scenario: Partition with new non-conflicting units on both sides merges cleanly
    Given a 5-node cluster split into side-A ["n-001", "n-002", "n-003"] and side-B ["n-004", "n-005"]
    And author "alice" (scoped to workload in "ops") creates workload unit "wl-alpha" on side-A
    And author "bob" (scoped to data in "ops") creates data unit "ds-beta" on side-B
    When the partition heals and CRDT merge executes
    Then both "wl-alpha" and "ds-beta" are present in the merged graph on all nodes
    And merge(side-A-state, side-B-state) == merge(side-B-state, side-A-state) (INV-C2)
    And the solver re-evaluates all compositions with the merged graph

  # --- Policy conflict during partition ---

  @consistency
  Scenario: Both sides author policy for same conflict -- supersession resolves (INV-C7)
    Given a 5-node cluster split into side-A and side-B
    And a capability conflict exists between "wl-x" and "wl-y" on capability "shared-db"
    And author "policy-alice" creates policy "pol-1" resolving the conflict with "allow" on side-A at timestamp T1
    And author "policy-bob" creates policy "pol-2" resolving the conflict with "deny" on side-B at timestamp T2 where T2 > T1
    When the partition heals and CRDT merge executes
    Then both "pol-1" and "pol-2" are in the graph
    And "pol-2" must explicitly supersede "pol-1" (versioned lineage chain)
    And if neither supersedes the other, a new conflict is surfaced requiring resolution
    And the solver uses the latest non-revoked policy in the supersession chain

  # --- Role-carrying units on minority side ---

  @security
  Scenario: Role-carrying units disabled on minority side during partition (INV-R2)
    Given a 5-node cluster split into side-A ["n-001", "n-002", "n-003"] (majority) and side-B ["n-004", "n-005"] (minority)
    And author "policy-admin" has policy scope and is reachable only on side-B
    When side-B attempts to use "policy-admin" to author a new policy unit
    Then the policy unit creation is blocked on side-B
    And the system logs "QuorumUnreachable: role-carrying author disabled on minority partition"
    And "policy-admin" can resume authoring after partition heals and majority is reachable

  # --- Stateful workload constraints ---

  @data
  Scenario: Stateful single-writer workload blocked on side without data
    Given workload "wl-writer" declares "state-recovery: single-writer" consuming data unit "ds-main"
    And "wl-writer" and "ds-main" are on side-A ["n-001", "n-002", "n-003"]
    When a network partition separates side-B ["n-004", "n-005"] from side-A
    And the solver on side-B attempts to place a replacement instance of "wl-writer"
    Then the solver refuses placement: "SingleWriterConstraint: data unit ds-main unreachable"
    And side-B does not start any writer instance for "wl-writer"
    And read-only access to cached data remains available on side-B if declared

  # --- Duplicate stateless placement tiebreaker ---

  @consistency
  Scenario: Duplicate stateless placement resolved by lowest NodeId tiebreaker (INV-C3)
    Given stateless workload "wl-stateless" was placed on "n-002" before partition
    And a partition causes side-A to place "wl-stateless" on "n-001" and side-B to place it on "n-004"
    When the partition heals and CRDT merge detects duplicate placements
    Then the deterministic tiebreaker selects "n-001" (lexicographically lowest NodeId)
    And the instance on "n-004" executes its declared on_shutdown handler and drains
    And only one instance of "wl-stateless" remains running after convergence

  # --- Erasure threshold and degraded mode ---

  @resilience
  Scenario: Partition side with fewer than k nodes cannot reconstruct -- degraded mode (INV-R4)
    Given a 7-node cluster with erasure parameters k=5 (resilience=30%)
    And graph shards are coded across all 7 nodes
    When a partition isolates side-B with only 3 nodes ["n-005", "n-006", "n-007"]
    Then side-B detects it has 3 nodes < k=5 required for reconstruction
    And side-B enters Degraded operational mode
    And side-B surfaces operator alert "ErasureThresholdExceeded: 3 nodes < k=5"
    And authoring, composition, and placement are frozen on side-B
    But existing running workloads on side-B continue operating

  Scenario: Majority side continues normally during partition
    Given the same 7-node cluster partitioned with side-A having 4 nodes and k=5
    When side-A has 4 nodes which is also < k=5
    Then side-A also enters Degraded mode
    And neither side can reconstruct shards independently
    And the partition heal is required to restore Normal operations

  # --- Multi-writer data unit ---

  @data @consistency
  Scenario: Multi-writer data unit merges with declared resolution strategy
    Given data unit "ds-shared" declares "consistency: multi-writer" with merge strategy "last-writer-wins"
    And "ds-shared" is accessible on both partition sides
    And side-A writes version V1 to "ds-shared" at timestamp T1
    And side-B writes version V2 to "ds-shared" at timestamp T2 where T2 > T1
    When the partition heals and CRDT merge executes
    Then "ds-shared" resolves to V2 using the declared "last-writer-wins" strategy
    And both V1 and V2 are recorded in the provenance chain for audit
    And the merge is deterministic: any node applying the same writes produces the same result
