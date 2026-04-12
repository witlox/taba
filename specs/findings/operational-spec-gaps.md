# Findings: Operational & Specification Gaps

## FINDING-300: No Upgrade/Rollback Procedures for Solver Version Skew
**Severity**: Critical
**Category**: Operational
**Component**: failure-modes.md, INV-C3
**Scenario**: Solver bug found. Operator deploys fixed binary via rolling update. During rollout, nodes running old vs new solver produce incompatible placements. INV-C3 violated until all nodes upgraded.
**Impact**: Cluster-wide split-brain placement during upgrades. No recovery path specified.
**Recommendation**: Define solver upgrade ceremony (pause placement until all nodes upgraded). Version-gated solver updates. Rollback path with state for downgrade.
**Traces to**: INV-C3, FM-06

---

## FINDING-301: "Degraded Mode" Mentioned But Never Defined
**Severity**: Critical
**Category**: Spec-Gap
**Component**: failure-modes.md, domain-model.md
**Scenario**: FM-02 says "system enters degraded mode, operator intervention required." What exactly is degraded mode? Can new workloads be placed? Can existing ones drain? Can units be authored? Can graph be queried?
**Impact**: Operators cannot execute recovery because degraded mode operations are undefined.
**Recommendation**: Create specs/operational-modes.md defining: Normal (all operations), Degraded (authoring/composition/placement frozen, drain only), Recovery (gradual re-coding, placement throttled). Feature scenarios for each transition.
**Traces to**: FM-02, FM-08

---

## FINDING-302: Graph Compaction Triggers and Thresholds Unspecified
**Severity**: High
**Category**: Spec-Gap
**Component**: FM-08, A8, INV-D2
**Scenario**: Data units declare retention="30 days." 30 days pass. Is compaction automatic? Manual? What happens if operator forgets? What happens at A8 memory limit?
**Impact**: Graph may consume unbounded memory. Expired data units accumulate. No alerting.
**Recommendation**: Automatic compaction triggers (>80% memory). Manual API with progress. Per-unit retention enforcement. Metrics: graph size, compaction latency, units pending archival.
**Traces to**: FM-08, A8, INV-D2

---

## FINDING-303: No Backpressure in Gossip/Placement — Cascading Amplification
**Severity**: High
**Category**: Operational
**Component**: node-lifecycle.feature, recovery.feature, FM-02
**Scenario**: Node fails. Solver re-places workloads on survivors. Survivors oversubscribed, become slow. Gossip probes timeout, false positives. More "failures." Cascade continues.
**Impact**: No circuit breaker. Recovery.feature has shallow "pending state" scenario but no thresholds, throttling, or alert conditions.
**Recommendation**: Circuit breaker thresholds (% placement failures, queue depth). Adaptive backoff (exponential jitter). Prefer blocking new workloads over evicting existing.
**Traces to**: FM-02, FM-09, recovery.feature

---

## FINDING-304: Circular Policy Dependencies Not Handled
**Severity**: High
**Category**: Spec-Gap
**Component**: conflict-resolution.feature
**Scenario**: P1 resolves conflict X by requiring C. C and A conflict. P2 resolves by requiring D. D and B conflict. Circular meta-policy chain. No cycle detection in solver.
**Impact**: Solver hangs or creates unbounded graph growth. Workloads stuck in pending.
**Recommendation**: Cycle detection algorithm (topological sort on policy dependency graph). Max composition depth limit. Feature: "Circular policy dependency detection and rejection."
**Traces to**: conflict-resolution.feature, INV-K1

---

## FINDING-305: Trust Domain Bootstrap Ceremony Not Specified
**Severity**: High
**Category**: Operational
**Component**: trust-domain.feature, ADR-006
**Scenario**: Fresh cluster. No trust domains exist. Creating the first requires multi-party policy. But the policy author needs a trust domain scope. Chicken-and-egg.
**Impact**: System cannot bootstrap. No runbook for initial operator ceremony.
**Recommendation**: Create specs/operator-ceremonies.md: bootstrap ceremony steps, root key reconstruction, initial trust domain creation, fail-safe for partial ceremony failure.
**Traces to**: trust-domain.feature, ADR-006, INV-S6

---

## FINDING-306: WAL Recovery After Mid-Recoding Crash Not Specified
**Severity**: High
**Category**: Operational
**Component**: node-lifecycle.feature, FM-07
**Scenario**: Node crashes mid-erasure-recoding. WAL contains in-progress recoding operation. On restart, should node complete, rollback, or re-pull? Incomplete recoding may violate (n-k) threshold.
**Impact**: Node recovery enters unknown state. Silent threshold violation.
**Recommendation**: Transaction markers for erasure operations. Recovery logic: complete, resume, or rollback. Post-WAL-replay validation checks.
**Traces to**: FM-07, node-lifecycle.feature

