# Findings: Adversarial Analyst Pass

Adversarial review of domain model additions from analyst session:
environment progression, runtime agnosticism, logical clock, workload lifecycle,
graph compaction, cross-trust-domain forwarding, progressive disclosure.

---

## FINDING-300: Lamport Clock Cannot Verify Causal Revocation Ordering
**Severity**: Critical
**Category**: Correctness
**Component**: INV-S3, INV-T1, INV-T2, A14
**Scenario**: Node A signs unit U at logical clock L=100. Node B revokes
the signing author's key at logical clock L=50. These are concurrent events
(no causal relationship). On merge, both nodes sync clocks. INV-S3 requires
checking "key was not revoked before the unit's creation logical clock."
But Lamport clocks only establish happens-before for causally related events.
L=100 > L=50 does NOT mean "U was signed after revocation" — the events
happened on different nodes with no causal link. The comparison is meaningless
for concurrent events.
**Impact**: Key revocation check (INV-S3) cannot be correctly implemented
with Lamport clocks alone. Units signed concurrently with revocation may
be incorrectly accepted or rejected. A14 acknowledges this risk but defers
resolution.
**Recommendation**: Evaluate Hybrid Logical Clocks (HLC) which combine
Lamport ordering with wall-clock bounds, giving "definitely before" semantics
with bounded uncertainty. Alternatively, use gossip-propagated revocation
with a grace window (already mentioned in domain model) and accept the
window as the security boundary.
**Traces to**: INV-S3, INV-T1, INV-T2, A14

---

## FINDING-301: Ephemeral Data Removal Breaks Provenance Chain
**Severity**: Critical
**Category**: Correctness
**Component**: INV-D4, INV-D1, INV-G2
**Scenario**: Bounded task T produces ephemeral data D. Workload W2 consumes
D and produces output O. T terminates, D is fully removed (no tombstone per
INV-D4 default). Later, a provenance query on O traverses: O → W2 → D → T.
But D is gone — the chain is broken. INV-D1 requires "unbroken" provenance.
INV-G2 says "tombstones preserve provenance structure" but INV-D4 explicitly
says ephemeral data gets NO tombstone by default.
**Impact**: Any downstream data with provenance through ephemeral data has a
broken chain. This is a structural violation of INV-D1. Governance can mandate
tombstones, but the default path creates the violation.
**Recommendation**: Ephemeral data that has been consumed by other units MUST
be tombstoned (not fully removed). Full removal is only safe when no
downstream provenance references exist. Add a reference check before removal.
**Traces to**: INV-D4, INV-D1, INV-G2

---

## FINDING-302: Spawned Task Signature Authority Undefined
**Severity**: Critical
**Category**: Correctness / Security
**Component**: INV-S3, INV-W4
**Scenario**: Service S (authored by author A at LC 100) spawns bounded task
T at runtime (LC 200). INV-W4 says T "inherits the parent's trust context."
INV-S3 requires every unit to be "signed by an author with valid scope."
Question: who signs T? If A re-signs T, when does this happen? The service
is running on a node — does the node hold A's private key? If T inherits S's
signature, then T's logical clock (200) doesn't match S's signing clock (100)
— is the signature valid for a unit created later?
**Impact**: The spawning mechanism is structurally undefined. Either: (a) the
node must hold author private keys (security risk), (b) signatures must be
delegated (new mechanism needed), or (c) spawned tasks use a different
signature model. Without resolution, spawning cannot be implemented.
**Recommendation**: Define a delegation token model: when a service is placed,
the author signs a time-bounded delegation token. The node uses this token to
sign spawned tasks on behalf of the author. Delegation tokens have a logical
clock range and are revocable independently.
**Traces to**: INV-S3, INV-W4

---

