# Composition & Placement

The solver is the only "active" component in taba. It is a
**deterministic pure function**: given an immutable graph snapshot
and a membership snapshot, any node produces identical placement
decisions (INV-C3).

## How composition works

1. **Capability matching** — For each unit with `needs`, the solver
   searches the graph for units with matching `provides`. Matching is
   typed: `(cap_type, name, purpose?)`. Purpose is optional — when
   declared, it must match exactly. When omitted, any provide of the
   same type+name is acceptable (INV-K2).

2. **Conflict detection** — The solver scans all capability need/provide
   pairs. A conflict exists when:
   - A need has no matching provide (missing capability)
   - A need matches a provide but purpose qualifiers conflict (INV-K2)
   - A need matches but security requirements are incompatible (INV-S2)
     and no policy resolves the incompatibility

3. **Cycle detection** — Recovery relationships declare dependency
   ordering on failure. Cycles in these relationships are unresolvable
   without explicit policy (INV-K5). The solver detects strongly
   connected components of size > 1.

4. **Placement scoring** — For each placeable unit, candidate nodes
   are scored using fixed-point Ppm arithmetic (10^6 scale, u64).
   No floating-point anywhere (INV-C3, DL-004). Higher scores are
   better. Ties are broken by lexicographically lowest NodeId (INV-C3).

5. **Scaling** — The solver evaluates declared triggers
   (e.g., `cpu_ppm > 700_000`). It does not invent scaling logic —
   it only evaluates what the unit declared (INV-K4).

## Determinism guarantees

- **Same input = same output on any node** (INV-C3)
- All arithmetic uses `Ppm(u64)` — fixed-point at 10^6 scale
- Division rounds toward zero (Rust integer division default)
- `#[deny(clippy::float_arithmetic)]` enforced at compile time
- Property tests verify determinism with 10,000+ cases

## Composition is order-independent

Units can be inserted in any order. The solver produces the same
result regardless (INV-C6). This is load-bearing for partition
tolerance — different nodes may receive units in different orders
via gossip, but all converge to the same placement.

## Performance

| Operation | N=10 | N=100 | N=500 | N=1000 (extrapolated) |
|-----------|------|-------|-------|----------------------|
| solve | 2.6 µs | 46 µs | 770 µs | ~3 ms |
| detect_conflicts | 490 ns | 23 µs | 590 µs | ~2.4 ms |
| detect_cycles | 1.6 µs | 25 µs | 189 µs | ~380 µs |
| rank_nodes | 158 ns | 3.6 µs | 64 µs | ~128 µs |

The solver's O(N²) conflict detection is the primary scaling
bottleneck. At N=500, solve takes ~770 µs (acceptable for
real-time placement). At N=10,000, it would take ~300 ms
(unacceptable). Sharding by trust domain is the planned solution
for N > 10,000.

## Using the solver

```sh
# Run the solver on the current graph
taba compose

# Output shows placements, unplaceable units, and conflicts
```

## Solver result structure

```
Placements:
  unit: 550e8400-e29b-41d4-a716-446655440000
  node: 6ba7b810-9dad-11d1-80b4-00c04fd430c8
  score: 850000 (85.0%)

Unplaceable:
  unit: 550e8400-e29b-41d4-a716-446655440001
  reason: NoCapableNode { unmet: [storage:postgres] }

Conflicts:
  conflict: [unit-a, unit-b] capability: network
  status: Unresolved
  has_policy: false
```
