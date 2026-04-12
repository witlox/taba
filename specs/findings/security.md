# Findings: Security

## FINDING-100: Signature Verification Race Window Before Merge
**Severity**: High
**Category**: Security
**Component**: specs/cross-context/interactions.md, INV-S3
**Scenario**: Node receives unit from network. If CRDT merge occurs before signature verification completes (async verification), unsigned/malformed units could temporarily enter the graph.
**Impact**: Graph integrity compromised if verification is not synchronous gate before merge.
**Recommendation**: Signature verification must be synchronous and block merge. No unit enters graph state before verification completes.
**Traces to**: INV-S3

---

## FINDING-101: Key Revocation — No Cascade Mechanism for Existing Units
**Severity**: High
**Category**: Security
**Component**: FM-05, cross-context/interactions.md
**Scenario**: Author's key revoked. Pre-revocation units persist in graph. New nodes joining receive these units without knowing the author is revoked. Cross-context says "re-verify entries by that author" but no reject/purge mechanism specified.
**Impact**: Units from compromised authors persist indefinitely. Data derived from malicious pre-revocation units inherits tainted provenance.
**Recommendation**: Design explicit revocation protocol with: revocation timestamp, cascade quarantine mechanism, grace period policy, multi-signed revocation events.
**Traces to**: FM-05, cross-context interactions

---

## FINDING-102: Taint Declassification — Single Policy Author Can Bypass
**Severity**: High
**Category**: Security
**Component**: data-lineage.feature, INV-S4
**Scenario**: Legitimate policy author with narrow scope creates declassification policy for PII data. Policy is validly signed. Taint drops silently from PII to public. No multi-party requirement for declassification specifically.
**Impact**: INV-S4 taint propagation silently reversed by any single policy-scoped author. Downstream data inherits false public classification.
**Recommendation**: Declassification policies require multi-party signing (not solo author). Include human-auditable business justification. Require data steward sign-off separate from policy author.
**Traces to**: INV-S4, ADR-006

---

## FINDING-103: Trust Domain Creation — Conflicting Policy Escalation Undefined
**Severity**: High
**Category**: Security
**Component**: trust-domain.feature, INV-S6, ADR-006
**Scenario**: Two policy authors submit conflicting resolutions for a trust domain creation conflict. Escalation mechanism undefined. Attacker with policy scope can block all trust domain creation by submitting conflicting policies indefinitely.
**Impact**: Trust domain creation can deadlock. INV-S6 requires multi-party agreement but doesn't define tie-breaking.
**Recommendation**: Formal quorum requirement (e.g., 3-of-5 multi-sig). Timeout with escalation to root key holders. Shamir root key as final arbiter.
**Traces to**: INV-S6, ADR-006

---

## FINDING-104: Shamir Root Key Ceremony Completely Unspecified
**Severity**: Critical
**Category**: Security
**Component**: specs/assumptions.md, docs/vision/SYSTEM_VISION.md
**Scenario**: System vision mentions "Shamir shared root key (5 shares, threshold 3)" but there is NO specification for: share creation, storage, protection, reconstitution, compromise detection, key rotation, or recovery if quorum holders are incapacitated.
**Impact**: Root of trust is undefined. Attacker with 3 of 5 shares can impersonate root authority. Complete compromise of trust domain authority, role assignment, all governance.
**Recommendation**: Create mandatory ADR-007 for Shamir ceremony: share generation/distribution, encrypted storage on separate nodes/geographies, audit trail for reconstitution, key rotation protocol, succession plan.
**Traces to**: SYSTEM_VISION.md, ADR-006 (meta-authority problem), A1

---

## FINDING-105: Gossip Membership Poisoning — No Authenticated Messages
**Severity**: High
**Category**: Security
**Component**: node-lifecycle.feature, ADR-004
**Scenario**: SWIM gossip messages are not signed. Compromised node broadcasts false membership messages ("node X is failed"). No verification of gossip message origin. In dev mode (no TPM), any node can poison membership.
**Impact**: False failure detection causes cascading re-placements. Entire cluster membership view becomes unreliable.
**Recommendation**: Cryptographically sign all gossip messages with node identity. Membership state transitions require N-witness verification. Add nonce to prevent replay.
**Traces to**: ADR-004, node-lifecycle.feature, A5

---

## FINDING-106: Signature Replay Attack — No Context Binding
**Severity**: High
**Category**: Security
**Component**: INV-S3, security-enforcement.feature
**Scenario**: Unit U signed for trust domain "dev". Attacker copies U to trust domain "prod" on different cluster. Signature verification passes (valid for U). But author has no scope in "prod". Signature doesn't bind to context.
**Impact**: Cross-cluster/cross-domain unit injection. Author scope check is separate from signature check.
**Recommendation**: Signature must bind context: Sign(key, hash(unit || trust_domain_id || cluster_id || timestamp_range)). Verification fails if context changed.
**Traces to**: INV-S3

---

## FINDING-107: Data Unit Purpose/Consent Enforcement Missing
**Severity**: High
**Category**: Security
**Component**: specs/domain-model.md, INV-S1
**Scenario**: Data unit X declares consent_scope="marketing only". Workload W declares needs data with purpose="analytics". Capability matching succeeds (both access X). But PURPOSE is not a matched capability dimension. Solver composes units that satisfy technical capability but violate legal purpose.
**Impact**: Regulatory violation. Data misuse without audit trail. INV-S1 violated.
**Recommendation**: Extend capability model to include PURPOSE as a dimension. Purpose mismatch triggers conflict requiring policy resolution.
**Traces to**: INV-S1, INV-K2, ADR-003

---

