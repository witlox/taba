@security @resilience
Feature: Security enforcement
  Zero-access default, fail-closed decisions, context-bound signatures,
  taint propagation, multi-party declassification, and gossip authentication.
  Security is structural -- enforced by the graph and solver, not bolted on.

  Background:
    Given a bootstrapped trust domain "acme-prod" with root governance unit
    And an author "alice" with scope (type: workload, trust_domain: "acme-prod")
    And an author "carol" with scope (type: policy, trust_domain: "acme-prod")
    And an author "dan" with scope (type: data-steward, trust_domain: "acme-prod")
    And a cluster "cluster-1" with 5 active nodes

  # --- Zero-access default (INV-S1) ---

  Scenario: Zero-access default -- undeclared capability denied
    # INV-S1: no implicit access; only explicitly declared AND policy-approved
    Given a workload unit "web-api" that declares needs "postgres-compatible"
    And "web-api" does NOT declare needs "redis-cache"
    When "web-api" attempts to access capability "redis-cache" at runtime
    Then access is denied with reason "capability not declared: redis-cache"
    And the denial is logged with unit_id "web-api" and attempted capability "redis-cache"
    And no implicit fallback or default-allow is applied
    And the workload continues running (denial is per-capability, not fatal)

  # --- Fail closed (INV-S2) ---

  Scenario: Fail closed on ambiguous security decision
    # INV-S2: ambiguity = denial until explicit policy resolves
    Given a workload unit "batch-job" that needs "shared-storage" with trust "zone-a"
    And a data unit "shared-fs" that provides "shared-storage" with trust "zone-a" and "zone-b"
    And the solver cannot determine whether "zone-a" trust on "batch-job" satisfies multi-zone "shared-fs"
    When the solver evaluates the security decision for this composition
    Then the solver fails closed: composition refused
    And the conflict is recorded as "ambiguous trust scope resolution"
    And no data flows between "batch-job" and "shared-fs" until policy resolves the ambiguity
    And the system does not guess or apply heuristics

  # --- Context-bound signatures (INV-S3) ---

  Scenario: Context-bound signature verification
    # INV-S3: Sign(key, hash(unit || trust_domain_id || cluster_id || validity_window))
    Given alice authors a workload unit "secure-api" signed with context:
      | field            | value                     |
      | trust_domain_id  | acme-prod                 |
      | cluster_id       | cluster-1                 |
      | validity_window  | 2026-01-01..2027-01-01    |
    When the unit is submitted for graph merge in trust_domain "acme-prod" on cluster "cluster-1"
    Then signature verification checks the hash of (unit_content || acme-prod || cluster-1 || 2026-01-01..2027-01-01)
    And the cryptographic signature is valid
    And the author's scope is valid at creation time
    And the author's key was not revoked before creation timestamp
    And the unit is accepted only after all three checks pass synchronously

  # --- Key not locally available ---

  Scenario: Key not locally available -- unit buffered until key arrives
    # INV-C4: causal buffering for pending units
    Given alice authors a workload unit "remote-unit" signed with a valid Ed25519 key
    And the verifying node does not yet have alice's public key in its local keystore
    When the unit is submitted for graph merge on the verifying node
    Then the unit is not rejected outright
    And the unit is placed in pending state with reason "author key not yet available"
    And the WAL contains a Pending("remote-unit", missing: "alice-public-key") entry
    When alice's public key arrives via gossip
    Then signature verification completes successfully
    And the unit is promoted from pending to merged
    And the WAL contains a Promoted("remote-unit") entry

  # --- Build provenance (SLSA) ---

  Scenario: Build provenance enforcement via SLSA attestation
    Given a workload unit "production-service" with build provenance:
      | field             | value                              |
      | builder           | github-actions/v3                  |
      | source_repo       | github.com/acme/service            |
      | source_digest     | sha256:abc123def456                |
      | slsa_level        | 3                                  |
    And the trust domain "acme-prod" requires minimum SLSA level 2 for workload units
    When the solver evaluates placement of "production-service"
    Then the SLSA attestation is verified against the declared builder
    And the source digest is checked for integrity
    And placement proceeds because SLSA level 3 >= required level 2

  # --- Taint propagation (INV-S4) ---

  @data
  Scenario: Taint propagation -- PII input produces PII output
    # INV-S4: output inherits input classification unless declassified
    Given a data unit "customer-emails" with classification "PII"
    And a workload unit "email-hasher" that needs "customer-emails" and produces "hashed-output"
    And no declassification policy exists for the output
    When the solver computes taint for "hashed-output" at query time
    Then "hashed-output" inherits classification "PII" from "customer-emails"
    And the taint is computed by traversing the provenance graph
    And the taint is NOT cached at merge time

  @data
  Scenario: Multi-input taint -- union of all input classifications
    # INV-S4: most restrictive (union) of all input classifications
    Given a data unit "public-stats" with classification "public"
    And a data unit "internal-metrics" with classification "internal"
    And a data unit "pii-records" with classification "PII"
    And a workload unit "aggregator" that consumes all three and produces "combined-report"
    When the solver computes taint for "combined-report" at query time
    Then "combined-report" inherits classification "PII" (the most restrictive)
    And the taint computation considers all three inputs: public, internal, PII
    And the lattice ordering public < internal < confidential < PII determines the union

  @data
  Scenario: Taint computed at query time, not cached
    # DL-007: query-time computation ensures eventual consistency
    Given a data unit "dataset-a" with classification "internal"
    And a workload unit "processor" that consumes "dataset-a" and produces "output-b"
    And "output-b" was queried and taint was computed as "internal"
    When "dataset-a" classification is updated to "PII" via a new data unit version
    And "output-b" taint is queried again
    Then "output-b" now shows classification "PII" (recomputed from updated provenance)
    And no cache invalidation was needed because taint is never cached

  # --- Declassification (INV-S9) ---

  Scenario: Declassification requires 2 distinct authors (policy + data-steward)
    # INV-S9: multi-party declassification
    Given a data unit "raw-pii" with classification "PII"
    And a workload unit "anonymizer" that consumes "raw-pii" and produces "anonymized-output"
    When carol (policy scope) and dan (data-steward scope) co-sign a declassification policy "declass-001" with:
      | field      | value                                             |
      | resolves   | taint on "anonymized-output"                      |
      | resolution | declassify from PII to internal                   |
      | rationale  | k-anonymity with k=50 verified by privacy team    |
    And the declassification policy is submitted for graph merge
    Then the policy is accepted (2 distinct authors: carol=policy, dan=data-steward)
    And "anonymized-output" taint is computed as "internal" at query time
    And the declassification is recorded in the provenance chain

  Scenario: Single-author declassification -- rejected
    # INV-S9: minimum 2 distinct authors required
    Given a data unit "sensitive-data" with classification "confidential"
    When carol alone signs a declassification policy "solo-declass" reducing "sensitive-data" to "public"
    And the policy is submitted for graph merge
    Then the policy is rejected with error "declassification requires minimum 2 distinct authors: need policy + data-steward"
    And "sensitive-data" retains classification "confidential"
    And no taint change occurs

  Scenario: Declassification signer revoked after policy merged -- policy remains valid
    # INV-S3 causal revocation: policies merged before revocation are grandfathered
    Given a declassification policy "declass-002" signed by carol and dan exists
    And "declass-002" reduced "processed-data" from "PII" to "internal"
    And "declass-002" was merged into the graph before any key revocation
    When dan's key revocation governance unit is merged into the graph
    And taint for "processed-data" is queried
    Then "processed-data" retains classification "internal"
    And the declassification policy remains valid (merged before revocation, no retroactive invalidation)

  Scenario: Declassification signer revoked before policy merged -- policy invalid
    # INV-S3 causal revocation: policies arriving after revocation merge are rejected
    Given dan's key revocation governance unit has been merged into the graph
    And carol and dan attempt to co-sign declassification policy "declass-003"
    When "declass-003" is submitted for graph merge
    Then "declass-003" is rejected because dan's key is revoked in the local graph
    And the declassification does not take effect
    And "processed-data" retains its original classification

  # --- Gossip authentication (DL-009) ---

  @resilience
  Scenario: Gossip message authentication -- unsigned gossip dropped
    # DL-009, INV-R3: all gossip messages signed with sending node's identity key
    Given node "node-alpha" with Ed25519 identity key sends a gossip membership update
    And the message is signed with node-alpha's key
    When node "node-beta" receives the gossip message
    Then node-beta verifies the signature against node-alpha's known public key
    And the message is accepted and processed
    But when an unsigned gossip message arrives claiming to be from "node-gamma"
    Then node-beta drops the message
    And the drop is logged with reason "unsigned gossip message from claimed sender node-gamma"
    And the membership state is not updated from the unsigned message

  # --- Scope uniqueness (INV-S8) ---

  @governance
  Scenario: Scope uniqueness -- no two authors with identical scope tuples
    # INV-S8: enforcement mechanism for assumption A1
    Given an author "alice" holds scope (type: workload, trust_domain: "acme-prod")
    And a new role assignment governance unit assigns author "frank" scope (type: workload, trust_domain: "acme-prod")
    When the governance unit for frank's role assignment is submitted for graph merge
    Then the assignment is rejected with error "scope uniqueness violation: (workload, acme-prod) already assigned to alice"
    And frank cannot create workload units in "acme-prod"
    But a role assignment for frank with scope (type: workload, trust_domain: "acme-staging") would succeed
    And scope tuples are compared as exact (type, trust_domain) pairs
