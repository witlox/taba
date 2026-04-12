Feature: Recovery
  Units declare failure semantics; the solver executes recovery.

  Scenario: Stateless workload recovery
    Given a workload unit declaring "state-recovery: stateless"
    When the workload crashes
    Then the solver restarts it on any available node
    And no state recovery is attempted

  Scenario: Stateful workload with replay
    Given a workload unit declaring "state-recovery: replay-from-offset"
    When the workload crashes
    Then the solver restarts it
    And the workload replays from its last committed offset

  Scenario: Recovery with dependency ordering
    Given workload A declaring "if I fail, drain B first, then restart me"
    When workload A crashes
    Then the solver drains workload B
    Then restarts workload A
    Then reconnects workload B

  @resilience
  Scenario: Circuit breaker on cascading failure
    Given 3 nodes fail in rapid succession
    And remaining nodes cannot absorb all orphaned workloads
    When the solver attempts re-placement
    Then it detects resource exhaustion
    And workloads enter pending state rather than overloading surviving nodes