## FINDING-108: Policy Unit Self-Signature — Single Author Bypasses Multi-Party
**Severity**: Medium
**Category**: Security
**Component**: domain-model.md, conflict-resolution.feature
**Scenario**: Single policy author with scope (policy, all_trust_domains) creates a policy resolving a trust domain creation conflict. Violates INV-S6 (multi-party required) despite correct signature.
**Impact**: Overly broad policy scope defeats multi-party security model.
**Recommendation**: Policy units requiring multi-party agreement must declare required_signers field. Solver validates all required signatures present.
**Traces to**: INV-S6, conflict-resolution.feature

---

## FINDING-109: Erasure Shard Reconstruction — No Post-Decode Signature Verification
**Severity**: High
**Category**: Security
**Component**: ADR-005, INV-C1
**Scenario**: Node reconstructs graph from erasure-coded shards. Compromised node injected a false shard. After decoding, reconstructed graph contains forged units. No specification of signature re-verification after reconstruction.
**Impact**: Graph corruption during node join/recovery. Forged units accepted.
**Recommendation**: After reconstruction: re-verify ALL unit signatures. Per-unit hash stored separately from erasure shards. If any signature fails, quarantine and re-request from different peer.
**Traces to**: ADR-005, INV-C1

---

## FINDING-110: Role Expiry — Clock-Based Enforcement Vulnerable
**Severity**: Medium
**Category**: Security
**Component**: trust-domain.feature
**Scenario**: Role assignment expires "2026-12-31". Enforcement uses local system clock. Compromised node sets clock to before expiry. Units authored after actual expiry accepted.
**Impact**: Expired roles continue operating on clock-tampered nodes.
**Recommendation**: Expiry verified by multiple nodes (quorum check). Add explicit revocation event separate from time-based expiry. Revocation is retroactive.
**Traces to**: trust-domain.feature, A1

---

## FINDING-111: Solver Determinism — Core Architecture Depends on Unproven Assumption
**Severity**: High
**Category**: Security
**Component**: A2, OQ-004
**Scenario**: A2 is "Unknown." If solver uses floating-point, determinism across platforms is not guaranteed. The entire no-masters architecture (ADR-004) depends on deterministic placement. Non-determinism causes duplicate placements, conflicting state, cascading divergence.
**Impact**: Fundamental architecture assumption is untested. Blast radius: entire system.
**Recommendation**: Implement proof-of-concept solver immediately. Verify byte-for-byte identical output across platforms. Mandate integer-only arithmetic.
**Traces to**: A2, OQ-004, INV-C3, ADR-004

---

## FINDING-112: Partition — Dual Placement of Role-Carrying Units
**Severity**: High
**Category**: Security
**Component**: network-partition.feature, FM-03
**Scenario**: During partition, both sides place the same policy author workload. Both create conflicting policies with same credentials. CRDT merge sees two policies from same author. INV-S2 (fail closed) and INV-C2 (deterministic merge) both violated.
**Impact**: Policy conflicts that cannot be resolved during partition window.
**Recommendation**: Role-carrying units (policy, governance authors) cannot be duplicated. Add affinity rule: such units require quorum placement. If quorum unreachable in partition, unit disabled on that side.
**Traces to**: network-partition.feature, FM-03, INV-C2, INV-S2

---

## FINDING-113: Gossip Membership — No Merkle Tree Verification
**Severity**: Medium
**Category**: Security
**Component**: node-lifecycle.feature, ADR-004
**Scenario**: Compromised node modifies membership state locally and re-gossips false version. No global consistency check. Different nodes may see different membership views for extended periods.
**Impact**: Solver produces different placements per node due to different membership views.
**Recommendation**: Periodic membership Merkle tree root broadcast. Nodes verify local state against root. Alert on mismatch.
**Traces to**: ADR-004, node-lifecycle.feature

---

## FINDING-114: Provenance Chain — No Cryptographic Integrity Binding
**Severity**: Medium
**Category**: Security
**Component**: INV-D1, data-lineage.feature
**Scenario**: Data unit Y claims provenance "produced by workload A from input X." But what if A never actually produced Y? Provenance is a claim, not cryptographically bound to execution.
**Impact**: False provenance possible. Taint propagation based on false ancestry. Regulatory compliance (FDA, HIPAA) depends on provenance accuracy.
**Recommendation**: Provenance cryptographically bound: workload signs output with execution key. Signature: sign(key, hash(data || input_list || workload_id)). Separate execution identity from authoring identity.
**Traces to**: INV-D1, data-lineage.feature

---

## FINDING-115: Capability Type System Underspecified
**Severity**: Medium
**Category**: Security
**Component**: INV-K2, composition.feature
**Scenario**: What defines "postgres-compatible"? No formal specification. Different nodes might have different type registries, causing different solver results.
**Impact**: Type ambiguity breaks determinism (INV-C3). Trusted component may be substituted with "compatible" untrusted one.
**Recommendation**: Formalize type system (exact match vs semver). Type registry as governance unit. Type mismatch causes conflict requiring policy resolution.
**Traces to**: INV-K2, INV-C3

---

## FINDING-116: Composition Order Dependency Despite Order-Independent CRDT
**Severity**: High
**Category**: Security
**Component**: INV-K1, INV-C1
**Scenario**: Three units: W1, W2 (conflicting needs), P1 (resolving policy). If composed in order W1→W2→P1, W2 fails before P1 arrives. If P1→W1→W2, all succeed. Composition is order-dependent despite CRDT being order-independent.
**Impact**: Composition graph state depends on insertion order. Violates INV-C1 and INV-C2.
**Recommendation**: Solver must re-evaluate entire affected composition set on any unit addition. Composition result must be independent of insertion order. Add INV-C6: "Composition is order-independent."
**Traces to**: INV-K1, INV-C1, INV-C2
