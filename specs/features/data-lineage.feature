Feature: Data lineage
  Data units carry provenance that is structural, not reconstructed.

  Scenario: Provenance chain created through composition
    Given workload unit A consumes data unit X
    And workload unit A produces data unit Y
    When the composition is resolved
    Then data unit Y's provenance records: produced by A, from input X

  @security
  Scenario: Taint propagation through composition
    Given data unit X classified as PII
    And workload unit A consumes X and produces Y
    When the composition is resolved
    Then data unit Y inherits PII classification
    And accessing Y requires PII-level authorization

  Scenario: Explicit declassification via policy
    Given data unit Y inherited PII from input X
    And a policy author creates a declassification policy for Y
    When the policy is applied
    Then Y's PII classification is removed
    And the policy is recorded in the audit trail

  Scenario: Retention enforcement
    Given a data unit with retention "30 days"
    When 30 days have elapsed since creation
    Then the data unit is eligible for compaction
    And its provenance subgraph can be archived

  Scenario: Hierarchical data unit constraint inheritance
    Given a parent data unit with classification "confidential"
    And a child data unit within it
    Then the child inherits "confidential" classification
    And the child can add further restrictions
    But the child cannot remove "confidential" without explicit policy
