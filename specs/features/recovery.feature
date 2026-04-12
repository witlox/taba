@resilience
Feature: Recovery
  Units declare failure semantics and recovery strategies. The solver
  executes recovery respecting dependency ordering. Cyclic dependencies
  fail closed (INV-K5). Reconstruction uses backpressure with priority
  queue (INV-R1). WAL provides causal buffering (INV-C4).

  # --- Stateless recovery ---

  Scenario: Stateless workload recovery restarts on any available node
    Given workload "wl-api" declares "state-recovery: stateless" on node "n-003"
    And nodes "n-001", "n-002", "n-004" are Active and have capacity
    When "wl-api" crashes on "n-003" and the node reports failure
    Then the solver recomputes placement for "wl-api"
    And "wl-api" is placed on one of ["n-001", "n-002", "n-004"] based on solver scoring
    And no state recovery or replay is attempted
    And "wl-api" transitions from Declared to Placed to Running on the new node

  # --- Stateful replay ---

  Scenario: Stateful workload replays from committed offset after crash
    Given workload "wl-ingest" declares "state-recovery: replay-from-offset"
    And "wl-ingest" last committed offset 42857 to the WAL
    When "wl-ingest" crashes on node "n-002"
    Then the solver restarts "wl-ingest" (on "n-002" or another node)
    And "wl-ingest" replays events starting from offset 42857
    And processing resumes from offset 42858 after replay completes
    And no data loss occurs for events at or before offset 42857

  # --- Dependency ordering ---

  Scenario: Recovery respects declared dependency ordering
    Given workload "wl-db" provides capability "postgres-store"
    And workload "wl-app" needs capability "postgres-store" and declares recovery dependency on "wl-db"
    And workload "wl-cache" provides capability "redis-cache" with no recovery dependency on "wl-db"
    When "wl-db", "wl-app", and "wl-cache" all crash due to node failure
    Then the solver recovers "wl-db" first
    And waits for "wl-db" to reach Running state
    Then recovers "wl-app" which depends on "wl-db"
    And recovers "wl-cache" in parallel with "wl-app" (no dependency)
    And all three reach Running state with correct startup ordering

  # --- Cyclic dependency ---

  @consistency
  Scenario: Cyclic recovery dependency fails closed and requires policy (INV-K5)
    Given workload "wl-alpha" declares recovery dependency on "wl-beta"
    And workload "wl-beta" declares recovery dependency on "wl-alpha"
    When both "wl-alpha" and "wl-beta" crash simultaneously
    Then the solver detects a circular recovery dependency chain
    And the solver reports an unresolvable conflict requiring explicit policy
    And both workloads remain in Pending state (fail closed)
    And an operator must author a policy unit declaring restart priority
    And if no policy exists, tiebreaker assigns priority to "wl-alpha" (lexicographically lowest UnitId)

  # --- Circuit breaker ---

  @resilience
  Scenario: Circuit breaker prevents cascading failure by pending rather than overloading
    Given a 5-node cluster with 4 nodes Active and 1 node Suspected
    And nodes "n-001", "n-002", "n-003" fail in rapid succession within 10 seconds
    And 15 workloads are orphaned from the failed nodes
    And surviving node "n-004" has capacity for only 5 workloads
    When the solver attempts re-placement of all 15 orphaned workloads
    Then the solver places 5 workloads on "n-004" up to capacity
    And the remaining 10 workloads enter Pending state
    And no workload is placed that would exceed "n-004" declared resource limits
    And an operator alert is surfaced: "PlacementExhausted: 10 workloads pending, insufficient capacity"

  # --- Reconstruction backpressure ---

  @resilience
  Scenario: Reconstruction backpressure uses priority queue governance > policy > data > workload (INV-R1)
    Given node "n-003" fails holding shards for 4 unit types:
      | shard_id | unit_type  | priority |
      | s-gov-1  | governance | 1        |
      | s-pol-2  | policy     | 2        |
      | s-dat-3  | data       | 3        |
      | s-wkl-4  | workload   | 4        |
    When erasure reconstruction begins on surviving nodes
    Then shard "s-gov-1" (governance) is reconstructed first
    And shard "s-pol-2" (policy) is reconstructed second
    And shard "s-dat-3" (data) is reconstructed third
    And shard "s-wkl-4" (workload) is reconstructed last
    And reconstruction is throttled to prevent I/O overload on surviving nodes

  Scenario: Reconstruction circuit breaker triggers when queue depth exceeds threshold
    Given 3 nodes fail in succession causing 50 shards to need reconstruction
    And the reconstruction queue depth threshold is configured at 30
    When the reconstruction queue reaches 35 pending shards
    Then the circuit breaker activates
    And new reconstruction requests are paused
    And an operator alert is surfaced: "ReconstructionCircuitBreaker: queue depth 35 > threshold 30"
    And in-progress reconstructions complete but no new ones start until queue drains below threshold

  @resilience
  Scenario: Post-reconstruction signature re-verification (INV-R1)
    Given shard "s-pol-2" is reconstructed from surviving erasure-coded fragments
    When the reconstructed shard is decoded into the original policy unit
    Then the unit's cryptographic signature is re-verified against the author's public key
    And the author's scope validity at creation time is re-checked
    And the author's key revocation status is re-checked
    And only after all verification passes is the unit merged into the local graph

  # --- WAL failures ---

  @resilience
  Scenario: WAL corruption causes node to enter degraded mode (FM-07)
    Given node "n-002" detects WAL corruption during a write operation
    When "n-002" cannot persist the graph mutation atomically
    Then "n-002" enters Degraded operational mode
    And "n-002" stops accepting new placements
    And "n-002" announces Degraded status via signed gossip
    And "n-002"'s graph shards are reconstructable from peers via erasure coding
    And "n-002" requires operator intervention to repair and rejoin

  # --- Causal buffering ---

  @consistency
  Scenario: WAL causal buffering promotes pending unit when refs arrive (INV-C4, DL-008)
    Given unit "u-child" references parent unit "u-parent" which is not yet in the local graph
    When "u-child" is received and its signature is verified
    Then "u-child" is written to WAL as Pending(u-child, missing_refs=[u-parent])
    And "u-child" is not visible to local queries
    When unit "u-parent" arrives and is verified and merged into the graph
    Then "u-child" is promoted: WAL records Promoted(u-child)
    And "u-child" becomes visible to local queries and solver evaluation
    And the promotion is atomic with respect to WAL ordering
