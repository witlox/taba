# Findings: Resilience & Scalability

## FINDING-200: Cascading Erasure Reconstruction Storm
**Severity**: Critical
**Category**: Resilience
**Component**: ADR-005, node-lifecycle.feature
**Scenario**: 20-node cluster (n=20, k=12). Node A fails, reconstruction begins across 8 nodes. Reconstruction I/O pressure causes Node B to fail. Now 2 down. Node C fails from cascading load. If failures exceed (n-k=8), undetectable data loss.
**Impact**: Reconstruction storms overwhelm surviving nodes. Each failure triggers exponential re-encoding. Silent data corruption if threshold exceeded.
**Recommendation**: Reconstruction backpressure: throttle rate, prioritize by shard criticality (governance > policy > workload > data), circuit breaker when queue depth exceeds threshold.
**Traces to**: INV-R1, FM-02, FM-07

---

## FINDING-201: Unbounded Graph Growth + Solver Time
**Severity**: Critical
**Category**: Scalability
**Component**: A8, FM-08, domain-model.md
**Scenario**: Large org after 6 months: 50K workloads, 200K data units, 5K policies, 10K governance units. Each node holds entire graph in memory. Solver evaluates all compositions on every mutation. Graph exceeds memory, solver stalls.
**Impact**: Solver starvation. Placement decisions delayed indefinitely. OOM kills of node daemon.
**Recommendation**: Mandatory graph sharding by trust domain. Hard memory limit (100MB active graph per node). Automated compaction with SLA. Archive cold trust domains.
**Traces to**: A8, FM-08, OQ-007

---

## FINDING-202: CRDT Merge with Pathological Policy Conflicts Post-Partition
**Severity**: High
**Category**: Resilience
**Component**: ADR-002, cross-context/interactions.md
**Scenario**: Partition heals. 200 conflicting policy units from both sides merge (same conflicts, different resolutions). Solver must handle "multiple valid but conflicting resolutions." Behavior undefined.
**Impact**: Solver determinism lost. Placement decisions diverge across nodes.
**Recommendation**: Add INV-C7: "Only one non-revoked policy resolves any given conflict." Automatic conflict detection on merge. Alert operators for resolution.
**Traces to**: INV-C2, INV-C3, OQ-001

---

## FINDING-203: Gossip False Positive Cascade at Scale
**Severity**: High
**Category**: Scalability
**Component**: ADR-004, A4, node-lifecycle.feature
**Scenario**: 5K-node cluster. One node has 100ms latency spike (GC). SWIM probes timeout. Node falsely declared dead. Reconstruction I/O causes congestion. Other nodes also spike. 30+ healthy nodes falsely declared failed.
**Impact**: At 5K+ nodes, transient latency triggers permanent removal of healthy nodes. Recovery requires manual intervention.
**Recommendation**: Adaptive gossip parameters based on observed latency. Jitter in probe timeouts (exponential backoff). Increased indirect probe count before permanent removal. Document A4 as hard scaling limit.
**Traces to**: A4, OQ-008, FM-09

---

## FINDING-204: Network Partition + Erasure Coding Double Fault
**Severity**: High
**Category**: Resilience
**Component**: network-partition.feature, ADR-005
**Scenario**: 30-node cluster (n=30, k=20). Partition: 16 vs 14 nodes. Side B has only 14 — cannot reconstruct (needs k=20). Shard distribution spans both sides. Neither side has superset. Partition lasts 4 hours, both sides author new units. On heal: some shards are truly lost.
**Impact**: Undetectable data loss + corruption during merge. Security policies may be missing. Silent failure.
**Recommendation**: Shard health check tracking which shards are in which partition. Alert if reconstruction impossible. Pre-merge integrity verification (Merkle tree hash). Manual reconciliation if mismatch.
**Traces to**: INV-R1, INV-R2, FM-03

---

## FINDING-205: Solver Floating-Point Divergence on Heterogeneous Hardware
**Severity**: High
**Category**: Resilience
**Component**: A2, OQ-004, domain-model.md
**Scenario**: x86 node and ARM node compute same placement score with different floating-point results. Tiebreaker disagrees. Different placements computed for same graph and membership.
**Impact**: INV-C3 violated. Silent inconsistency on mixed-architecture clusters.
**Recommendation**: Integer-only arithmetic. Fixed-point (basis points or ppm). Cross-platform determinism property tests.
**Traces to**: INV-C3, A2, OQ-004

---

## FINDING-206: WAL Corruption + Simultaneous Node Failure
**Severity**: High
**Category**: Resilience
**Component**: FM-07, cross-context/interactions.md
**Scenario**: Node N's WAL corrupted silently (fsync ignored). Graph mutation applied locally but not persisted. Node M fails. N selected as shard contributor for reconstruction. N encodes stale state. Stale state propagated. N later restarts and recovers from (now stale) erasure shards.
**Impact**: Silent graph data loss. State appears consistent but is outdated.
**Recommendation**: WAL verification with periodic checksums. Pre-reconstruction verification that contributor state matches claims. Write verification (fsync confirmation). Divergence detector: if reconstructed shard differs from WAL, alert and enter read-only mode.
**Traces to**: INV-R1, FM-07, INV-C4