## FINDING-303: Compaction Determinism Breaks Under Wall Clock Skew
**Severity**: High
**Category**: Consistency
**Component**: INV-G1, INV-T2, A16
**Scenario**: Data unit D has retention = 30 days, created at wall time T.
Node 1's wall clock shows T+31d (expired). Node 2's wall clock shows T+30d
(not expired, 1 day clock skew). Node 1 compacts D into a tombstone.
Node 2 still considers D live. INV-G1 says "given the same graph state, all
nodes agree on eligibility." But wall clock skew means they DON'T have the
same temporal view. INV-T2 says wall clock is authoritative for retention.
**Impact**: Nodes disagree on compaction eligibility for time-based retention.
The tombstone (monotonic) eventually wins on merge, but during the skew
window, nodes have inconsistent graph state. A unit might be tombstoned on
one node while another node is still serving queries against it.
**Recommendation**: Add a compaction grace period for retention-based
eligibility (e.g., retention + max_expected_clock_skew). Or use logical
clock for compaction timing and only use wall clock for the retention
_declaration_ (converting to logical clock at creation time).
**Traces to**: INV-G1, INV-T2, A16

---

## FINDING-304: Tier 0 Single Key Is All Authority — No Recovery
**Severity**: High
**Category**: Security
**Component**: domain-model (Root Key), FM-18
**Scenario**: Solo developer uses `taba init` (Tier 0). The generated key
is simultaneously: node identity, author identity, and root key. Developer's
laptop is stolen. Attacker has ALL authority — can author units, create trust
domains, revoke keys, and re-assign roles. FM-18's break-glass escape hatch
(root key re-assigns roles) doesn't work because the root key IS the
compromised key. There is no higher authority to fall back to.
**Impact**: Total authority loss in Tier 0. No recovery path without
starting from scratch. The "backup of Tier 0 root key" scenario in
trust-domain.feature assumes the backup exists and isn't also compromised.
**Recommendation**: Tier 0 should generate two keys: an operational key
(node + author) and a recovery key (stored offline, never used in normal
operation). The recovery key can revoke the operational key. This adds one
setup step but provides a recovery path.
**Traces to**: domain-model (Root Key Tier 0), FM-18

---

## FINDING-305: Stale Cross-Domain Cache Violates Bilateral Authorization
**Severity**: High
**Category**: Security
**Component**: INV-X1, INV-X3, FM-27
**Scenario**: Domain A has cached cross-domain query results from Domain B.
Domain B revokes the bilateral policy (governance update). Bridge is down.
INV-X3 says serve stale cache (fail-open). INV-X1 says bilateral policy is
required. The stale cache was created BEFORE revocation, but serving it AFTER
revocation violates INV-X1. Domain A's workloads continue accessing
Domain B's data without authorization.
**Impact**: Unbounded authorization bypass during bridge downtime. Window =
bridge outage duration (could be hours). FM-27 acknowledges this but the
default (fail-open) creates the vulnerability.
**Recommendation**: Cross-domain cache entries must carry a "policy valid at"
logical clock. On cache hit, verify local bilateral policy still exists
(local check, no bridge needed). Only the remote side's policy staleness is
the true exposure. This narrows the window significantly.
**Traces to**: INV-X1, INV-X3, FM-27

---

## FINDING-306: Solver Resource Ranking Non-Deterministic Across Nodes
**Severity**: High
**Category**: Consistency
**Component**: INV-C3, INV-N3
**Scenario**: Resource snapshots are dynamic (reported periodically via
gossip). Node 1 receives prod-A's resource snapshot at time T, showing 8GB
free. Node 2 receives prod-A's snapshot at time T+5s, showing 6GB free
(workload started). Both nodes run the solver. INV-C3 requires identical
placement given identical inputs. But resource snapshots are NOT synchronized
— each node has a different view of resource state.
**Impact**: Two nodes may rank nodes differently for resource-based placement,
producing different placements. This violates INV-C3 (solver determinism).
The violation is transient (resources converge) but creates a window where
different nodes disagree on placement.
**Recommendation**: Resource snapshots must be versioned (logical clock
stamped). The solver uses a specific resource snapshot version as input
(recorded in decision trail). Or: resource ranking is advisory only and
doesn't affect placement determinism — the solver uses capabilities (hard)
for placement and resources (soft) only for tie-breaking within a
deterministic algorithm that doesn't depend on exact values.
**Traces to**: INV-C3, INV-N3

---

