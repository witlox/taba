@security @governance
Feature: Trust domain management
  Trust domains are governance units defining authorization boundaries.
  Creation requires multi-party agreement (INV-S10). Role assignments
  are scoped, unique (INV-S8), and optionally time-bounded. Cross-domain
  roles require explicit policy -- no implicit inheritance.

  # --- Creation ---

  Scenario: Trust domain creation requires 2+ distinct authors (INV-S10)
    Given author "alice" with governance scope in the root trust domain
    And author "bob" with governance scope in the root trust domain
    When "alice" submits a TrustDomain governance unit "pharma-trials" listing required signers ["alice", "bob"]
    And "bob" cosigns the TrustDomain governance unit "pharma-trials"
    Then the solver verifies 2 distinct cryptographic signatures are present
    And trust domain "pharma-trials" is created in the composition graph
    And a governance unit records the creation with both author signatures

  @security
  Scenario: Single author cannot create trust domain unilaterally (INV-S10)
    Given author "alice" with governance scope in the root trust domain
    When "alice" submits a TrustDomain governance unit "secret-lab" listing required signers ["alice"]
    Then the solver rejects the trust domain creation
    And the error is "TrustDomainRequiresMultiParty: minimum 2 distinct signers required"
    And no governance unit is persisted for "secret-lab"

  Scenario: Trust domain creation with exactly the threshold of signers
    Given 3 authors "alice", "bob", "carol" with governance scope
    When "alice" submits a TrustDomain governance unit "multi-org" listing required signers ["alice", "bob", "carol"]
    And "bob" cosigns the TrustDomain governance unit "multi-org"
    And "carol" cosigns the TrustDomain governance unit "multi-org"
    Then the solver verifies 3 distinct cryptographic signatures are present
    And trust domain "multi-org" is created in the composition graph

  # --- Role assignment ---

  Scenario: Role assignment within trust domain (happy path)
    Given trust domain "pharma-trials" exists with authors "alice" and "bob"
    And "alice" has governance scope in "pharma-trials"
    When "alice" creates a RoleAssignment governance unit granting "carol" workload scope in "pharma-trials"
    And "bob" cosigns the RoleAssignment governance unit
    Then "carol" can create workload units in "pharma-trials"
    And "carol" cannot create policy units in "pharma-trials"
    And "carol" cannot create units in any other trust domain

  @security
  Scenario: Role scope uniqueness -- duplicate (type_scope, trust_domain) rejected (INV-S8)
    Given trust domain "pharma-trials" exists
    And "carol" already has workload scope in "pharma-trials"
    When a governance author attempts to assign "dave" workload scope in "pharma-trials" with identical type_scope tuple
    Then the role assignment is rejected
    And the error is "ScopeOverlap: (workload, pharma-trials) already assigned to carol"
    And no RoleAssignment governance unit is persisted

  # --- Time-bounded roles ---

  Scenario: Time-bounded role assignment grants access until expiry
    Given trust domain "pharma-trials" exists
    And a RoleAssignment governance unit granting "extern-1" data scope in "pharma-trials" with expiry "2026-12-31T23:59:59Z"
    When the current time is "2026-06-15T10:00:00Z"
    Then "extern-1" can create data units in "pharma-trials"
    And the role assignment shows remaining validity of approximately 199 days

  Scenario: Expired role revokes authoring ability
    Given trust domain "pharma-trials" exists
    And "extern-1" has data scope in "pharma-trials" with expiry "2026-12-31T23:59:59Z"
    When the current time advances past "2026-12-31T23:59:59Z"
    And "extern-1" attempts to create a data unit in "pharma-trials"
    Then the unit is rejected with error "ScopeExpired: role assignment expired at 2026-12-31T23:59:59Z"
    And the signature verification fails at the scope validity check (INV-S3 clause b)

  Scenario: Expired author's existing units remain valid but cannot be modified
    Given "extern-1" created data unit "dataset-42" in "pharma-trials" at "2026-06-15T10:00:00Z"
    And "extern-1" role expired at "2026-12-31T23:59:59Z"
    When the current time is "2027-01-15T10:00:00Z"
    Then data unit "dataset-42" remains valid in the composition graph
    And data unit "dataset-42" signature verification passes (key valid at creation time)
    But "extern-1" cannot submit modifications or new versions of "dataset-42"

  # --- Cross-domain ---

  @security
  Scenario: Cross-domain role requires explicit policy, no implicit inheritance
    Given trust domain "pharma-trials" exists with "alice" having governance scope
    And trust domain "finance-ops" exists with "bob" having governance scope
    And "carol" has workload scope in "pharma-trials"
    When "carol" attempts to create a workload unit in "finance-ops"
    Then the unit is rejected with error "ScopeViolation: carol has no scope in trust domain finance-ops"
    And no implicit role inheritance from "pharma-trials" to "finance-ops" is applied

  Scenario: Cross-domain role granted via explicit policy
    Given trust domain "pharma-trials" and trust domain "shared-data" both exist
    And a policy unit authored by governance holders of both domains grants "carol" data scope in "shared-data"
    When "carol" creates a data unit in "shared-data"
    Then the unit is accepted
    And the policy unit is recorded in both trust domains' governance lineage

  # --- Bootstrap ---

  @security
  Scenario: Shamir root key signs first governance unit at bootstrap
    Given a completed Shamir ceremony producing root key with public key "pk_root_abc123"
    When the root key signs the first TrustDomain governance unit "root-domain" with signers ["ceremony-witness-1", "ceremony-witness-2"]
    Then the governance unit signature is verified against "pk_root_abc123"
    And "root-domain" becomes the root trust domain seeding the composition graph
    And the root key material is zeroized after signing
    And the ceremony completion is recorded as a governance unit in the graph

  # --- Break-glass recovery ---

  @security
  Scenario: Break-glass recovery via root key when all policy authors leave (FM-18)
    Given trust domain "acme-prod" bootstrapped with Shamir ceremony
    And author "carol" was the sole policy-scope author in "acme-prod"
    And carol's key has been revoked (left the organization)
    And no other author has policy scope
    When the root key is reconstructed via Shamir ceremony (3 of 5 shares)
    Then the root key can author a new RoleAssignment governance unit
    And a new author "eve" is assigned policy scope in "acme-prod"
    And eve can now create policies to resolve pending conflicts
    And existing policies authored by carol remain valid (signed before revocation)
    And the root key material is zeroized after the role assignment

  Scenario: Break-glass recovery in Tier 0 (solo key is root key)
    Given a Tier 0 trust domain "solo-domain" with author "alice"
    And alice is the sole author with all scopes
    And alice's laptop is lost (key compromised)
    When alice uses a backup of the Tier 0 root key
    Then alice can revoke the compromised key
    And create a new author identity with the root key
    And existing units signed before revocation remain valid
    And units signed by the compromised key after revocation are rejected

  # --- Role succession with overlapping scopes ---

  @governance
  Scenario: Multiple policy authors prevent succession gap (INV-S8a)
    Given authors "carol" and "dan" both have policy scope in "acme-prod"
    And carol has authored 5 active policies
    When carol's key is revoked (leaves the org)
    Then carol's existing policies remain valid (signed before revocation)
    And dan can continue authoring new policies (no succession gap)
    And dan can supersede carol's policies if needed
    And the system is never locked out of policy authoring

  Scenario: Scope uniqueness still enforced for workload authors (INV-S8)
    Given author "alice" has scope (type: workload, trust_domain: "acme-prod")
    And a role assignment attempts to give "frank" identical scope
    When the governance unit for frank's assignment is submitted
    Then it is rejected: "scope uniqueness violation for state-producing type"
    And alice remains the sole workload-scope author in "acme-prod"
    But frank CAN be assigned policy scope (overlapping allowed for decision types)
