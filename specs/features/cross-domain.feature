@cross-domain @security
Feature: Cross-trust-domain forwarding
  When the graph is sharded by trust domain, cross-domain interactions
  require bridge nodes (participating in multiple domains), bilateral
  policy (mutual consent), and a forwarding query protocol. Bridges are
  emergent by default, governance-restricted when needed. Cross-domain
  cache is fail-open by default, governance-overridable to fail-closed.

  Background:
    Given trust domain "acme-prod" with root governance unit
    And trust domain "partner-payments" with root governance unit
    And the following nodes:
      | node_id   | domains                       | env  |
      | acme-1    | acme-prod                     | prod |
      | acme-2    | acme-prod                     | prod |
      | bridge-1  | acme-prod, partner-payments   | prod |
      | partner-1 | partner-payments              | prod |
      | partner-2 | partner-payments              | prod |

  # --- Bridge discovery ---

  Scenario: Bridge node is emergent from multi-domain membership
    # INV-X4: any node in multiple domains is a bridge by default
    Given "bridge-1" is admitted to both "acme-prod" and "partner-payments"
    And no governance unit restricts bridging
    When acme-1's solver queries "who is a bridge for partner-payments?"
    Then "bridge-1" responds as an available bridge
    And no explicit bridge designation was needed

  Scenario: Governance restricts bridges to designated nodes only
    # INV-X4: governance can restrict bridging
    Given "acme-prod" governance unit declares: bridge_policy = "explicit_only"
    And "acme-prod" governance designates "bridge-1" as authorized bridge to "partner-payments"
    And "acme-2" is also admitted to "partner-payments" (multi-domain node)
    When acme-1's solver queries "who is a bridge for partner-payments?"
    Then "bridge-1" responds as an authorized bridge
    And "acme-2" does NOT respond (not designated, governance restricts)

  # --- Cross-domain capability advertisement ---

  Scenario: Domain publishes capability advertisement via bridge
    # INV-X5: advertisements are governance units propagated via bridges
    Given "partner-payments" publishes a CrossDomainCapability governance unit:
      | field      | value                       |
      | provides   | payment-api                 |
      | conditions | bilateral policy required   |
    When "bridge-1" receives the governance unit in "partner-payments"
    Then "bridge-1" gossips the advertisement to nodes in "acme-prod"
    And "acme-1" learns that "partner-payments" offers "payment-api"
    And "acme-2" learns the same via gossip

  # --- Cross-domain composition with bilateral policy ---

  Scenario: Cross-domain composition succeeds with bilateral policy
    # INV-X1: both domains must authorize
    Given workload "checkout-service" in "acme-prod" needs capability "payment-api"
    And no provider for "payment-api" exists in "acme-prod"
    And "partner-payments" advertises "payment-api" via cross-domain capability
    And bilateral policy exists:
      | domain             | policy                                                |
      | acme-prod          | "checkout-service may consume payment-api from partner-payments" |
      | partner-payments   | "acme-prod may access payment-api under SLA conditions"  |
    When the solver in "acme-prod" evaluates composition for "checkout-service"
    Then the solver detects unresolved need "payment-api" in local graph
    And the solver finds cross-domain advertisement from "partner-payments"
    And the solver sends a signed forwarding query to "bridge-1"
    And "bridge-1" verifies bilateral policy in both domains
    And "bridge-1" executes the query against "partner-payments" graph
    And "bridge-1" returns a signed result with the "payment-api" provider details
    And the solver creates a cross-domain composition linking "checkout-service" to the foreign provider
    And the result is cached in "acme-prod" for future queries

  Scenario: Cross-domain composition fails closed without bilateral policy
    # INV-X1: fail closed on missing policy
    Given workload "data-sync" in "acme-prod" needs capability "payment-api"
    And "partner-payments" advertises "payment-api"
    And policy exists in "acme-prod" authorizing access to "partner-payments"
    But NO policy exists in "partner-payments" authorizing "acme-prod" access
    When the solver sends a forwarding query to "bridge-1"
    Then "bridge-1" checks bilateral policy
    And rejects the query: "missing authorization in partner-payments for acme-prod"
    And the composition fails closed (INV-S2 across boundaries)
    And "data-sync" remains with unresolved need "payment-api"

  Scenario: Cross-domain composition fails when no policy exists in either domain
    Given workload "rogue-service" in "acme-prod" needs capability "payment-api"
    And NO bilateral policy exists in either domain
    When the solver detects the cross-domain advertisement
    Then the solver does not even send a forwarding query (no local policy)
    And "rogue-service" has unresolved need "payment-api"

  # --- Forwarding query protocol ---

  Scenario: Forwarding query returns read-only view, not merged into graph
    # INV-X2: read-only, never merged
    Given a cross-domain forwarding query from "acme-1" to "bridge-1"
    When "bridge-1" returns the result (provider details from partner-payments)
    Then the result is stored as a cached cross-domain reference in "acme-prod"
    And the foreign unit is NOT inserted into "acme-prod"'s composition graph
    And the foreign unit's full content stays in "partner-payments" graph only
    And "acme-prod" references it by UnitId only

  # --- Cache and fail-open ---

  @resilience
  Scenario: Stale cache served when bridge is unavailable (fail open)
    # INV-X3: fail open by default
    Given "checkout-service" has an existing cross-domain composition with "payment-api"
    And the cached query result was refreshed at logical clock 5000
    When "bridge-1" goes offline
    And the solver re-evaluates the composition
    Then the solver uses the cached result from LC 5000 (stale but available)
    And "checkout-service" continues operating with the cached composition
    And an alert is raised: "bridge-1 unavailable, serving stale cross-domain cache"

  Scenario: Governance requires fail-closed freshness
    Given "acme-prod" governance declares: cross_domain_cache = "strict_freshness" for "partner-payments"
    And "checkout-service" has a cached cross-domain composition
    When "bridge-1" goes offline
    And the solver re-evaluates the composition
    Then the solver rejects the stale cache (governance requires freshness)
    And "checkout-service" cross-domain composition enters pending state
    And the workload continues with last-known placement but new compositions are blocked
    When "bridge-1" comes back online
    Then the cache is refreshed and the composition is re-evaluated

  # --- No bridge exists ---

  Scenario: No bridge between domains surfaces as unresolved capability
    # INV-X6: solver surfaces the need, operator decides
    Given trust domain "new-partner" exists with no shared nodes with "acme-prod"
    And "new-partner" advertises "ml-inference" capability (via manual config)
    And workload "ai-service" in "acme-prod" needs "ml-inference"
    When the solver evaluates composition
    Then the solver finds no bridge for "new-partner"
    And the composition is blocked with: "no bridge between acme-prod and new-partner"
    And an alert is raised for the operator
    And the solver does NOT automatically create a bridge

  Scenario: Admitting a node to both domains creates a bridge
    Given no bridge exists between "acme-prod" and "new-partner"
    And the operator admits "acme-2" to "new-partner" trust domain
    When "acme-2" completes admission to "new-partner"
    Then "acme-2" becomes an emergent bridge between "acme-prod" and "new-partner"
    And "acme-2" begins gossiping cross-domain capability advertisements
    And the solver re-evaluates compositions that were blocked on the missing bridge

  # --- Bridge security ---

  @security
  Scenario: Compromised bridge cannot inject cross-domain units
    Given "bridge-1" is compromised by an attacker
    When the attacker attempts to forge a forwarding query result
    Then the result signature does not match (bridge key compromised but forgery detectable)
    When the attacker attempts to inject units into "partner-payments" via the bridge
    Then signature verification rejects the units (attacker doesn't have author keys)
    And the attacker can observe both domains' graph state (wider blast radius)
    But cannot modify either domain's graph

  Scenario: Bridge eviction isolates domains
    Given "bridge-1" is the only bridge between "acme-prod" and "partner-payments"
    When "bridge-1" is evicted via gossip (compromise detected)
    Then cross-domain compositions enter pending state
    And cached results serve existing compositions (fail open)
    And new cross-domain compositions are blocked
    And alert raised: "sole bridge evicted, domains isolated"

  # --- Inter-domain discovery ---

  Scenario: Bridge auto-discovers cross-domain capabilities
    Given "bridge-1" participates in both domains
    And "partner-payments" adds a new CrossDomainCapability: "fraud-detection"
    When "bridge-1" receives the new governance unit via "partner-payments" gossip
    Then "bridge-1" automatically gossips the advertisement to "acme-prod" nodes
    And "acme-1" can now discover "fraud-detection" from "partner-payments"
    And no manual configuration was needed

  Scenario: Manual configuration for domains without shared nodes
    Given "acme-prod" has no bridge to "external-vendor"
    And the operator configures known domain: external-vendor at seed nodes [ext-1, ext-2]
    When "acme-1" queries capabilities of "external-vendor"
    Then the query is sent to configured seed nodes (not via bridge gossip)
    And the response includes advertised capabilities from "external-vendor"
    And this bootstraps discovery until a bridge is established

  # --- Cross-domain provenance ---

  @data
  Scenario: Provenance query traverses cross-domain boundary via bridge
    Given data unit "enriched-orders" in "acme-prod" was produced by composition with "payment-api" from "partner-payments"
    And the provenance chain crosses the domain boundary
    When an operator queries provenance of "enriched-orders"
    Then the local provenance is returned from "acme-prod" graph
    And the cross-domain segment issues a forwarding query to "bridge-1"
    And "bridge-1" returns the provenance from "partner-payments" (read-only)
    And the full cross-domain provenance chain is assembled and displayed
