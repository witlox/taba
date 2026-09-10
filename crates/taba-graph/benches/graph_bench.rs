//! Criterion benchmarks for taba-graph operations.
//!
//! Answers OQ-007: "How large can the active graph get before
//! performance degrades?" Benchmarks insert, merge, snapshot,
//! `query_get`, `query_provenance`, and `memory_estimate` at graph
//! sizes 10, 100, 1000, and 5000 units.
//!
//! All benchmarks run single-threaded, in-memory, no disk I/O.
//! Graph setup uses direct state insertion (bypassing the async
//! `Graph::insert` path) to keep setup time O(N) rather than O(N^2).
//! The benchmarked routines call the real `DefaultGraph` methods.

use std::collections::BTreeSet;

use criterion::BatchSize;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::black_box;
use criterion::criterion_group;
use criterion::criterion_main;
use taba_common::ClusterId;
use taba_common::DualClockEvent;
use taba_common::LogicalClock;
use taba_common::TrustDomainId;
use taba_common::UnitId;
use taba_common::ValidityWindow;
use taba_common::WallTime;
use taba_core::Provenance;
use taba_core::Unit;
use taba_graph::DefaultGraph;
use taba_graph::Graph;
use taba_graph::GraphDelta;
use taba_graph::GraphEntry;
use taba_graph::GraphQuery;
use taba_security::PublicKey;
use taba_security::Signature;
use taba_security::SignatureContext;
use taba_security::SignedUnit;
use taba_test_harness::DataUnitBuilder;
use taba_test_harness::WorkloadUnitBuilder;
use tokio::runtime::Runtime;

// ===========================================================================
// Constants
// ===========================================================================

/// Graph sizes to benchmark.
const SIZES: &[usize] = &[10, 100, 1000, 5000];

/// Very large memory limit so the graph never hits INV-R6 during benchmarks.
const MEMORY_LIMIT: u64 = u64::MAX;

// ===========================================================================
// Helpers
// ===========================================================================

/// Wraps a [`Unit`] in a [`SignedUnit`] with placeholder crypto.
///
/// Mirrors the helper used throughout taba-graph's internal tests.
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

/// Creates a simple [`Unit::Workload`] with the given [`UnitId`].
///
/// The default builder produces a valid unit (provides `compute:http`,
/// Oci artifact, no needs) that passes `DefaultValidator::validate`.
fn make_workload(id: UnitId) -> Unit {
    Unit::Workload(WorkloadUnitBuilder::new().with_id(id).build())
}

/// Creates a [`Unit::Data`] with provenance pointing to `produced_by`
/// and `inputs`.
fn make_derived_data(
    id: UnitId,
    produced_by: UnitId,
    inputs: Vec<UnitId>,
    produced_at: DualClockEvent,
) -> Unit {
    Unit::Data(
        DataUnitBuilder::new()
            .with_id(id)
            .with_provenance(Provenance {
                produced_by,
                inputs,
                produced_at,
                governing_policies: Vec::new(),
            })
            .build(),
    )
}

/// Returns a minimal [`DualClockEvent`] for setup.
fn setup_clock() -> DualClockEvent {
    DualClockEvent {
        logical_clock: LogicalClock(1),
        wall_time: WallTime { millis: 1000 },
        timezone: "UTC".to_string(),
    }
}

/// Builds a [`DefaultGraph`] with `n` workload units using direct state
/// insertion.
///
/// This is O(n) total — one `recompute_memory` call at the end — instead
/// of O(n^2) if each unit went through the async `Graph::insert` path
/// (which calls `recompute_memory` per insert).
fn build_graph(n: usize) -> DefaultGraph {
    let graph = DefaultGraph::new(MEMORY_LIMIT);
    let state = graph.shared_state();
    let mut data = state
        .lock()
        .expect("graph state mutex should not be poisoned during setup");
    for i in 0..n {
        let id = UnitId(uuid::Uuid::from_u128(
            u128::try_from(i + 1).unwrap_or(u128::MAX),
        ));
        data.entries.insert(id, make_entry(make_workload(id)));
    }
    data.recompute_memory();
    data.generation = 1;
    drop(data);
    graph
}

/// Builds a [`DefaultGraph`] with `n` total units, including a depth-5
/// provenance chain for the `query_provenance` benchmark.
///
/// The chain: 5 producer workloads and 5 data units, where
/// `data_k` is produced by `workload_k` with inputs `[data_{k-1}]`
/// (or `[]` for `k == 0`). Traversing from `data_4` yields 5 links.
///
/// The remaining `n - 10` slots are filled with simple workloads.
/// Provenance-chain UUIDs start at `n + 1` to avoid collisions with
/// filler UUIDs `1..=n`.
fn build_graph_with_provenance(n: usize) -> (DefaultGraph, UnitId) {
    let graph = DefaultGraph::new(MEMORY_LIMIT);
    let state = graph.shared_state();
    let mut data = state
        .lock()
        .expect("graph state mutex should not be poisoned during setup");

    let base = u128::try_from(n + 1).unwrap_or(u128::MAX);
    let workloads: Vec<UnitId> = (0..5)
        .map(|i| UnitId(uuid::Uuid::from_u128(base + i)))
        .collect();
    let data_units: Vec<UnitId> = (0..5)
        .map(|i| UnitId(uuid::Uuid::from_u128(base + 5 + i)))
        .collect();

    // Insert producer workloads.
    for &wid in &workloads {
        data.entries.insert(wid, make_entry(make_workload(wid)));
    }

    // Build the provenance chain: data_0 (root) -> data_1 -> ... -> data_4.
    for k in 0..5 {
        let inputs = if k == 0 {
            Vec::new()
        } else {
            vec![data_units[k - 1]]
        };
        let unit = make_derived_data(data_units[k], workloads[k], inputs, setup_clock());
        data.entries.insert(data_units[k], make_entry(unit));
    }

    // Fill remaining slots with simple workloads (UUIDs 1..=filler_count).
    let filler_count = n.saturating_sub(10);
    for i in 0..filler_count {
        let id = UnitId(uuid::Uuid::from_u128(
            u128::try_from(i + 1).unwrap_or(u128::MAX),
        ));
        data.entries.insert(id, make_entry(make_workload(id)));
    }

    data.recompute_memory();
    data.generation = 1;
    drop(data);

    // The traversal target is the deepest data unit (data_4).
    (graph, data_units[4])
}

