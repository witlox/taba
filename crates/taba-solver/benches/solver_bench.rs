//! Criterion benchmarks for taba-solver operations.
//!
//! Answers OQ-007: "How large can the active graph get before
//! performance degrades?" Benchmarks `solve`, `detect_conflicts`,
//! `detect_cycles`, and `rank_nodes` at graph sizes 10, 100, and
//! 500 units.
//!
//! All benchmarks run single-threaded, in-memory, no disk I/O.
//! `GraphSnapshot` objects are built directly (bypassing the async
//! `DefaultGraph::insert` path) for fast O(N) setup.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::black_box;
use criterion::criterion_group;
use criterion::criterion_main;
use taba_common::ClusterId;
use taba_common::DualClockEvent;
use taba_common::LogicalClock;
use taba_common::NodeId;
use taba_common::TrustDomainId;
use taba_common::UnitId;
use taba_common::ValidityWindow;
use taba_common::WallTime;
use taba_core::Capability;
use taba_core::RecoveryAction;
use taba_core::RecoveryRelationship;
use taba_core::Unit;
use taba_graph::GraphEntry;
use taba_graph::GraphSnapshot;
use taba_security::PublicKey;
use taba_security::Signature;
use taba_security::SignatureContext;
use taba_security::SignedUnit;
use taba_solver::ConflictDetector;
use taba_solver::CycleDetector;
use taba_solver::DefaultConflictDetector;
use taba_solver::DefaultCycleDetector;
use taba_solver::DefaultPlacementScorer;
use taba_solver::DefaultSolver;
use taba_solver::MembershipSnapshot;
use taba_solver::NodeHealth;
use taba_solver::PlacementScorer;
use taba_solver::Solver;
use taba_test_harness::NodeCapabilitySetBuilder;
use taba_test_harness::WorkloadUnitBuilder;

// ===========================================================================
// Constants
// ===========================================================================

/// Graph sizes to benchmark.
const SOLVER_SIZES: &[usize] = &[10, 100, 500];

// ===========================================================================
// Helpers
// ===========================================================================

/// Wraps a [`Unit`] in a [`SignedUnit`] with placeholder crypto.
const fn wrap_signed(unit: Unit) -> SignedUnit<Unit> {
    SignedUnit {
        unit,
        signature: Signature([0u8; 64]),
        context: SignatureContext {
            trust_domain_id: TrustDomainId(uuid::Uuid::nil()),
            cluster_id: ClusterId(uuid::Uuid::nil()),
            validity_window: ValidityWindow {
                lc_range: None,
                wall_time_deadline: None,
            },
        },
        signer: PublicKey([0u8; 32]),
    }
}

/// Creates a [`GraphEntry`] from a [`Unit`] with a minimal timestamp and
/// no reference edges.
fn make_entry(unit: Unit) -> GraphEntry {
    let signed = wrap_signed(unit);
    GraphEntry::from_signed_unit(
        signed,
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1000 },
            timezone: "UTC".to_string(),
        },
        BTreeSet::new(),
    )
}

/// Returns the [`UnitId`] for sequential index `i` (1-based).
fn seq_id(i: usize) -> UnitId {
    UnitId(uuid::Uuid::from_u128(
        u128::try_from(i + 1).unwrap_or(u128::MAX),
    ))
}

/// Creates a simple [`Unit::Workload`] with the given [`UnitId`].
///
/// The default builder produces a valid unit (provides `compute:http`,
/// Oci artifact, no needs).
fn make_workload(id: UnitId) -> Unit {
    Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build())
}

/// Creates a [`Unit::Workload`] that needs `storage:redis` (no provider
/// in the graph — exercises the conflict detection path).
fn make_needing_workload(id: UnitId) -> Unit {
    Unit::Workload(
        WorkloadUnitBuilder::new()
            .with_id(id)
            .with_needs(vec![Capability::new("storage", "redis")])
            .build(),
    )
}

/// Creates a [`Unit::Workload`] whose recovery relationship depends on
/// `depends_on`.
fn make_cyclic_workload(id: UnitId, depends_on: UnitId) -> Unit {
    let mut workload = WorkloadUnitBuilder::new().with_id(id).build();
    workload.recovery_relationships = vec![RecoveryRelationship {
        depends_on,
        action: RecoveryAction::DrainFirst,
    }];
    Unit::Workload(workload)
}

/// Builds a [`GraphSnapshot`] with `n` units: 80% with default
/// capabilities (placeable), 20% with unmet needs (unplaceable).
///
/// This exercises the full solver pipeline: conflict detection,
/// cycle detection, capability filtering, scoring, and placement.
fn build_solver_graph(n: usize) -> GraphSnapshot {
    let mut entries = BTreeMap::new();
    for i in 0..n {
        let id = seq_id(i);
        let unit = if n > 1 && i % 5 == 0 {
            make_needing_workload(id)
        } else {
            make_workload(id)
        };
        entries.insert(id, make_entry(unit));
    }
    GraphSnapshot::new(1, entries, BTreeMap::new())
}