## FINDING-307: Policy Supersession Breaks Environment Independence
**Severity**: High
**Category**: Correctness
**Component**: INV-E1, INV-E2, INV-C7
**Scenario**: Promotion policy P1 promotes workload W to env:test AND
env:prod. P1 is superseded by P2 which only promotes W to env:test. INV-E2
says promotions are "cumulative and non-exclusive" — environments are
independent. But INV-C7's supersession replaces the ENTIRE policy (P1 → P2).
P2 only covers env:test. Is W still valid in env:prod? INV-E1 requires a
promotion policy for each environment. P1 no longer applies (superseded).
P2 doesn't cover env:prod. W has no valid promotion for env:prod.
**Impact**: Policy supersession can inadvertently de-promote workloads from
environments that the new policy forgot to include. INV-E2's promise of
independence is violated by INV-C7's monolithic supersession.
**Recommendation**: Either: (a) promotion policies are per-environment
(not batched), so superseding one environment's promotion doesn't affect
another. Or (b) supersession must explicitly list which environments are
affected, with unmentioned environments retaining prior promotion.
**Traces to**: INV-E1, INV-E2, INV-C7

---

## FINDING-308: Bridge Node Has Unscoped Read Access to Both Domains
**Severity**: High
**Category**: Security
**Component**: INV-X2, INV-X4, FM-26
**Scenario**: Bridge node B participates in domains A and B. B holds full
graph shards for BOTH domains. A compromised bridge (FM-26) can read all
units in both domains — workloads, data declarations, policies, governance.
Bilateral policy (INV-X1) only controls composition/query access. It does NOT
control what graph data the bridge node stores locally. The bridge has raw
access to both graphs by virtue of domain membership.
**Impact**: In a multi-org setup, a compromised bridge exposes Organization
A's entire internal graph to an attacker targeting Organization B. The
bilateral policy creates a false sense of access control — it controls
solver behavior, not node-level data access.
**Recommendation**: Consider graph-level encryption per trust domain. Bridge
nodes hold encrypted shards for domains they bridge. Decryption only occurs
for authorized forwarding queries (key released per-query from the owning
domain). This is significant complexity — may be Phase 4+.
**Traces to**: INV-X2, INV-X4, FM-26

---

## FINDING-309: Local-Only Data Classification Bypass
**Severity**: Medium
**Category**: Security
**Component**: INV-D5
**Scenario**: Author creates local-only data and declares classification =
"public" when the actual data is PII. INV-D5 requires policy for local-only
data above "public." But classification is self-declared by the author.
Local-only data never enters the graph, so there's no verification mechanism.
The author can bypass the entire taint propagation model (INV-S4) by lying
about classification and keeping data local.
**Impact**: Data governance is honor-system for local-only data. Any author
can circumvent classification controls by declaring data as public + local.
**Recommendation**: Accept as a known limitation with documentation: local-
only data is outside taba's governance model. Governance-mandated audit
requirements can restrict local-only usage in regulated trust domains. The
ultimate enforcement is that downstream data entering the graph inherits
taint from its inputs — if the classified data was truly consumed, the output
enters the graph and gets classified.
**Traces to**: INV-D5, INV-S4

---

## FINDING-310: Git-Native Versioning Doesn't Cover All Workload Sources
**Severity**: Medium
**Category**: Correctness / Operational
**Component**: A9, domain-model (Workload Unit)
**Scenario**: Admin deploys commercial software (SQL Server MSI). There is
no git repo. The version field is specified as "a git ref (commit SHA or
tag)." What goes in the version field for non-git-sourced workloads? OCI
images from third-party registries, manually downloaded binaries, operator-
authored TOML without a git workflow.
**Impact**: Version field is mandatory for provenance but the git-native
assumption doesn't hold for all workload types. A9 acknowledges this risk
("OCI images built externally may not have a git commit").
**Recommendation**: Version field should be "content-addressable identifier"
with git ref as the default convention. Alternatives: artifact digest (for
OCI), package version string (for native), or operator-declared version.
Provenance links by version field regardless of format.
**Traces to**: A9, domain-model (Workload Unit)

---

