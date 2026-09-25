# BDD Completeness Plan

## Current State

- 20 feature files, 268 scenarios, 1720 unique step texts
- **30 steps** have real implementations (smoke.rs + critical.rs)
- **1677 steps** are no-ops in common.rs (function called, no assertion)
- **20 step files** (one per feature) are empty 8-line stubs
- The BDD infrastructure compiles and all 268 scenarios "pass" —
  but only 1 scenario (@smoke) has real assertions on observable artifacts

## Goal

Replace the 1677 no-op step definitions with real implementations
that exercise actual code and assert on observable artifacts. The
step definitions should call into the real taba crates
(taba-graph, taba-solver, taba-security, taba-node, taba-observe,
taba-erasure, taba-gossip) and verify that the features work as
described.

## Constraints

1. **Real code, not mocks**: Steps must call actual functions and
   assert on real return values (graph state, solver results, WAL
   entries, error messages, etc.)
2. **No new production code**: If a feature step requires
   functionality that doesn't exist yet, implement a MINIMAL
   version in the step definition (e.g., create a unit and insert
   it, then assert on graph state). The follow-up TDD step will
   move this into production code with proper tests.
3. **No ambiguous matches**: Each step must match exactly ONE
   definition. Remove the corresponding no-op from common.rs when
   adding a real implementation.
4. **Parallelizable**: Group features by related functionality so
   multiple implementers can work simultaneously.
5. **Each step file is self-contained**: A feature's step
   definitions live in `crates/taba-acceptance/tests/steps/<name>.rs`.
   Shared steps (background, common assertions) stay in
   `common.rs` (real) or `critical.rs` (real).

## Architecture: What the TabaWorld provides

```
TabaWorld {
    graph: Arc<DefaultGraph>       // insert, snapshot, stats, archive, compact, orphaned_policies, wal
    solver: DefaultSolver          // solve(&snapshot, &membership) -> SolverResult
    runtime: SimulatedRuntime      // start, stop, check_state
    health: DefaultHealthReporter  // set_mode, set_units_running, set_units_failed
    mode: DefaultModeManager       // current_mode, transition
    scope_checker: DefaultScopeChecker  // add_assignment, check_author_scope, validate_scope_uniqueness
    verifier: DefaultVerifier      // add_key, verify, revoke_key
    key_pair: KeyPair              // public_key, signing_key
    units: BTreeMap<String, Unit>  // by name
    last_solver_result: Option<SolverResult>
    last_graph_error: Option<GraphError>
    membership: MembershipSnapshot
    alerts: Vec<String>
    events: Vec<String>
    ceremony_id, ceremony_state, ceremony_shares_received, ...
    placement_on_node: BTreeMap<String, NodeId>
}

TabaWorld methods:
    register_author(name) -> author_id
    author_id_by_name(name) -> AuthorId
    register_trust_domain(name)
    trust_domain_id_by_name(name) -> TrustDomainId
    store_unit(name, unit)
    unit_by_name(name) -> Option<&Unit>
    unit_id_by_name(name) -> Option<UnitId>
    reset_errors()
    add_alert(alert)
    add_event(event)
```

### Key assertion patterns

- **Unit accepted**: `assert!(world.last_graph_error.is_none())`
- **Unit rejected**: `assert!(world.last_graph_error.is_some())`
- **Unit state**: `assert_eq!(unit.header().state, UnitState::Declared)`
- **WAL entry**: `world.graph.wal().lock().replay().iter().any(|e| matches!(e, WalEntry::Merged { .. }))`
- **Solver placement**: `assert!(!world.last_solver_result.unwrap().placements.is_empty())`
- **Solver conflict**: `assert!(!world.last_solver_result.unwrap().conflicts.is_empty())`
- **Graph stats**: `world.graph.stats().active_units == N`
- **Orphaned policies**: `world.graph.orphaned_policies().len() == N`
- **Scope check**: `world.scope_checker.check_author_scope(&author, kind, &td).is_ok()`
- **Taint**: `taint_computer.compute_taint(&data_unit_id).unwrap()`
- **Classification**: `match unit.classification { Classification::Pii => ... }`