/// Builds a [`GraphSnapshot`] with `n` units, where every even-indexed
/// pair forms a recovery dependency cycle.
///
/// Unit[2k] depends on Unit[2k+1], and Unit[2k+1] depends on Unit[2k],
/// giving `n / 2` cycles. The remaining unit (if `n` is odd) has no
/// dependencies.
fn build_cyclic_graph(n: usize) -> GraphSnapshot {
    let mut entries = BTreeMap::new();
    for i in 0..n {
        let id = seq_id(i);
        let unit = if i % 2 == 0 && i + 1 < n {
            // Even unit depends on the next (odd) unit.
            make_cyclic_workload(id, seq_id(i + 1))
        } else if i % 2 == 1 {
            // Odd unit depends on the previous (even) unit — closes the cycle.
            make_cyclic_workload(id, seq_id(i - 1))
        } else {
            // Last unit when n is odd: no dependencies.
            make_workload(id)
        };
        entries.insert(id, make_entry(unit));
    }
    GraphSnapshot::new(1, entries, BTreeMap::new())
}

/// Builds a [`MembershipSnapshot`] with `n` nodes, all Active with Oci
/// runtime and `env:dev` (default `NodeCapabilitySetBuilder`).
fn build_membership(n: usize) -> MembershipSnapshot {
    let nodes: Vec<_> = (0..n)
        .map(|i| {
            (
                NodeId(uuid::Uuid::from_u128(
                    u128::try_from(i + 1).unwrap_or(u128::MAX),
                )),
                NodeCapabilitySetBuilder::new().build(),
                NodeHealth::Active,
            )
        })
        .collect();
    MembershipSnapshot {
        nodes,
        generation: 1,
    }
}

/// Builds a single-node [`MembershipSnapshot`] suitable for the
/// `solve` benchmark.
fn single_node_membership() -> MembershipSnapshot {
    MembershipSnapshot::single_node(
        NodeId(uuid::Uuid::from_u128(1)),
        NodeCapabilitySetBuilder::new().build(),
    )
}

// ===========================================================================
// Benchmark: solve
// ===========================================================================

/// Benchmarks a full solver run: graph snapshot + single-node membership.
///
/// The graph has 80% placeable units (default, no needs) and 20% units
/// with unmet needs (exercising conflict detection). The single node
/// has Oci runtime and `env:dev`, so all placeable units land on it.
fn bench_solve(c: &mut Criterion) {
    let solver = DefaultSolver::new();
    let membership = single_node_membership();
    let mut group = c.benchmark_group("solve");

    for &n in SOLVER_SIZES {
        let graph = build_solver_graph(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let result = solver.solve(black_box(&graph), black_box(&membership));
                black_box(result);
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: detect_conflicts
// ===========================================================================

/// Benchmarks conflict detection on a graph of N units.
///
/// The graph has 20% of units with unmet needs (`storage:redis`),
/// so the detector iterates all units and their provides. This is
/// O(N^2 * needs * provides) in the general case.
fn bench_detect_conflicts(c: &mut Criterion) {
    let detector = DefaultConflictDetector::new();
    let mut group = c.benchmark_group("detect_conflicts");

    for &n in SOLVER_SIZES {
        let graph = build_solver_graph(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let conflicts = detector.detect_conflicts(black_box(&graph));
                black_box(conflicts);
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: detect_cycles
// ===========================================================================

/// Benchmarks cycle detection on a graph of N units (some with cycles).
///
/// Every even-indexed pair forms a recovery dependency cycle, giving
/// `N / 2` cycles. The detector builds an adjacency list (O(N + E))
/// and performs DFS (O(N + E)), where E = N (two edges per pair).
fn bench_detect_cycles(c: &mut Criterion) {
    let detector = DefaultCycleDetector::new();
    let mut group = c.benchmark_group("detect_cycles");

    for &n in SOLVER_SIZES {
        let graph = build_cyclic_graph(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let cycles = detector.detect_cycles(black_box(&graph));
                black_box(cycles);
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: rank_nodes
// ===========================================================================

/// Benchmarks placement scoring for a unit against N candidate nodes.
///
/// The scorer iterates all N nodes (O(N) scoring), then sorts by score
/// descending with ties broken by `NodeId` ascending (O(N log N)).
/// All nodes have Oci runtime and `env:dev`, so all are eligible.
fn bench_rank_nodes(c: &mut Criterion) {
    let scorer = DefaultPlacementScorer::new();
    let graph = GraphSnapshot::new(0, BTreeMap::new(), BTreeMap::new());
    let unit = make_workload(UnitId(uuid::Uuid::new_v4()));
    let mut group = c.benchmark_group("rank_nodes");

    for &n in SOLVER_SIZES {
        let membership = build_membership(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let ranked =
                    scorer.rank_nodes(black_box(&unit), black_box(&graph), black_box(&membership));
                black_box(ranked);
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Entry point
// ===========================================================================

criterion_group!(
    benches,
    bench_solve,
    bench_detect_conflicts,
    bench_detect_cycles,
    bench_rank_nodes,
);
criterion_main!(benches);
