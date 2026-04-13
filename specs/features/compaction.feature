@compaction @lifecycle
Feature: Graph compaction and archival
  Compaction reclaims graph space by replacing eligible units with tombstones.
  Eligibility is deterministic (all nodes agree). Timing is local. Eviction
  is a separate node-local operation for memory pressure relief. Archival
  optionally preserves full unit content in cold storage before tombstoning.

  Background:
    Given a bootstrapped trust domain "acme" with root governance unit
    And the following nodes in the cluster:
      | node_id  | env  |
      | prod-1   | prod |
      | prod-2   | prod |
    And the graph memory limit is set to 100MB per node

  # --- Bounded task compaction ---

  Scenario: Terminated bounded task is eligible for compaction
    # INV-W2, INV-G1: terminated tasks are deterministically eligible
    Given service "web-api" spawned bounded task "migrate-v2" at logical clock 1000
    And "migrate-v2" completed successfully at logical clock 1050
    When the compaction scan runs
    Then "migrate-v2" is eligible for compaction
    And "migrate-v2" is replaced with a tombstone:
      | field              | value                     |
      | unit_id            | migrate-v2                |
      | unit_type           | bounded_task              |
      | created_at          | LC 1000                   |
      | terminated_at       | LC 1050                   |
      | termination_reason  | completed                 |
      | references          | [web-api (spawned-by)]    |
      | original_digest     | sha256:abc123             |
    And the tombstone preserves the provenance link to "web-api"
    And both prod-1 and prod-2 agree on eligibility (INV-G1)

  # --- Ephemeral data removal ---

  Scenario: Ephemeral data auto-removed on producing task termination
    # INV-D4: ephemeral data removed, no tombstone by default
    Given bounded task "etl-job" produces ephemeral data unit "temp-staging"
    And "temp-staging" has retention = "ephemeral"
    When "etl-job" terminates (completed)
    Then "temp-staging" is fully removed from the graph (no tombstone)
    And no archive is created for "temp-staging"
    And graph space is immediately reclaimed

  Scenario: Governance mandates tombstone for ephemeral data
    Given a governance unit in "acme" declares: ephemeral_data_tombstone = true
    And bounded task "audit-job" produces ephemeral data unit "temp-audit-data"
    And "temp-audit-data" has retention = "ephemeral"
    When "audit-job" terminates (completed)
    Then "temp-audit-data" is tombstoned (NOT fully removed)
    And the tombstone preserves references for audit trail
    And governance override takes precedence over default removal

  # --- Provenance integrity through compaction ---

  @data
  Scenario: Tombstoned workload preserves provenance chain for live data
    # INV-G2, INV-D1: provenance chain intact through tombstones
    Given workload "data-processor" (terminated) produced data unit "output-dataset" (live)
    And "output-dataset" has provenance referencing "data-processor"
    When "data-processor" is compacted into a tombstone
    Then "output-dataset" provenance query returns: "produced by data-processor (tombstoned)"
    And the tombstone's references field includes "output-dataset"
    And the provenance chain from "output-dataset" back through "data-processor" is intact
    And INV-D1 (unbroken provenance) is satisfied

  Scenario: Full details of tombstoned unit retrieved from archive
    Given "data-processor" was archived to local path before tombstoning
    And the tombstone records original_digest = "sha256:proc456"
    When an operator queries full details of "data-processor"
    Then the system retrieves from archive by digest "sha256:proc456"
    And verifies the content matches the digest
    And returns the full original unit content

  # --- Superseded policy compaction ---

  Scenario: Superseded policy compacted after successor is stable
    # INV-G5: superseded policies compacted at priority 4
    Given policy "promo-v1" was superseded by "promo-v2" at logical clock 2000
    And "promo-v2" has not itself been superseded (stable)
    And a grace period has elapsed since supersession
    When the compaction scan runs
    Then "promo-v1" is eligible for compaction
    And "promo-v1" is tombstoned with termination_reason = "superseded"
    And the tombstone references "promo-v2" as successor

  # --- Never-compact entities ---

  @governance
  Scenario: Governance units are never compacted
    # INV-G3: governance, active policies, and root ceremony are exempt
    Given trust domain governance unit "acme-root" created at logical clock 1
    And role assignment governance unit "alice-role" created at logical clock 5
    And both are active and not superseded
    When the node is under memory pressure (95% of limit)
    And the compaction scan runs
    Then "acme-root" is NOT eligible for compaction
    And "alice-role" is NOT eligible for compaction
    And compaction targets lower-priority units first (ephemeral > trails > tasks > policies > services)

  Scenario: Active policy is never compacted even under pressure
    Given policy "security-policy-1" resolves an active conflict between live units
    And "security-policy-1" has NOT been superseded
    When the node is under severe memory pressure
    Then "security-policy-1" is NOT eligible for compaction
    And eviction (node-local content drop) may occur for other units instead

  # --- Compaction vs eviction ---

  @resilience
  Scenario: Eviction drops content locally without tombstoning
    # INV-G4: eviction is a cache operation, not a lifecycle event
    Given node "prod-1" is at 92% memory limit
    And workload unit "large-service" has a 5MB unit declaration
    And "large-service" is still active (Running state)
    When "prod-1" evicts "large-service" content to relieve pressure
    Then "large-service" is NOT tombstoned (still live in the graph)
    And "prod-1" drops the full unit content from local memory
    And "prod-1" retains a minimal reference (UnitId + shard location)
    And if "prod-1" needs the full content later, it reconstructs from peers (erasure coding)
    And "prod-2" still has the full content (eviction is node-local)

  # --- Compaction priority order ---

  Scenario: Compaction follows priority order under memory pressure
    # INV-G5: least valuable compacted first
    Given the graph contains:
      | unit_id           | type              | status          |
      | temp-data         | ephemeral data    | task terminated |
      | old-trail         | decision trail    | past retention  |
      | finished-task     | bounded task      | terminated      |
      | old-policy        | superseded policy | successor stable|
      | stopped-service   | service           | terminated      |
      | expired-dataset   | data unit         | retention expired|
      | active-governance | governance        | active          |
    When compaction runs under memory pressure
    Then units are compacted in order:
      | order | unit_id         | treatment |
      | 1     | temp-data       | remove    |
      | 2     | old-trail       | remove    |
      | 3     | finished-task   | tombstone |
      | 4     | old-policy      | tombstone |
      | 5     | stopped-service | tombstone |
      | 6     | expired-dataset | tombstone |
    And "active-governance" is never compacted (INV-G3)

  # --- Archival ---

  @compliance
  Scenario: Governance-mandated archival before compaction
    Given trust domain "acme" has governance: archive_required = true for data units
    And data unit "financial-records" has retention expired (wall time)
    And an archive backend is configured (local path: /archive)
    When compaction targets "financial-records"
    Then "financial-records" full content is written to /archive with digest "sha256:fin789"
    And the archive write is verified (read-back + digest check)
    And only THEN is "financial-records" tombstoned in the graph
    And the tombstone's original_digest matches the archived content

  Scenario: Archival backend unavailable blocks mandatory compaction
    # FM-21: archive unavailable prevents compaction of mandatory-archive units
    Given trust domain "acme" requires archival for data units
    And the archive backend (S3) is unreachable
    When compaction targets data unit "important-records"
    Then compaction is BLOCKED for "important-records"
    And "important-records" remains as a full unit in the active graph
    And an alert is raised: "archival backend unavailable, compaction blocked"
    And compaction of non-mandatory-archive units proceeds normally

  # --- Logical clock in compaction ---

  @consistency
  Scenario: Compaction eligibility uses logical clock for bounded task deadlines
    Given bounded task "timeout-job" has validity_window: LC 1000 to LC 2000
    And the cluster logical clock is currently at LC 2500
    When the compaction scan runs
    Then "timeout-job" is eligible (terminated: deadline exceeded at LC 2000)
    And both nodes agree on eligibility because logical clock comparison is deterministic

  Scenario: Compaction eligibility uses wall clock for retention-based data
    Given data unit "legal-docs" has retention: "7 years" from wall_time 2019-01-01
    And the current wall time is 2026-04-13 (within retention period)
    When the compaction scan runs
    Then "legal-docs" is NOT eligible for compaction (retention not expired)
    And retention expiry is computed from wall clock (compliance requirement)
