@governance @security
Feature: Compliance audit
  Every security decision, role assignment, and data lineage event is
  traceable through the composition graph. Audit queries traverse
  governance units, policy chains, and provenance links to produce
  compliance evidence.

  # --- Data lineage audit ---

  @data
  Scenario: Query data unit full lineage for audit
    Given workload "wl-ingest" consumed raw data "ds-raw" and produced "ds-cleaned"
    And workload "wl-transform" consumed "ds-cleaned" and produced "ds-final"
    And each workload recorded provenance links at production time
    When an auditor queries the full lineage of "ds-final"
    Then the lineage chain returned is: "ds-raw" -> "wl-ingest" -> "ds-cleaned" -> "wl-transform" -> "ds-final"
    And each link includes the producing workload's UnitId, timestamp, and author
    And the lineage is verified by traversing provenance graph references (INV-D1)
    And no gaps exist in the provenance chain

  Scenario: Lineage query handles archived units
    Given "ds-raw" has been archived but its provenance metadata is preserved
    When an auditor queries the full lineage of "ds-final" which depends on "ds-raw"
    Then "ds-raw" appears in the lineage with status "archived"
    And the lineage chain is complete despite "ds-raw" being out of the active graph
    And the auditor is informed that "ds-raw" content requires archive retrieval

  # --- Policy audit trail ---

  @security
  Scenario: Every security decision has a traceable policy unit
    Given workload "wl-analytics" needs capability "patient-data-read"
    And data unit "ds-patients" provides capability "patient-data-read" with classification "PII"
    And a security conflict was detected between "wl-analytics" and "ds-patients"
    When policy unit "pol-analytics-access" was created resolving the conflict with "allow" and rationale "IRB-approved study #2026-01"
    Then querying security decisions for "wl-analytics" returns "pol-analytics-access"
    And the policy references the specific conflict (unit IDs + capability name)
    And the policy includes rationale "IRB-approved study #2026-01"
    And no implicit (undocumented) security resolution exists for this capability match

  Scenario: Policy audit trail includes rejection decisions
    Given a capability conflict between "wl-external" and "ds-internal" on "internal-api"
    And policy unit "pol-deny-external" resolves it with "deny" and rationale "external access prohibited"
    When an auditor queries all policy decisions for trust domain "ops"
    Then "pol-deny-external" appears in the results
    And the denial rationale and timestamp are included
    And the conflicting unit IDs are traceable

  # --- Author scope audit ---

  @governance
  Scenario: Audit who had what scope when
    Given author "carol" was assigned workload scope in "pharma-trials" at "2026-01-15T10:00:00Z"
    And author "carol"'s scope was narrowed to data-only at "2026-06-01T10:00:00Z"
    And author "carol"'s scope was revoked at "2026-09-01T10:00:00Z"
    When an auditor queries "carol"'s scope history in "pharma-trials"
    Then the audit trail shows 3 RoleAssignment governance units:
      | timestamp                | scope    | action  |
      | 2026-01-15T10:00:00Z    | workload | granted |
      | 2026-06-01T10:00:00Z    | data     | narrowed |
      | 2026-09-01T10:00:00Z    | none     | revoked  |
    And each governance unit is signed by the assigning authority
    And the full chain is immutable and tamper-evident (signed governance units)

  Scenario: Audit shows all authors with active scope in a trust domain
    Given trust domain "pharma-trials" has the following active role assignments:
      | author  | scope      | assigned_at          |
      | alice   | governance | 2026-01-01T00:00:00Z |
      | bob     | policy     | 2026-02-01T00:00:00Z |
      | carol   | data       | 2026-06-01T10:00:00Z |
    When an auditor queries active scopes in "pharma-trials" at "2026-08-15T00:00:00Z"
    Then the result includes all 3 authors with their current scopes
    And expired or revoked assignments are excluded from the active view
    And the query can be filtered by scope type

  # --- Supersession chain audit ---

  @consistency
  Scenario: Full policy history for a conflict via supersession chain
    Given a capability conflict between "wl-a" and "wl-b" on "shared-resource"
    And policy "pol-v1" resolved it with "allow" at "2026-01-10T00:00:00Z"
    And policy "pol-v2" superseded "pol-v1" with "conditional" at "2026-03-15T00:00:00Z"
    And policy "pol-v3" superseded "pol-v2" with "deny" at "2026-07-20T00:00:00Z"
    When an auditor queries the supersession chain for the conflict ("wl-a", "wl-b", "shared-resource")
    Then the chain returned is: "pol-v1" -> "pol-v2" -> "pol-v3"
    And each policy includes its resolution, rationale, author, and timestamp
    And "pol-v1" and "pol-v2" are marked as superseded
    And "pol-v3" is the current active (non-revoked) policy
    And the chain is immutable: no policy can be removed, only superseded (INV-C7)

  # --- Key revocation audit ---

  @security
  Scenario: Key revocation is recorded as governance unit with full audit trail
    Given author "dave" had workload scope in "ops" with key "pk_dave_123"
    And "dave" authored 12 units between "2026-01-01" and "2026-06-15"
    When "dave"'s key "pk_dave_123" is revoked at "2026-06-15T14:30:00Z"
    Then a governance unit records the revocation event with:
      | field              | value                    |
      | author_id          | dave                     |
      | revoked_key        | pk_dave_123              |
      | revocation_time    | 2026-06-15T14:30:00Z     |
      | reason             | key_compromise           |
      | revoking_authority | governance-admin         |
    And the revocation is propagated via priority gossip to all nodes
    And querying "dave"'s audit trail shows all 12 units authored before revocation
    And each of the 12 units remains valid (signed before revocation timestamp per INV-S3)
    And any unit submitted by "dave" after "2026-06-15T14:30:00Z" is rejected