/// Builds a [`GraphDelta`] containing `count` entries with unique random IDs.
fn build_random_delta(count: usize) -> GraphDelta {
    let mut delta = GraphDelta::new();
    for _ in 0..count {
        let id = UnitId(uuid::Uuid::new_v4());
        delta.add_entry(make_entry(make_workload(id)));
    }
    delta
}

// ===========================================================================
// Benchmark: insert
// ===========================================================================

/// Benchmarks inserting a single unit into a graph of N units.
///
/// Uses `iter_batched` with `PerIteration` so each measurement starts
/// with a fresh graph of exactly N units. The inserted unit uses a
/// random UUID to avoid collision.
fn bench_insert(c: &mut Criterion) {
    let rt = Runtime::new().expect("failed to create tokio runtime for async benchmarks");
    let mut group = c.benchmark_group("insert");

    for &n in SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_graph(n),
                |graph| {
                    let unit = make_workload(UnitId(uuid::Uuid::new_v4()));
                    rt.block_on(async { graph.insert(black_box(unit)).await })
                        .expect("insert should succeed in benchmark");
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: merge
// ===========================================================================

/// Benchmarks merging a delta of 10 units into a graph of N units.
///
/// Uses `iter_batched` with `PerIteration` for a fresh graph each
/// measurement. The delta contains 10 entries with random UUIDs.
fn bench_merge(c: &mut Criterion) {
    let rt = Runtime::new().expect("failed to create tokio runtime for async benchmarks");
    let mut group = c.benchmark_group("merge");

    for &n in SIZES {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_graph(n),
                |graph| {
                    let delta = build_random_delta(10);
                    rt.block_on(async { graph.merge(black_box(delta)).await })
                        .expect("merge should succeed in benchmark");
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: snapshot
// ===========================================================================

/// Benchmarks taking a point-in-time snapshot of a graph with N units.
///
/// The snapshot clones all entries and policy chains into an immutable
/// `GraphSnapshot`. This is a read-only operation — the same graph can
/// be reused across iterations.
fn bench_snapshot(c: &mut Criterion) {
    let rt = Runtime::new().expect("failed to create tokio runtime for async benchmarks");
    let mut group = c.benchmark_group("snapshot");

    for &n in SIZES {
        let graph = build_graph(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                rt.block_on(async { graph.snapshot().await })
                    .expect("snapshot should succeed in benchmark");
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: query_get
// ===========================================================================

/// Benchmarks retrieving a unit by ID from a graph of N units.
///
/// The target ID is the unit at the midpoint of the graph (UUID = N/2).
/// This is a synchronous `GraphQuery::get` call — no async runtime needed.
fn bench_query_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_get");

    for &n in SIZES {
        let graph = build_graph(n);
        let target_id = UnitId(uuid::Uuid::from_u128(
            u128::try_from(n / 2).unwrap_or(1).max(1),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                black_box(
                    graph
                        .get(black_box(&target_id))
                        .expect("unit should exist in graph"),
                );
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: query_provenance
// ===========================================================================

/// Benchmarks traversing a depth-5 provenance chain in a graph of N units.
///
/// The graph includes a 5-link provenance chain. The `traverse_provenance`
/// method first clones all active entries into a filtered `BTreeMap`
/// (O(N)), then walks the chain (O(5 * log N)). For large N, the clone
/// dominates.
fn bench_query_provenance(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_provenance");

    for &n in SIZES {
        let (graph, target_id) = build_graph_with_provenance(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let links = graph
                    .traverse_provenance(black_box(&target_id))
                    .expect("provenance traversal should succeed");
                black_box(links);
            });
        });
    }
    group.finish();
}

// ===========================================================================
// Benchmark: memory_estimate
// ===========================================================================

/// Benchmarks computing current memory usage of a graph with N units.
///
/// The `stats()` method locks the state, iterates all entries to count
/// active/archived units (O(N)), and reads the cached `memory_estimate_bytes`
/// field (O(1)).
fn bench_memory_estimate(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_estimate");

    for &n in SIZES {
        let graph = build_graph(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                black_box(graph.stats());
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
    bench_insert,
    bench_merge,
    bench_snapshot,
    bench_query_get,
    bench_query_provenance,
    bench_memory_estimate,
);
criterion_main!(benches);