## Implementation Plan (6 parallel batches)

### Batch A: unit-authoring (16) + composition (9) + placement (10) = 35 scenarios

**Files**: `unit_authoring.rs`, `composition.rs`, `placement.rs`

**Key steps to implement:**
- `alice authors a workload unit "X" with:` → parse table, create real WorkloadUnit, store in world
- `alice authors a bounded task unit "X":` → parse table, create BoundedTask with validity window
- `alice authors a service workload unit "X":` → parse table, create Service workload
- `alice authors workload unit "X" at version "Y" (git commit)` → create unit with version
- `bob authors a data unit "X" with:` → parse table, create real DataUnit (already in critical.rs)
- `alice signs the unit binding trust_domain "X" and cluster "Y"` → mark signed (already in smoke.rs)
- `the unit is submitted for graph merge` → graph.insert (already in smoke.rs)
- `the unit is accepted into the composition graph` → assert no error (already in smoke.rs)
- `the unit is rejected with error "X"` → assert error matches
- `the unit state is "X"` → assert state (already in smoke.rs)
- `the WAL contains a Merged("X") entry` → assert WAL (already in smoke.rs)
- `the composition graph does not contain "X"` → assert not in graph
- `the capability "X" has purpose qualifier "Y"` → assert on unit's capabilities
- `the solver evaluates composition of "X" and "Y"` → snapshot + solve
- `the solver evaluates placement for "X"` → snapshot + solve
- `the solver places "X" on "Y"` → assert placement.node == Y
- `the composition succeeds` → assert no conflicts
- `the composition fails closed with conflict "X"` → assert conflict exists
- `the composition is blocked with unmatched need "X"` → assert unplaceable

**Existing code to leverage:**
- `WorkloadUnitBuilder` (taba-test-harness)
- `DataUnitBuilder` (taba-test-harness)
- `PolicyUnitBuilder` (taba-test-harness)
- `NodeCapabilitySetBuilder` (taba-test-harness)
- `DefaultGraph::insert`, `snapshot`, `stats`, `archive`, `compact`
- `DefaultSolver::solve` returns `SolverResult { placements, unplaceable, conflicts }`
- `DefaultScopeChecker::check_author_scope`, `add_assignment`
- `DefaultVerifier::add_key`, `verify`

### Batch B: ceremony (16) + trust-domain (15) + security-enforcement (14) = 45 scenarios

**Files**: `ceremony.rs`, `trust_domain.rs`, `security_enforcement.rs`

**Key steps to implement:**
- `an operator initiates a Shamir ceremony` → create ceremony manager, store ceremony_id
- `the ceremony is configured with total_shares=N and threshold=N` → set ceremony params
- `share holder "X" submits share N of N` → add share to ceremony
- `the ceremony enters "X" state` → assert ceremony_state
- `the ceremony records N of N required shares received` → assert ceremony_shares_received
- `the witness confirms and the ceremony is finalized` → complete ceremony
- `the root key signs the first TrustDomain governance unit "X"` → create signed governance unit
- `the root Ed25519 keypair is reconstructed` → assert ceremony completed
- `the ceremony is cancelled and share material zeroized` → cancel ceremony
- `alice submits a TrustDomain governance unit "X" listing required signers [...]` → create TrustDomainDef
- `alice has governance scope in "X"` → register author with governance scope
- `carol can create workload units in "X"` → scope check passes
- `carol cannot create policy units in "X"` → scope check fails
- `carol attempts to create a workload unit in "X"` → insert, expect rejection
- `the unit is rejected with error "ScopeViolation: ..."` → assert error
- `combined-report inherits classification "PII"` → taint computation
- `declass-002 reduced "X" from "PII" to "internal"` → declassification

**Existing code to leverage:**
- `taba_security::ceremony::DefaultCeremonyManager` (start, add_share, complete)
- `taba_security::DefaultSoloBootstrap` (Tier 0)
- `taba_security::DefaultScopeChecker` (check_author_scope, validate_scope_uniqueness)
- `taba_security::DefaultTaintComputer` (compute_taint, validate_declassification)
- `taba_security::DefaultVerifier` (add_key, verify, revoke_key)