## FINDING-311: Environment Tags Are Unverified Soft Convention
**Severity**: Medium
**Category**: Operational
**Component**: INV-E1, INV-N4
**Scenario**: Operator configures a dev laptop with env:prod (mistake or
malice). INV-N4 says custom tags are "treated identically to auto-discovered
capabilities." The solver happily places prod workloads on the dev laptop.
There is no verification that an env:prod node actually meets production
requirements (redundancy, security, clock quality).
**Impact**: Environment-based placement is only as reliable as the operator's
tag configuration. Mistagging creates silent misplacement.
**Recommendation**: Governance can define minimum capability requirements
per environment tag (e.g., env:prod requires privilege:root + clock:ntp +
storage:encrypted). The solver validates that the node meets the minimum
before accepting the tag. This is enforcement, not just convention.
**Traces to**: INV-E1, INV-N4

---

## FINDING-312: Spawned Task Declassification Authority Ambiguous
**Severity**: Medium
**Category**: Security
**Component**: INV-S9, INV-W4
**Scenario**: Service S (author A, data-steward scope) spawns task T. T
inherits A's trust context. T consumes classified data and needs
declassification. INV-S9 requires 2 distinct authors (policy + data-steward).
Is T considered "authored by A" (making A effectively both the data-steward
signer via inheritance and a potential policy co-signer)? Or is T an
independent unit that happens to inherit A's scope? If the former,
single-author declassification could be disguised as spawned-task
delegation.
**Impact**: Declassification multi-party requirement (INV-S9) may be
bypassable through spawning if the inherited authority counts as a
separate signer.
**Recommendation**: Spawned tasks CANNOT initiate declassification. Only
directly-authored policy units (not spawned) can participate in multi-party
declassification signing. Spawned tasks inherit operational authority, not
governance authority.
**Traces to**: INV-S9, INV-W4

---

## FINDING-313: Cross-Domain Forwarding Bridge Bottleneck
**Severity**: Medium
**Category**: Scalability
**Component**: INV-X4, INV-X6, cross-context/interactions.md
**Scenario**: 50 workloads in domain A consume capabilities from domain B.
All queries route through the single bridge node. The bridge handles 50
forwarding queries per solver run. During high-churn periods (many graph
mutations), queries multiply. Bridge node becomes CPU/network bottleneck.
**Impact**: Cross-domain composition latency scales linearly with bridge
query count. Single bridge = single point of throughput.
**Recommendation**: Allow multiple bridge nodes per domain pair. Forwarding
queries are load-balanced across bridges. Cache TTL reduces query frequency.
Consider pre-computing cross-domain "materialized views" for frequently-
queried capabilities.
**Traces to**: INV-X4, INV-X6

---

## FINDING-314: Fleet Refresh Governance Command Has No Rate Limit
**Severity**: Medium
**Category**: Operational
**Component**: domain-model (Governance Unit / OperationalCommand)
**Scenario**: Author with governance scope issues rapid-fire
refresh-capabilities commands (bug in automation or deliberate DoS). Each
command propagates via gossip, causing every node to re-probe capabilities.
5 commands in 10 seconds = 5 fleet-wide re-probes. Gossip traffic and CPU
usage spike across the cluster.
**Impact**: Self-inflicted denial of service via governance commands.
No dedup or rate limiting specified for operational commands.
**Recommendation**: Operational commands should be deduplicated: if a
refresh-capabilities command is already in flight (within gossip convergence
window), subsequent commands are dropped or queued. Rate limit: one
operational command of each type per configurable window (e.g., 60 seconds).
**Traces to**: domain-model (Governance Unit)

---

## FINDING-315: INV-W1 Language Conflicts with INV-S3 on Key Revocation
**Severity**: Low
**Category**: Correctness
**Component**: INV-W1, INV-S3
**Scenario**: INV-W1 says services are valid "until explicitly terminated
or their author's key is revoked." INV-S3 says units signed BEFORE
revocation remain valid. These contradict: if author A's key is revoked,
INV-W1 implies service S (authored by A) becomes invalid. INV-S3 says
S remains valid because it was signed before revocation.
**Impact**: Ambiguity in the spec. Implementer may interpret either way.
**Recommendation**: Clarify INV-W1: "Services are valid indefinitely. Key
revocation prevents new units from being authored but does NOT invalidate
existing services signed before revocation (per INV-S3)."
**Traces to**: INV-W1, INV-S3
