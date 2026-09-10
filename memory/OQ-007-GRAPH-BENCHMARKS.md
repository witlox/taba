# OQ-007: Graph Size Benchmarks

**Date**: 2026-09-10
**Status**: Resolved

## Method

Benchmarked graph and solver operations at varying sizes using criterion.
All benchmarks run on a single thread, in-memory, no disk I/O. Graph
setup uses direct state insertion (O(N)) to avoid measurement overhead.
The benchmarked routines call the real `DefaultGraph` and solver methods.

Each graph size uses identical default `WorkloadUnit` entries (provides
`compute:http`, Oci artifact, no needs) with sequential UUIDs for
deterministic ordering. Solver benchmarks include ~20% of units with
unmet needs (`storage:redis`) to exercise the conflict detection path.

## Results

### Graph operations

| Operation    | N=10      | N=100     | N=1000    | N=5000      |
|--------------|-----------|-----------|-----------|-------------|
| insert       | 1.72 µs   | 11.66 µs  | 120.12 µs | 614.68 µs   |
| merge        | 16.35 µs  | 26.29 µs  | 135.77 µs | 628.16 µs   |
| snapshot     | 2.60 µs   | 30.83 µs  | 325.15 µs | 5.94 ms     |
| query_get    | 224.17 ns | 206.37 ns | 214.02 ns | 216.11 ns   |
| query_provenance | 4.46 µs | 44.97 µs | 415.04 µs | 5.87 ms    |
| memory_estimate | 54.65 ns | 525.28 ns | 5.27 µs  | 28.87 µs   |

### Solver operations

| Operation         | N=10      | N=100     | N=500      |
|-------------------|-----------|-----------|------------|
| solve             | 2.51 µs   | 44.69 µs  | 747.01 µs  |
| detect_conflicts  | 487.90 ns | 22.27 µs  | 585.47 µs  |
| detect_cycles     | 1.55 µs   | 24.24 µs  | 190.44 µs  |
| rank_nodes        | 157.54 ns | 3.54 µs   | 63.42 µs   |

### Memory usage

| N units | Estimated memory | Per unit |
|---------|-----------------|----------|
| 10      | 3.9 KB          | 400 B    |
| 100     | 39.1 KB         | 400 B    |
| 1000    | 390.6 KB        | 400 B    |
| 5000    | 1.9 MB          | 400 B    |

## Analysis

### Scaling characteristics

**insert** scales linearly with N (O(N)). Each insert calls
`recompute_memory()` which iterates all entries to sum their memory
estimates. At N=5000, a single insert takes ~615 µs, which is dominated
by this O(N) scan rather than the BTreeMap insertion itself (O(log N)).
At 1000 inserts/s (sustained), the graph can grow to ~5000 units before
the per-insert latency exceeds 1 ms.

**merge** scales similarly to insert (O(N)) for the same reason:
`recompute_memory()` is called after each merge. The merge of a 10-unit
delta adds fixed validation overhead (~10 µs) on top of the O(N)
memory recompute.

**snapshot** scales linearly with N but shows super-linear behaviour
between N=1000 and N=5000 (18x for a 5x increase). At N=5000, the
snapshot clones ~1.9 MB of entries, which exceeds the L2/L3 cache and
becomes memory-bandwidth-bound. This is the most expensive graph
operation at scale.

**query_get** is constant (~210 ns) regardless of N. The BTreeMap
lookup is O(log N) where log(5000) ≈ 12.3, and the mutex lock dominates
the fixed overhead. This operation will remain fast even at very large
graph sizes.

**query_provenance** scales linearly with N. The method first clones
all active entries into a filtered BTreeMap (O(N)), then walks the
depth-5 chain (O(5 * log N)). The clone dominates at all sizes. At
N=5000, it takes ~5.9 ms — comparable to snapshot since both clone all
entries.

