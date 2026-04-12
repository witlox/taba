# Findings: Correctness & Consistency

## FINDING-001: Author Scope Isolation (A1) Has No Enforcement Mechanism
**Severity**: Critical
**Category**: Correctness
**Component**: specs/assumptions.md, specs/invariants.md, ADR-006-role-model.md
**Scenario**: Two authors both have scope (unit_type=workload, trust_domain=production). Both create units affecting the same composition space. CRDT merges both — but the merge function cannot resolve semantic conflicts between units from overlapping scopes. The system assumes A1 (non-overlapping scopes) but provides no mechanism to enforce it during role assignment.
**Impact**: CRDT works without consensus ONLY if scopes are non-overlapping. Without enforcement, concurrent mutations from overlapping authors produce semantic conflicts the CRDT cannot resolve. This is the fundamental load-bearing assumption.
**Recommendation**: Add INV-S8: "No two distinct authors can have identical (unit_type_scope, trust_domain_scope) tuples. Role assignment governance units must validate this before persisting."
**Traces to**: A1 (load-bearing), INV-S5

---

## FINDING-002: Policy Unit Orphaning Race Condition
**Severity**: High
**Category**: Consistency
**Component**: specs/invariants.md, specs/cross-context/interactions.md
**Scenario**: Policy P resolves conflict between units A and B. Node1 receives superseded A', Node2 receives deletion of B. After CRDT merge, P references non-existent B — violating INV-C5 (orphaned policies invalid). No mechanism to detect or prevent this during merge.
**Impact**: Dangling policy references. Graph queries return invalid policy state.
**Recommendation**: Policy validity checked at query time (not merge time). Compaction rule: policies referencing non-existent units are eligible for archival. Add policy supersession operation with lineage tracking.
**Traces to**: INV-C5, INV-D1

---

## FINDING-003: Circular Recovery Dependencies Unresolvable
**Severity**: High
**Category**: Correctness
**Component**: docs/vision/SYSTEM_VISION.md, specs/domain-model.md, FM-06
**Scenario**: WorkloadA declares "on_crash: require B running". WorkloadB declares "on_crash: require A running". Both fail. Solver cannot break the cycle. System vision acknowledges this ("recovery plans compose less cleanly") but provides no resolution strategy.
**Impact**: Recovery deadlock. No deterministic choice specified for cycle-breaking.
**Recommendation**: Add INV-K5: "Cyclic recovery dependencies fail closed — require explicit policy declaring restart priority." Tiebreaker: lexicographically lowest UnitId gets priority.
**Traces to**: INV-C3, INV-K1, FM-06

---

## FINDING-004: Taint Propagation Inconsistency During CRDT Merge
**Severity**: High
**Category**: Correctness
**Component**: specs/invariants.md (INV-S4, INV-D1), ADR-003
**Scenario**: DataUnit_Output inherits PII from DataUnit_Input. Declassification policy P1 exists but arrives at Node2 after Output. During the window before P1 arrives, Node2 computes Output as PII while Node1 (with P1) computes it as public. Security decision is temporarily inconsistent.
**Impact**: Different nodes make different security decisions about the same data during eventual consistency window.
**Recommendation**: Taint computed at query time (graph traversal), not merge time. Document: "Taint classification is eventually consistent." Add security audit step: recompute taint before granting access.
**Traces to**: INV-S4, INV-C2

---

## FINDING-005: WAL-Before-Effect Conflicts with CRDT Causal Ordering
**Severity**: High
**Category**: Consistency
**Component**: specs/invariants.md (INV-C4), cross-context/interactions.md
**Scenario**: Node receives unit U2 (references U1) before U1 arrives via gossip. WAL requires persisting before effect, but U2 has broken references. If WAL'd before U1 arrives, reference integrity is violated. If blocked, INV-C4 is violated for U2.
**Impact**: INV-C4 assumes total order but CRDT delivers partial (causal) order. WAL format doesn't handle out-of-order arrivals.
**Recommendation**: Redefine INV-C4: "Each unit mutation is WAL'd atomically before its effects become visible to queries. Mutations form a partial order." Add pending queue for units with unsatisfied references.
**Traces to**: INV-C4, INV-D1

---

