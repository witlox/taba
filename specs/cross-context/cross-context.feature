@consistency @security
Feature: Cross-context interactions
  Verify correct behavior at bounded context boundaries. The flow is:
  Unit Management (validate) -> Security (sign) -> Composition Graph
  (verify + insert via WAL) -> Solver (compose + place) -> Node
  (reconcile). Distribution handles gossip and erasure coding across
  all contexts.

  # --- Unit insertion pipeline ---

  Scenario: Unit insertion flows through validate, sign, verify, and WAL insert
    Given author "alice" with workload scope in trust domain "ops"
    And a valid workload unit "wl-api" declaring needs ["http-port"] and provides ["rest-api"]
    When "alice" submits "wl-api" for insertion
    Then Unit Management validates the declaration schema and capability types
    And Security signs the unit: Sign(alice_key, hash(wl-api || ops || cluster-1 || validity-window))
    And the Composition Graph verifies the signature synchronously (blocking gate)
    And the Graph checks alice's scope is valid for workload type in "ops"
    And the Graph checks alice's key was not revoked before the creation timestamp (INV-S3)
    And "wl-api" is written to WAL as Merged(wl-api) before becoming visible
    And "wl-api" is visible to local queries only after WAL persistence (INV-C4)

  Scenario: Unit with invalid signature is rejected at sync gate
    Given author "alice" submits workload unit "wl-bad" with a corrupted signature
    When the Composition Graph verifies the signature
    Then insertion is rejected with error "InvalidSignature"
    And "wl-bad" is not written to WAL
    And "wl-bad" is not visible in the graph
    And the rejection event is logged for security audit

  # --- Graph -> Solver ---

  Scenario: Graph update triggers solver re-evaluation of affected compositions
    Given workload "wl-app" needs capability "postgres-store" and is currently unresolved
    And the composition graph contains no unit providing "postgres-store"
    When a new data unit "ds-pg" providing capability "postgres-store" is inserted
    Then the solver is notified of the graph update
    And the solver re-evaluates compositions affected by "ds-pg"
    And "wl-app" + "ds-pg" form a valid composition (capability matched)
    And the solver computes placement for the new composition

  # --- Solver -> Node ---

  Scenario: Solver placement triggers node reconciliation loop
    Given the solver places workload "wl-app" on node "n-003"
    And the placement decision is recorded in the composition graph
    When "n-003"'s reconciliation loop detects the new placement
    Then "n-003" starts the workload runtime for "wl-app"
    And "n-003" performs runtime capability checks via Security context
    And "n-003" reports health status for "wl-app" back to the graph
    And if "wl-app" fails to start, "n-003" reports the failure and solver re-places

  # --- Node failure cascade ---

  @resilience
  Scenario: Node failure triggers signed gossip, erasure reconstruction with backpressure, and re-placement
    Given workloads ["wl-a", "wl-b"] are running on node "n-004"
    And "n-004" holds 8 erasure-coded graph shards
    When "n-004" fails and becomes unresponsive
    Then "n-001" detects "n-004" unresponsive via SWIM direct probe
    And "n-001" sends signed indirect probe requests to "n-002" and "n-005"
    And both "n-002" and "n-005" confirm "n-004" unresponsive (2-witness per INV-R3)
    And "n-004" is declared Failed in the membership view
    And erasure reconstruction begins with backpressure (governance shards first per INV-R1)
    And the solver recomputes placement for "wl-a" and "wl-b"
    And surviving nodes' reconciliation loops start the re-placed workloads

  # --- Key revocation cascade ---

  @security
  Scenario: Key revocation propagates via priority gossip, rejects new units, preserves existing
    # INV-S3: causal revocation — merge order determines validity
    Given author "dave" has authored 5 units ["u-1", "u-2", "u-3", "u-4", "u-5"] in "ops"
    And all 5 units are merged into the graph before any revocation
    When "dave"'s key revocation governance unit is authored and propagated via priority gossip
    Then all nodes receive and merge the revocation within the gossip convergence window
    And units ["u-1" through "u-5"] remain valid (merged before revocation, no retroactive rejection)
    And a new unit "u-6" submitted by "dave" after the revocation is merged is rejected with "KeyRevoked"
    And the solver does not remove existing compositions involving "dave"'s pre-revocation units

  # --- Taint propagation across boundaries ---

  @security @data
  Scenario: Taint crosses context boundaries via provenance traversal at query time
    Given data unit "ds-patient-raw" has classification "PII"
    And workload "wl-transform" consumes "ds-patient-raw" and produces "ds-patient-agg"
    And no declassification policy exists for "ds-patient-agg"
    When a consumer queries the classification of "ds-patient-agg"
    Then Security computes taint by traversing the provenance graph (INV-S4)
    And "ds-patient-agg" inherits classification "PII" from "ds-patient-raw"
    And the taint is computed at query time (not cached at merge time)
    And any workload consuming "ds-patient-agg" must declare capability for "PII" data access

  Scenario: Multi-input workload inherits union (most restrictive) of all input classifications
    Given data unit "ds-public" has classification "public"
    And data unit "ds-internal" has classification "internal"
    And data unit "ds-pii" has classification "PII"
    And workload "wl-join" consumes all three and produces "ds-combined"
    When a consumer queries the classification of "ds-combined"
    Then "ds-combined" inherits classification "PII" (most restrictive in lattice: public < internal < confidential < PII)
    And any workload consuming "ds-combined" must have PII access capability

  # --- Bootstrap flow ---

  @security @governance
  Scenario: Bootstrap ceremony produces root governance unit that seeds the graph
    Given a Shamir ceremony completes with root public key "pk_root"
    And the root key signs a TrustDomain governance unit "root-domain" with 2 ceremony witnesses
    When the bootstrap process inserts "root-domain" into the composition graph
    Then "root-domain" is the first unit in the graph (special-case: no prior graph state)
    And signature verification passes against "pk_root"
    And the WAL records Merged(root-domain) as the first entry
    And the composition graph transitions from empty to seeded
    And subsequent unit insertions follow the normal validate-sign-verify-insert pipeline

  # --- Degraded mode propagation ---

  @operational
  Scenario: Degraded node announces via gossip and solver stops placing on it
    Given node "n-002" detects memory limit exceeded at 100% (INV-R6)
    When "n-002" transitions to Degraded operational mode
    Then "n-002" sends a signed gossip message announcing Degraded status
    And all nodes update their membership view: "n-002" health = "degraded"
    And the solver on every node excludes "n-002" from new placement decisions
    And existing workloads on "n-002" continue running
    And the solver re-places any pending workloads that were targeting "n-002"

  # --- Causal buffering across contexts ---

  @consistency
  Scenario: Unit with missing refs enters pending state and is promoted when refs arrive
    Given unit "u-child" references unit "u-parent" in its provenance declaration
    And "u-child" arrives at node "n-001" before "u-parent"
    When "n-001" verifies "u-child"'s signature (valid)
    Then "u-child" is written to WAL as Pending(u-child, missing_refs=[u-parent])
    And "u-child" is NOT visible to local queries or solver evaluation
    When "u-parent" arrives at "n-001" and is verified and merged
    Then "u-child" is promoted: WAL records Promoted(u-child)
    And "u-child" becomes visible to local queries and solver evaluation
    And the solver re-evaluates compositions that include "u-child"
    And provenance queries for "u-child" now return "u-parent" as the valid reference