---

## FINDING-207: Solver Starvation from Pathological Compositions
**Severity**: High
**Category**: Scalability
**Component**: domain-model.md, placement.feature, ADR-001
**Scenario**: Workload with 100 capability declarations, each satisfiable by 5-10 providers, with placement constraints (no co-location). Solver explores exponential search space. Timeout fires, placement incomplete. Queue builds up.
**Impact**: System unresponsive to all new workloads. Single pathological composition blocks entire cluster.
**Recommendation**: Solver timeout with greedy fallback. Composition complexity budget with author warnings. Solver queue with priority scheduling (governance > policy > workload > data).
**Traces to**: INV-K1

---

## FINDING-208: Bootstrap Cold-Start — Seed Node All-Fail
**Severity**: High
**Category**: Resilience
**Component**: ADR-004, node-lifecycle.feature
**Scenario**: 3 seed nodes hold initial graph (governance, role assignments). Cluster grows to 20. Seed nodes decommissioned. All 3 fail. Remaining nodes have erasure shards but governance updates only existed on seed nodes. Latest governance lost. System reverts to weeks-old state.
**Impact**: Authority loss. Operators cannot author new units. Cluster stuck with old governance.
**Recommendation**: Seed node shards are special: never allow all to be simultaneously offline. Persistent snapshot at bootstrap stored externally. Graph snapshot verifier periodically checkpoints to external storage.
**Traces to**: INV-R1, ADR-004, ADR-005

---

## FINDING-209: Byzantine Node Poisons Gossip Membership
**Severity**: Medium
**Category**: Resilience
**Component**: FM-04, ADR-004, node-lifecycle.feature
**Scenario**: Compromised node N sends false gossip claiming healthy nodes A, B, C failed. Forges indirect probe responses. Some nodes believe the claim, others don't. Membership diverges. Different solver inputs on different nodes.
**Impact**: Corrupted membership view. Unnecessary reconstruction cascades.
**Recommendation**: Signed gossip messages. Membership decision log. Multi-node consensus for "node failed" decisions. Detect Byzantine behavior by comparing membership views.
**Traces to**: FM-04, ADR-004, INV-R3

---

## FINDING-210: Policy Scope Overlap Bug Allows Role Escalation
**Severity**: High
**Category**: Resilience
**Component**: A1, domain-model.md, ADR-006
**Scenario**: Bug in role assignment validation allows Author A scope (policy, [X, Y]) when only (policy, X) was intended. A creates malicious policy in domain Y. Solver applies it deterministically. Author B (sole authority in Y) loses control.
**Impact**: Role escalation. A1 (load-bearing) violated. CRDT has no mechanism to detect scope overlap.
**Recommendation**: Add INV-A1: "At assignment time, verify no scope overlap with existing authors in same trust domain." Pre-merge verification cross-checks policy units against author scope.
**Traces to**: A1, INV-S5, ADR-006

---

## FINDING-211: Erasure Re-Coding Storm During Rolling Updates
**Severity**: High
**Category**: Scalability
**Component**: ADR-005, node-lifecycle.feature
**Scenario**: 100-node cluster (n=100, k=70). Rolling update: each node removal triggers 30 re-coding operations. Node return triggers 30 more. 100 nodes = 6,000 re-coding events. Solver starved throughout.
**Impact**: Rolling updates take hours. Cluster unavailable during update window.
**Recommendation**: Shard rebalancing prioritization during updates. Pre-compute new shard assignments. Consider temporary full replication during rolling updates, then return to erasure coding.
**Traces to**: ADR-005, FM-02, node-lifecycle.feature

---

## FINDING-212: Partition Creates Divergent Policy Resolutions
**Severity**: Medium
**Category**: Resilience
**Component**: network-partition.feature, composition.feature
**Scenario**: Side A authors "choose postgres." Side B authors "choose mysql." Same conflict. Both applied during partition. On heal, both policies in graph. Solver sees two non-revoked policies for same conflict.
**Impact**: Solver behavior undefined. Placement potentially non-deterministic.
**Recommendation**: Deterministic rule for conflicting policies: lowest policy unit ID wins, or earliest timestamp wins. Alert operators for manual resolution rather than silent application.
**Traces to**: INV-C3, network-partition.feature

---