### Batch C: data-lineage (18) + data-retention (8) + compaction (15) = 41 scenarios

**Files**: `data_lineage.rs`, `data_retention.rs`, `compaction.rs`

**Key steps to implement:**
- `workload "X" consumed "Y" and produced "Z"` → create 3 units with provenance chain
- `workload "X" produces data unit "Y"` → create unit with provenance
- `the provenance chain is: X -> Y -> Z` → assert chain via graph.traverse_provenance
- `the classification lattice is: public(1) < internal(2) < confidential(3) < PII(4)` → assertion only
- `taint propagation compares classifications` → run taint computer
- `the chain returns: X -> Y (tombstoned) -> Z` → assert provenance with tombstone
- `data unit "X" declares retention "Y" created at "Z"` → create data unit with retention
- `data unit "X" is in Locked state` → set locked state
- `data unit "X" is eligible for compaction` → assert compaction eligibility
- `the compaction scan runs` → graph.compact()
- `the graph memory limit is set to NMB per node` → set memory limit
- `compaction of non-mandatory-archive units proceeds normally` → compact and verify
- `"X" is fully removed from the graph (no tombstone)` → assert not in graph
- `"X" is tombstoned (NOT fully removed)` → assert archived, not removed
- `governance units are never compacted` → compact, verify governance still present

**Existing code to leverage:**
- `DefaultGraph::traverse_provenance` (provenance chain)
- `DefaultGraph::compact` (compaction)
- `DefaultGraph::archive` (tombstone)
- `taba_core::RetentionChecker` (expiry)
- `taba_core::Classification` (lattice)
- `taba_security::DefaultTaintComputer` (taint)

### Batch D: node-lifecycle (13) + operational-modes (10) + recovery (10) = 33 scenarios

**Files**: `node_lifecycle.rs`, `operational_modes.rs`, `recovery.rs`

**Key steps to implement:**
- `a N-node cluster [...]` → create membership snapshot with N nodes
- `the following nodes in the cluster:` → parse table, create membership
- `node "X" is in Normal/Degraded/Recovery operational mode` → set mode
- `node "X" is Active/Suspected` → set health
- `node "X" goes offline` → set health to Suspected/Failed
- `node "X" comes back online` → set health to Active
- `node "X" transitions to Degraded operational mode` → mode.transition
- `node "X" refuses new placements` → assert mode gating
- `node "X" detects WAL corruption` → set wal_failure
- `the node checks: is alice's key revoked in the local graph? (yes)` → verifier check
- `u-child is written to WAL as Pending(u-child, missing_refs=[u-parent])` → assert WAL entry
- `u-child is promoted: WAL records Promoted(u-child)` → assert WAL entry
- `wl-api crashes on "n-003" and the node reports failure` → runtime.stop
- `wl-api is placed on one of [...] based on solver scoring` → assert placement

**Existing code to leverage:**
- `DefaultModeManager` (current_mode, transition)
- `DefaultHealthReporter` (set_mode, set_units_running, set_units_failed, set_wal_failure)
- `SimulatedRuntime` (start, stop, check_state)
- `MembershipSnapshot` (multi-node)
- `DefaultGraph::wal` (WAL entries)
- `DefaultSolver::solve` (placement)

### Batch E: network-partition (9) + conflict-resolution (12) + cross-domain (16) = 37 scenarios

**Files**: `network_partition.rs`, `conflict_resolution.rs`, `cross_domain.rs`

