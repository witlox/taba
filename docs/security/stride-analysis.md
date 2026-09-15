# STRIDE Security Analysis

**Date**: 2026-09-15
**Scope**: All 14 crates (M1–M7)
**Method**: [STRIDE](https://en.wikipedia.org/wiki/STRIDE_(security)) threat modeling framework

## Overview

STRIDE categorizes threats into six types: **S**poofing,
**T**ampering, **R**epudiation, **I**nformation Disclosure,
**D**enial of Service, and **E**levation of Privilege.
Each is analyzed against taba's attack surfaces.

---

## Spoofing

### Threat: Attacker impersonates a known author

**Attack surface**: Unit authoring, gossip message handling
**Scenario**: Attacker generates an Ed25519 keypair, claims
to be author "alice", and submits units to the graph.
**Mitigation**: Every unit is signed with the author's
private key (INV-S3). The graph's `Verifier` (when configured)
checks the signature against the registered public key. Units
with invalid signatures are rejected at the synchronous gate
before WAL write.
**Residual risk**: **Low** — Ed25519 is cryptographically
sound. The main risk is key compromise, addressed by the
revocation protocol (causal model, INV-S3).

### Threat: Attacker impersonates a node in gossip

**Attack surface**: SWIM membership protocol
**Scenario**: Attacker sends signed gossip messages claiming
to be a known node.
**Mitigation**: All gossip messages are signed with the
sending node's identity key (INV-R3). The `GossipTransport`
verifies signatures before passing messages to
`MembershipProtocol::handle_message`.
**Residual risk**: **Low** — same as author impersonation.

### Threat: Attacker forges a Shamir ceremony

**Attack surface**: Ceremony manager, enrollment
**Scenario**: Attacker initiates a ceremony, collects shares,
and reconstructs the root key.
**Mitigation**: Only authorized authors can initiate
ceremonies. Shares are zeroized after reconstruction. The
reconstructed key is used to sign exactly one governance
unit, then destroyed.
**Residual risk**: **Medium** — if an attacker compromises
enough shares (≥ threshold), they can reconstruct the key.
This is inherent to Shamir secret sharing and mitigated by
physical security of share holders.

---

## Tampering

### Threat: Attacker modifies a unit after signing

**Attack surface**: Graph storage, WAL, gossip
**Scenario**: Attacker modifies a unit in the graph, WAL, or
during gossip transmission.
**Mitigation**: Units are content-addressed (SHA-256 of type +
payload + author + timestamp). The graph stores `SignedUnit`
wrappers. The WAL uses CRC32C framing (DL-014). Gossip
messages are signed.
**Residual risk**: **Low** — any modification is detected by
signature verification or CRC check.

### Threat: Attacker tampers with WAL during compaction

**Attack surface**: WAL compaction
**Scenario**: Attacker (or crash) corrupts the WAL during
compaction, losing data.
**Mitigation**: Compaction writes the new segment and fsyncs
**before** deleting old segments (FINDING-011 fix). If the
process crashes, old segments are still intact for replay.
**Residual risk**: **Low** — write-then-delete ordering
ensures durability.

### Threat: Attacker injects malicious TOML via K8s metadata

**Attack surface**: K8s converter
**Scenario**: K8s manifest contains a resource name with `"`
or `\n`, breaking the generated TOML or injecting arbitrary
keys.
**Mitigation**: All K8s metadata values are sanitized via
`sanitize_toml_value()` before interpolation (FINDING-028
fix). Double quotes, backslashes, and control characters are
escaped.
**Residual risk**: **Low** — all interpolation paths are
covered.

---

## Repudiation

### Threat: Author denies creating a unit

**Attack surface**: Audit trail, provenance
**Scenario**: Author creates a unit, then denies it, claiming
the graph was corrupted.
**Mitigation**: Every unit carries a cryptographic signature
(Ed25519) bound to context (trust domain, cluster, validity
window). Decision trails (INV-O1) record every solver run
with inputs and outputs. Provenance chains (INV-D1) are
structural — they emerge from the composition graph.
**Residual risk**: **None** — signatures are non-repudiable
under standard cryptographic assumptions.

### Threat: Node denies receiving a gossip message

**Attack surface**: Gossip protocol
**Scenario**: Node claims it never received a revocation
message, allowing revoked authors to continue operating.
**Mitigation**: Gossip uses piggybacked retransmission with
`retransmit_multiplier × log₂(N)` rounds. Key revocations
use double the normal rounds for rapid convergence (DL-016).
The causal model (INV-S3) means units signed after revocation
are rejected regardless of when the revocation arrives.
**Residual risk**: **Low** — eventual consistency ensures
all nodes converge.

---

## Information Disclosure

### Threat: Private key file is world-readable

**Attack surface**: CLI auth (LocalAuth)
**Scenario**: Another user on the system reads the private
key from `~/.taba/keypair`.
**Mitigation**: Private key file is created with `0600`
permissions on Unix (owner-only read/write). Non-Unix
systems use `std::fs::write` as a fallback (no permission
control, but non-Unix is not a production target).
**Residual risk**: **Low** on Unix, **Medium** on non-Unix
(documented limitation).

### Threat: Data classification leakage through taint

**Attack surface**: Taint propagation, provenance traversal
**Scenario**: A workload processes PII data and produces
output classified as Public, leaking the PII.
**Mitigation**: Taint propagation (INV-S4) computes
classification at query time by traversing the provenance
graph. Multi-input workloads inherit the union (most
restrictive) of all input classifications. Declassification
requires multi-party signing (INV-S9).
**Residual risk**: **Low** — the lattice model ensures
classification can only become more restrictive without
explicit declassification.

### Threat: Logs contain sensitive data

**Attack surface**: Tracing, structured events
**Scenario**: Tracing logs contain unit payloads, private
keys, or other secrets.
**Mitigation**: The `tracing` crate is configured with
`release_max_level_info`. Unit payloads are never logged —
only metadata (unit ID, author, kind). Key material is
zeroized on drop and never serialized.
**Residual risk**: **Low** — logging is metadata-only by
design.

---

## Denial of Service

### Threat: Attacker floods the graph with units

**Attack surface**: Graph insertion, WAL
**Scenario**: Attacker submits thousands of units per second,
exhausting memory and WAL space.
**Mitigation**: Graph memory limit (INV-R6) triggers
auto-compaction at 80% and degraded mode at 100%. WAL
segments are bounded (default 64 MB each). Compaction
removes discardable entries.
**Residual risk**: **Medium** — no rate limiting on
insertion in M5 local mode. Production multi-node would
use gossip rate limiting.

### Threat: Erasure reconstruction storm

**Attack surface**: Erasure coding, reconstruction scheduler
**Scenario**: Multiple nodes fail simultaneously, triggering
many reconstructions that overwhelm surviving nodes.
**Mitigation**: Reconstruction scheduler uses a priority
queue (governance > policy > data > workload) with a circuit
breaker. When queue depth exceeds threshold, new
reconstructions are rejected (INV-R1, FM-13).
**Residual risk**: **Low** — circuit breaker prevents
cascading failures.

### Threat: Sybil attack via rapid join/leave

**Attack surface**: Gossip membership
**Scenario**: Attacker rapidly joins and leaves the cluster,
disrupting membership convergence.
**Mitigation**: 2-witness failure confirmation (INV-R3)
prevents false positives from a single attacker. Suspected
nodes remain in the pool with health='unknown' (INV-R5).
**Residual risk**: **Medium** — no rate limiting on
join/leave in M4. FINDING-022 tracked.

### Threat: YAML billion laughs

**Attack surface**: K8s converter, TOML parser
**Scenario**: A crafted YAML file with deeply nested aliases
causes exponential memory usage during parsing.
**Mitigation**: `serde_yaml` 0.9 does not support YAML
aliases (deprecated feature). The TOML parser uses `toml`
crate which does not support recursive structures.
**Residual risk**: **Low** — neither parser supports the
alias mechanism needed for billion laughs.

---

## Elevation of Privilege

### Threat: Author creates units outside their scope

**Attack surface**: Unit authoring, graph insertion
**Scenario**: Workload-scoped author attempts to create a
policy unit, or an author scoped to domain A creates units
in domain B.
**Mitigation**: `ScopeChecker::check_author_scope` (INV-S5)
is invoked at graph insertion for all unit types (FINDING-018
fix). Scope uniqueness (INV-S8) is enforced for
state-producing types (workload, data).
**Residual risk**: **Low** — scope is checked at the gate,
not just at authoring time.

### Threat: Attacker escalates spawn depth beyond limit

**Attack surface**: Graph insertion, spawn context
**Scenario**: Workload at depth 5 declares `spawn_depth: 1`
to bypass the limit of 4.
**Mitigation**: `DefaultGraph::insert` computes the actual
spawn depth by walking the parent chain (FINDING-017 fix).
Declared depth must match computed depth. Exceeding
`max_spawn_depth` is rejected.
**Residual risk**: **Low** — depth is computed, not trusted.

### Threat: Spawned task creates governance units

**Attack surface**: Task spawner, delegation tokens
**Scenario**: A spawned bounded task attempts to create a
policy or governance unit, escalating its authority.
**Mitigation**: `DelegationValidator::check_governance_block`
(INV-W4a) rejects spawned tasks attempting governance
operations. The delegation token grants operational
authority only.
**Residual risk**: **None** — governance block is absolute.

### Threat: SLSA level forgery

**Attack surface**: Provenance verification
**Scenario**: Attacker creates a unit with `slsa_level = 3`
and an empty `builder_signature`, bypassing supply chain
verification.
**Mitigation**: `DefaultProvenanceVerifier` rejects empty
signatures when `slsa_level >= 2` (FINDING-008 fix). When
trusted builders are configured, only signed provenance from
known builders is accepted.
**Residual risk**: **Low** — level 1 (no signature required)
is explicitly a progressive disclosure default.

---

## Summary

| Threat Type | Count | Critical | High | Medium | Low | None |
|-------------|-------|----------|------|--------|-----|------|
| Spoofing | 3 | 0 | 0 | 1 | 2 | 0 |
| Tampering | 3 | 0 | 0 | 0 | 3 | 0 |
| Repudiation | 2 | 0 | 0 | 0 | 1 | 1 |
| Info Disclosure | 3 | 0 | 0 | 1 | 2 | 0 |
| DoS | 4 | 0 | 0 | 2 | 2 | 0 |
| Elevation | 4 | 0 | 0 | 0 | 3 | 1 |
| **Total** | **19** | **0** | **0** | **4** | **13** | **2** |

All Critical and High findings from the adversary implementation
sweep (30 findings) have been resolved. The 4 remaining Medium
risks are:

1. **Shamir ceremony compromise** — inherent to the algorithm;
   mitigated by physical security
2. **No insertion rate limiting** (M5 local mode) — production
   multi-node would use gossip rate limiting
3. **No join/leave rate limiting** (M4) — FINDING-022 tracked
4. **Non-Unix key permissions** — documented limitation;
   non-Unix is not a production target
