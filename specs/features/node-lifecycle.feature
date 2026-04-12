Feature: Node lifecycle
  Nodes join, operate, and leave the cluster.

  Scenario: Node joins the cluster
    Given an existing cluster of 5 nodes
    When a new node starts and initiates gossip
    Then existing nodes verify the join (attestation if available)
    And the new node receives graph shards via erasure coding
    And the new node begins participating in the solver

  Scenario: Node leaves gracefully
    Given a node running 3 workloads
    When the operator initiates drain on the node
    Then workloads are re-placed on other nodes
    And graph shards are redistributed
    And the node is removed from membership

  Scenario: Node failure detected by gossip
    Given a node that becomes unresponsive
    When gossip protocol times out after indirect probes
    Then the node is declared failed
    And erasure coding reconstructs its graph shards
    And its workloads are re-placed

  Scenario: Single node cluster operates normally
    Given a single node running taba
    When a workload unit is authored and submitted
    Then the solver places it on the only available node
    And no erasure coding is needed (single shard)
    And the system is fully functional