**Key steps to implement:**
- `a 5-node cluster split into side-A [...] and side-B [...]` → two membership snapshots
- `CRDT merge on all 5 nodes produces identical graph state` → merge, assert equality
- `a network partition separates side-B from side-A` → set partition state
- `policy-admin can resume authoring after partition heals` → merge memberships
- `carol authors a policy unit "X" resolving "Y"` → create PolicyUnit
- `the policy is submitted for graph merge` → graph.insert
- `the policy is accepted into the composition graph` → assert no error
- `the policy is rejected with error "..."` → assert error
- `policy-v1 is marked as superseded (not deleted)` → supersede, assert revoked but present
- `the solver uses "X" for conflict "Y"` → assert active policy
- `governance designates "bridge-1" as authorized bridge to "partner-payments"` → create governance unit
- `acme-1 queries capabilities of "external-vendor"` → query cross-domain
- `the solver finds cross-domain advertisement from "X"` → assert discovery
- `the composition fails closed (INV-S2 across boundaries)` → assert conflict

**Existing code to leverage:**
- `DefaultMembershipView` (merge, apply_membership_change)
- `DefaultGraph::merge` (CRDT merge)
- `DefaultGraph::supersede` (policy chain)
- `DefaultConflictDetector` (conflict detection)
- `taba_gossip::membership::MemberInfo` (cross-domain)

### Batch F: observability (15) + runtime-matching (15) + spawned-tasks (24) + compliance-audit (8) = 62 scenarios

**Files**: `observability.rs`, `runtime_matching.rs`, `spawned_tasks.rs`, `compliance_audit.rs`

**Key steps to implement:**
- `a decision trail entry is recorded in the graph:` → record trail
- `the Prometheus endpoint exposes ...` → assert metrics
- `a node is configured with an alerting webhook URL` → set alert config
- `the node monitors "X" via OS-level process check` → set health check
- `dev-laptop matches via runtime:oci-rootless` → assert capability match
- `compute-heavy is placed on prod-1 (most available memory, lowest load)` → assert placement
- `alice pre-signed a delegation token for "X" on "Y"` → create delegation token
- `bounded task "X" is running (spawned via delegation token)` → create bounded task
- `cleanup-temp attempts to spawn "deep-task" (would be depth 5)` → assert spawn depth check
- `the governance block prevents spawned tasks from creating policy units` → assert rejection
- `an auditor queries the full lineage of "X"` → traverse provenance
- `the audit trail shows N RoleAssignment governance units` → assert trail count
- `the supersession chain is: X -> Y -> Z` → assert policy chain

**Existing code to leverage:**
- `DefaultDecisionTrailRecorder` (record, query)
- `taba_observe::Events` (emit_typed, drain)
- `DefaultHealthReporter` (set health check config)
- `taba_solver::CapabilityFilter` (runtime matching)
- `taba_solver::DefaultPlacementScorer` (scoring)
- `taba_security::DefaultDelegationManager` (tokens)
- `taba_node::DefaultTaskSpawner` (spawn, governance block)
- `DefaultGraph::traverse_provenance` (lineage)
- `DefaultGraph::orphaned_policies` (audit)

## Execution Order

1. **Save this plan** to `specs/bdd-completeness-plan.md`
2. **Batch A** (unit-authoring + composition + placement) — foundation,
   other batches depend on these step patterns
3. **Batches B-F** in parallel — each is independent once Batch A
   establishes the shared step patterns
4. **After all batches**: remove all remaining no-ops from common.rs,
   run full BDD suite, verify 268 scenarios pass with real assertions

## What "real assertions" means per batch

| Batch | Real assertions |
|-------|----------------|
| A | graph.insert returns Ok/Err, unit.header().state, WAL entries, solver placements, conflicts |
| B | ceremony state transitions, scope check results, taint computation, verifier rejection |
| C | provenance chain length, classification values, compaction results, archive state |
| D | mode transitions, health status, WAL entries (Pending/Promoted), runtime state |
| E | CRDT merge equality, policy supersession, conflict detection, orphaned policies |
| F | decision trail entries, capability filter results, delegation token validation, spawn depth |

## Follow-up: TDD→Impl

After BDD completeness, the follow-up step is to identify any gaps
where the BDD steps required functionality that doesn't exist in
production code. For each gap:

1. Write a failing unit test (`scenario_<context>_<behavior>`)
2. Implement the minimal production code to make it pass
3. Verify the BDD scenario still passes

This ensures the BDD scenarios are not just testing test code —
they're testing real production functionality.