---

## FINDING-307: Data Unit Hierarchy — No Maximum Depth
**Severity**: Medium
**Category**: Spec-Gap
**Component**: domain-model.md, data-lineage.feature
**Scenario**: Operator accidentally creates 1000-level hierarchy. Graph memory consumption explodes. Lineage queries become O(depth).
**Impact**: A8 violation. Unbounded complexity.
**Recommendation**: Maximum hierarchy depth (10-16 levels). Reject deeper declarations.
**Traces to**: domain-model.md, A8, INV-D3

---

## FINDING-308: Expired Roles — Unit Lifecycle Unclear
**Severity**: Medium
**Category**: Spec-Gap
**Component**: trust-domain.feature, INV-S3
**Scenario**: Author's role expires. Their signed workload is still in graph. Can it be re-placed? Modified? Deleted? Can it keep running?
**Impact**: INV-S3 says "signed by author with valid scope" but post-expiry validity undefined.
**Recommendation**: Expired role still validates historically signed units. New modifications rejected. Cleanup via policy for archived units.
**Traces to**: INV-S3, INV-S5

---

## FINDING-309: Forged Unit Detection — Propagation Before Rejection
**Severity**: High
**Category**: Spec-Gap
**Component**: security-enforcement.feature, cross-context/interactions.md
**Scenario**: Attacker injects fake signed unit into WAL. Node broadcasts via gossip. How many nodes merge before signature check rejects? Latency to propagate rejection?
**Impact**: Forged units may propagate before rejection.
**Recommendation**: Signature verification synchronous (blocks merge). Nodes that accepted forged units quarantined. Feature: "Byzantine node detection via signature validation."
**Traces to**: INV-S3, FM-04

---

## FINDING-310: Taint Propagation — Multi-Input Cases Under-Specified
**Severity**: Medium
**Category**: Spec-Gap
**Component**: data-lineage.feature, INV-S4
**Scenario**: Workload consumes PII data X and public data Y. Produces Z. Is Z's taint the union? Can Y's "publicness" cancel X's PII? What about aggregation/anonymization?
**Impact**: Multi-input taint logic undefined. Single-input cases covered but not multi-input.
**Recommendation**: Union model by default (most restrictive). Explicit taint narrowing via policy. Aggregation-based declassification requires analyst review.
**Traces to**: INV-S4, data-lineage.feature

---

## FINDING-311: No Monitoring/Observability Specification
**Severity**: High
**Category**: Operational
**Component**: All specs
**Scenario**: Operator deploys cluster. No metrics defined. Cannot detect split-brain, silent inconsistencies, or graph growth approaching limits.
**Impact**: FM-06 says detection "requires external validation" but no monitoring spec exists.
**Recommendation**: Create specs/operational-metrics.md: key metrics (graph size, placement latency, conflict rate, gossip latency, erasure progress, WAL size), SLO targets, alert conditions.
**Traces to**: FM-06, FM-08, FM-09

---

## FINDING-312: Retention Policy Conflict Escalation Incomplete
**Severity**: Medium
**Category**: Spec-Gap
**Component**: FM-10, data-lineage.feature
**Scenario**: "Retain 7 years" vs "delete after 30 days" on parent/child. FM-10 says "locked" but for how long? Can lock be overridden? What about subsequent regulatory changes?
**Impact**: Locked units block compaction and system operation indefinitely.
**Recommendation**: Define lock semantics (advisory vs hard). Retention conflict resolution process. Archive behavior for locked units.
**Traces to**: FM-10, INV-D2

---

## FINDING-313: Key Revocation Mid-Transaction — Partial Authoring
**Severity**: Medium
**Category**: Spec-Gap
**Component**: security-enforcement.feature, FM-05
**Scenario**: Author composing 5 units. Unit 1 signed and merged. Key revoked. Units 2-5 cannot be signed. Partial composition with orphaned units.
**Impact**: Boundary between "completed" and "in-progress" authoring undefined.
**Recommendation**: Atomic authoring: all-or-nothing for compositions. Key revocation blocks pending signatures immediately.
**Traces to**: FM-05, INV-S3

---