## FINDING-213: Memory Exhaustion During Partition Heal Merge
**Severity**: Medium
**Category**: Scalability
**Component**: A8, network-partition.feature
**Scenario**: Graph at 500MB per node. 2-hour partition, each side adds 100MB. On heal, merge computes union: 700MB. Nodes with 512MB free OOM during merge.
**Impact**: Partition recovery becomes catastrophic failure. System cannot heal.
**Recommendation**: Streaming merge (don't allocate full union). Pre-merge memory budget check. Trigger compaction before merge if needed.
**Traces to**: A8, network-partition.feature

---

## FINDING-214: Governance Unit Authority Loss During Cascading Failure
**Severity**: High
**Category**: Resilience
**Component**: domain-model.md, ADR-006, FM-02
**Scenario**: 3 designated nodes hold complete governance copies. All 3 fail (data center outage). Governance updates not yet propagated to worker nodes. System reverts to old governance. Operators locked out.
**Impact**: Authority loss. Trust domain expansion blocked.
**Recommendation**: Governance units actively replicated (full copies on N nodes), not just erasure coded. 3-way ack gossip heartbeat for critical governance updates.
**Traces to**: ADR-004, domain-model.md, FM-02

---

## FINDING-215: Multi-Writer Data Unit Silent Data Loss During Partition
**Severity**: Medium
**Category**: Resilience
**Component**: network-partition.feature, domain-model.md, FM-03
**Scenario**: Multi-writer data unit. Both partition sides write. On heal, CRDT merge includes both histories. LWW tiebreaker (lowest node ID) silently drops loser's writes. No notification to losing workload.
**Impact**: Silent data loss without application awareness.
**Recommendation**: Multi-writer declarations must include conflict resolution strategy (CRDTs, semantic merge, error-on-divergence). Alert on divergence during partition heal. Read-only mode until manual resolution.
**Traces to**: INV-D1, network-partition.feature, FM-03

---

## FINDING-216: Composition Deadlock in Cyclic Dependencies
**Severity**: Medium
**Category**: Scalability
**Component**: composition.feature, domain-model.md
**Scenario**: A needs B before starting, B needs A before starting. Solver deadlocks. No cycle detection.
**Impact**: Workloads never start. No diagnostic.
**Recommendation**: Cycle detection in solver. Cyclic dependencies treated as composition conflicts requiring policy resolution. Reject cycles proactively during unit insertion.
**Traces to**: composition.feature, INV-K1

---

## FINDING-217: Data Retention vs Consent Withdrawal — Zombie State
**Severity**: Medium
**Category**: Resilience
**Component**: FM-10, domain-model.md
**Scenario**: Legal: "retain 7 years." Consent: "delete immediately." Solver detects conflict, fails closed. Data locked: unreadable, undeletable, unmovable.
**Impact**: Data unit permanently locked. System unusable for any workload touching it.
**Recommendation**: Policy conflict escalation: auto-escalate to human resolver with timeout. During escalation, allow read-only. If timeout expires, fail open (delete) with full audit trail.
**Traces to**: FM-10

---

## FINDING-218: Key Compromise Window — Malicious Policy Persists
**Severity**: High
**Category**: Resilience
**Component**: cross-context/interactions.md, INV-S3
**Scenario**: Author A's key compromised at T. Attacker authors malicious policy at T+5s. Revocation at T+30s. Malicious policy already merged. Spec says "existing units remain (valid when signed)." Malicious policy is now canonical.
**Impact**: Any policy authored during compromise window is permanent.
**Recommendation**: Revocation verification with timestamps: reject units signed after author's key was revoked. Priority gossip for revocation propagation. "Key compromise detection" system.
**Traces to**: cross-context/interactions.md, INV-S3

---

## FINDING-219: Taint Propagation Doesn't Track Historical Input Classification
**Severity**: Medium
**Category**: Resilience
**Component**: domain-model.md, INV-S4
**Scenario**: Data unit D_v1 is PII. Workload W processes D_v1, produces O_v1 (PII). D_v2 supersedes D_v1 (declassified). W processes D_v2, produces O_v2 (not PII). But O_v2 was produced by same workload that handles PII — may contain inferred PII.
**Impact**: Information leakage. Taint only tracks current classification, not historical derivations.
**Recommendation**: Extend taint to track "derived-from" across superseded units. Workloads that processed tainted data inherit taint on future outputs unless explicit policy declassifies.
**Traces to**: INV-S4

---

## FINDING-220: Solver Thundering Herd After Graph Broadcast
**Severity**: Medium
**Category**: Scalability
**Component**: cross-context/interactions.md, placement.feature
**Scenario**: Critical governance unit updated. 50 nodes receive via gossip over ~1 second. All 50 trigger solver re-evaluation. CPU/memory spike. Slow nodes timeout, creating staggered retry herd.
**Impact**: Temporary system overload after any significant graph broadcast.
**Recommendation**: Solver coalescing: mutations within 100ms window merged into single evaluation. Deterministic scheduling (fixed delay after broadcast). Backpressure: reject mutations if solver queue depth exceeds threshold.
**Traces to**: cross-context/interactions.md, placement.feature
