Feature: Cross-context interactions
  Verify correct behavior at bounded context boundaries.

  Scenario: Unit insertion flows through validation and signature
    Given a valid workload unit authored by a scoped author
    When the unit is submitted
    Then Unit Management validates the declaration
    And Security signs the unit
    And the Composition Graph verifies the signature and accepts the unit

  Scenario: Graph update triggers solver re-evaluation
    Given an existing composition with units A and B
    When a new unit C is inserted that provides a capability A needs
    Then the solver re-evaluates
    And the composition is updated to include C

  Scenario: Solver placement triggers node reconciliation
    Given the solver places unit X on node N
    When node N's reconciliation loop runs
    Then node N detects the new placement in the graph
    And starts the workload for unit X
    And reports health status back to the graph

  Scenario: Node failure cascades through distribution to solver
    Given a running workload on node N
    When node N fails and gossip detects it
    Then Distribution removes N from membership
    And erasure coding reconstructs N's graph shards
    And the solver recomputes placement for N's workloads
    And another node's reconciliation loop starts the re-placed workloads

  @security
  Scenario: Key revocation invalidates authored units
    Given author A has created 5 units in the graph
    When author A's key is revoked
    Then new units from A are rejected
    And existing units remain (they were valid when signed)
    But new compositions involving A's units require review policy

  Scenario: Security context crosses all boundaries
    Given a workload unit with capability "needs data-store"
    When the full flow executes (validate → sign → insert → solve → place → reconcile)
    Then capability checks occur at composition time (solver)
    And capability checks occur at runtime (node enforcement)
    And both check points agree on the authorization decision
