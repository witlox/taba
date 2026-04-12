# Failure Modes

## FM-01: Single node failure
**Component**: Node
**Trigger**: Hardware failure, OS crash, process kill
**Expected response**: Gossip detects (SWIM suspicion → confirm). Erasure coding
reconstructs graph shards. Solver recomputes placement for orphaned units.
New node reconciles locally.
**Degradation**: Brief under-provisioning during re-placement. Acceptable.
**Unacceptable**: Graph data loss. Silent placement inconsistency.

## FM-02: Multiple simultaneous node failures
**Component**: Node cluster
**Trigger**: Power failure, rack loss, network switch failure
**Expected response**: Same as FM-01 but parallelized. Erasure coding must
survive up to (n-k) failures where k is reconstruction threshold.
**Degradation**: If failures exceed erasure threshold, graph shards are lost.
System enters degraded mode. Operator intervention required.
**Unacceptable**: Silent data loss. Continued operation with corrupted graph.
**Operational mode**: System enters Degraded mode. See domain-model.md.

## FM-03: Network partition
**Component**: Network
**Trigger**: Switch failure, cable cut, firewall misconfiguration
**Expected response**: Both sides maintain graph via CRDT. Both sides run solver
independently. On heal: CRDT merge, duplicate placements resolved by tiebreaker
(lexicographically lowest NodeId wins per INV-C3), loser drains. Role-carrying
units (policy/governance authors) cannot be duplicated — disabled on minority side
(INV-R2). If both sides author policies for the same conflict, supersession chain
determines active policy (INV-C7).
**Degradation**: During partition, stateful workloads may have split-brain
depending on data unit consistency declaration (single-writer blocked, multi-writer
diverges then merges).
**Unacceptable**: Unresolvable state after partition heals. Conflicting policies
without deterministic resolution.

## FM-04: Compromised node
**Component**: Security
**Trigger**: Attacker gains root on a node
**Expected response**: Compromised node cannot inject false units (signature
verification is synchronous gate per INV-S3). Cannot access undeclared
capabilities (zero-default). Cannot poison gossip membership (signed messages
per INV-R3, witness confirmation required). Can lie about local actual state
(Byzantine). Peer health checks detect discrepancy. Eviction via gossip.
**Degradation**: Attacker can disrupt workloads on the compromised node.
**Unacceptable**: Graph corruption. Lateral movement to other nodes.
Gossip membership poisoning.

## FM-05: Compromised author credentials
**Component**: Security
**Trigger**: Key theft, insider threat
**Expected response**: Attacker can create units within the stolen author's scope.
Blast radius limited by scope. Audit trail captures all authored units.
Key revocation removes future authoring ability.
**Degradation**: Malicious units within scope until detection. Units authored
during compromise window (between compromise and revocation) persist if signed
before revocation timestamp (INV-S3). Revocation propagated via priority gossip.
**Unacceptable**: Scope escalation beyond stolen credentials. Units authored
after revocation timestamp accepted.

## FM-06: Solver produces wrong placement
**Component**: Solver
**Trigger**: Bug in solver logic
**Expected response**: Determinism means every node computes the same wrong
answer. No amount of redundancy helps. Detection requires external validation
(property tests, monitoring).
**Degradation**: Workloads placed incorrectly until bug fixed and solver re-run.
**Unacceptable**: Undetected incorrect placement in production.
**Mitigation**: Extensive property-based testing. Formal specification of solver invariants.

## FM-07: WAL corruption or disk full
**Component**: Node (local persistence)
**Trigger**: Disk failure, filesystem corruption, space exhaustion
**Expected response**: Node cannot persist state mutations. Must stop accepting
new placements. Graph shards reconstructable from peers via erasure coding.
Node re-joins after disk issue resolved.
**Degradation**: One fewer node for placement. Self-healing if disk recovers.
**Unacceptable**: Silent data loss. Node continues operating with corrupt WAL.

## FM-08: Graph unbounded growth
**Component**: Composition Graph
**Trigger**: No compaction, historical units accumulate
**Expected response**: Data unit retention declarations trigger compaction.
Completed workloads age out. Archival moves cold subgraphs to external storage.
**Degradation**: If compaction fails, graph exceeds memory (see A8). Performance
degrades, then placement fails.
**Unacceptable**: System grinds to halt with no diagnostic.

## FM-09: Gossip protocol false positive (healthy node declared dead)
**Component**: Gossip
**Trigger**: Temporary network blip, high load causing probe timeouts
**Expected response**: SWIM indirect probes reduce false positives. Node
re-joins if incorrectly evicted. Graph shards and placements recovered.
**Degradation**: Brief unnecessary churn (re-placement, shard reconstruction).
**Unacceptable**: Permanent eviction of healthy node. Cascading false positives.

## FM-10: Conflicting legal requirements on data unit
**Component**: Data/Policy
**Trigger**: "Must retain 7 years" conflicts with "patient withdrew consent"
**Expected response**: Solver detects conflict. Requires explicit policy
resolution. This is a genuinely hard legal question — the system surfaces it,
humans resolve it.
**Degradation**: Data unit locked (neither deleted nor fully accessible) until
policy resolves the conflict.
**Unacceptable**: Automatic resolution of legal conflicts. Silent non-compliance.

## FM-11: Solver determinism regression
**Component**: Solver
**Trigger**: Code change that alters internal computation (e.g., introduces
floating-point, changes iteration order) without visibly changing output on
the developer's platform.
**Expected response**: Property-based tests comparing solver output across
platforms detect the regression. CI/CD runs solver determinism tests on multiple
architectures (x86_64, aarch64 at minimum).
**Degradation**: If undetected, nodes on different architectures diverge in
placement decisions. Duplicate workloads or missed placements.
**Unacceptable**: Undetected determinism violation in production.
**Mitigation**: All solver arithmetic is fixed-point ppm (DL-004). No
floating-point anywhere in solver paths. Cross-platform property tests mandatory.

## FM-12: Solver upgrade version skew
**Component**: Solver / Operations
**Trigger**: Rolling upgrade of node binaries containing solver changes
**Expected response**: Solver upgrade ceremony — all nodes pause placement
decisions until upgrade completes across the cluster. Version-gated: nodes
announce solver version in gossip. Solver will not produce placements until
all active nodes report the same version.
**Degradation**: Placement paused during upgrade window. Existing workloads
continue running.
**Unacceptable**: Mixed-version solver producing divergent placements.

## FM-13: Cascading erasure reconstruction storm
**Component**: Distribution (taba-erasure)
**Trigger**: Multiple node failures in rapid succession causing reconstruction
I/O to overload surviving nodes
**Expected response**: Reconstruction backpressure per INV-R1. Throttled rate.
Priority queue: governance > policy > data constraints > workload shards.
Circuit breaker: if reconstruction queue depth exceeds threshold, pause new
reconstructions and alert operator.
**Degradation**: Temporarily reduced redundancy. If failures exceed erasure
threshold during reconstruction, system enters Degraded mode.
**Unacceptable**: Reconstruction storm causing additional node failures.
