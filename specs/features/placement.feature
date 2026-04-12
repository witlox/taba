Feature: Placement
  The solver assigns composed units to nodes deterministically.

  Scenario: Deterministic placement
    Given a composition graph G and node set N
    When node X runs the solver on (G, N)
    And node Y runs the solver on (G, N)
    Then both produce identical placement decisions

  Scenario: Placement respects resource constraints
    Given a workload unit requiring 4 GPUs
    And no node with 4 available GPUs
    When the solver computes placement
    Then the unit enters pending state
    And the solver reports insufficient resources

  Scenario: Placement respects tolerance declarations
    Given a workload unit declaring "tolerates max 10ms latency to data-store"
    And data-store placed on node A
    When the solver evaluates placement on node B with 50ms latency to A
    Then node B is excluded from placement candidates

  @resilience
  Scenario: Re-placement after node failure
    Given a workload running on node A
    When node A is detected as failed via gossip
    Then the solver recomputes placement for orphaned workloads
    And surviving nodes reconcile to start the workload
