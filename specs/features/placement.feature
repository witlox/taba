@consistency @resilience
Feature: Placement
  The solver assigns composed units to nodes. Placement is deterministic:
  same graph + same node membership = same result on any node. All scoring
  uses fixed-point ppm arithmetic (no floating-point). Placement respects
  resource constraints, tolerance declarations, and node health.

  Background:
    Given a bootstrapped trust domain "acme-prod" with root governance unit
    And a cluster "cluster-1" with active nodes:
      | node_id   | cpu_ppm   | memory_mb | zone   | health  |
      | node-aaa  | 800000    | 8192      | zone-a | active  |
      | node-bbb  | 600000    | 4096      | zone-b | active  |
      | node-ccc  | 400000    | 2048      | zone-a | active  |
    And all solver arithmetic uses fixed-point ppm (10^6 scale, u64/i64)

  # --- Determinism (INV-C3) ---

  Scenario: Deterministic placement -- same graph + nodes = same result
    # INV-C3: any node produces identical placement given identical input
    Given a composed workload unit "web-api" requiring cpu:200000ppm and memory:1024mb
    And the composition graph state is snapshot-id "snap-001"
    When node "node-aaa" runs the solver with graph "snap-001" and node membership [node-aaa, node-bbb, node-ccc]
    And the placement result is saved as "placement-A"
    And node "node-bbb" runs the solver with graph "snap-001" and node membership [node-aaa, node-bbb, node-ccc]
    And the placement result is saved as "placement-B"
    Then "placement-A" and "placement-B" assign "web-api" to the same node
    And all scoring values are identical between the two results
    And no floating-point arithmetic was used in the computation

  Scenario: Fixed-point ppm scoring -- no floating-point
    # DL-004, INV-C3: all arithmetic is u64/i64 at 10^6 scale
    Given a workload unit "compute-heavy" requiring cpu:750000ppm
    And node "node-aaa" has available cpu:800000ppm
    And node "node-bbb" has available cpu:600000ppm
    When the solver computes placement scores
    Then the score for node-aaa is computed using integer arithmetic at ppm scale
    And the score for node-bbb indicates insufficient resources (600000ppm < 750000ppm required)
    And all intermediate values are u64 or i64
    And division rounds toward zero (Rust integer division semantics)

  # --- Resource constraints ---

  Scenario: Placement respects resource constraints
    Given a workload unit "memory-hungry" requiring cpu:100000ppm and memory:5000mb
    And node "node-aaa" has 8192mb total with 2500mb used (5692mb available)
    And node "node-bbb" has 4096mb total with 1000mb used (3096mb available)
    And node "node-ccc" has 2048mb total with 500mb used (1548mb available)
    When the solver evaluates placement for "memory-hungry"
    Then "memory-hungry" is not placed on node-bbb (3096mb < 5000mb required)
    And "memory-hungry" is not placed on node-ccc (1548mb < 5000mb required)
    And "memory-hungry" is placed on node-aaa (5692mb >= 5000mb required)

  # --- Tolerance declarations (INV-K3) ---

  Scenario: Placement respects tolerance declarations
    # INV-K3: latency, failure mode, resource requirements all honored
    Given a workload unit "latency-sensitive" with tolerance declarations:
      | constraint     | value        |
      | latency        | max:10ms     |
      | failure_mode   | restart      |
      | zone           | zone-a only  |
    And node "node-aaa" is in zone-a with measured latency 5ms
    And node "node-bbb" is in zone-b with measured latency 8ms
    And node "node-ccc" is in zone-a with measured latency 12ms
    When the solver evaluates placement for "latency-sensitive"
    Then node-bbb is excluded (zone-b violates zone constraint)
    And node-ccc is excluded (12ms > 10ms latency tolerance)
    And "latency-sensitive" is placed on node-aaa (zone-a, 5ms latency)
    And the placement records which tolerances constrained the decision

  # --- Suspected node avoidance (INV-R5) ---

  Scenario: Suspected node avoided when alternatives exist
    # INV-R5: suspected nodes remain in pool but solver prefers healthy
    Given node "node-ccc" health is changed to "suspected" via SWIM protocol
    And a composed workload unit "web-api" requiring cpu:100000ppm and memory:512mb
    And both node-aaa and node-bbb are healthy and have sufficient resources
    When the solver evaluates placement for "web-api"
    Then "web-api" is placed on node-aaa or node-bbb (both healthy)
    And node-ccc is not selected because alternatives exist
    But node-ccc is NOT removed from the placement pool
    And if node-aaa and node-bbb were both unavailable, node-ccc would be eligible

  # --- Re-placement after failure ---

  Scenario: Re-placement after node failure
    Given a workload unit "web-api" is currently placed on node "node-bbb"
    And node "node-bbb" is declared failed via SWIM multi-probe consensus (2 witnesses)
    When the solver detects the placement is on a failed node
    Then the solver recomputes placement for "web-api" using remaining nodes [node-aaa, node-ccc]
    And "web-api" is placed on the highest-scoring available node
    And the old placement on node-bbb is marked as terminated
    And the re-placement is deterministic (same result on any evaluating node)

  # --- Partition tiebreaker (INV-C3) ---

  Scenario: Partition tiebreaker -- lexicographically lowest NodeId wins
    # INV-C3: deterministic tiebreaker during partition heal
    Given a network partition splits the cluster into side-A [node-aaa, node-bbb] and side-B [node-ccc]
    And both sides independently place workload "web-api":
      | side   | placed_on |
      | side-A | node-aaa  |
      | side-B | node-ccc  |
    When the partition heals and CRDT merge detects duplicate placement
    Then the tiebreaker selects the side containing lexicographically lowest NodeId
    And "node-aaa" < "node-ccc" lexicographically, so side-A wins
    And "web-api" remains on node-aaa
    And the duplicate on node-ccc is marked for drain

  Scenario: Loser drains using declared on_shutdown
    # INV-C3: loser drains immediately using unit's declared shutdown behavior
    Given partition tiebreaker determined side-B (node-ccc) lost for workload "web-api"
    And "web-api" declares on_shutdown: "drain:30s, notify:webhook"
    When node-ccc receives the drain directive
    Then node-ccc initiates drain of "web-api" with 30s timeout
    And in-flight requests are allowed to complete within the drain window
    And after drain completes (or 30s timeout), the workload is terminated on node-ccc
    And the webhook notification is sent as declared in on_shutdown

  # --- Stale snapshot ---

  Scenario: Stale snapshot detected -- solver re-snapshots and retries
    Given the solver on node-aaa begins evaluation with snapshot "snap-old" at version 42
    And during evaluation, new units are merged advancing the graph to version 45
    When the solver detects its snapshot is stale (version 42 < current 45)
    Then the solver aborts the current evaluation
    And the solver takes a fresh snapshot "snap-new" at version 45
    And the solver retries evaluation with the fresh snapshot
    And the retry produces a placement based on the latest graph state

  # --- Solver version gating (FM-12) ---

  Scenario: Solver version gating -- mixed-version cluster pauses placement
    # FM-12: solver will not produce placements until all nodes report same version
    Given node-aaa reports solver version "2.1.0" via gossip
    And node-bbb reports solver version "2.1.0" via gossip
    And node-ccc reports solver version "2.0.0" via gossip (upgrade in progress)
    When a composed workload "web-api" is ready for placement
    Then the solver detects version mismatch: [2.1.0, 2.1.0, 2.0.0]
    And placement is paused with reason "solver version skew: 2.0.0 != 2.1.0"
    And existing workloads continue running on their current nodes
    When node-ccc upgrades to version "2.1.0" and reports via gossip
    Then all nodes report "2.1.0" and placement resumes
    And the solver evaluates pending placements including "web-api"
