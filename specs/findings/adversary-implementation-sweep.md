# Adversary Implementation Sweep — M1–M7

**Sweep**: Full implementation-level adversarial pass across all 14 crates
**Date**: 2026-09-15
**Status**: Complete
**Mode**: Implementation (source code exists)

## Methodology

Read all target source files line-by-line, cross-referenced against
invariants (`specs/invariants.md`), failure modes (`specs/failure-modes.md`),
previous findings (`specs/findings/INDEX.md`), and project coding standards
(`.opencode/guidelines/rust.md`). Every finding includes a concrete scenario
that triggers it, grounded in the actual code.

---

## Shamir Secret Sharing (`crates/taba-security/src/shamir.rs`)

### FINDING-001: `gf_div` uses production `assert!` — panics on zero denominator
**Severity**: Medium
**Category**: Correctness > coding standard / panic on unexpected input
**Component**: taba-security/src/shamir.rs:107
**Scenario**: Any code path that calls `gf_div(a, 0)` panics the process.
  While `reconstruct_secret` checks for duplicate share indices before
  calling `lagrange_interpolate_at_zero` (which calls `gf_div`), the
  function is `pub(crate)` and could be called from a future code path
  where duplicate indices are not pre-checked. The denominator in
  Lagrange interpolation is `product(x_i ^ x_j)` which is zero iff
  `x_i == x_j` — i.e., duplicate indices. An adversarial caller who
  constructs `Share` objects with duplicate indices and calls
  `reconstruct_secret` directly gets a panic (the duplicate check
  catches it with an error, but if that check were ever bypassed or
  removed, the panic would surface).
**Impact**: Process crash instead of a typed error. Denial of service
  for the node daemon if reached.
**Recommendation**: Return `SecurityError::CeremonyError` instead of
  `assert!`. Add a `// INVARIANT: denominator is non-zero because
  duplicate indices are rejected in reconstruct_secret` comment if the
  assert is kept, per the project's `unwrap`/`assert` policy.
**Traces to**: Project Rust guidelines (no bare `assert!` in production
  without `// INVARIANT:`)

### FINDING-002: `gf_pow` produces wrong results — dead code but latent
**Severity**: Low
**Category**: Correctness > arithmetic error
**Component**: taba-security/src/shamir.rs:129
**Scenario**: `gf_pow(3, 2)` returns `EXP_TABLE[(1*2 - 1) % 255 + 1] =
  EXP_TABLE[1] = 3`, but the correct result is `gf_mul(3, 3) = 5` (since
  `3^2 = 5` in GF(2^8) with generator 3). The formula
  `EXP_TABLE[(log_a * n - 1) % 255 + 1]` incorrectly shifts the exponent
  by -1/+1. The correct formula is `EXP_TABLE[(log_a * n) % 255]`. The
  `-1` and `+1` appear to be an erroneous attempt to avoid index 0, but
  index 0 is valid (`EXP_TABLE[0] = 1 = g^0`).
**Impact**: If `gf_pow` is ever called in production, GF(2^8)
  exponentiation returns wrong values, potentially corrupting Shamir
  share computation or any other GF arithmetic.
**Recommendation**: Replace with `EXP_TABLE[(log_a * n as usize) % 255]`.
  Remove `#[allow(dead_code)]` once fixed and add a test.
**Traces to**: Shamir arithmetic correctness

### FINDING-003: `reconstruct_secret` does not validate the threshold
**Severity**: Medium
**Category**: Correctness > missing negative input handling
**Component**: taba-security/src/shamir.rs:275–311
**Scenario**: Call `split_secret(secret, 5, 7)` to produce 5-of-7
  shares. Then call `reconstruct_secret(&shares[0..2])` with only 2
  shares (fewer than threshold k=5). The function interpolates a
  degree-1 polynomial through 2 points and returns a wrong secret
  **without any error**. The `Share` struct carries no metadata about
  the original threshold, so there is no way to detect the
  under-threshold condition.
**Impact**: A caller who receives fewer shares than the original
  threshold (due to partial loss, programming error, or adversary
  withholding) gets a deterministic but incorrect secret. If the caller
  trusts the result, they operate on wrong key material — e.g., a root
  key ceremony participant reconstructs a wrong root key.
**Recommendation**: Add a `threshold` field to `Share` (set during
  `split_secret`) and have `reconstruct_secret` reject fewer shares than
  the recorded threshold. Alternatively, document that callers must
  externally track the threshold.
**Traces to**: FM-19 (Tier 0 → Tier 1 upgrade failure), ceremony
  correctness

