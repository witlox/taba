@data @governance
Feature: Data retention and compaction
  Data units declare retention periods. Expired units are eligible for
  compaction (INV-D2). Governance units are permanent and cannot be
  archived. Conflicting legal requirements are surfaced for human
  resolution (FM-10).

  # --- Expiry and compaction ---

  Scenario: Expired data unit automatically eligible for compaction (INV-D2)
    Given data unit "ds-logs-jan" declares retention "90 days" created at "2026-01-01T00:00:00Z"
    When the current time is "2026-04-02T00:00:00Z" (91 days after creation)
    Then "ds-logs-jan" is marked as expired
    And "ds-logs-jan" is eligible for compaction
    And the next auto-compaction cycle removes "ds-logs-jan" from the active graph
    And "ds-logs-jan"'s provenance links are preserved in archived lineage

  Scenario: Non-expired data unit is not eligible for compaction
    Given data unit "ds-logs-feb" declares retention "90 days" created at "2026-02-01T00:00:00Z"
    When the current time is "2026-04-02T00:00:00Z" (60 days after creation)
    Then "ds-logs-feb" is NOT marked as expired
    And "ds-logs-feb" remains in the active graph
    And compaction skips "ds-logs-feb"

  # --- Archival ---

  Scenario: Archived data unit remains in lineage but not in active graph
    Given data unit "ds-results-2025" is manually archived by an operator
    When a provenance query traces lineage through "ds-results-2025"
    Then "ds-results-2025" appears in the lineage chain
    And "ds-results-2025" metadata (schema, classification, provenance) is queryable
    But "ds-results-2025" is not in the active composition graph
    And the solver does not consider "ds-results-2025" for placement or composition

  Scenario: Manual archival of a subgraph
    Given data unit "ds-parent" has children ["ds-child-1", "ds-child-2", "ds-child-3"]
    And all four units are expired or marked for archival
    When the operator issues an archive command for subgraph rooted at "ds-parent"
    Then "ds-parent", "ds-child-1", "ds-child-2", and "ds-child-3" are all archived
    And all four units are removed from the active graph atomically
    And provenance links for all four units are preserved in archived lineage
    And the memory freed by archival is reported to the memory monitor

  # --- Retention conflict ---

  @governance
  Scenario: Retention conflict -- legal retain vs consent withdraw is locked and escalated (FM-10)
    Given data unit "ds-patient-42" declares retention "7 years" with legal basis "clinical-trial-regulation"
    And data unit "ds-patient-42" has consent scope "patient-42-consent"
    When "patient-42" withdraws consent for "ds-patient-42"
    Then the solver detects a conflict: retention requirement vs consent withdrawal
    And "ds-patient-42" enters Locked state (neither deleted nor fully accessible)
    And an operator alert is surfaced: "RetentionConflict: ds-patient-42 -- legal retain vs consent withdraw"
    And a governance author must create a policy unit resolving the conflict
    And automatic resolution is NOT attempted (human decision required)

  Scenario: Retention conflict resolved by explicit policy
    Given data unit "ds-patient-42" is in Locked state due to retention conflict
    When governance author "legal-admin" creates a policy unit resolving the conflict with "pseudonymize and retain"
    And a second governance author "data-steward" cosigns the policy (multi-party per INV-S9)
    Then "ds-patient-42" is pseudonymized per the policy resolution
    And "ds-patient-42" exits Locked state and resumes its retention period
    And the policy unit is recorded in "ds-patient-42"'s provenance chain

  # --- Compaction effects ---

  Scenario: Compaction frees memory and WAL space
    Given node "n-002" has 50 expired data units totaling 120 MB in the active graph
    And the WAL contains entries for all 50 expired units
    When auto-compaction runs on "n-002"
    Then all 50 expired data units are removed from the active graph
    And WAL entries for the 50 units are tombstoned (marked for cleanup)
    And the memory monitor reports approximately 120 MB freed
    And the WAL space is reclaimed during the next WAL compaction cycle

  # --- Governance unit permanence ---

  @governance
  Scenario: Governance units cannot be archived
    Given governance unit "gov-root-domain" defines the root trust domain
    And governance unit "gov-role-alice" assigns "alice" workload scope
    When an operator attempts to archive "gov-root-domain"
    Then the archive is rejected with error "GovernanceUnitPermanent: governance units cannot be archived"
    And "gov-root-domain" remains in the active graph
    When an operator attempts to archive "gov-role-alice"
    Then the archive is rejected with the same error
    And "gov-role-alice" remains in the active graph
