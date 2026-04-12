@security @governance
Feature: Shamir key ceremony
  The root key ceremony is the pre-graph bootstrap. It produces the
  Ed25519 keypair that signs the first governance unit, seeding the
  composition graph. Tier 1 ceremony: start, add shares, complete
  with witness. Key material is zeroized after use.

  # --- Happy path ---

  Scenario: Start Tier 1 ceremony with 5 shares and threshold 3
    Given an operator initiates a Shamir ceremony
    When the ceremony is configured with total_shares=5 and threshold=3
    Then the ceremony enters "awaiting_shares" state
    And the ceremony ID is returned for subsequent share submissions
    And no key material exists yet

  Scenario: Add shares one by one and ceremony progresses
    Given a ceremony in "awaiting_shares" state with total_shares=5 and threshold=3
    When share holder "holder-1" submits share 1 of 5
    Then the ceremony records 1 of 3 required shares received
    And the ceremony remains in "awaiting_shares" state
    When share holder "holder-2" submits share 2 of 5
    Then the ceremony records 2 of 3 required shares received
    And the ceremony remains in "awaiting_shares" state
    When share holder "holder-3" submits share 3 of 5
    Then the ceremony records 3 of 3 required shares received
    And the ceremony transitions to "threshold_met" state

  Scenario: Complete ceremony with witness generates root key
    Given a ceremony in "threshold_met" state with 3 of 3 shares received
    And witness node "n-witness" is designated
    When the witness confirms and the ceremony is finalized
    Then the root Ed25519 keypair is reconstructed from the Shamir shares
    And the public key "pk_root" is recorded
    And the ceremony transitions to "completed" state
    And a ceremony audit event is generated

  Scenario: Root key signs first governance unit to bootstrap trust domain
    Given a completed ceremony with root public key "pk_root"
    When the root key signs the first TrustDomain governance unit "root-domain"
    Then the governance unit signature is valid against "pk_root"
    And "root-domain" is inserted as the first unit in the composition graph
    And the composition graph is seeded and operational
    And the root key private material is zeroized immediately after signing

  # --- Validation errors ---

  @security
  Scenario: Ceremony with threshold greater than total_shares is rejected
    Given an operator initiates a Shamir ceremony
    When the ceremony is configured with total_shares=3 and threshold=5
    Then the ceremony is rejected with error "InvalidCeremonyConfig: threshold 5 > total_shares 3"
    And no ceremony state is created

  Scenario: Ceremony with threshold of zero is rejected
    Given an operator initiates a Shamir ceremony
    When the ceremony is configured with total_shares=5 and threshold=0
    Then the ceremony is rejected with error "InvalidCeremonyConfig: threshold must be >= 1"
    And no ceremony state is created

  Scenario: Ceremony with threshold of 1 is rejected (requires multi-party)
    Given an operator initiates a Shamir ceremony
    When the ceremony is configured with total_shares=5 and threshold=1
    Then the ceremony is rejected with error "InvalidCeremonyConfig: threshold must be >= 2 for multi-party security"
    And no ceremony state is created

  @security
  Scenario: Duplicate share from same holder is rejected
    Given a ceremony in "awaiting_shares" state with total_shares=5 and threshold=3
    And share holder "holder-1" has already submitted share 1
    When share holder "holder-1" attempts to submit share 1 again
    Then the submission is rejected with error "DuplicateShare: holder-1 already submitted"
    And the ceremony share count remains unchanged at 1
    And the ceremony remains in "awaiting_shares" state

  # --- Cancellation ---

  Scenario: Cancel ceremony zeroizes all share material
    Given a ceremony in "awaiting_shares" state with 2 shares received from "holder-1" and "holder-2"
    When the operator cancels the ceremony
    Then all received share material is zeroized from memory
    And the ceremony transitions to "cancelled" state
    And no key material can be recovered from the cancelled ceremony
    And a ceremony cancellation audit event is generated

  # --- State queries ---

  Scenario: Ceremony state is queryable during progress
    Given a ceremony in "awaiting_shares" state with total_shares=5 and threshold=3
    And 2 shares have been received from ["holder-1", "holder-2"]
    When an authorized operator queries the ceremony status
    Then the response includes ceremony_id, state "awaiting_shares"
    And shares_received=2, threshold=3, total_shares=5
    And the list of holders who submitted: ["holder-1", "holder-2"]
    But no share values or key material are included in the response

  # --- Verification ---

  @security
  Scenario: Reconstructed public key verified against expected value
    Given a ceremony configured with expected public key fingerprint "fp_expected_abc"
    And 3 shares have been submitted meeting the threshold
    When the root keypair is reconstructed
    Then the reconstructed public key fingerprint is computed
    And it is compared against "fp_expected_abc"
    And if they match, the ceremony proceeds to completion
    And if they do not match, the ceremony fails with "KeyMismatch: reconstructed key does not match expected fingerprint"

  @security
  Scenario: Key material zeroized after use
    Given a completed ceremony with root key used to sign the bootstrap governance unit
    When the signing operation completes
    Then the private key bytes are overwritten with zeros (zeroize crate)
    And the Shamir share bytes held in memory are overwritten with zeros
    And a memory audit confirms no residual key material remains
    And only the public key "pk_root" persists for future verification
