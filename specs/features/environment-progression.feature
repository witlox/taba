@environment @promotion
Feature: Environment progression
  Workloads progress from dev to test to prod via git-native versioning
  and promotion policies. Environment tags on nodes control placement.
  Promotion gates in governance control which transitions auto-promote
  and which require human approval. Progressive disclosure: zero config
  means full auto-promote; governance adds gates where needed.

  Background:
    Given a bootstrapped trust domain "acme" (Tier 0, solo developer)
    And author "alice" with full scope in trust domain "acme"
    And the following nodes in the cluster:
      | node_id    | env   | author_affinity | runtimes        |
      | dev-laptop | dev   | alice           | oci-rootless    |
      | ci-runner  | test  |                 | oci             |
      | prod-1     | prod  |                 | oci             |
      | prod-2     | prod  |                 | oci             |

  # --- Dev placement (no promotion needed) ---

  Scenario: Dev workload placed on author's dev node without promotion policy
    # INV-E1: env:dev requires only author match, no promotion policy
    Given alice authors workload unit "web-api" at version "abc123" (git commit)
    And "web-api" declares artifact.type = "oci" and artifact.ref = "acme/web-api:abc123"
    When alice runs "taba apply" on her dev laptop
    Then the solver places "web-api" on "dev-laptop"
    And placement matches on: env:dev + author:alice affinity
    And no promotion policy is required
    And "web-api" enters state "Running" on "dev-laptop"

  Scenario: Dev workload not placed on another developer's dev node
    Given author "bob" with workload scope in trust domain "acme"
    And a dev node "dev-desktop" with env:dev and author:bob
    And alice authors workload unit "web-api" at version "abc123"
    When the solver evaluates placement for alice's "web-api"
    Then "web-api" is NOT placed on "dev-desktop" (author affinity mismatch)
    And "web-api" IS placed on "dev-laptop" (author:alice matches)

  # --- Multiple dev nodes ---

  Scenario: Author with multiple dev nodes gets workload on all matching nodes
    Given alice has a second dev node:
      | node_id     | env | author_affinity | runtimes     |
      | dev-desktop | dev | alice           | oci, native  |
    And alice authors workload unit "web-api" with artifact.type = "oci"
    When the solver evaluates placement
    Then "web-api" is placed on both "dev-laptop" and "dev-desktop"
    And both nodes satisfy: env:dev + author:alice + runtime:oci

  # --- Promotion to test ---

  Scenario: CI auto-promotes workload to test after merge to main
    # INV-E1: env:test requires promotion policy
    Given alice's "web-api" at version "abc123" is running on dev-laptop
    And alice merges branch to main (git merge produces commit "main-001")
    And no PromotionGate governance unit exists (default: all auto-promote)
    When CI authors a promotion policy "promo-test-001":
      | field       | value                            |
      | unit_ref    | web-api                          |
      | version     | main-001                         |
      | environment | env:test                         |
      | rationale   | CI merge to main, build passed   |
    And the promotion policy is signed and inserted into the graph
    Then the solver evaluates placement for "web-api" version "main-001"
    And "web-api" is placed on "ci-runner" (env:test match)
    And "web-api" remains on "dev-laptop" (INV-E2: promotion is cumulative)

  Scenario: Workload without promotion policy cannot be placed on test
    Given alice authors workload "experimental" at version "exp-001"
    And no promotion policy exists for "experimental" in env:test
    When the solver evaluates placement for "experimental"
    Then "experimental" is placed on "dev-laptop" only (env:dev, author match)
    And "experimental" is NOT placed on "ci-runner" (no promotion for env:test)

  # --- Promotion to prod ---

  Scenario: Git tag triggers prod promotion
    Given "web-api" version "main-001" is running on "ci-runner" (env:test)
    And alice tags the release: git tag v1.0 at commit "main-001"
    When a promotion policy "promo-prod-001" is authored:
      | field       | value                              |
      | unit_ref    | web-api                            |
      | version     | main-001                           |
      | environment | env:prod                           |
      | rationale   | Tagged v1.0, all tests passed      |
    And the promotion policy is signed and inserted into the graph
    Then the solver places "web-api" on "prod-1" and "prod-2"
    And "web-api" continues on "ci-runner" and "dev-laptop" (INV-E2)

  # --- Promotion gates ---

  @governance
  Scenario: PromotionGate requires human approval for test-to-prod
    Given a PromotionGate governance unit exists in "acme":
      | transition    | mode           |
      | dev -> test   | auto           |
      | test -> prod  | human-approval |
    And CI authors a promotion policy for "web-api" to env:prod
    When the solver evaluates the promotion policy
    Then the solver checks the PromotionGate governance unit
    And the promotion is blocked with "human approval required for test -> prod"
    And the workload is NOT placed on prod nodes
    When alice explicitly authors a human-approved promotion policy for env:prod
    Then the solver accepts the promotion
    And "web-api" is placed on prod nodes

  Scenario: No PromotionGate means all transitions auto-promote (INV-E3)
    Given no PromotionGate governance unit exists in trust domain "acme"
    When CI authors a promotion policy for "web-api" to env:prod
    Then the solver accepts the promotion without human approval
    And "web-api" is placed on prod nodes

  # --- Parallel developers ---

  Scenario: Three developers working in parallel, git merge selects winner
    Given three authors with workload scope:
      | author | dev_node      |
      | alice  | dev-laptop    |
      | bob    | dev-bob       |
      | carol  | dev-carol     |
    And each authors a version of "web-api":
      | author | version  | branch       |
      | alice  | aaa111   | feature-auth |
      | bob    | bbb222   | feature-perf |
      | carol  | ccc333   | feature-ui   |
    When each runs "taba apply" on their dev node
    Then alice's version runs on "dev-laptop"
    And bob's version runs on "dev-bob"
    And carol's version runs on "dev-carol"
    And no version is placed on test or prod (no promotion policies)
    When bob's branch is merged to main (git merge produces "main-002")
    And CI authors a promotion policy for "web-api" version "main-002" to env:test
    Then only "main-002" (bob's merged code) runs on "ci-runner"
    And alice's and carol's branches continue on their dev nodes unaffected

  # --- Dev node failure ---

  @resilience
  Scenario: Dev node goes offline -- workloads left dead by default
    # INV-N5: env:dev defaults to leave-dead
    Given "web-api" is running on alice's "dev-laptop"
    When "dev-laptop" goes offline (laptop closed)
    And gossip detects "dev-laptop" as failed
    Then the solver does NOT re-place "web-api" to another node
    And "web-api" remains in state "Running" in the graph (desired state unchanged)
    And actual state on "dev-laptop" is unknown until it returns
    When "dev-laptop" comes back online
    Then the node reconciliation loop detects "web-api" is still placed here
    And "web-api" resumes (or is restarted based on failure semantics)

  Scenario: Dev workload with override placement_on_failure=replace
    Given alice authors workload "dev-service" with placement_on_failure = "replace"
    And "dev-service" is running on "dev-laptop"
    And alice has a second dev node "dev-desktop" with author:alice
    When "dev-laptop" goes offline
    Then the solver re-places "dev-service" to "dev-desktop"
    And the override takes precedence over the env:dev default

  # --- Prod node failure (contrast with dev) ---

  @resilience
  Scenario: Prod node failure triggers automatic re-placement
    # INV-N5: env:prod defaults to auto-replace
    Given "web-api" is running on "prod-1" and "prod-2"
    When "prod-1" fails
    And gossip detects "prod-1" as failed
    Then the solver recomputes placement for "web-api"
    And if another prod node exists, "web-api" is re-placed there
    And if no other prod node exists, "web-api" continues on "prod-2" only
