@data @security @governance
Feature: Data lineage
  Data units carry provenance, classification, retention, and hierarchical
  constraints. Provenance chains are structural (emerge from composition),
  not a separate system. Taint propagation, declassification, retention
  enforcement, and hierarchy rules are all verified at query time.

  Background:
    Given a bootstrapped trust domain "acme-prod" with root governance unit
    And an author "alice" with scope (type: workload, trust_domain: "acme-prod")
    And an author "bob" with scope (type: data, trust_domain: "acme-prod")
    And an author "carol" with scope (type: policy, trust_domain: "acme-prod")
    And an author "dan" with scope (type: data-steward, trust_domain: "acme-prod")
    And the classification lattice is: public(1) < internal(2) < confidential(3) < PII(4)

  # --- Provenance chain ---

  Scenario: Provenance chain created through composition
    # INV-D1: every data unit links back to inputs and producing workload
    Given bob authors a data unit "raw-logs" with:
      | field          | value                    |
      | classification | internal                 |
      | schema         | { timestamp: u64, msg: string } |
      | retention      | 90 days                  |
    And alice authors a workload unit "log-parser" that needs "raw-logs" and produces "parsed-events"
    And the composition of "log-parser" and "raw-logs" succeeds
    When "log-parser" produces data unit "parsed-events"
    Then "parsed-events" provenance records:
      | field             | value        |
      | producing_workload | log-parser  |
      | input_data        | [raw-logs]   |
      | produced_at       | 2026-04-12T10:00:00Z |
    And the provenance chain is: raw-logs -> log-parser -> parsed-events
    And the chain is navigable in both directions (forward and backward)

  Scenario: Multi-input workload -- output provenance tracks all inputs
    Given data units exist:
      | unit_id        | classification | schema                |
      | user-profiles  | PII            | { name: string }      |
      | click-events   | internal       | { page: string }      |
      | session-data   | confidential   | { session_id: string } |
    And a workload unit "enricher" consumes all three and produces "enriched-dataset"
    When "enricher" produces "enriched-dataset"
    Then "enriched-dataset" provenance records input_data as [user-profiles, click-events, session-data]
    And the provenance includes the producing_workload "enricher"
    And all three input lineage chains are reachable from "enriched-dataset"

  # --- Taint propagation (INV-S4) ---

  @security
  Scenario: Taint propagation -- PII inherits through chain
    # INV-S4: output inherits classification unless declassified
    Given data unit "customer-emails" with classification "PII"
    And workload "hasher" consumes "customer-emails" and produces "hashed-emails"
    And workload "aggregator" consumes "hashed-emails" and produces "email-stats"
    And no declassification policy exists in the chain
    When taint is computed for "email-stats" at query time
    Then "email-stats" has classification "PII"
    And the taint was inherited: customer-emails(PII) -> hashed-emails(PII) -> email-stats(PII)
    And the full provenance chain is traversed for each query

  @security
  Scenario: Multi-input taint -- most restrictive wins (union)
    # INV-S4: union of all input classifications
    Given a workload "merger" consumes:
      | input_unit     | classification |
      | public-stats   | public         |
      | internal-logs  | internal       |
      | confidential-config | confidential |
    And "merger" produces "combined-output"
    When taint is computed for "combined-output" at query time
    Then "combined-output" has classification "confidential" (most restrictive input)
    And the lattice comparison is: max(public=1, internal=2, confidential=3) = confidential
    And if "pii-records" (PII=4) were added as an input, classification would become "PII"

  # --- Declassification (INV-S9) ---

  Scenario: Declassification with multi-party policy
    # INV-S9: 2 distinct authors (policy + data-steward)
    Given data unit "raw-pii" with classification "PII"
    And workload "anonymizer" consumes "raw-pii" and produces "anonymized-data"
    And carol (policy scope) and dan (data-steward scope) co-sign declassification policy "declass-anon" with:
      | field      | value                                      |
      | target     | anonymized-data                            |
      | from       | PII                                        |
      | to         | internal                                   |
      | rationale  | k-anonymity k=100, verified by privacy team |
    When taint is computed for "anonymized-data" at query time
    Then "anonymized-data" has classification "internal" (declassified from PII)
    And the declassification is recorded in the provenance chain
    And downstream consumers of "anonymized-data" inherit "internal" (not PII)

  Scenario: Single-author declassification rejected
    # INV-S9: single author is insufficient
    Given data unit "sensitive-report" with classification "confidential"
    When carol alone signs a declassification policy "solo-declass" for "sensitive-report" from "confidential" to "public"
    And the policy is submitted for graph merge
    Then the policy is rejected with error "declassification requires minimum 2 distinct authors: need policy + data-steward"
    And "sensitive-report" retains classification "confidential"
    And taint computation for any downstream consumer reflects "confidential"

  # --- Retention enforcement (INV-D2) ---

  Scenario: Retention enforcement -- expired data eligible for compaction
    # INV-D2: retention is enforced, not optional
    Given bob authors a data unit "temp-cache" with:
      | field       | value                  |
      | classification | internal            |
      | retention   | 30 days from 2026-03-01 |
      | schema      | { key: string, val: bytes } |
    And the current date is 2026-04-12 (42 days since creation)
    When the retention enforcer evaluates "temp-cache"
    Then "temp-cache" is marked as "expired"
    And "temp-cache" is eligible for compaction
    And compaction does not occur immediately (scheduled by compactor)
    But "temp-cache" is no longer valid for new compositions
    And provenance references to "temp-cache" are preserved (lineage is not broken)

  # --- Hierarchical constraints (INV-S7) ---

  Scenario: Hierarchical constraint -- child narrows freely
    # INV-S7: narrowing = adding restrictions, always allowed
    Given bob authors a parent data unit "company-data" with:
      | field          | value      |
      | classification | internal   |
      | retention      | 365 days   |
      | jurisdiction   | EU         |
    When bob authors a child data unit "hr-records" under "company-data" with:
      | field          | value         |
      | classification | confidential  |
      | retention      | 730 days      |
      | jurisdiction   | EU, Germany   |
    Then the child "hr-records" is accepted
    And classification confidential > internal (narrowing: more restrictive)
    And retention 730 > 365 days (narrowing: longer retention)
    And jurisdiction EU+Germany is narrower (more specific)
    And no policy is required for narrowing

  Scenario: Hierarchical constraint -- child widens requires policy
    # INV-S7: widening = removing restrictions, requires policy
    Given bob authors a parent data unit "restricted-data" with:
      | field          | value        |
      | classification | confidential |
      | retention      | 365 days     |
    When bob authors a child data unit "shared-subset" under "restricted-data" with:
      | field          | value    |
      | classification | internal |
      | retention      | 90 days  |
    Then the child "shared-subset" is blocked with conflict "widening requires policy: classification confidential -> internal"
    And the retention widening is also flagged: "365 days -> 90 days"
    And the child is not accepted until a policy unit resolves both widenings

  # --- Classification lattice ---

  Scenario: Classification lattice ordering is enforced
    # INV-S7: public < internal < confidential < PII
    Given data units at each classification level:
      | unit_id          | classification |
      | open-data        | public         |
      | team-docs        | internal       |
      | financial-data   | confidential   |
      | customer-pii     | PII            |
    When taint propagation compares classifications
    Then public(1) < internal(2) < confidential(3) < PII(4)
    And a workload consuming "open-data" and "customer-pii" produces output classified as "PII"
    And a workload consuming "team-docs" and "financial-data" produces output classified as "confidential"
    And the lattice is a total order with no ambiguous comparisons

  # --- Max hierarchy depth ---

  Scenario: Max hierarchy depth enforced at 16 levels
    Given bob authors a chain of 16 nested data units (parent -> child_1 -> ... -> child_16)
    And each child narrows the parent's classification by one level where possible
    When bob attempts to author a 17th child data unit at depth 17
    Then the unit is rejected with error "max hierarchy depth exceeded: 17 > 16"
    And the 16-level hierarchy remains valid
    And the rejection prevents unbounded nesting

  # --- Provenance verification at query time (INV-D1, INV-C4) ---

  Scenario: Provenance verified at query time, pending refs buffered
    # INV-D1: provenance verified at query, not merge
    # INV-C4: causal buffering for references not yet in local graph
    Given a workload unit "remote-producer" on node-bbb produces data unit "remote-output"
    And "remote-output" provenance references input data unit "remote-input" (not yet replicated to node-aaa)
    When node-aaa receives "remote-output" via CRDT merge
    Then "remote-output" is accepted into node-aaa's graph
    And the provenance reference to "remote-input" is marked as "pending"
    And the WAL records Pending("remote-output", missing_refs: ["remote-input"])
    When "remote-input" arrives at node-aaa via CRDT replication
    Then the pending reference is resolved
    And the WAL records Promoted("remote-output")
    And provenance query for "remote-output" now returns the complete chain including "remote-input"
