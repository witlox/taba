Feature: Composition
  The solver matches units' capabilities to compose them.

  Scenario: Two compatible units compose automatically
    Given a workload unit declaring "needs postgres-store"
    And a workload unit declaring "provides postgres-store"
    When the solver evaluates the composition
    Then the composition succeeds with no policy required

  Scenario: Unmatched capability need blocks composition
    Given a workload unit declaring "needs redis-cache"
    And no unit in the graph provides "redis-cache"
    When the solver evaluates the composition
    Then the composition is incomplete
    And the solver reports unsatisfied capability "redis-cache"

  Scenario: Ambiguous capability match requires policy
    Given a workload unit declaring "needs key-value-store"
    And two units providing "key-value-store"
    When the solver evaluates the composition
    Then the solver reports an ambiguity conflict
    And composition is blocked until policy resolves it

  @security
  Scenario: Composition with security conflict fails closed
    Given workload unit A declaring "needs access to data-store X"
    And data unit X declaring "no external access"
    When the solver evaluates the composition
    Then the solver detects a security conflict
    And composition is refused until explicit policy resolves it
