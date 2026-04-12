Feature: Network partition
  The system handles network splits via CRDT properties.

  @resilience
  Scenario: Partition and heal with no state conflicts
    Given a cluster split into partition A (3 nodes) and partition B (2 nodes)
    And no new units authored during the partition
    When the partition heals
    Then CRDT merge produces identical graph on all nodes
    And no duplicate placements exist

  @resilience
  Scenario: Partition with new units authored on both sides
    Given a cluster split into two partitions
    And different authors create non-conflicting units on each side
    When the partition heals
    Then CRDT merge includes all units from both sides
    And the solver re-evaluates compositions with the merged graph

  @resilience
  Scenario: Partition causes duplicate placement of stateful workload
    Given a stateful workload with "single-writer" data unit
    And a partition separates the workload from its data
    When partition side B attempts to start a replacement writer
    Then the solver refuses (single-writer constraint)
    And only read replicas are permitted on side B

  @resilience
  Scenario: Duplicate placement resolution after heal
    Given both sides placed the same stateless workload during partition
    When the partition heals
    Then deterministic tiebreaker selects one placement (lowest node ID)
    And the other placement drains