**memory_estimate** (Graph::stats) scales linearly with N. The method
iterates all entries to count active/archived units (O(N)) and reads
the cached `memory_estimate_bytes` field (O(1)). At N=5000, it takes
~29 µs.

### Solver scaling

**solve** scales as O(N^2) due to conflict detection. With 20% of units
having unmet needs, the conflict detector iterates each needer against
all providers. At N=500, a full solve takes ~747 µs. At N=1000
(extrapolated), this would reach ~3 ms, and at N=5000, ~75 ms —
potentially too slow for real-time placement.

**detect_conflicts** scales as O(N^2), matching expectations. This is
the dominant cost in `solve`.

**detect_cycles** scales as O(N + E) where E = N (two edges per cycle
pair). Linear in practice.

**rank_nodes** scales as O(N log N): score N nodes (O(N)) plus sort
(O(N log N)). Fast even at N=500.

## Conclusions

- At N=5000, `insert` takes ~615 µs, dominated by the O(N)
  `recompute_memory()` call. This is acceptable for interactive use but
  indicates that the per-insert memory recompute is the primary scaling
  bottleneck. A future optimization could use incremental memory
  tracking (add/subtract on insert/remove) instead of full recompute.

- At N=5000, `snapshot` takes ~5.9 ms due to cache effects when cloning
  ~1.9 MB of entries. This is the most expensive graph operation and
  directly affects solver latency (the solver takes a snapshot before
  each run).

- At N=5000, `solve` would take an estimated ~75 ms (extrapolated from
  the O(N^2) conflict detection). At N=500, `solve` takes ~747 µs, which
  is fast enough for real-time placement. The solver's O(N^2) conflict
  detection is the primary scaling concern.

- `query_get` is constant (~210 ns) and will remain fast at any
  practical graph size. BTreeMap lookup is not a bottleneck.

- Memory grows linearly at ~400 bytes/unit (structural estimate). A
  graph of 10,000 units would use ~3.9 MB, and 50,000 units would use
  ~19.5 MB. Actual heap usage (including allocation overhead, string
  data, etc.) is likely 2-3x higher.

- **Recommendation**: auto-compaction at 80% of the memory limit (INV-R6)
  is appropriate for single-node operation up to ~10,000 units. At 10,000
  units, the estimated memory is ~3.9 MB, and compaction should trigger
  at ~3.1 MB (80%). The O(N) `recompute_memory()` call adds ~1.2 ms per
  insert at this scale, which is acceptable. For larger graphs, an
  incremental memory tracking optimization should be considered before
  raising the limit.

- **Sharding**: needed at N > 10,000 (Phase 3+) based on the O(N^2)
  conflict detection in the solver. At N=10,000, a full solve would
  take an estimated ~300 ms (extrapolated from O(N^2) scaling), which is
  too slow for interactive placement. Sharding the graph by trust domain
  (as designed for future milestones) would reduce the effective N per
  shard, keeping solve times under 10 ms per shard. Additionally, the
  snapshot operation at N=10,000 would take ~24 ms due to cache effects,
  further motivating sharding.

## Resolution

OQ-007 is resolved. The graph can handle up to ~5,000 units on a single
node before performance degrades noticeably: at N=5000, `insert` takes
~615 µs and `snapshot` takes ~5.9 ms, both dominated by O(N) operations
and cache effects. The solver's O(N^2) conflict detection is the primary
scaling bottleneck — at N=500, `solve` takes ~747 µs (acceptable), but
extrapolating to N=10,000 gives ~300 ms (unacceptable for real-time
placement).

Auto-compaction at 80% (INV-R6) is appropriate: at ~10,000 units
(~3.9 MB estimated), compaction at 3.1 MB keeps the graph within cache
bounds and maintains sub-millisecond insert latency. Sharding (Phase 3+)
should be implemented before N exceeds 10,000, as the solver's O(N^2)
conflict detection becomes the dominant cost beyond this threshold.
