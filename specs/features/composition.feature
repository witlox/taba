@consistency @security
Feature: Composition
  The solver composes units by matching capability needs to provides.
  Composition is deterministic, order-independent, and fails closed
  on security conflicts. Purpose qualifiers on capabilities must match
  when declared. Unmatched capabilities block composition until resolved.

  Background:
    Given a bootstrapped trust domain "acme-prod" with root governance unit
    And an author "alice" with scope (type: workload, trust_domain: "acme-prod")
    And an author "bob" with scope (type: data, trust_domain: "acme-prod")
    And an author "carol" with scope (type: policy, trust_domain: "acme-prod")
    And all units are signed and accepted into the composition graph

  # --- Happy path ---

  Scenario: Two compatible units compose successfully
    # INV-K1: all needs satisfied by provides, no security conflicts
    Given a workload unit "web-api" that needs "postgres-compatible"
    And a workload unit "pg-primary" that provides "postgres-compatible"
    And "pg-primary" tolerates latency:10ms and failure:restart
    And "web-api" tolerates latency:50ms and failure:restart
    When the solver evaluates composition of "web-api" and "pg-primary"
    Then the composition succeeds
    And "web-api.needs:postgres-compatible" is matched to "pg-primary.provides:postgres-compatible"
    And the composition has no unresolved conflicts
    And the composition is recorded in the graph as a single aggregate

  Scenario: Capability with matching purpose qualifier composes
    # INV-K2: purpose qualifier declared on both sides matches
    Given a workload unit "analytics-worker" that needs "postgres-compatible(purpose:analytics)"
    And a data unit "analytics-db" that provides "postgres-compatible(purpose:analytics)"
    And "analytics-db" has classification "internal" and consent_scope "purpose:analytics"
    When the solver evaluates composition of "analytics-worker" and "analytics-db"
    Then the composition succeeds
    And the capability match includes purpose qualifier "analytics"
    And no policy is required because purposes align

  # --- Purpose mismatch (DL-010) ---

  Scenario: Purpose mismatch triggers conflict requiring policy
    # DL-010: purpose qualifier mismatch is a conflict, not a rejection
    Given a workload unit "ml-trainer" that needs "customer-data(purpose:training)"
    And a data unit "customer-profiles" that provides "customer-data(purpose:analytics)"
    And "customer-profiles" has consent_scope "purpose:analytics"
    When the solver evaluates composition of "ml-trainer" and "customer-profiles"
    Then the composition is blocked with conflict "purpose mismatch: training != analytics on capability customer-data"
    And the conflict references both "ml-trainer" and "customer-profiles"
    And the conflict type is "purpose_mismatch"
    But the composition does not fail closed because purpose mismatch is a policy-resolvable conflict

  # --- Unmatched capability ---

  Scenario: Unmatched capability blocks composition
    # INV-K1: unresolved needs block composition
    Given a workload unit "web-api" that needs "postgres-compatible" and "redis-cache"
    And a workload unit "pg-primary" that provides "postgres-compatible"
    But no unit in the graph provides "redis-cache"
    When the solver evaluates composition of "web-api" and "pg-primary"
    Then the composition is blocked with unmatched need "redis-cache"
    And "web-api" remains in state "Declared" (not "Composed")
    And the solver reports the specific unmatched capability, not a generic failure

  # --- Ambiguous match ---

  Scenario: Ambiguous match requires policy
    # INV-K2: typed matching; two providers for same capability = ambiguity
    Given a workload unit "web-api" that needs "postgres-compatible"
    And a workload unit "pg-primary" that provides "postgres-compatible"
    And a workload unit "pg-replica" that provides "postgres-compatible"
    When the solver evaluates composition involving "web-api", "pg-primary", and "pg-replica"
    Then the composition is blocked with conflict "ambiguous match: 2 providers for postgres-compatible"
    And the conflict lists both "pg-primary" and "pg-replica" as candidates
    And the solver does not arbitrarily pick a provider

  # --- Security conflict (INV-S2) ---

  @security
  Scenario: Security conflict fails closed
    # INV-S2: incompatible security declarations refuse composition
    Given a workload unit "external-api" that needs "customer-data" and trusts only "external-zone"
    And a data unit "customer-pii" that provides "customer-data" with classification "PII" and requires trust "internal-zone"
    When the solver evaluates composition of "external-api" and "customer-pii"
    Then the composition fails closed with security conflict "trust zone mismatch: external-zone vs internal-zone on PII data"
    And no partial composition is created
    And the conflict requires explicit policy resolution before retry
    And the system logs the security conflict with full context

  # --- Order independence (INV-C6) ---

  Scenario: Composition order-independent -- same result regardless of insertion order
    # INV-C6: composition result independent of unit insertion order
    Given 4 workload units forming a service mesh:
      | unit_id       | needs                   | provides                |
      | gateway       | auth-service, backend   | http-ingress            |
      | auth          |                         | auth-service            |
      | backend       | postgres-compatible     | backend                 |
      | pg            |                         | postgres-compatible     |
    When the solver evaluates composition after inserting in order: gateway, auth, backend, pg
    And the composition result is saved as "result-A"
    And the graph is reset and units are inserted in order: pg, backend, auth, gateway
    And the solver evaluates composition again
    And the composition result is saved as "result-B"
    Then "result-A" and "result-B" are identical in all fields
    And capability matches are the same in both results
    And no conflicts differ between the two results

  # --- Cyclic recovery dependencies (INV-K5) ---

  @resilience
  Scenario: Composition with cyclic recovery dependencies fails closed
    # INV-K5: cyclic recovery chains are unresolvable without policy
    Given a workload unit "service-a" with recovery dependency on "service-b"
    And a workload unit "service-b" with recovery dependency on "service-c"
    And a workload unit "service-c" with recovery dependency on "service-a"
    And all three units are signed and in the composition graph
    When the solver evaluates composition involving "service-a", "service-b", and "service-c"
    Then the composition fails closed with conflict "cyclic recovery dependency: service-a -> service-b -> service-c -> service-a"
    And the solver reports the full cycle path
    And resolution requires explicit policy declaring restart priority
    And without policy the tiebreaker is lexicographically lowest UnitId ("service-a" gets priority)

  # --- Deterministic capability sorting (INV-K2) ---

  @consistency
  Scenario: Capability lists sorted before matching ensures determinism
    # INV-K2: sorted lexicographically by (type, name, purpose)
    Given a workload unit "multi-need" that needs:
      | type    | name       | purpose    |
      | store   | redis      |            |
      | store   | postgres   | analytics  |
      | service | auth       |            |
      | store   | postgres   | serving    |
    When the solver normalizes capability lists for matching
    Then the capabilities are sorted as:
      | position | type    | name     | purpose   |
      | 1        | service | auth     |           |
      | 2        | store   | postgres | analytics |
      | 3        | store   | postgres | serving   |
      | 4        | store   | redis    |           |
    And the sorted order is identical on any node evaluating the same unit