## FINDING-006: Author Scope Escalation via Policy Embedding
**Severity**: High
**Category**: Correctness
**Component**: ADR-006-role-model.md, specs/invariants.md (INV-S5)
**Scenario**: Developer D (scope: workload) creates WorkloadW needing root_capability. D cannot create a Policy unit (outside scope). But spec is silent on whether inline policy declarations within a workload unit bypass scope checking.
**Impact**: Potential scope escalation if workload units can embed policy-like declarations.
**Recommendation**: Units are atomic and single-authored. Policy overrides must be separate Policy units signed by policy-scoped authors. Add INV-S8: "A unit of type T can only be authored by an author with unit_type_scope containing T."
**Traces to**: INV-S5, ADR-006

---

## FINDING-007: Conflicting Policies for Same Conflict — No Resolution Order
**Severity**: High
**Category**: Correctness
**Component**: ADR-003, specs/invariants.md (INV-S2), ADR-002
**Scenario**: Two SecurityTeam members create conflicting Policy units for the same capability conflict (P1 allows, P2 denies). CRDT merges both. Solver sees two policies for the same conflict — which one wins? "Fail closed" (INV-S2) doesn't help when both are valid.
**Impact**: Solver behavior undefined when multiple valid policies conflict. Determinism lost.
**Recommendation**: Policy uniqueness: only one non-revoked policy per conflict tuple (unit IDs + capability name). Second policy for same conflict requires supersession (versioned lineage). Solver uses latest version.
**Traces to**: A1, INV-S2, ADR-003

---

## FINDING-008: Data Unit Hierarchy Constraint Direction Ambiguous
**Severity**: Medium
**Category**: Correctness
**Component**: specs/domain-model.md, specs/invariants.md (INV-D3)
**Scenario**: Parent is "PII" (high restriction). Child wants to be "public" (low restriction). Spec says "children can narrow freely." Is removing a restriction "narrowing" or "widening"? Direction is undefined.
**Impact**: Enforcement of INV-D3 depends on interpretation. Wrong interpretation = PII data silently declassified without policy.
**Recommendation**: Clarify: "Narrowing = adding restrictions (free). Widening = removing restrictions (requires policy)." Define classification lattice explicitly (public < internal < confidential < PII).
**Traces to**: INV-D3

---

## FINDING-009: Solver Tiebreaker Logic Unspecified
**Severity**: Medium
**Category**: Correctness
**Component**: specs/invariants.md (INV-C3), FM-03
**Scenario**: After partition heal, duplicate placements exist. FM-03 says "lowest node ID wins, other side drains." But: string sort vs numeric sort? What does "draining" mean for stateful workloads? Drain timeout?
**Impact**: Different nodes could interpret tiebreaker differently, violating determinism.
**Recommendation**: Add to INV-C3: lexicographically lowest node_id wins. Define drain procedure (declared on_shutdown, then remove). Drain timeout: solver_cycle_interval x 3.
**Traces to**: INV-C3, FM-03

---

## FINDING-010: Erasure Coding Parameters Undefined
**Severity**: Medium
**Category**: Correctness
**Component**: ADR-005, FM-02, A3
**Scenario**: How are (n, k) computed? Who decides? What happens if failures exceed threshold during reconstruction? Is reconstruction synchronous or async?
**Impact**: Core resilience mechanism has no parameter specification.
**Recommendation**: Specify formula: k_min = ceil(N x (1 - R/100)). Add INV-R4: graph reconstructable if failures <= floor(N - k_min). Reconstruction is async, doesn't block solver.
**Traces to**: ADR-005, A3, INV-R1

---

## FINDING-011: Gossip False Positives + Partition Recovery Interaction
**Severity**: Medium
**Category**: Consistency
**Component**: cross-context/interactions.md, FM-03, FM-09, ADR-004
**Scenario**: SWIM suspects alive node N as dead. N declares rest of cluster dead. Both sides evict and re-place. Partition heal causes churn cascade.
**Impact**: Repeated churn. Spec doesn't define suspected-vs-confirmed state transition or how suspected nodes affect placement pool.
**Recommendation**: Add INV-R5: suspected nodes remain in placement pool with health='unknown'. Solver avoids suspected nodes when alternatives exist. Require multi-probe consensus before confirmation.
**Traces to**: INV-R2, FM-03, FM-09

---

