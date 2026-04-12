Feature: Security enforcement
  Capability-based security with zero-access default.

  @security
  Scenario: Unit cannot access undeclared capability
    Given a running workload unit that declared "needs postgres-store"
    When the workload attempts to access "redis-cache"
    Then access is denied
    And the denial is logged

  @security
  Scenario: Zero-access default
    Given a newly placed workload unit with no capability declarations
    Then the workload can access nothing
    And any access attempt is denied and logged

  @security
  Scenario: Build provenance enforcement
    Given a trust domain requiring SLSA build attestation
    And a workload unit without build provenance
    When the solver evaluates placement in that trust domain
    Then placement is refused
    And the error indicates missing attestation

  @security
  Scenario: Signed unit verification on graph merge
    Given a unit signed by author A
    When a node receives the unit for graph merge
    Then the node verifies the signature against author A's public key
    And verifies author A has scope for this unit type and trust domain
    And merges only if both checks pass