## FINDING-314: Latency Tolerance — No Enforcement Mechanism
**Severity**: Medium
**Category**: Spec-Gap
**Component**: placement.feature, domain-model.md
**Scenario**: Workload declares "max 10ms latency." How measured? When? What happens if latency exceeds tolerance after placement?
**Impact**: Tolerance declarations are specified but enforcement mechanism is undefined.
**Recommendation**: Define measurement method (p99 RTT). Measurement frequency. Violation handling (warning → drain → evict).
**Traces to**: placement.feature, INV-K3

---

## FINDING-315: Role Inheritance Across Trust Domains Not Specified
**Severity**: Medium
**Category**: Spec-Gap
**Component**: trust-domain.feature, ADR-006
**Scenario**: Operator creates trust domain TD2 from TD1. Does operator automatically get governance scope in TD2?
**Impact**: Role scope semantics for domain creation ambiguous.
**Recommendation**: Explicit policy required for cross-domain roles. No implicit inheritance.
**Traces to**: trust-domain.feature, ADR-006, INV-S5

---

## FINDING-316: Wire Format Version Compatibility Not Specified
**Severity**: Medium
**Category**: Operational
**Component**: cross-context/interactions.md, guidelines/BUILD_ORDER.md
**Scenario**: Old node (v1.0) serializes graph with "tolerance_ms." New node (v1.1) expects "tolerance_us." Gossip delivers incompatible shard.
**Impact**: Silent data misinterpretation or partition between versions.
**Recommendation**: Schema versioning strategy (protobuf forward/backward compatibility rules). Version check on merge.
**Traces to**: cross-context/interactions.md

---

## FINDING-317: Property Tests Don't Specify Cross-Platform Determinism
**Severity**: High
**Category**: Spec-Gap
**Component**: guidelines/TESTING_STRATEGY.md, A2, OQ-004
**Scenario**: Testing strategy says "determinism: same input = same output" but doesn't specify "on any platform." If solver uses floating-point internally, x86 and ARM may produce same results by coincidence.
**Impact**: Determinism regression invisible until real divergence appears.
**Recommendation**: Resolve OQ-004 before Phase 2. Property tests must verify across architectures. Integer-only or deterministic float.
**Traces to**: A2, OQ-004, INV-C3

---

## FINDING-318: Partition + Conflicting Policies — No BDD Coverage
**Severity**: High
**Category**: Spec-Gap
**Component**: network-partition.feature, conflict-resolution.feature
**Scenario**: Partition separates cluster. Both sides author conflicting policies for same conflict. On heal, graph merge includes both. Solver behavior undefined. network-partition.feature covers "new units" but not "conflicting policies."
**Impact**: Critical interaction between partition and policy not validated by BDD.
**Recommendation**: Feature: "Partition with conflicting policies authored on each side." Specify merge behavior and solver resolution.
**Traces to**: network-partition.feature, conflict-resolution.feature

---

## FINDING-319: Orphaned Workloads — No Escalation SLO
**Severity**: Medium
**Category**: Operational
**Component**: recovery.feature, node-lifecycle.feature
**Scenario**: Node fails. 8 of 10 workloads re-placed. 2 cannot be placed. Stuck in "pending" forever.
**Impact**: No timeout, alert, or manual intervention path for irremediable pending state.
**Recommendation**: Pending state SLO (max N minutes). Feature: "Orphaned workload escalation." CLI commands: list pending, force-drain, force-place.
**Traces to**: recovery.feature

---

## BDD Feature Gaps

### FINDING-331: No Scenario for Unit Deletion or Archival
**Severity**: Medium
**Category**: Spec-Gap
**Scenario**: Domain model references archival but no feature validates: how units are marked, operator vs automatic, recovery from archive, audit trail.
**Recommendation**: Create specs/features/data-retention.feature.

### FINDING-332: No Scenario for Operator Error Recovery
**Severity**: High
**Category**: Spec-Gap
**Scenario**: No feature for "operator over-grants scope," "misconfigures policy," or "how to detect and correct mistakes."
**Recommendation**: Create specs/features/operator-error-recovery.feature.

### FINDING-333: No Scenario for Compliance Audit
**Severity**: Medium
**Category**: Spec-Gap
**Scenario**: System designed with audit trails but no BDD validates operator ability to query lineage, generate compliance reports, or prove authorization.
**Recommendation**: Create specs/features/compliance-audit.feature.

### FINDING-336: Missing Failure Mode — Solver Determinism Regression
**Severity**: High
**Category**: Spec-Gap
**Scenario**: FM-06 covers solver bug but not "code change that violates determinism without changing visible output." Property tests should catch this but specs don't require cross-platform verification.
**Recommendation**: Add FM-11: "Solver determinism regression." Continuous multi-platform property testing in CI/CD.
**Traces to**: FM-06, INV-C3, A2
