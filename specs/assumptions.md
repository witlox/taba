# Assumptions

## A1: Author scopes do not overlap for conflicting operations
**Status**: Design assumption (load-bearing) — enforcement mechanism specified
**Rationale**: The CRDT works without consensus because the role model prevents
concurrent conflicting writes. If two authors could legitimately produce
contradictory state for the same scope, the CRDT cannot resolve it.
**Enforcement**: INV-S8 requires that no two distinct authors have identical
(unit_type_scope, trust_domain_scope) tuples. Role assignment governance units
validate non-overlap before persisting.
**Breaks if**: Bug in role assignment validation allows overlapping scopes.
**Review**: Architect phase — trait interface must enforce at assignment time.

## A2: Deterministic solver uses fixed-point arithmetic (ppm)
**Status**: Resolved (DL-004)
**Rationale**: Solver determinism across platforms requires identical computation.
Floating point is not deterministic across architectures.
**Decision**: All solver arithmetic uses fixed-point at ppm scale (10^6 factor).
All calculations in u64/i64 — deterministic across all platforms. Division
rounds toward zero (Rust default for integer division). No floating-point in
any solver path.
**Breaks if**: ppm precision (6 decimal digits) is insufficient for some
scoring scenario. Would need higher-precision fixed-point (e.g., ppb).
**Review**: During implementer phase — verify precision is adequate for
resource ratio calculations.

## A3: Erasure coding overhead is acceptable for graph shard sizes
**Status**: Accepted (acknowledged risk)
**Rationale**: Typical composition graphs should be manageable (KBs to low MBs
per shard). Erasure coding CPU cost is proportional to shard size.
**Breaks if**: Graph shards grow to GBs (pathological composition complexity).
Would need graph sharding strategy beyond erasure coding.
**Review**: After M2 milestone (single-node compose working, can measure graph sizes).

## A4: SWIM gossip scales to target cluster sizes
**Status**: Accepted (acknowledged risk)
**Rationale**: SWIM is O(n) in membership dissemination. Works well for hundreds
to low thousands of nodes.
**Breaks if**: Target exceeds ~10,000 nodes. Would need hierarchical gossip or
federation layer.
**Review**: After M4 milestone (multi-node working, can benchmark gossip).

## A5: TPM availability is optional
**Status**: Design decision
**Rationale**: Dev/small deployments should work without hardware attestation.
TPM is a security hardening that activates when available, not a requirement.
**Breaks if**: Threat model requires mandatory hardware attestation for all
environments. Would need to rethink progressive security model.
**Review**: During adversary review of security model.

## A6: K8s configs are mappable to taba units
**Status**: Accepted (acknowledged risk)
**Rationale**: Core K8s resources (Deployment, Service, ConfigMap) have clear
unit equivalents. CRDs are unbounded and may not map.
**Breaks if**: Majority of real-world K8s deployments rely heavily on CRDs that
have no taba equivalent. Migration tool would need extension mechanism.
**Review**: Phase 5b (K8s migration tool).

## A7: Unit declarations are human-authorable in TOML
**Status**: Design decision (adoption-critical)
**Rationale**: Dockerfile succeeded because any developer could read/write one.
taba unit declarations must be similarly approachable for simple cases.
**Breaks if**: The type system makes simple declarations complex. Would need
templating, code generation, or IDE support to compensate.
**Review**: Continuously during analyst and architect phases.

## A8: The composition graph fits in working memory per node
**Status**: Addressed — hard limits and sharding strategy defined
**Rationale**: Each node needs enough graph state to run the solver. If the
active graph exceeds available memory, the system architecture changes fundamentally.
**Mitigation**: INV-R6 enforces configurable memory limit with auto-compaction
at 80%. Phase 1-2: single-domain, memory-bounded with aggressive compaction.
Phase 3+: trust domain sharding (each node holds graphs for trust domains it
participates in). Cross-domain compositions use a forwarding protocol via gossip.
**Breaks if**: Single trust domain graph exceeds node memory even after compaction.
Would need intra-domain sharding.
**Review**: After M2 milestone — benchmark graph sizes. After M4 — validate
cross-domain forwarding protocol.

## A9: Git-native versioning is sufficient for workload lineage
**Status**: Design decision (adoption-critical)
**Rationale**: Developers already use git for version control. Making workload
unit versions map to git refs (commit SHA, tag) eliminates a separate versioning
system and makes provenance chains natural (git history IS lineage). Promotion
flow maps directly to git workflow: branch → dev, merge to main → test,
tag → prod.
**Breaks if**: Workloads need versioning that doesn't map to git (e.g., dynamically
generated configurations, runtime-modified state). Would need a complementary
versioning scheme for non-git-sourced units.
**Review**: During implementer phase — verify git ref format covers all
artifact sources (OCI images built externally may not have a git commit).

