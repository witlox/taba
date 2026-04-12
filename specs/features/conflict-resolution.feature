Feature: Conflict resolution
  Security and capability conflicts require explicit policy.

  Scenario: Policy resolves a security conflict
    Given a security conflict between units A and B
    And a policy author creates a policy unit resolving the conflict
    When the solver re-evaluates the composition
    Then the composition succeeds
    And the policy unit is part of the audit trail

  Scenario: Policy cannot be created by workload author
    Given a security conflict between units A and B
    And an author with workload scope only
    When the author attempts to create a policy unit
    Then the attempt is rejected due to scope violation

  Scenario: Conflicting policies on the same conflict
    Given a security conflict between units A and B
    And two policy units resolving the same conflict differently
    When the solver evaluates
    Then the solver detects a policy conflict
    And requires meta-policy or escalation

  Scenario: Orphaned policy referencing non-existent conflict
    Given a policy unit referencing a conflict that does not exist
    When the policy is submitted to the graph
    Then it is rejected as invalid
