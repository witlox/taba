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

## FM-14: Promotion policy collision (different decisions)
**Component**: Policy / Solver
**Trigger**: Two policy authors simultaneously create conflicting promotion
policies for the same workload version (one approves, one denies)
**Expected response**: Both policies enter graph via CRDT merge (merge is
permissive). Solver detects conflicting policies for same conflict tuple at
query time. Fails closed (INV-S2) — workload is not promoted until conflict
is resolved via explicit supersession or governance unit.
**Degradation**: Promotion blocked for the affected workload version until
human resolution. Other workloads unaffected.
**Unacceptable**: Silent winner selection. Implicit resolution of policy
disagreement. Workload placed in prod while a deny policy exists.

## FM-15: Artifact unavailable at placement time
**Component**: Artifact Distribution / Node Operations
**Trigger**: Node is assigned a workload but cannot fetch the artifact (registry
down, peer cache miss, air-gapped without push, digest mismatch)
**Expected response**: Node reports fetch failure to graph as health status.
Solver does not re-place immediately (transient failure). Retry with
exponential backoff. If persistent, node marks workload as failed and solver
re-places to another node.
**Degradation**: Delayed workload start. If artifact is genuinely unavailable
(deleted from registry, never pushed), workload remains unplaced.
**Unacceptable**: Running a workload with unverified artifact (digest
mismatch). Silent failure with no health status update.

## FM-16: Dev node goes offline
**Component**: Node / Solver
**Trigger**: Developer closes laptop, dev box loses power, network disconnect
**Expected response**: Gossip detects node failure per normal SWIM protocol.
For `env:dev` workloads: default is leave-dead (INV-N5) — solver does not
re-place. Workloads on the offline dev node simply stop. Developer restarts
them when the node returns.
**Degradation**: Dev workloads unavailable until node returns. No impact on
test/prod.
**Unacceptable**: Dev node failure triggering prod re-placement. Dev workloads
consuming cluster resources after developer has gone home.

## FM-17: Capability auto-discovery returns stale results
**Component**: Node Operations
**Trigger**: Docker daemon removed but socket file remains. K8s API endpoint
cached but cluster decommissioned. Runtime upgraded but old version detected.
**Expected response**: Workload placement fails at runtime (node cannot
actually execute the artifact despite advertising the capability). Node
reports runtime failure to graph. Solver re-places to another node.
Operator alerted to capability mismatch.
**Degradation**: One failed placement attempt + re-placement delay.
**Unacceptable**: Repeated placement on the same node with stale capabilities.
Solver must mark the capability as suspect after runtime failure.
**Mitigation**: `taba refresh` or fleet-wide refresh to re-probe. Auto-
discovery should verify capabilities (not just detect presence) where
possible.

## FM-18: Role succession gap (all policy authors leave)
**Component**: Governance / Security
**Trigger**: All authors with policy scope in a trust domain have their keys
revoked or leave the organization without transferring scope
**Expected response**: No new policies can be authored. Existing policies
remain valid (signed before revocation). System continues operating with
existing policy set. Break-glass: root key (Shamir ceremony, or Tier 0 solo
key) can re-assign policy roles to new authors.
**Degradation**: New conflicts cannot be resolved until new policy authors
are assigned. Workloads with existing policies continue normally.
**Unacceptable**: System permanently locked out of policy authoring. Root
key unable to recover.

## FM-19: Tier 0 → Tier 1 upgrade failure
**Component**: Governance / Ceremony
**Trigger**: Solo developer attempts to upgrade from self-signed trust domain
to multi-party Shamir ceremony but the migration fails (network issue, key
generation error, ceremony aborted)
**Expected response**: Original Tier 0 trust domain remains fully operational.
Upgrade is non-destructive — the new Tier 1 trust domain is created alongside,
not replacing. Failed ceremony is cleaned up (key material zeroized). Developer
can retry.
**Degradation**: Developer remains on Tier 0 until upgrade succeeds.
**Unacceptable**: Tier 0 trust domain invalidated by a failed upgrade attempt.
Existing units requiring re-signing.
