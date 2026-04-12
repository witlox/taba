Feature: Unit authoring
  Authors create typed units with capability declarations.

  Scenario: Author creates a valid workload unit
    Given an author with workload scope in trust domain "dev"
    When the author declares a workload unit with capability "needs postgres"
    Then the unit is accepted into the composition graph
    And the unit is signed by the author's key

  Scenario: Author creates unit outside their type scope
    Given an author with workload scope only in trust domain "dev"
    When the author attempts to create a policy unit
    Then the unit is rejected
    And the error indicates scope violation

  Scenario: Author creates unit outside their trust domain
    Given an author with workload scope in trust domain "dev"
    When the author attempts to create a workload unit in trust domain "prod"
    Then the unit is rejected
    And the error indicates trust domain violation

  Scenario: Unit with missing required declaration is rejected
    Given an author with workload scope in trust domain "dev"
    When the author declares a workload unit without any capability declarations
    Then the unit is rejected as malformed

  Scenario: Unsigned unit is rejected on graph merge
    Given a unit that is not signed
    When the unit is submitted to the composition graph
    Then the graph rejects the merge
    And the error indicates missing signature