### FINDING-004: Intermediate share values not zeroized after reconstruction
**Severity**: Low
**Category**: Security > secret handling
**Component**: taba-security/src/shamir.rs:301–308
**Scenario**: After `reconstruct_secret` returns, the `points` vector
  (containing individual share bytes) and local variables in
  `lagrange_interpolate_at_zero` (`numerator`, `denominator`, `result`)
  are dropped but not zeroized. While `Share` derives `ZeroizeOnDrop`
  (so the caller's shares are zeroized when they go out of scope), the
  intermediate `Vec<(u8, u8)>` in `reconstruct_secret` is not.
**Impact**: Share-derived values remain in memory after reconstruction.
  An attacker with a memory dump (e.g., via `/proc/<pid>/mem` or a core
  dump) could extract partial share data.
**Recommendation**: Use `zeroize::Zeroize` on the `points` vector and
  local variables in `lagrange_interpolate_at_zero`, or restructure to
  avoid intermediates.
**Traces to**: Project Rust guidelines (secret handling)

### FINDING-005: Duplicate assignment in `build_exp_table`
**Severity**: Info
**Category**: Correctness > code quality
**Component**: taba-security/src/shamir.rs:47–48
**Scenario**: `table[i] = x as u8;` appears twice consecutively. The
  second assignment is a no-op. This is likely a copy-paste artifact.
**Impact**: None — produces correct output. But indicates
  insufficient review of the table generation code.
**Recommendation**: Remove the duplicate line.
**Traces to**: Code quality

---

## Attestation (`crates/taba-security/src/attestation.rs`)

### FINDING-006: Software attestation forgeable — no runtime gate against production use
**Severity**: Medium
**Category**: Security > trust boundary bypass
**Component**: taba-security/src/attestation.rs:124–167
**Scenario**: `SoftwareAttestation::new()` is a public constructor with
  no feature gate, no `#[cfg(test)]`, and no runtime check. A node
  configured with `AttestationProvider::Software` generates a
  self-signed attestation (`signature = quote.clone()`, line 155).
  `verify_software_attestation` only checks that `quote ==
  SHA256(binary_hash || os || arch || nonce)` and `signature == quote`.
  An attacker who knows the binary hash string and nonce can forge a
  valid attestation with any `binary_hash` value — the verifier never
  compares `binary_hash` against an expected value.
**Impact**: A misconfigured production node accepts software
  attestations, which provide no hardware-rooted integrity guarantee.
  An attacker who compromises a node can forge attestations for other
  nodes by computing SHA256 of arbitrary data.
**Recommendation**: Add a `debug_assert!` or runtime check that
  `AttestationProvider::Software` is only used when a `--dev` or
  `--allow-software-attestation` flag is set. At minimum, document in
  bold that production deployments must configure TPM attestation.
**Traces to**: FM-04 (compromised node), attestation trust model

### FINDING-007: `verify_software_attestation` does not verify `node_id`
**Severity**: Low
**Category**: Security > input validation
**Component**: taba-security/src/attestation.rs:182–211
**Scenario**: The verification function accepts an `AttestationResult`
  with any `node_id`. It recomputes the quote from `binary_hash`, `os`,
  `arch`, and `expected_nonce`, but never checks that
  `result.node_id` matches the expected node. An attacker could present
  a valid attestation for node A when the verifier expects node B.
**Impact**: Attacker can substitute one node's attestation for
  another's. Blast radius limited if the caller separately checks
  `node_id`, but the API does not enforce it.
**Recommendation**: Add an `expected_node_id: &NodeId` parameter to
  `verify_software_attestation` and check it.
**Traces to**: FM-04, attestation trust model

---

## SLSA Provenance (`crates/taba-security/src/provenance.rs`)

### FINDING-008: Empty `builder_signature` accepted by default — fail-open
**Severity**: High
**Category**: Security > fail-open default
**Component**: taba-security/src/provenance.rs:135–220
**Scenario**: `DefaultProvenanceVerifier::new()` creates a verifier
  with an empty `trusted_builders` list. When a provenance's builder is
  not in `trusted_builders`, the signature verification block (lines
  189–217) is skipped entirely — any `builder_signature` (including
  empty) is accepted. A workload unit with `slsa_level = 3` and an
  empty `builder_signature` passes verification as long as the minimum
  level is ≤ 3.
**Impact**: By default, SLSA provenance provides no integrity
  guarantee. An attacker who can inject a workload unit with a forged
  `SlsaProvenance` (setting `slsa_level = 3` and empty signature) bypasses
  the SLSA minimum level enforcement. This violates INV-A1 (artifact
  integrity) and the spec's claim that SLSA provides supply-chain
  integrity.
**Recommendation**: Change the default to fail-closed: if no trusted
  builders are configured, reject any provenance with a non-empty
  `builder_signature` (since there's no key to verify against) and
  require explicit configuration to trust builders without signatures.
  Alternatively, add a `require_signatures: bool` flag defaulting to
  `true`.
**Traces to**: INV-A1, INV-S2 (fail-closed on security conflicts)

### FINDING-009: SLSA level is self-attested, not cryptographically verified
**Severity**: Medium
**Category**: Security > trust model gap
**Component**: taba-security/src/provenance.rs:160–168, 189–217
**Scenario**: A provenance with `slsa_level = 3` passes the minimum
  level check (`provenance.slsa_level >= min_level`). The SLSA level is
  a field in the `SlsaProvenance` struct, set by the builder. When no
  trusted builder key is configured, the level is accepted at face
  value. Even when a trusted builder IS configured, the signature
  covers the entire payload including `slsa_level`, but a compromised
  builder can set any level.
**Impact**: An attacker who controls a builder (or forges provenance
  when no trusted builder is configured) can claim any SLSA level,
  bypassing minimum level requirements.
**Recommendation**: Document that SLSA level enforcement relies on
  trusted builder configuration. Consider a separate, higher-authority
  attestation for the SLSA level itself.
**Traces to**: INV-A1, provenance trust model

### FINDING-010: `provenance_payload` silently returns empty on serialization failure
**Severity**: Low
**Category**: Robustness > error handling
**Component**: taba-security/src/provenance.rs:247
**Scenario**: `serde_json::to_vec(&payload).unwrap_or_default()`
  returns an empty `Vec<u8>` if serialization fails. The signature is
  then computed over an empty payload. During verification, the same
  empty payload is used, so the signature would match — but the
  signed content is empty, not the actual provenance fields.
**Impact**: If a future change to `SlsaProvenance` introduces a
  non-serializable field, signatures would be over empty data,
  defeating the integrity guarantee.
**Recommendation**: Return `Result<Vec<u8>, SecurityError>` and propagate
  the error.
**Traces to**: Provenance integrity

---

## WAL (`crates/taba-node/src/wal.rs`)

### FINDING-011: Compaction deletes old segments before writing new ones — data loss on crash
**Severity**: Critical
**Category**: Correctness > failure cascade / data loss
**Component**: taba-node/src/wal.rs:696–711
**Scenario**: `compact()` reads kept entries from old segments (lines
  657–685), then deletes all old segment files (lines 696–698:
  `std::fs::remove_file(&seg.path)` — errors ignored!), then creates a
  new segment and writes kept entries (lines 702–709). If the process
  crashes, `Segment::create` fails, or `append_frame` fails after
  step 2, **all WAL data is lost** — old segments are deleted and the
  new segment is empty or partially written.
  Concrete steps: (1) Append 100 entries. (2) Call `compact(WalPosition(50))`
  to keep entries 50–100. (3) Process crashes after old segment files
  are deleted but before the new segment is fully written. (4) On
  restart, `DiskWalManager::new` finds no segments (or a partial one),
  and the WAL is unrecoverable.
**Impact**: Complete WAL data loss, violating FM-07 ("Unacceptable:
  Silent data loss") and INV-C4 (WAL-before-effect). All merged units,
  pending entries, and promotions since the last compaction are lost.
**Recommendation**: Write the new segment and `fsync` it BEFORE
  deleting old segments. Use a temporary name for the new segment,
  rename it atomically after fsync, then delete old segments.
**Traces to**: FM-07, INV-C4, DL-008

### FINDING-012: `replay` ignores the `from` parameter — replays from beginning
**Severity**: High
**Category**: Correctness > interface contract violation
**Component**: taba-node/src/wal.rs:587–639
**Scenario**: Call `replay(WalPosition(500), callback)` expecting only
  entries from position 500 onward. The function iterates every segment
  from the beginning, decoding every frame and passing it to the
  callback. The `from` parameter is used only as the initial value of
  `last_position` (line 597), which is immediately overwritten by
  `WalPosition(pos_before)` for each entry (line 615). No entries are
  skipped.
**Impact**: Callers that rely on incremental replay (e.g., "replay
  only new entries since last checkpoint") receive all entries every
  time, wasting I/O and potentially re-applying already-applied
  mutations. If the callback is not idempotent, this causes duplicate
  state mutations.
**Recommendation**: Track the current segment and offset. Skip entries
  in the first segment that are before `from`. For subsequent segments,
  replay from the beginning (since `WalPosition` is per-segment, this
  requires rethinking the position model — see FINDING-013).
**Traces to**: INV-C4, WalManager trait contract

### FINDING-013: `WalPosition` is per-segment offset, not global — ambiguous across segments
**Severity**: Medium
**Category**: Correctness > implicit coupling
**Component**: taba-node/src/wal.rs:574, 725–733
**Scenario**: `append()` returns `WalPosition(pos_in_segment)` — the
  offset within the current segment only (line 574). `latest_position()`
  returns `WalPosition(s.size)` — the size of the last segment (line
  732). With multiple segments, the same `WalPosition` value can refer
  to different entries in different segments. `replay` and `compact`
  accept `WalPosition` but cannot determine which segment it refers to.
**Impact**: Any consumer that stores a `WalPosition` and later passes
  it to `replay` or `compact` gets undefined behavior — the position
  may refer to a different segment than expected. Incremental replay
  and compaction correctness depend on the position being meaningful,
  which it is not across segment boundaries.
**Recommendation**: Make `WalPosition` a `(segment_sequence_start,
  offset_within_segment)` tuple, or a global monotonic offset that
  spans all segments.
**Traces to**: INV-C4, WAL design

### FINDING-014: Compaction ignores `remove_file` errors
**Severity**: Medium
**Category**: Robustness > error handling
**Component**: taba-node/src/wal.rs:697
**Scenario**: `let _ = std::fs::remove_file(&seg.path);` silently
  ignores any error. If the file cannot be deleted (permissions, NFS,
  file in use), the compaction proceeds as if it was deleted. On
  restart, `DiskWalManager::new` loads both the old (not deleted)
  segment and the new segment, potentially duplicating entries.
**Impact**: Silent data duplication on WAL recovery. If the old segment
  contains entries that were supposed to be compacted away, they
  reappear on restart, potentially re-applying old mutations.
**Recommendation**: Return an error if `remove_file` fails, or at
  minimum log a warning and track the stale segment for retry.
**Traces to**: FM-07, WAL integrity

### FINDING-015: `replay` error classification uses fragile string matching
**Severity**: Low
**Category**: Robustness > error handling
**Component**: taba-node/src/wal.rs:619–633
**Scenario**: The `replay` function distinguishes "end of file" (non-
  fatal, stop replay) from "CRC corruption" (fatal, return error) by
  matching on substrings of the error `reason` field:
  `reason.contains("failed to read frame length")` vs.
  `reason.contains("CRC32C mismatch")`. If the error message format
  changes (e.g., reworded, localized, or truncated), the matching
  breaks and EOF may be treated as corruption (or vice versa).
**Impact**: False corruption reports stop replay prematurely, causing
  incomplete state recovery. Missed corruption causes silent state
  inconsistency.
**Recommendation**: Use typed error variants (e.g.,
  `NodeError::WalEndOfFile` vs. `NodeError::WalCorrupted`) instead of
  string matching.
**Traces to**: FM-07, error taxonomy

---

## CRDT Graph (`crates/taba-graph/src/graph.rs`)

### FINDING-016: Default graph accepts unsigned units — violates INV-S3
**Severity**: Critical
**Category**: Security > specification compliance
**Component**: taba-graph/src/graph.rs:258–268, 470–494
**Scenario**: `DefaultGraph::new(memory_limit_bytes)` creates a graph
  with `verifier: None` (line 264). `DefaultGraph::with_validator()`
  also sets `verifier: None` (line 283). When `verifier` is `None`,
  `insert()` (lines 477–494) and `merge()` (lines 682–701) skip
  signature verification entirely. The `wrap_signed()` method (lines
  368–382) creates a `SignedUnit` with a zero-valued signature (`[0u8;
  64]`), nil trust domain, nil cluster ID, and zero public key. This
  placeholder is accepted without any cryptographic check.
  Concrete: `let graph = DefaultGraph::new(1_000_000_000);
  graph.insert(any_unit).await.unwrap();` — any unit enters the graph
  with a zero signature. An attacker who can call `insert` or `merge`
  can inject arbitrary units with forged authorship.
**Impact**: Violates INV-S3 ("Every unit in the graph is signed by an
  author with valid scope. Unsigned or wrongly-signed units are
  rejected on merge."). In the default configuration, there is no
  signature verification gate — any unit, including forged governance
  and policy units, enters graph state.
**Recommendation**: Make the verifier required (not `Option`). If M2
  needs a "no crypto" mode, gate it behind an explicit
  `DefaultGraph::without_verification()` constructor that logs a
  prominent warning, rather than making it the default.
**Traces to**: INV-S3, FM-04, FM-05, FM-09

### FINDING-017: Spawn depth taken from unit's own declaration, not computed
**Severity**: High
**Category**: Security > bypass validation
**Component**: taba-graph/src/graph.rs:499–511
**Scenario**: INV-W3 states: "Spawn depth is computed by traversing
  the spawn provenance chain." The code instead checks
  `spawn_ctx.spawn_depth > self.max_spawn_depth` (line 501), taking the
  depth directly from the unit's own `SpawnContext` field. An attacker
  (or buggy workload) can set `spawn_depth: 1` while actually being 10
  levels deep in the spawn chain. The graph accepts the unit because
  `1 <= 4`, bypassing the depth limit.
  Concrete: A workload at depth 5 sets `spawn_context.spawn_depth = 3`.
  The graph accepts it. That workload then spawns a child at declared
  depth 4, which also passes. The actual spawn chain is now 6 levels
  deep, exceeding the maximum of 4.
**Impact**: Unlimited spawn depth via lying about the depth field.
  Resource exhaustion attacks per FM-23 ("Unlimited spawn depth
  allowing resource exhaustion attacks").
**Recommendation**: Compute spawn depth by traversing the
  `spawned_by` chain in the graph (following `spawn_context.spawned_by`
  references until a unit with no spawn context is reached). Compare
  the computed depth, not the declared one, against `max_spawn_depth`.
**Traces to**: INV-W3, FM-23

### FINDING-018: Scope checker not invoked for workload units (and None by default)
**Severity**: High
**Category**: Security > specification compliance
**Component**: taba-graph/src/graph.rs:519–560, 266
**Scenario**: INV-S8 requires: "For state-producing unit types
  (workload, data), no two distinct authors can have identical
  (unit_type_scope, trust_domain_scope) tuples." The scope checker
  (`self.scope_checker`) is only invoked for
  `Unit::Governance(GovernanceUnit::RoleAssignment(new_ra))` (line 520).
  It is NOT invoked when a workload or data unit is inserted.
  Furthermore, `scope_checker` is `None` by default (line 266), so even
  role assignments are not checked unless the caller explicitly calls
  `with_scope_checker()`.
  Concrete: `let graph = DefaultGraph::new(1_000_000_000);` —
  `scope_checker` is `None`. Two role assignments with identical scope
  tuples are both accepted. Two workloads from different authors with
  overlapping scopes are both accepted.
**Impact**: INV-S8 is not enforced. An attacker can create role
  assignments with overlapping scopes, potentially enabling scope
  escalation (the original F-001 finding that was supposedly resolved
  by adding INV-S8).
**Recommendation**: Make the scope checker required by default (same
  approach as FINDING-016 for the verifier). At minimum, invoke the
  scope checker for workload and data unit insertions, not just role
  assignments.
**Traces to**: INV-S8, F-001, F-210

### FINDING-019: Self-references removed but no independent cycle check at insertion
**Severity**: Medium
**Category**: Correctness > missing negative handling
**Component**: taba-graph/src/graph.rs:211–212
**Scenario**: `compute_references` removes self-references at line 212:
  `refs.remove(&unit.id())`. This means a unit with
  `recovery_relationships: [RecoveryRelationship { depends_on: self_id,
  ... }]` has the self-reference stripped, so it enters the active set
  without cycle detection. While a self-reference alone is not a cycle
  (it's removed), the cycle detection for recovery relationships is
  performed by `taba-solver/src/cycle.rs` (the `CycleDetector`), not
  at graph insertion time. A unit with a recovery relationship to a
  not-yet-present unit goes to the pending queue; when the referenced
  unit arrives and creates a cycle, the cycle is detected by the
  solver at query time, not at insertion time.
  This is mostly by design (INV-K5: "Cyclic recovery dependencies fail
  closed"), but the gap is that between insertion and solver run, a
  cyclic dependency exists in the graph without being flagged. If the
  solver is not run (e.g., single-node M2 without compose), the cycle
  is never detected.
**Impact**: Cyclic recovery dependencies can exist in the graph
  indefinitely if the solver is not run. The spec says "the solver
  reports an unresolvable conflict" — but if no solver run is
  triggered, the cycle is silent.
**Recommendation**: Consider running a lightweight cycle check at
  insertion time when references are satisfied (i.e., during
  `promote_pending`), not just at solver query time.
**Traces to**: INV-K5, FM-23

---

## Gossip (`crates/taba-gossip/src/swim.rs`, `transport.rs`)

### FINDING-020: `declare_failed` does not verify witnesses are known or distinct
**Severity**: High
**Category**: Security > input validation
**Component**: taba-gossip/src/swim.rs:480–522
**Scenario**: `declare_failed(&target, &[witness1, witness2])` accepts
  any `NodeId` values as witnesses. There is no check that:
  (a) The witnesses are known members of the cluster.
  (b) The witnesses are distinct from the target.
  (c) The witnesses are distinct from the declaring node.
  (d) The witnesses actually sent `WitnessConfirmation` messages.
  Concrete: An attacker calls `declare_failed(&victim,
  &[NodeId::nil(), NodeId::from_u128(999)])` with two arbitrary,
  non-existent witness IDs. The function checks only
  `witnesses.len() >= self.params.witness_count` (default 2) and that
  the target is in `Suspected` state. It then adds the fake witnesses
  and sets the target to `Failed`.
**Impact**: An attacker can declare any suspected node as failed by
  providing two arbitrary witness IDs. This enables eviction of
  legitimate nodes from the cluster, violating INV-R3 ("Membership
  state changes require corroboration from at least 2 independent
  witnesses").
**Recommendation**: Verify that each witness: (a) is a known member,
  (b) is not the target, (c) is not the declaring node, (d) has a
  recorded `WitnessConfirmation` message. Track witness confirmations
  separately and check the accumulated count, not the argument count.
**Traces to**: INV-R3, DL-009, FM-09

### FINDING-021: Key confusion in `handle_message` for `MembershipChange`
**Severity**: Medium
**Category**: Security > semantic drift
**Component**: taba-gossip/src/swim.rs:572–583
**Scenario**: When handling a `GossipPayload::MembershipChange(change)`,
  the code calls `self.remember_member(change.node_id, pk, addr)` where
  `pk` is `sender_key` — the public key of the message SENDER
  (`message.sender`), not the key of the node being changed
  (`change.node_id`). If `change.node_id != message.sender` (which is
  the normal case — node A gossips about node B's state change), the
  sender's key is registered as the changed node's key.
  Mitigating factor: `remember_member` uses `or_insert`, so if the
  target node already has a key, it is not overwritten. And the
  `if let Some(addr)` guard requires the target to already have an
  address, which (in the current code) is only set by
  `remember_member`, which also sets the key. So the bug is currently
  not exploitable — but it is a latent correctness error that becomes
  exploitable if the address is ever set through a different path.
**Impact**: Latent — if addresses are ever populated independently of
  keys (e.g., via a future join protocol), an attacker can register
  their key as another node's key, enabling message forgery.
**Recommendation**: Use the public key from the `MembershipChange`
  payload (or a separate field) instead of `sender_key`. The
  `MemberInfo` in the membership view already carries a `public_key`
  field — use that.
**Traces to**: INV-R3, FM-04

### FINDING-022: No rate limiting on join/leave — Sybil attack
**Severity**: Medium
**Category**: Security > resource exhaustion
**Component**: taba-gossip/src/swim.rs:336–433
**Scenario**: `join()` and `leave()` have no rate limiting, cost, or
  cooldown. An attacker can rapidly join and leave the cluster by
  generating new `KeyPair`s and calling `join()` with seed addresses.
  Each join/leave cycle triggers membership gossip propagation,
  potential shard re-coding, and solver re-placement across the
  cluster.
**Impact**: Membership churn degrades cluster performance. Rapid
  join/leave can trigger cascading erasure reconstruction (FM-13),
  solver re-placement storms, and gossip overhead. The spec (F-A314)
  notes "fleet refresh governance command has no rate limit" — this is
  the same pattern for join/leave.
**Recommendation**: Add a join cooldown (e.g., minimum time between
  joins from the same IP or key fingerprint). Require a proof-of-work
  or ceremony for new node admission.
**Traces to**: FM-13, F-A314, INV-R3

### FINDING-023: Incarnation number not bounded — `u64::MAX` locks out legitimate updates
**Severity**: Medium
**Category**: Security > input validation
**Component**: taba-gossip/src/swim.rs:585–588, membership module
**Scenario**: An attacker who has stolen a node's signing key sends a
  `MembershipChange` with `incarnation: u64::MAX`. The "higher
  incarnation wins" semantics (DL-009) mean that no legitimate update
  from that node can ever have a higher incarnation. The node is
  permanently stuck at the attacker's chosen state — even after the
  key is revoked, the membership entry with `u64::MAX` incarnation
  cannot be overridden by a lower incarnation.
**Impact**: Permanent membership state lockout for the compromised
  node. Even after key revocation and re-keying, the stale membership
  entry with maximum incarnation persists until manually removed.
**Recommendation**: Bound incarnation numbers (e.g., reject
  incarnations above `current + 1000`). Alternatively, use a separate
  revocation mechanism that clears membership entries regardless of
  incarnation.
**Traces to**: DL-009, FM-05, INV-S3

---

## Erasure Coding (`crates/taba-erasure/src/coding.rs`)

### FINDING-024: No integrity verification of reconstructed data
**Severity**: Medium
**Category**: Correctness > missing verification
**Component**: taba-erasure/src/coding.rs:213–263
**Scenario**: `decode()` uses `reed-solomon-erasure` to reconstruct
  missing shards, then calls `reassemble_data()` to produce the
  original data. There is no checksum, hash, or signature verification
  of the reconstructed data. The `reassemble_data` function only checks
  the length prefix — it does not verify that the data matches any
  expected digest.
  If a shard is corrupted (not missing, but wrong data), the
  Reed-Solomon reconstruction may produce silently corrupted output.
  The `reed-solomon-erasure` library's `reconstruct()` does not
  guarantee detection of corrupted shards — it only fills in missing
  ones. If more than `m` shards are corrupted (but all `n` are
  present), the reconstruction produces wrong data without error.
**Impact**: Corrupted shards cause silent data corruption during
  reconstruction. INV-R1 says "After reconstruction, all unit
  signatures are re-verified" — but the erasure coding layer itself
  provides no integrity check, and the higher-level re-verification is
  a separate concern that must be correctly wired.
**Recommendation**: Add a SHA-256 hash of the original data to the
  shard metadata (or as a separate integrity shard) and verify it
  after reconstruction. Document that callers must verify signatures
  after reconstruction per INV-R1.
**Traces to**: INV-R1, FM-02

### FINDING-025: Duplicate shards silently overwrite without detection
**Severity**: Low
**Category**: Correctness > input validation
**Component**: taba-erasure/src/coding.rs:223–235
**Scenario**: If the input `shards` slice contains two shards with the
  same index, the second one silently overwrites the first (line 234:
  `opts[idx] = Some(shard.data.clone())`). The `present_count` is only
  incremented if `opts[idx].is_none()` (line 231), so the duplicate
  doesn't affect the count. But the data used for reconstruction is
  from the last shard with that index — the first is silently
  discarded.
**Impact**: If two shards with the same index have different data
  (e.g., from different sources, one corrupted), the corrupted one
  may silently win. No error is reported.
**Recommendation**: Return an error if a duplicate index is detected,
  or log a warning.
**Traces to**: INV-R1, data integrity

---

## CLI (`crates/taba-cli/src/`)

### FINDING-026: Private key file created with default permissions (world-readable)
**Severity**: Critical
**Category**: Security > secret handling
**Component**: taba-cli/src/auth.rs:141
**Scenario**: `LocalAuth::init()` writes the Ed25519 private key as
  hex to a file using `std::fs::write(self.keypair_path(), &hex_key)`
  (line 141). `std::fs::write` creates the file with default
  permissions, which on most Unix systems (with umask 022) is `0644` —
  owner read/write, group read, world read. **Any user on the system
  can read the private key.**
  Concrete: `taba init` creates `~/.taba/keypair` with permissions
  `-rw-r--r--`. Running `cat ~/.taba/keypair` as any user reveals the
  private key in hex.
**Impact**: Complete compromise of the node's identity. Any local user
  can read the private key, sign arbitrary units, impersonate the
  author, and escalate privileges within the trust domain. This
  violates the core security pillar: "Security as first class —
  zero-access default."
**Recommendation**: Use `std::os::unix::fs::PermissionsExt` to set
  the file permissions to `0o600` (owner read/write only) after
  writing: `std::fs::set_permissions(&path,
  std::os::unix::fs::Permissions::from_mode(0o600))?;` Alternatively,
  use `OpenOptions::new().write(true).create(true).truncate(true)
  .mode(0o600).open(&path)` on Unix.
**Traces to**: Project security pillar, INV-S3, FM-05

### FINDING-027: `push` command: cache filename derived from file extension without sanitization
**Severity**: Low
**Category**: Security > path handling
**Component**: taba-cli/src/commands.rs:416–420
**Scenario**: The `push` command computes the destination filename as
  `format!("{hex_digest}{ext}")` where `ext` is derived from
  `file.extension()` (line 418). File extensions are generally safe
  (they can't contain `/` on most filesystems), but a file named
  `artifact.` (trailing dot) would produce `ext = "."` and a
  destination of `<hash>.`, which is harmless. A file with a very long
  extension could exceed filesystem path limits.
  More concerning: the cache directory is created with
  `create_dir_all` (line 411) using default permissions (0755), and the
  cached artifact is written with default permissions (0644). If the
  artifact is a secret (e.g., a user pushes a private key file), it is
  world-readable in the cache.
**Impact**: Low for path traversal (extension is filesystem-safe).
  Medium for secret leakage if users push sensitive artifacts.
**Recommendation**: Set cache file permissions to 0600 for artifacts.
  Document that `push` is for artifacts, not secrets.
**Traces to**: FINDING-026 (same pattern), secret handling

---

## K8s Converter (`crates/taba-k8s/src/converter.rs`)

### FINDING-028: TOML injection via K8s metadata — unsanitized interpolation
**Severity**: High
**Category**: Security > input validation / injection
**Component**: taba-k8s/src/converter.rs:178, 191, 260, 387, 419, 521, 577
**Scenario**: The converter interpolates K8s metadata values (resource
  names, container images, port names, selector values) directly into
  TOML strings using `format!`. For example, line 178:
  `format!("...name = \"{name}\"...image = \"{image}\"...")`
  If a K8s manifest contains:
  ```yaml
  metadata:
    name: 'evil"\nimage = "malicious:latest'
  ```
  The generated TOML becomes:
  ```toml
  [unit]
  name = "evil"
  image = "malicious:latest"
  image = "nginx:1.25"
  ```
  The attacker has injected an arbitrary `image` field. Similar
  injection is possible via container names, port names, ConfigMap
  keys, role names, and NetworkPolicy selectors.
  The `image` field (line 179) is particularly dangerous — a crafted
  image string like `foo"\n[classification]\nlevel = "public` can
  downgrade the classification of the generated unit.
**Impact**: An attacker who can supply a crafted K8s manifest can
  inject arbitrary TOML keys and values into generated taba units,
  potentially bypassing security controls (classification, retention,
  scaling, health checks). The generated TOML is not validated before
  being returned to the caller.
**Recommendation**: Escape all interpolated values for TOML string
  context (at minimum, escape `\` and `"`). Better: use a TOML
  serializer (`toml::to_string`) to generate the output instead of
  `format!`. Validate the generated TOML by parsing it back before
  returning.
**Traces to**: INV-S2 (fail-closed on security conflicts), K8s
  migration trust boundary

### FINDING-029: Potential YAML billion laughs via generic `Value` deserialization
**Severity**: Medium
**Category**: Robustness > resource exhaustion
**Component**: taba-k8s/src/converter.rs:56
**Scenario**: The converter calls
  `serde_yaml::from_str::<serde_yaml::Value>(doc)` (line 56) to parse
  each YAML document into a generic `Value` before deserializing it
  into typed structs. A crafted YAML document with deeply nested
  anchors and aliases (the "billion laughs" attack) can cause
  exponential memory expansion during this generic deserialization.
  While `serde_yaml` (based on `yaml-rust2` or similar) may have
  recursion depth limits, the generic `Value` deserialization is more
  vulnerable than typed deserialization because it accepts arbitrary
  structure.
**Impact**: Memory exhaustion and process crash when processing a
  crafted K8s manifest. The converter is used during K8s migration
  (M7), where untrusted manifests may be processed.
**Recommendation**: Set a recursion depth limit on the YAML parser.
  Prefer deserializing directly into typed structs (skipping the
  generic `Value` intermediate step) where possible. Add a maximum
  document size check.
**Traces to**: K8s migration trust boundary, resource exhaustion

### FINDING-030: `is_crd` heuristic misses CRDs and misidentifies non-CRDs
**Severity**: Low
**Category**: Correctness > input validation
**Component**: taba-k8s/src/converter.rs:139–141
**Scenario**: The `is_crd` method returns `kind.contains('.') &&
  !kind.starts_with('v')`. This misses CRDs whose kind starts with
  'v' (e.g., `volumesnapshot.example.com` — unlikely but possible) and
  misidentifies non-CRDs that contain a dot and don't start with 'v'
  (e.g., `my.resource` — a hypothetical resource name).
**Impact**: CRDs may be processed as unknown resources instead of
  being flagged as unmappable. Non-CRDs may be incorrectly classified
  as CRDs.
**Recommendation**: Use the `apiVersion` field (which contains the
  group, e.g., `example.com/v1`) to identify CRDs, rather than
  pattern-matching on `kind`.
**Traces to**: K8s migration correctness

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 3 |
| High | 7 |
| Medium | 11 |
| Low | 8 |
| Info | 1 |
| **Total** | **30** |

### Critical findings (must resolve before next phase)

| ID | Title | Component |
|----|-------|-----------|
| FINDING-011 | Compaction deletes old segments before writing new ones | taba-node/wal.rs |
| FINDING-016 | Default graph accepts unsigned units — violates INV-S3 | taba-graph/graph.rs |
| FINDING-026 | Private key file created with world-readable permissions | taba-cli/auth.rs |

### High findings (resolve before next feature)

| ID | Title | Component |
|----|-------|-----------|
| FINDING-008 | Empty builder_signature accepted by default — fail-open | taba-security/provenance.rs |
| FINDING-012 | replay ignores `from` parameter | taba-node/wal.rs |
| FINDING-017 | Spawn depth taken from unit's own declaration | taba-graph/graph.rs |
| FINDING-018 | Scope checker not invoked for workload units (None default) | taba-graph/graph.rs |
| FINDING-020 | declare_failed doesn't verify witnesses | taba-gossip/swim.rs |
| FINDING-028 | TOML injection via K8s metadata | taba-k8s/converter.rs |
| FINDING-029 | Potential YAML billion laughs | taba-k8s/converter.rs |

### Recurring themes

1. **Fail-open defaults** — FINDING-008, FINDING-016, FINDING-018. Security
   invariants (INV-S3, INV-S8, SLSA verification) are `Option` fields
   that default to `None`, meaning the default configuration does not
   enforce them. This is the same pattern as the original F-001 finding
   ("A1 scope isolation has no enforcement mechanism"). The pattern
   persists across multiple modules.

2. **Trusted-but-unverified inputs** — FINDING-003 (threshold),
   FINDING-017 (spawn depth), FINDING-020 (witnesses), FINDING-028
   (K8s metadata). The system accepts self-declared values without
   independent verification. Authors declare their own spawn depth;
   callers declare their own witnesses; builders declare their own
   SLSA level; K8s metadata is interpolated without escaping.

3. **Data loss on crash** — FINDING-011 (WAL compaction). The WAL — the
   core durability mechanism (INV-C4) — has a window where a crash
   causes complete data loss. This is the most critical finding because
   it undermines the entire persistence story.

4. **Secret handling** — FINDING-026 (private key permissions),
   FINDING-027 (artifact cache permissions), FINDING-004 (intermediate
   values not zeroized). The private key — the root of all
   authentication — is world-readable by default.

5. **WAL position ambiguity** — FINDING-012, FINDING-013. The
   `WalPosition` type is per-segment but used as if it were global,
   making incremental replay and compaction semantics undefined.

### Highest-risk area

The **WAL** (`taba-node/src/wal.rs`) is the highest-risk area. It
contains one Critical finding (FINDING-011: data loss on crash during
compaction) and two High/Medium findings (FINDING-012: replay ignores
`from`, FINDING-013: position ambiguity). The WAL is the foundation
of INV-C4 (WAL-before-effect) — if it is unreliable, every state
mutation is at risk.

### Recommendation: what blocks next phase

The three Critical findings must be resolved before any further
feature work:
1. **FINDING-011**: Rewrite compaction to write-then-delete (not
   delete-then-write). This is an architect-level redesign of the
   compaction sequence.
2. **FINDING-016**: Make the signature verifier required by default.
   This may require changes throughout the codebase (tests, CLI,
   node daemon) that currently rely on the `None` default.
3. **FINDING-026**: Set private key file permissions to 0600. This is
   a small, surgical fix but critical for security.
