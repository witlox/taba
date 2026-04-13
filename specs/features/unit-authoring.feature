@security @governance
Feature: Unit authoring
  Authors create typed, signed units within their scoped authority.
  Units are the fundamental primitive -- self-describing, capability-aware,
  cryptographically signed. The graph rejects malformed, unsigned, or
  out-of-scope units at merge time.

  Background:
    Given a bootstrapped trust domain "acme-prod" with root governance unit
    And an author "alice" with scope (type: workload, trust_domain: "acme-prod")
    And an author "bob" with scope (type: data, trust_domain: "acme-prod")
    And an author "carol" with scope (type: policy, trust_domain: "acme-prod")
    And author keys are Ed25519 and not revoked

  # --- Happy paths ---

  Scenario: Author creates a valid workload unit (happy path)
    # INV-S3: signed unit accepted; INV-S5: within scope
    Given alice authors a workload unit "web-api" with:
      | field          | value                        |
      | needs          | postgres-compatible(purpose:analytics) |
      | provides       | http-rest(purpose:serving)   |
      | tolerates      | latency:50ms, failure:restart |
      | scaling        | min:2, max:10, trigger:cpu>70 |
      | on_shutdown    | drain:30s                    |
    When alice signs the unit binding trust_domain "acme-prod" and cluster "cluster-1" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge
    Then the unit is accepted into the composition graph
    And the unit state is "Declared"
    And the WAL contains a Merged("web-api") entry

  Scenario: Author creates a valid data unit with classification lattice
    # INV-S7: classification lattice public < internal < confidential < PII
    Given bob authors a data unit "customer-profiles" with:
      | field            | value                             |
      | schema           | { name: string, email: string }   |
      | classification   | PII                               |
      | retention        | 7 years, legal_basis: GDPR Art.6   |
      | consent_scope    | purpose:analytics                  |
      | storage          | encryption:AES-256, jurisdiction:EU |
    When bob signs the unit binding trust_domain "acme-prod" and cluster "cluster-1" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge
    Then the unit is accepted into the composition graph
    And the unit classification is positioned at level 4 in the lattice (public=1 < internal=2 < confidential=3 < PII=4)

  # --- Scope violations (INV-S5) ---

  Scenario: Author creates unit outside type scope -- rejected
    # INV-S5: alice has workload scope, cannot create policy units
    Given alice authors a policy unit "sneaky-policy" resolving conflict between "unit-x" and "unit-y"
    When alice signs the unit binding trust_domain "acme-prod" and cluster "cluster-1" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge
    Then the unit is rejected with error "author scope violation: alice lacks type scope for policy"
    And the WAL does not contain any entry for "sneaky-policy"
    And the composition graph does not contain "sneaky-policy"

  Scenario: Author creates unit outside trust domain -- rejected
    # INV-S5: alice is scoped to acme-prod, cannot author in acme-staging
    Given a trust domain "acme-staging" exists
    And alice authors a workload unit "staging-api" with:
      | field    | value         |
      | needs    | redis         |
      | provides | http-rest     |
    When alice signs the unit binding trust_domain "acme-staging" and cluster "cluster-2" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge
    Then the unit is rejected with error "author scope violation: alice lacks trust_domain scope for acme-staging"
    And the composition graph does not contain "staging-api"

  # --- Declaration validation ---

  Scenario: Unit with missing required declarations -- rejected
    Given alice authors a workload unit "incomplete-unit" with:
      | field    | value   |
      | needs    | postgres |
    But the unit is missing the "provides" declaration
    And the unit is missing the "tolerates" declaration
    When alice signs the unit binding trust_domain "acme-prod" and cluster "cluster-1" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge
    Then the unit is rejected with error "missing required declarations: provides, tolerates"
    And the rejection lists all missing fields, not just the first

  # --- Signature enforcement (INV-S3) ---

  Scenario: Unsigned unit rejected on merge
    # INV-S3: every unit in the graph must be signed
    Given alice authors a workload unit "unsigned-api" with:
      | field    | value     |
      | needs    | postgres  |
      | provides | http-rest |
      | tolerates | latency:100ms |
    But alice does not sign the unit
    When the unit is submitted for graph merge
    Then the unit is rejected with error "signature verification failed: unit is unsigned"
    And signature verification blocks before any graph state change
    And the WAL does not contain any entry for "unsigned-api"

  Scenario: Signature with wrong context binding (wrong trust domain) -- rejected
    # INV-S3: Sign(key, hash(unit || trust_domain_id || cluster_id || validity_window))
    Given alice authors a workload unit "misbound-api" with:
      | field    | value     |
      | needs    | redis     |
      | provides | grpc-api  |
      | tolerates | latency:20ms |
    When alice signs the unit binding trust_domain "wrong-domain" and cluster "cluster-1" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge in trust domain "acme-prod"
    Then the unit is rejected with error "signature context mismatch: signed for trust_domain wrong-domain but submitted to acme-prod"
    And the composition graph does not contain "misbound-api"

  Scenario: Unit from revoked author rejected when revocation merged first
    # INV-S3: causal revocation — effect on local graph merge order
    Given alice's key revocation governance unit has been merged into the local graph
    And alice authors a workload unit "late-unit" with:
      | field    | value     |
      | needs    | postgres  |
      | provides | http-rest |
      | tolerates | latency:50ms |
    When "late-unit" arrives at the node for graph merge
    Then the node checks: is alice's key revoked in the local graph? (yes)
    And "late-unit" is rejected with error "author key revoked (revocation merged before unit arrival)"
    And the composition graph does not contain "late-unit"

  Scenario: Unit from revoked author accepted when it merged before revocation
    # INV-S3: causal revocation — no retroactive rejection
    Given alice authors a workload unit "early-unit" with:
      | field    | value     |
      | needs    | postgres  |
      | provides | http-rest |
      | tolerates | latency:50ms |
    And alice signs the unit binding trust_domain "acme-prod" and cluster "cluster-1"
    And "early-unit" is submitted and merged into the local graph
    When alice's key revocation governance unit arrives later and is merged
    Then "early-unit" remains valid in the composition graph
    And no retroactive rejection occurs (INV-S3 causal model)
    And future units from alice will be rejected (revocation now in local graph)

  Scenario: Grace window fallback rejects units near revocation boundary
    # INV-S3: optional grace window for slow-propagation edge cases
    Given governance configures revocation_grace_window = 100 (logical clock delta)
    And alice's key is revoked at logical clock 5000
    And a unit from alice with creation_LC = 5050 arrives at a node
    And the node has NOT yet merged the revocation governance unit
    When the revocation governance unit arrives and is merged
    Then the node retroactively checks: creation_LC 5050 > revocation_LC 5000 + grace 100? No (5050 < 5100)
    And the unit is grandfathered (within grace window)
    But a unit with creation_LC = 5200 would be rejected (5200 > 5100, outside grace window)

  # --- Scope uniqueness (INV-S8) ---

  Scenario: Duplicate author scope assignment -- rejected
    # INV-S8: no two distinct authors with identical (type_scope, trust_domain_scope)
    Given an author "dave" requests scope (type: workload, trust_domain: "acme-prod")
    And alice already holds scope (type: workload, trust_domain: "acme-prod")
    When the governance unit for dave's role assignment is submitted for graph merge
    Then the role assignment is rejected with error "scope uniqueness violation: (workload, acme-prod) already assigned to alice"
    And dave is not granted any authoring scope

  # --- Purpose qualifier on capability ---

  Scenario: Unit with purpose qualifier on capability
    # INV-K2: purpose is optional qualifier; when declared, must match during composition
    Given alice authors a workload unit "analytics-worker" with:
      | field    | value                              |
      | needs    | postgres-compatible(purpose:analytics) |
      | provides | report-output(purpose:compliance)  |
      | tolerates | latency:500ms, failure:restart    |
      | scaling  | min:1, max:5, trigger:queue>100    |
    When alice signs the unit binding trust_domain "acme-prod" and cluster "cluster-1" with validity window 2026-01-01..2027-01-01
    And the unit is submitted for graph merge
    Then the unit is accepted into the composition graph
    And the capability "needs:postgres-compatible" has purpose qualifier "analytics"
    And the capability "provides:report-output" has purpose qualifier "compliance"

  # --- Bounded task authoring ---

  @lifecycle
  Scenario: Author creates bounded task with logical clock validity window
    # INV-W1, INV-W2: bounded tasks have validity windows
    Given alice authors a bounded task unit "nightly-backup":
      | field             | value                     |
      | artifact.type     | oci                       |
      | artifact.ref      | acme/backup:v1            |
      | artifact.digest   | sha256:backup123          |
      | validity_window   | LC 5000..LC 6000          |
    When alice signs the unit binding trust_domain "acme-prod"
    Then the unit is accepted with subtype "bounded_task"
    And the validity window is recorded as logical clock range LC 5000..LC 6000
    And the unit will auto-terminate if the cluster logical clock exceeds LC 6000

  Scenario: Author creates bounded task with wall-time deadline
    Given alice authors a bounded task unit "quarterly-report":
      | field                | value                     |
      | artifact.type        | native                    |
      | artifact.ref         | acme/report-gen:v2        |
      | wall_time_deadline   | 2026-04-14T00:00:00Z      |
    When alice signs the unit
    Then the unit is accepted with wall-time deadline recorded
    And the unit will auto-terminate after "2026-04-14T00:00:00Z"

  Scenario: Service unit authored without validity window (indefinite)
    # INV-W1: services are valid indefinitely
    Given alice authors a service workload unit "web-api":
      | field           | value           |
      | artifact.type   | oci             |
      | artifact.ref    | acme/api:v3     |
    And alice does NOT declare a validity_window
    When alice signs the unit
    Then the unit is accepted with subtype "service" (default)
    And no validity window is recorded
    And the unit is valid indefinitely until terminated or key revoked

  # --- Git-native versioning ---

  @versioning
  Scenario: Unit version references git commit SHA
    Given alice authors workload unit "web-api" at version "abc123def" (git commit)
    And the previous version "web-api" at "789fed012" exists in the graph
    When alice signs the new version
    Then the unit is accepted with version = "abc123def"
    And provenance links: "abc123def" versioned-from "789fed012"
    And the composition graph records the version lineage
