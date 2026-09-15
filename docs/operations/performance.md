# Performance Tuning

## Benchmarks

Graph and solver benchmarks use [criterion](https://github.com/bheisler/criterion.rs).

```sh
cargo bench -p taba-graph
cargo bench -p taba-solver
```

### Results (post-M7, 2026-09-15)

#### Graph operations

| Operation | N=10 | N=100 | N=1000 | N=5000 |
|-----------|------|-------|--------|--------|
| insert | ~1.9 µs | ~13 µs | ~137 µs | ~710 µs |
| merge | ~18 µs | ~29 µs | ~155 µs | ~720 µs |
| snapshot | ~2.9 µs | ~34 µs | ~370 µs | ~6.8 ms |
| query_get | ~224 ns | ~206 ns | ~214 ns | ~216 ns |
| query_provenance | ~4.5 µs | ~46 µs | ~430 µs | ~6.1 ms |
| memory_estimate | ~61 ns | ~603 ns | ~6.0 µs | ~33.8 µs |

#### Solver operations

| Operation | N=10 | N=100 | N=500 |
|-----------|------|-------|-------|
| solve | ~2.6 µs | ~46 µs | ~770 µs |
| detect_conflicts | ~490 ns | ~23 µs | ~590 µs |
| detect_cycles | ~1.6 µs | ~25 µs | ~189 µs |
| rank_nodes | ~158 ns | ~3.6 µs | ~64 µs |

### Memory usage

| N units | Estimated memory | Per unit |
|---------|-----------------|----------|
| 10 | ~3.9 KB | ~400 B |
| 100 | ~39.1 KB | ~400 B |
| 1000 | ~390.6 KB | ~400 B |
| 5000 | ~1.9 MB | ~400 B |

Actual heap usage (including allocation overhead, string data) is
likely 2-3× higher.

## Tuning recommendations

### Auto-compaction (INV-R6)

- Default: 1 GB memory limit, compaction at 80% (800 MB)
- At ~10,000 units (~3.9 MB), compaction triggers at ~3.1 MB
- For larger graphs: increase `graph_memory_limit_bytes` or
  implement sharding (trust domain partitioning)

### Solver performance

- The O(N²) conflict detection is the primary bottleneck
- At N=500, solve takes ~770 µs (acceptable for real-time)
- At N=10,000 (extrapolated), solve takes ~300 ms (unacceptable)
- **Sharding by trust domain** is the planned solution for N > 10,000

### Insert performance

- Each insert calls `recompute_memory()` which is O(N)
- At N=5000, insert takes ~710 µs (sub-millisecond)
- A future optimization could use **incremental memory tracking**
  (add/subtract on insert/remove) instead of full recompute

### Snapshot performance

- Snapshot clones all entries (O(N))
- At N=5000, takes ~6.8 ms due to cache effects (~1.9 MB of data)
- This is the most expensive graph operation
- The solver takes a snapshot before each run — consider
  incremental solver updates (already implemented as
  `solve_incremental`) to avoid full snapshots