## A10: Userspace installation is viable for dev nodes
**Status**: Design decision (adoption-critical)
**Rationale**: If taba requires root to install, the developer on-ramp is too
steep. `taba init` in userspace (no root, no sudo) must produce a working
node with a narrower capability set (rootless containers, unprivileged ports,
local storage only).
**Breaks if**: Core taba functionality (WAL, gossip, erasure coding) requires
privileged system calls. Would need privilege separation or a helper daemon.
**Review**: During implementer phase — verify WAL (file I/O), gossip
(unprivileged UDP/TCP), and erasure coding (pure computation) all work in
userspace.

## A11: P2P artifact distribution scales for typical artifact sizes
**Status**: Accepted (acknowledged risk)
**Rationale**: Typical artifacts are 50MB-2GB (OCI images, binaries). P2P
distribution via content-addressed chunks is well-understood (BitTorrent
model). The peer cache avoids redundant external downloads.
**Breaks if**: Artifacts are very large (10GB+ ML models, monolithic
installers) and the P2P overhead exceeds direct download. Would need
streaming/chunked transfer with progress tracking.
**Review**: After M3 milestone — benchmark artifact distribution with
realistic sizes.

## A12: Auto-discovery correctly identifies runtime capabilities
**Status**: Accepted (acknowledged risk)
**Rationale**: Probing for Docker socket, K8s API, wasmtime binary, etc.
covers the common cases. Custom runtimes are handled by operator-declared
freeform tags.
**Breaks if**: Runtime detection is unreliable (Docker socket exists but
daemon isn't running, K8s API available but node lacks scheduling permission).
Would need health-check-style verification of discovered capabilities.
**Review**: During implementer phase — define probe contracts (what
constitutes a valid detection vs. a stale artifact).

## A13: Progressive disclosure does not create hidden complexity
**Status**: Design assumption (load-bearing)
**Rationale**: The progressive disclosure principle means every subsystem has
a simple default. The risk is that the "simple" path silently omits critical
behavior that only surfaces when the user scales up. Example: Tier 0 has no
Shamir — upgrading to Tier 1 should not require re-signing all existing units.
**Breaks if**: The upgrade path between tiers introduces breaking changes or
requires migration ceremonies that are harder than starting fresh.
**Review**: Continuously during analyst and architect phases. Each new feature
must document its progressive disclosure path and verify the upgrade is
non-destructive.

## A14: Logical clock is sufficient for causal ordering
**Status**: Design decision (load-bearing)
**Rationale**: A Lamport-style logical clock provides causal ordering without
NTP dependency. Every system action increments locally; inter-node
communication syncs via `max(local, remote) + 1`. This eliminates clock skew
as a correctness concern for signature validity, key revocation, and conflict
resolution. Wall clock is used only for duration-based operations (retention,
compliance).
**Breaks if**: The system needs partial ordering beyond happens-before (e.g.,
concurrent event detection for conflict resolution). Would need vector clocks
or hybrid logical clocks (HLC). Lamport clocks cannot detect concurrency —
they only establish "definitely before" relationships.
**Review**: During implementer phase — verify that "definitely before" is
sufficient for all correctness-critical operations (key revocation, signature
validity, supersession chains). If concurrency detection is needed, evaluate
HLC as an upgrade path.

## A15: Tombstone-based compaction preserves provenance integrity
**Status**: Design decision
**Rationale**: Tombstones retain unit identity and references while removing
content. This preserves the provenance graph shape (INV-D1) without the
storage cost of full units. For full details, the archive provides retrieval.
**Breaks if**: A correctness-critical operation requires full unit content
(not just identity/references) from a compacted unit. Would need the archive
to be mandatory (not optional) for those unit types.
**Review**: During architect phase — identify which operations require full
unit content vs. identity/references only.

## A16: Compaction eligibility determinism is achievable
**Status**: Design assumption
**Rationale**: All nodes can determine compaction eligibility from graph state
alone (task terminated, retention expired, policy superseded). These are
objective facts visible to all nodes with the same graph state. Compaction
timing varies (local pressure), but all nodes converge on the same tombstones
via CRDT merge.
**Breaks if**: Compaction eligibility depends on node-local state (e.g.,
"compact if my memory exceeds X"). That's eviction, not compaction. The
distinction must be maintained: compaction = graph-level deterministic,
eviction = node-level local.
**Review**: During implementer phase — verify all compaction rules are
derivable from graph state only.
