@observability
Feature: Observability
  Structural observability falls out of the composition graph (decision
  trails, promotion audit, drift detection). Integration observability
  plugs into external systems (OpenTelemetry, Prometheus, alerting hooks).
  Health checks are progressive: OS-level by default, HTTP probe or custom
  command when declared. All follow progressive disclosure.

  Background:
    Given a bootstrapped trust domain "acme" with root governance unit
    And the following nodes in the cluster:
      | node_id   | env  |
      | dev-box   | dev  |
      | ci-runner | test |
      | prod-1    | prod |
      | prod-2    | prod |
    And workload "web-api" is placed on "prod-1"

  # --- Decision trail ---

  Scenario: Solver run produces queryable decision trail
    # INV-O1: every solver run records inputs + outputs
    Given the solver evaluates placement for "web-api" version "main-001"
    When the solver run completes with placement on "prod-1"
    Then a decision trail entry is recorded in the graph:
      | field               | value                              |
      | graph_snapshot_id   | snap-2026-04-13-001                |
      | node_membership     | [dev-box, ci-runner, prod-1, prod-2] |
      | solver_version      | 0.1.0                              |
      | placements          | {web-api: prod-1}                  |
      | conflicts           | []                                 |
    And the decision trail is queryable via graph API
    And the entry is signed by the node that ran the solver

  Scenario: Decision trail enables solver replay
    Given a decision trail entry exists for "web-api" placed on "prod-1" at time T
    When an operator queries "why was web-api placed on prod-1?"
    Then the system retrieves the decision trail for time T
    And replays the solver with the recorded graph snapshot and node membership
    And the replay produces the same placement (prod-1) because the solver is deterministic (INV-C3)
    And the operator sees: capability filter results, resource rankings, and the winning node

  Scenario: Decision trail retention defaults to since-last-compaction
    # INV-O2: default retention, overridable per unit
    Given the graph was last compacted at time T-7d (7 days ago)
    And decision trails exist from T-10d, T-7d, T-3d, T-1d
    When the operator queries decision trails
    Then trails from T-7d, T-3d, T-1d are available (since last compaction)
    And the trail from T-10d has been compacted (before last compaction)

  Scenario: Unit with extended decision retention
    Given workload "critical-service" declares decision_retention = "90d"
    And a governance unit sets trust-domain-wide decision_retention = "30d"
    When decision trails are evaluated for compaction
    Then "critical-service" decision trails are retained for 90 days (unit override)
    And other workloads' decision trails are retained for 30 days (governance default)
    And workloads with no override or governance default retain since-last-compaction

  # --- Promotion audit trail ---

  Scenario: Full promotion audit from git commit to prod placement
    Given the following promotion history for "web-api":
      | event              | timestamp | actor | detail                    |
      | git commit         | T-4h      | alice | commit abc123 on feature  |
      | taba apply (dev)   | T-4h      | alice | placed on dev-box         |
      | git merge to main  | T-3h      | alice | merge to main → main-001  |
      | promotion to test  | T-3h      | CI    | promo-test-001 authored   |
      | placed on test     | T-3h      | solver| placed on ci-runner       |
      | git tag v1.0       | T-1h      | alice | tagged main-001 as v1.0   |
      | promotion to prod  | T-1h      | alice | promo-prod-001 authored   |
      | placed on prod     | T-1h      | solver| placed on prod-1, prod-2  |
    When an operator queries the promotion audit for "web-api" v1.0
    Then the full chain is returned in chronological order
    And every event is signed and verifiable
    And the audit trail is structural (composed from graph events, not a separate log)

  # --- Health checks (progressive) ---

  Scenario: Default health check is OS-level process monitoring
    # INV-O3: default is always active, zero config
    Given workload "simple-service" declares NO health check
    When "simple-service" is running on "prod-1"
    Then the node monitors "simple-service" via OS-level process check (is the process alive?)
    And if the process exits, the node reports health status "unhealthy" to the graph
    And the solver reacts per the workload's failure semantics

  Scenario: Workload with HTTP health check endpoint
    Given workload "web-api" declares health check:
      | field    | value          |
      | type     | http           |
      | path     | /healthz       |
      | port     | 8080           |
      | interval | 10s            |
      | timeout  | 2s             |
    When "web-api" is running on "prod-1"
    Then the node probes GET http://localhost:8080/healthz every 10 seconds
    And a 2xx response means healthy
    And a non-2xx or timeout means unhealthy
    And health status is reported to the graph

  Scenario: Workload with custom command health check
    Given workload "database" declares health check:
      | field    | value                    |
      | type     | command                  |
      | command  | /usr/bin/pg_isready      |
      | interval | 30s                      |
      | timeout  | 5s                       |
    When "database" is running on "prod-1"
    Then the node executes "/usr/bin/pg_isready" every 30 seconds
    And exit code 0 means healthy
    And non-zero exit code means unhealthy
    And health status is reported to the graph

  Scenario: Health check failure triggers workload failure semantics
    Given workload "web-api" has HTTP health check on /healthz
    And "web-api" has failure semantics: restart_on_failure = true, max_restarts = 3
    When the health check returns non-2xx 3 consecutive times
    Then the node marks "web-api" as unhealthy
    And the node restarts "web-api" (attempt 1 of 3)
    And if health check passes after restart, health status returns to "healthy"
    And if all 3 restart attempts fail, the node reports permanent failure
    And the solver re-places "web-api" to another eligible node

  # --- Drift detection ---

  Scenario: Drift detected between desired and actual state
    Given the composition graph shows "web-api" should be running on "prod-1"
    And the actual state on "prod-1" shows "web-api" is not running (process crashed)
    When the node reconciliation loop runs
    Then drift is detected: desired = running, actual = not running
    And a drift detection event is recorded with timestamp
    And the node attempts to reconcile (restart the workload)
    And the drift event is queryable via graph API

  # --- Capability change logging ---

  Scenario: Capability change logged when node re-probes
    Given "prod-1" had capabilities: [runtime:oci, runtime:k8s, os:linux]
    When Docker is removed from "prod-1" and "taba refresh" is run
    Then a capability change event is recorded:
      | field    | value                              |
      | node_id  | prod-1                             |
      | removed  | runtime:oci                        |
      | added    | (none)                             |
      | trigger  | manual refresh                     |
    And the event is queryable via graph API
    And the solver re-evaluates placements that depended on runtime:oci on prod-1

  # --- Integration: structured events ---

  @integration
  Scenario: Structured events emitted for external consumption
    Given the node is configured with log forwarding to stdout (default)
    When the following events occur on "prod-1":
      | event                  |
      | workload placed        |
      | health check passed    |
      | drift detected         |
      | capability re-probed   |
    Then each event is emitted as a structured JSON log line
    And each event includes: timestamp, event_type, node_id, details
    And events can be forwarded to external sinks (syslog, file, log aggregator)

  @integration
  Scenario: Prometheus endpoint exposes node-level metrics
    Given the node exposes a metrics endpoint on a configured port
    When a Prometheus scraper queries the endpoint
    Then the response includes:
      | metric                           | type    |
      | taba_node_memory_available_bytes | gauge   |
      | taba_node_cpu_load               | gauge   |
      | taba_workloads_running           | gauge   |
      | taba_solver_runs_total           | counter |
      | taba_gossip_messages_total       | counter |
      | taba_artifact_cache_size_bytes   | gauge   |
    And metrics are in standard Prometheus exposition format

  # --- Alerting hooks ---

  @integration
  Scenario: Webhook fired on degraded mode transition
    Given the node is configured with an alerting webhook URL
    When "prod-1" transitions to Degraded operational mode
    Then a webhook POST is sent to the configured URL
    And the payload includes: node_id, event "degraded_mode_entered", reason, timestamp
    And the webhook is best-effort (failure to deliver does not block the mode transition)

  Scenario: Webhook fired on promotion policy conflict
    Given the node is configured with an alerting webhook URL
    When two conflicting promotion policies are detected for "web-api" (FM-14)
    Then a webhook POST is sent with event "promotion_conflict"
    And the payload includes: unit_ref, conflicting policy IDs, details
