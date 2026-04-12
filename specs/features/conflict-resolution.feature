@governance @security @consistency
Feature: Conflict resolution
  Policy units resolve capability conflicts between other units.
  Policies are scoped, versioned, and subject to strict authoring rules.
  Only one non-revoked policy may resolve a given conflict tuple.
  Supersession creates an immutable chain. Orphaned policies are
  detected at query time.

  Background:
    Given a bootstrapped trust domain "acme-prod" with root governance unit
    And an author "alice" with scope (type: workload, trust_domain: "acme-prod")
    And an author "carol" with scope (type: policy, trust_domain: "acme-prod")
    And an author "dan" with scope (type: data-steward, trust_domain: "acme-prod")
    And all author keys are Ed25519 and not revoked

  # --- Happy path ---

  Scenario: Policy resolves security conflict
    # INV-S2, INV-C5: explicit policy resolves a conflict the solver detected
    Given a workload unit "external-api" authored by alice that needs "customer-data" trusting "external-zone"
    And a data unit "customer-pii" that provides "customer-data" with classification "PII" requiring trust "internal-zone"
    And the solver has detected security conflict "trust-zone-mismatch-001" between "external-api" and "customer-pii"
    When carol authors a policy unit "resolve-trust-001" with:
      | field      | value                                                 |
      | resolves   | conflict:trust-zone-mismatch-001                      |
      | resolution | allow with condition: external-api must encrypt in transit |
      | scope      | trust_domain:acme-prod                                |
      | rationale  | External API uses mTLS; risk accepted per security review SR-42 |
    And carol signs the policy binding trust_domain "acme-prod"
    And the policy is submitted for graph merge
    Then the policy is accepted into the composition graph
    And the solver re-evaluates the composition of "external-api" and "customer-pii"
    And the composition succeeds with policy "resolve-trust-001" applied

  # --- Scope enforcement ---

  Scenario: Policy author scope enforced -- workload author cannot create policy
    # INV-S5: alice has workload scope, not policy scope
    Given the solver has detected conflict "cap-mismatch-002" between "unit-x" and "unit-y"
    When alice authors a policy unit "unauthorized-policy" resolving conflict "cap-mismatch-002"
    And alice signs the policy binding trust_domain "acme-prod"
    And the policy is submitted for graph merge
    Then the policy is rejected with error "author scope violation: alice lacks type scope for policy"
    And the conflict "cap-mismatch-002" remains unresolved
    And the composition graph does not contain "unauthorized-policy"

  # --- Supersession (INV-C7) ---

  Scenario: Policy supersession -- new policy supersedes old for same conflict
    # INV-C7: versioned lineage chain; solver uses latest non-revoked
    Given a conflict "purpose-mismatch-003" exists between "ml-trainer" and "customer-profiles"
    And carol has authored policy "policy-v1" resolving "purpose-mismatch-003" with resolution "deny"
    And "policy-v1" is accepted and the solver uses it
    When carol authors a policy unit "policy-v2" with:
      | field      | value                                          |
      | resolves   | conflict:purpose-mismatch-003                  |
      | resolution | allow with condition: anonymize before training |
      | supersedes | policy-v1                                      |
      | rationale  | Updated after privacy review PR-17 approved anonymization pipeline |
    And carol signs the policy binding trust_domain "acme-prod"
    And the policy is submitted for graph merge
    Then "policy-v2" is accepted into the composition graph
    And "policy-v1" is marked as superseded (not deleted)
    And the solver uses "policy-v2" for conflict "purpose-mismatch-003"
    And the supersession chain is: policy-v1 -> policy-v2

  Scenario: Superseded policy is revoked, solver uses latest version
    # INV-C7: revocation + supersession interaction
    Given a supersession chain exists: "policy-v1" -> "policy-v2" -> "policy-v3" for conflict "cap-conflict-004"
    And the solver currently uses "policy-v3"
    When "policy-v2" is explicitly revoked
    Then "policy-v2" was already superseded so revocation is a no-op for solver behavior
    And the solver still uses "policy-v3" (latest non-revoked in the chain)
    And the immutable chain remains: policy-v1 -> policy-v2(revoked) -> policy-v3

  # --- Duplicate policy rejection (INV-C7) ---

  Scenario: Duplicate policy for same conflict tuple -- rejected
    # INV-C7: only one non-revoked policy per conflict tuple
    Given a conflict "resource-mismatch-005" exists between "api-server" and "gpu-worker"
    And carol has authored policy "existing-policy" resolving "resource-mismatch-005"
    And "existing-policy" is accepted and not revoked
    When carol authors a policy unit "duplicate-policy" resolving "resource-mismatch-005"
    But "duplicate-policy" does not declare supersedes "existing-policy"
    And carol signs the policy binding trust_domain "acme-prod"
    And the policy is submitted for graph merge
    Then the policy is rejected with error "conflict tuple already resolved by existing-policy; must explicitly supersede"
    And the composition graph does not contain "duplicate-policy"
    And the solver continues using "existing-policy"

  # --- Orphaned policy detection (INV-C5) ---

  Scenario: Orphaned policy detected at query time, flagged for archival
    # INV-C5: policy referencing non-existent conflict detected at query, not merge
    Given carol has authored policy "orphan-policy" resolving conflict "old-conflict-006"
    And "orphan-policy" was accepted into the graph when "old-conflict-006" existed
    And the units referenced by "old-conflict-006" have since been archived
    When the solver queries active policies
    Then "orphan-policy" is detected as orphaned because "old-conflict-006" no longer references active units
    And "orphan-policy" is flagged as "eligible for archival"
    But "orphan-policy" is not automatically deleted
    And the detection happens at query time, not at merge time

  # --- Partition-heal scenario ---

  @resilience
  Scenario: Partition-heal -- both sides authored policies for same conflict
    # FM-03, INV-C7: partition produces two policies for same conflict; supersession resolves
    Given a network partition splits the cluster into side-A and side-B
    And conflict "latency-conflict-007" exists on both sides
    And carol (on side-A) authors policy "policy-A" resolving "latency-conflict-007" at timestamp 2026-03-01T10:00:00Z
    And an author "eve" with policy scope (on side-B) authors policy "policy-B" resolving "latency-conflict-007" at timestamp 2026-03-01T10:05:00Z
    When the partition heals and CRDT merge occurs
    Then the merge detects two non-revoked policies for conflict tuple "latency-conflict-007"
    And the solver uses the supersession chain: later-timestamped policy "policy-B" must explicitly supersede "policy-A"
    But if neither supersedes the other, the conflict is escalated requiring manual resolution
    And the system does not silently pick one policy over the other

  # --- Legal conflict (FM-10) ---

  @governance
  Scenario: Policy conflict between legal retention and consent withdrawal
    # FM-10: genuinely hard legal question surfaced to humans
    Given a data unit "patient-records" with retention "7 years, legal_basis: healthcare regulation"
    And a consent withdrawal event for the data subject of "patient-records"
    And the solver detects conflict "legal-conflict-008" between retention obligation and consent withdrawal
    When carol authors a policy unit "legal-resolution-008" with:
      | field      | value                                                   |
      | resolves   | conflict:legal-conflict-008                             |
      | resolution | retain with restricted access: audit-only until retention expires |
      | rationale  | Legal counsel opinion LC-2026-003: retention overrides withdrawal for healthcare data |
    And carol signs the policy binding trust_domain "acme-prod"
    And the policy is submitted for graph merge
    Then the policy is accepted into the composition graph
    And "patient-records" access is restricted to audit-only
    And the data unit is neither deleted nor fully accessible
    And the conflict resolution is logged with full rationale for compliance audit
