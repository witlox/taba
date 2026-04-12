@resilience @operational
Feature: Node lifecycle
  Nodes are peers that join, operate, and leave the cluster. Gossip is
  SWIM-based with signed messages (DL-009, INV-R3). Failure detection
  requires 2-witness confirmation. Operational modes govern permitted
  operations (INV-R6).

  # --- Join ---

  Scenario: Node joins cluster via authenticated gossip and begins solving
    Given an existing cluster of 5 nodes ["n-001", "n-002", "n-003", "n-004", "n-005"]
    And node "n-006" has a valid Ed25519 identity key pair
    When "n-006" sends a signed join request via gossip to seed node "n-001"
    Then "n-001" verifies the gossip message signature
    And "n-001" propagates the join to the membership view
    And "n-006" receives graph shards via erasure coding within 30 seconds
    And "n-006" transitions from Joining to Attesting to Active
    And "n-006" begins participating in solver placement decisions

  @security
  Scenario: Unsigned gossip message is dropped (DL-009)
    Given an existing cluster of 5 nodes
    When an unsigned join request arrives at node "n-001"
    Then "n-001" drops the message without processing
    And "n-001" logs "GossipAuthFailure: unsigned message from unknown sender"
    And the sending address is flagged for investigation
    And no membership state changes occur

  @security
  Scenario: Gossip message with invalid signature is rejected
    Given an existing cluster of 5 nodes
    When a gossip message arrives at "n-001" with a cryptographically invalid signature
    Then "n-001" rejects the message
    And "n-001" logs "GossipAuthFailure: invalid signature"
    And the sender node is flagged for investigation
    And no membership state changes occur

  # --- Graceful leave ---

  Scenario: Node leaves gracefully with drain, shard redistribution, and membership removal
    Given node "n-003" is Active and running workloads ["wl-a", "wl-b", "wl-c"]
    And node "n-003" holds 12 erasure-coded graph shards
    When the operator initiates drain on "n-003"
    Then "n-003" transitions to Draining state
    And workloads ["wl-a", "wl-b", "wl-c"] are re-placed on other nodes by the solver
    And each workload executes its declared on_shutdown handler
    And the 12 graph shards are redistributed via erasure re-coding
    And "n-003" transitions to Left state
    And "n-003" is removed from the membership view on all nodes

  # --- Failure detection ---

  @resilience
  Scenario: Node failure requires 2-witness confirmation before declaring failed (DL-009, INV-R3)
    Given node "n-004" becomes unresponsive at time T
    When "n-001" detects "n-004" unresponsive via direct SWIM probe
    And "n-001" requests indirect probes from "n-002" and "n-005"
    And both "n-002" and "n-005" confirm "n-004" is unresponsive
    Then "n-004" is declared Failed with 2 independent witness confirmations
    And erasure coding reconstructs "n-004"'s graph shards from surviving nodes
    And the solver recomputes placement for all workloads previously on "n-004"
    And membership view converges to exclude "n-004" on all nodes

  Scenario: Single witness cannot declare node failed
    Given node "n-004" becomes unresponsive
    When "n-001" detects "n-004" unresponsive via direct SWIM probe
    And "n-001" requests indirect probes from "n-002" and "n-005"
    And "n-002" confirms unresponsive but "n-005" reports "n-004" is alive
    Then "n-004" is NOT declared Failed
    And "n-004" transitions to Suspected state
    And additional probe rounds are scheduled

  @resilience
  Scenario: Suspected node remains in placement pool with health=unknown (INV-R5)
    Given node "n-004" is in Suspected state with health "unknown"
    And nodes "n-001", "n-002", "n-003", "n-005" are Active with health "healthy"
    When the solver computes placement for a new workload unit
    Then the solver prefers Active nodes "n-001", "n-002", "n-003", "n-005"
    But "n-004" remains in the placement pool (not removed)
    And if all Active nodes are at capacity, "n-004" is eligible for placement
    And "n-004" remains Suspected until SWIM multi-probe consensus resolves

  # --- Single node ---

  Scenario: Single node cluster operates normally
    Given a single node "n-solo" running taba with no peers
    When author "admin" submits a workload unit "wl-solo"
    Then the solver places "wl-solo" on "n-solo"
    And no erasure coding is performed (single shard, no redundancy needed)
    And the composition graph is fully stored on "n-solo"
    And the system reports Normal operational mode

  # --- Operational modes ---

  @operational
  Scenario: Normal to Degraded on memory limit exceeded (INV-R6)
    Given node "n-002" has a configured memory limit of 512 MB for graph state
    And node "n-002"'s active graph currently uses 520 MB
    When the memory monitor detects usage exceeds 100% of limit
    Then "n-002" transitions to Degraded operational mode
    And "n-002" announces Degraded status via signed gossip message
    And the solver stops placing new workloads on "n-002"
    And "n-002" refuses new unit insertions locally

  @operational
  Scenario: Degraded to Recovery when trigger resolved
    Given node "n-002" is in Degraded mode due to memory limit exceeded
    When auto-compaction completes and graph usage drops to 380 MB (74% of 512 MB)
    Then "n-002" transitions to Recovery operational mode
    And erasure re-coding begins for any under-replicated shards
    And "n-002" announces Recovery status via signed gossip

  @operational
  Scenario: Recovery to Normal when re-coding complete
    Given node "n-002" is in Recovery mode with erasure re-coding underway
    When all shard re-coding completes and redundancy is restored
    Then "n-002" transitions to Normal operational mode
    And "n-002" announces Normal status via signed gossip
    And the solver resumes normal placement on "n-002"

  @operational
  Scenario: Degraded mode permits only drain and evacuation
    Given node "n-002" is in Degraded operational mode
    When an author attempts to submit a new workload unit targeting "n-002"
    Then the unit submission is rejected with "NodeDegraded: only drain/evacuation permitted"
    But an operator can initiate drain of existing workloads from "n-002"
    And existing workloads on "n-002" continue running until drained

  # --- Rolling upgrade ---

  @operational
  Scenario: Solver version gating during rolling upgrade (FM-12)
    Given a 5-node cluster all running solver version "1.2.0"
    When nodes "n-001" and "n-002" are upgraded to solver version "1.3.0"
    And "n-001" announces solver version "1.3.0" via gossip
    Then the solver pauses all new placement decisions cluster-wide
    And existing workloads continue running unaffected
    And when all 5 nodes report solver version "1.3.0"
    Then the solver resumes placement using version "1.3.0" logic
    And no mixed-version placement decisions were produced