## FINDING-012: Composition Graph Unbounded Growth — No Hard Guardrails
**Severity**: Medium
**Category**: Consistency
**Component**: FM-08, A8, INV-C1
**Scenario**: Production cluster accumulates 100K+ units over years. Graph exceeds node memory. No automatic compaction policy specified. No definition of what happens when memory exceeded.
**Impact**: A8 violation. System crashes with no graceful degradation.
**Recommendation**: Hard guardrail: active graph <= 50% available memory. Auto-compaction at 40%. Add INV-R6: node with insufficient memory enters degraded mode (refuses placements until compacted).
**Traces to**: A8, FM-08, INV-C1

---

## FINDING-013: Key Revocation — Unit Validity Temporal Ambiguity
**Severity**: Medium
**Category**: Correctness
**Component**: specs/invariants.md (INV-S3), FM-05
**Scenario**: Author A's key revoked at T2. Unit U signed at T1 (before revocation) exists in graph. Is U still valid? INV-S3 says "signed by author with valid scope" — but "valid" when?
**Impact**: Unclear whether revocation cascades to existing units or only blocks new ones.
**Recommendation**: Units signed before revocation remain valid (signature was valid at creation). Add INV-S9: "A unit is valid iff signature is cryptographically valid AND author's key was not revoked before unit's creation timestamp."
**Traces to**: INV-S3, FM-05

---

## FINDING-014: Solver Determinism — Floating Point Unresolved
**Severity**: Medium
**Category**: Correctness
**Component**: A2, OQ-004
**Scenario**: Solver scores placements using ratios. Different CPU architectures produce different floating-point results. Tiebreaker disagrees across platforms.
**Impact**: INV-C3 violated on heterogeneous hardware. OQ-004 is open and unresolved.
**Recommendation**: Decide before architect phase: integer-only or fixed-point arithmetic. No floating-point in solver scoring. All calculations in basis points or parts-per-million.
**Traces to**: A2, INV-C3, OQ-004

---

## FINDING-015: Trust Domain Creation Quorum Undefined
**Severity**: Medium
**Category**: Correctness
**Component**: specs/domain-model.md, ADR-006, INV-S6
**Scenario**: INV-S6 requires "multi-party policy resolution" for trust domain creation. But who are the parties? What's the quorum? How does the Policy unit enforce multi-party signing?
**Impact**: Trust domain creation ceremony is undefined. Could be bypassed by a single policy author with broad scope.
**Recommendation**: Specify ceremony: TrustDomainDefinition + RoleAssignment units + Policy unit with multi-sig requirement. Add INV-S10: threshold of 2+ distinct author signatures required.
**Traces to**: INV-S6, ADR-006

---

## FINDING-016: Capability Matching — Set Ordering Could Break Determinism
**Severity**: Low
**Category**: Correctness
**Component**: INV-C3, INV-K2
**Scenario**: Two units declare same capabilities in different order. If solver iterates in declaration order, matching depends on order — breaking determinism.
**Impact**: Implementation detail could violate INV-C3.
**Recommendation**: INV-K2 update: capabilities sorted lexicographically by (type, name) before matching.
**Traces to**: INV-C3, INV-K2

---

## FINDING-017: CRDT Merge Idempotency for Signed Units Not Proven
**Severity**: Low
**Category**: Correctness
**Component**: INV-C2, ADR-002
**Scenario**: If signature metadata includes relay/nonce data, merging the same unit twice could produce different results (duplicate metadata entries). merge(A, A) != A violates idempotency.
**Impact**: CRDT property violated if unit identity isn't properly defined.
**Recommendation**: Specify unit identity key: (UnitId, Author, CreationTimestamp). Signatures are immutable. Graph is a set (no duplicates). Add INV-C6: duplicate tuples never occur.
**Traces to**: INV-C2

---

## FINDING-018: Provenance Chain Completeness Not Enforced During Merge
**Severity**: Low
**Category**: Correctness
**Component**: INV-D1
**Scenario**: DataUnit_Output arrives before its referenced WorkloadA and DataUnit_Input. Provenance references are broken until referenced units arrive.
**Impact**: INV-D1 ("unbroken provenance") is violated during eventual consistency windows.
**Recommendation**: Provenance verified at query time, not merge time. References marked 'pending' until resolved. Add INV-D5: provenance traversal must reach all sources given reasonable timeout for eventual arrival.
**Traces to**: INV-D1, INV-C2
