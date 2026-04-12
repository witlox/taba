Feature: Trust domain management
  Trust domains are governance units requiring multi-party creation.

  Scenario: Trust domain creation requires policy resolution
    Given manager A proposes trust domain "clinical-trial-X"
    And manager B requests access for their team
    When the proposals are submitted
    Then the solver detects a governance conflict (competing scope claims)
    And requires explicit policy resolution before the trust domain is created

  Scenario: Role assignment within trust domain
    Given trust domain "clinical-trial-X" exists
    And an operator with governance scope in that domain
    When the operator assigns author A workload scope in the domain
    Then author A can create workload units in "clinical-trial-X"
    And author A cannot create units in other trust domains

  Scenario: Trust domain with time-bounded access
    Given a trust domain for external collaborator access
    And a role assignment with expiry "2026-12-31"
    When the expiry date passes
    Then the role assignment is revoked
    And the collaborator can no longer author units in the domain

  @security
  Scenario: Trust domain cannot be created by single author
    Given an author with governance scope
    When the author unilaterally submits a trust domain unit
    Then the solver detects this requires multi-party agreement
    And the trust domain is not created until policy resolves it
