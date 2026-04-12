@operational @resilience
Feature: Operational modes
  System-wide state affecting permitted operations. Three modes:
  Normal (all operations), Degraded (drain/evacuation only), and
  Recovery (throttled placement, re-coding underway). Transitions
  are triggered by resource thresholds (INV-R6), erasure threshold
  violations (INV-R4), or operator action.

  # --- Normal mode ---

  Scenario: Normal mode permits all operations
    Given node "n-001" is in Normal operational mode
    When author "alice" submits a new workload unit "wl-new"
    Then the unit is accepted for graph insertion
    And the solver evaluates composition and placement
    And "wl-new" is placed and transitions to Running
    And authoring, composition, placement, and drain are all permitted

  # --- Degraded mode restrictions ---

  Scenario: Degraded mode freezes authoring, composition, and placement
    Given node "n-002" is in Degraded operational mode
    When author "alice" attempts to submit a new workload unit "wl-blocked"
    Then the submission is rejected with error "NodeDegraded: authoring frozen"
    And when the solver attempts to place a unit on "n-002"
    Then placement is rejected with error "NodeDegraded: placement frozen"
    And composition evaluation for units targeting "n-002" is suspended

  Scenario: Degraded mode permits drain and evacuation
    Given node "n-002" is in Degraded operational mode
    And "n-002" is running workloads ["wl-a", "wl-b"]
    When the operator initiates drain on "n-002"
    Then "wl-a" and "wl-b" are re-placed on other Active nodes
    And each workload executes its declared on_shutdown handler
    And the drain completes successfully despite Degraded state

  # --- Recovery mode ---

  @resilience
  Scenario: Recovery mode throttles placement while re-coding underway
    Given node "n-002" is in Recovery operational mode
    And erasure re-coding is 60% complete
    When the solver has 5 pending placements
    Then placements are throttled to 1 per re-coding cycle
    And re-coding operations have priority over new placements
    And existing running workloads are unaffected

  # --- Memory limit triggers ---

  @operational
  Scenario: Auto-compaction triggers at 80% memory limit (INV-R6)
    Given node "n-003" has a configured graph memory limit of 1024 MB
    And the active graph on "n-003" currently uses 820 MB (80.1%)
    When the memory monitor detects usage exceeds 80% threshold
    Then auto-compaction is triggered on "n-003"
    And expired data units (per INV-D2) are compacted first
    And archived subgraphs are removed from active memory
    And "n-003" remains in Normal mode during compaction
    And graph usage decreases after compaction completes

  @operational
  Scenario: Memory limit exceeded at 100% triggers Degraded mode (INV-R6)
    Given node "n-003" has a configured graph memory limit of 1024 MB
    And auto-compaction is running but graph usage reaches 1030 MB (100.6%)
    When the memory monitor detects usage exceeds 100% of limit
    Then "n-003" transitions from Normal to Degraded operational mode
    And "n-003" announces Degraded status via signed gossip message
    And "n-003" refuses new placements until compaction reduces usage below limit
    And an operator alert is surfaced: "MemoryLimitExceeded: 1030MB > 1024MB limit"

  # --- Erasure threshold triggers ---

  @resilience
  Scenario: Erasure threshold exceeded triggers Degraded mode (INV-R4)
    Given a 7-node cluster with erasure parameters k=5 (resilience=30%)
    And 3 nodes fail leaving only 4 surviving nodes
    When the system detects surviving nodes (4) < k (5)
    Then all surviving nodes enter Degraded operational mode
    And an operator alert is surfaced: "ErasureThresholdExceeded: 4 nodes < k=5, shards may be unrecoverable"
    And authoring, composition, and placement are frozen cluster-wide
    And existing running workloads continue operating

  # --- Mode transitions ---

  @operational
  Scenario: Degraded to Recovery when trigger resolved
    Given node "n-003" is in Degraded mode due to memory limit exceeded
    And auto-compaction reduces graph usage to 700 MB (68% of 1024 MB)
    When the memory monitor confirms usage is below 80% threshold
    Then "n-003" transitions from Degraded to Recovery mode
    And erasure re-coding begins for any under-replicated shards on "n-003"
    And "n-003" accepts placement at throttled rate during Recovery

  @operational
  Scenario: Recovery to Normal when re-coding complete
    Given node "n-003" is in Recovery mode
    And erasure re-coding is underway for 8 under-replicated shards
    When all 8 shards complete re-coding and redundancy is fully restored
    Then "n-003" transitions from Recovery to Normal mode
    And full placement rate resumes on "n-003"
    And "n-003" announces Normal status via signed gossip

  # --- Operator-triggered ---

  @operational
  Scenario: Operator manually triggers Degraded mode
    Given node "n-004" is in Normal operational mode
    When the operator issues a manual degraded command for "n-004" with reason "planned-maintenance"
    Then "n-004" transitions to Degraded operational mode
    And the reason "planned-maintenance" is recorded in the mode transition event
    And "n-004" announces Degraded status via signed gossip
    And authoring, composition, and placement are frozen on "n-004"
    And the operator can later trigger Recovery by resolving the manual hold
