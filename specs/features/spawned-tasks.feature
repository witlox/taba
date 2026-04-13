@lifecycle @spawning
Feature: Spawned bounded tasks
  Services can spawn bounded tasks at runtime with delegated authority.
  Spawned tasks are full graph units with provenance linking to parent.
  Tasks terminate on completion, failure, or deadline. Max spawn depth: 4.
  Ephemeral data produced by tasks is auto-removed on termination.

  Background:
    Given a bootstrapped trust domain "acme" with root governance unit
    And author "alice" with workload scope in "acme"
    And service "web-api" running on node "prod-1" authored by alice

  # --- Spawning lifecycle ---

  Scenario: Service spawns a bounded task with delegated authority
    # INV-W4: spawned task inherits parent trust context
    Given "web-api" has authority to spawn bounded tasks in "acme"
    When "web-api" spawns bounded task "migrate-v2":
      | field             | value                          |
      | artifact.type     | oci                            |
      | artifact.ref      | acme/migrate:v2                |
      | validity_window   | LC 1000..LC 2000               |
      | spawned_by        | web-api                        |
    Then "migrate-v2" is signed with "web-api"'s delegated authority
    And "migrate-v2" is inserted into the graph as a full workload unit
    And provenance links "migrate-v2" → spawned-by → "web-api"
    And "migrate-v2" inherits trust domain "acme" from parent
    And the solver evaluates placement for "migrate-v2"

  Scenario: Bounded task terminates on successful completion
    # INV-W2: auto-terminate on completion
    Given bounded task "migrate-v2" is running on "prod-1"
    When "migrate-v2" completes successfully (exit code 0)
    Then "migrate-v2" transitions to Terminated state
    And termination reason is "completed"
    And "migrate-v2" is eligible for compaction (INV-G5 priority 3)
    And the parent "web-api" is notified of completion via graph event

  Scenario: Bounded task terminates on failure after retry exhaustion
    # INV-W2: auto-terminate on failure
    Given bounded task "import-job" with failure semantics: max_retries = 3
    And "import-job" is running on "prod-1"
    When "import-job" fails (exit code 1)
    Then the node restarts "import-job" (attempt 1 of 3)
    When "import-job" fails again 3 times
    Then "import-job" transitions to Terminated state
    And termination reason is "failed (retries exhausted)"
    And the parent service is notified of failure

  Scenario: Bounded task terminates on logical clock deadline
    # INV-W2: deadline termination, FM-22
    Given bounded task "timeout-job" with validity_window LC 1000..LC 1500
    And the cluster logical clock advances past LC 1500
    When the node detects "timeout-job" has exceeded its deadline
    Then the node forcefully terminates the task process
    And "timeout-job" transitions to Terminated with reason "deadline exceeded"
    And partial output is handled per the spawning service's failure semantics

  Scenario: Bounded task terminates on wall-time deadline
    Given bounded task "batch-report" with wall_time_deadline = "2026-04-13T18:00:00Z"
    And the current wall time passes "2026-04-13T18:00:00Z"
    When the node detects "batch-report" has exceeded its wall-time deadline
    Then the node forcefully terminates "batch-report"
    And termination reason is "wall-time deadline exceeded"

  # --- Spawn depth enforcement ---

  Scenario: Spawn depth enforced at graph merge (max depth 4)
    # INV-W3: depth enforced at merge
    Given the following spawn chain:
      | depth | unit_id          | type          | spawned_by       |
      | 1     | web-api          | service       | (root)           |
      | 2     | migrate-v2       | bounded task  | web-api          |
      | 3     | validate-tables  | bounded task  | migrate-v2       |
      | 4     | cleanup-temp     | bounded task  | validate-tables  |
    And all 4 units are in the graph
    When "cleanup-temp" attempts to spawn "deep-task" (would be depth 5)
    Then the spawned unit is rejected at graph merge
    And the error is "spawn depth exceeded: max 4, attempted 5"
    And "cleanup-temp" is notified of the rejection

  Scenario: Governance can override max spawn depth
    Given trust domain "acme" has governance: max_spawn_depth = 6
    And a spawn chain at depth 4
    When the depth-4 task spawns a sub-task (depth 5)
    Then the spawn succeeds (governance allows depth 6)

  # --- Ephemeral data from bounded tasks ---

  @data
  Scenario: Ephemeral data auto-removed on task termination
    # INV-D4: ephemeral data removed on producing task termination
    Given bounded task "etl-job" produces data unit "staging-data" with retention = "ephemeral"
    And "staging-data" is visible in the graph while "etl-job" runs
    When "etl-job" completes successfully
    Then "staging-data" is fully removed from the graph
    And no tombstone is created for "staging-data" (default for ephemeral)
    And graph space is immediately reclaimed

  @data
  Scenario: Ephemeral data with classification requires governance policy for local-only
    # INV-D5: local-only for classified data needs policy
    Given bounded task "pii-processor" needs to produce ephemeral data
    And the data has classification "PII"
    When the author attempts to declare the data as local-only
    Then the declaration is rejected: "local-only requires policy for classification > public"
    But declaring it as ephemeral (in-graph) succeeds
    And "PII" taint propagation applies during the task's lifetime

  # --- Spawned task composition ---

  Scenario: Spawned task participates in normal composition
    Given bounded task "data-loader" needs capability "postgres-compatible"
    And data unit "pg-primary" provides "postgres-compatible"
    When the solver evaluates composition for "data-loader"
    Then "data-loader" composes with "pg-primary" normally
    And capability matching follows standard rules (INV-K2)
    And the composition includes the spawn provenance link

  Scenario: Spawned task placed on appropriate node by solver
    Given bounded task "gpu-process" with artifact.type = "native" and needs "gpu:cuda"
    And node "prod-gpu" has capability "gpu:cuda"
    And node "prod-1" does NOT have "gpu:cuda"
    When the solver evaluates placement for "gpu-process"
    Then "gpu-process" is placed on "prod-gpu" (capability match)
    And placement follows standard rules (INV-N2 hard constraints, INV-N3 soft ranking)

  # --- Spawned task recovery ---

  @resilience
  Scenario: Node failure during bounded task execution
    Given bounded task "long-import" running on "prod-1" (spawned by "web-api")
    And "long-import" has placement_on_failure = "replace" (non-default for bounded tasks)
    When "prod-1" fails
    Then the solver re-places "long-import" to another eligible node
    And "long-import" restarts from scratch (or replay-from-offset per state recovery declaration)
    And the spawn provenance link to "web-api" is preserved

  Scenario: Bounded task default placement-on-failure respects environment
    Given bounded task "dev-test" running on dev node "dev-laptop" (env:dev)
    And "dev-test" does not override placement_on_failure
    When "dev-laptop" goes offline
    Then "dev-test" is left dead (env:dev default per INV-N5)

  # --- Parent-child failure semantics ---

  Scenario: Parent service termination cascades to spawned tasks
    Given "web-api" has spawned bounded tasks "task-a" and "task-b"
    And both tasks are currently running
    When "web-api" is terminated (drained)
    Then "task-a" and "task-b" receive termination signals
    And both tasks are drained per their declared failure semantics
    And both tasks transition to Terminated
    And ephemeral data from both tasks is auto-removed

  Scenario: Spawned task failure does not terminate parent service
    Given "web-api" spawned "task-c" for a one-off migration
    When "task-c" fails after exhausting retries
    Then "web-api" is notified of "task-c"'s failure via graph event
    But "web-api" continues running unaffected
    And "web-api" can spawn a new task to retry the migration

  # --- Observability ---

  Scenario: Spawned task appears in decision trail with parent link
    Given "web-api" spawns "cleanup-job"
    And the solver places "cleanup-job" on "prod-2"
    When the decision trail is recorded for "cleanup-job" placement
    Then the decision trail includes: spawned_by = "web-api"
    And the audit chain shows: web-api → spawned → cleanup-job → placed on prod-2
    And the spawning event is queryable as a graph event

  Scenario: Health check applies to spawned tasks independently
    Given bounded task "long-job" declares health check: type = "command", command = "/check.sh"
    And "long-job" is running on "prod-1"
    When the node executes the health check
    Then health status is reported independently from parent "web-api"
    And if "long-job" is unhealthy, it is restarted per its own failure semantics
    And parent "web-api" health is unaffected
