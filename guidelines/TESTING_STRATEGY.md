# Testing Strategy

## Test pyramid

```
        /  E2E  \          Few, slow, high-value
       / Integr. \         Cross-crate, multi-component
      / Property   \       Invariant verification, 10k+ cases
     / Unit tests    \     Fast, isolated, per-function
    /__________________\   Many, fast, foundation
```

## Unit tests

- Location: `#[cfg(test)] mod tests` in each source file
- Scope: single function or small group of related functions
- Dependencies: none external. Use in-memory fakes.
- Speed: entire unit test suite completes in < 30 seconds
- Coverage target: 80%+ line coverage on non-trivial code

## Property-based tests

- Framework: `proptest`
- Location: dedicated test modules, or `tests/properties/` per crate
- When to use: any code that maintains an invariant. Specifically:
  - CRDT merge operations (commutativity, associativity, idempotency)
  - Solver placement (determinism: same input = same output)
  - Capability matching (reflexivity, no false positives, no false negatives)
  - Erasure coding (encode then decode = original)
  - Graph operations (insert then query = found)
  - Taint propagation (PII in → PII out unless explicit policy)
- Minimum cases: 10,000 per property
- Shrinking: enable for all properties (default in proptest)

## BDD / Acceptance tests

- Format: Gherkin feature files in `specs/features/`
- Step definitions: Rust code in `tests/bdd/` or per-crate `tests/`
- Framework: `cucumber-rs` (Rust Cucumber implementation)
- Scope: behavioral requirements from specs
- Each feature file maps to a user-visible capability

## Integration tests

- Location: `tests/integration/` per crate, or top-level `tests/`
- Scope: cross-crate interactions
- May use real networking (localhost), real disk I/O
- Docker/containers for multi-node tests when needed
- Speed target: full integration suite < 5 minutes

## Chaos / resilience tests

- For multi-node scenarios only (later implementation phases)
- Node failure injection (kill process, drop network)
- Partition simulation (iptables rules or network namespaces)
- Clock skew injection
- Disk full / slow disk simulation
- Framework: custom test harness or `toxiproxy`

## What to test per crate

### taba-core
- Unit validation: well-formed declarations accepted, malformed rejected
- Capability matching: correct matches, no false positives
- Type system: type checking of unit declarations
- **Property**: for all valid units u, validate(serialize(deserialize(u))) == Ok

### taba-graph
- CRDT operations: merge commutativity, associativity, idempotency
- Graph queries: traverse, filter, provenance chains
- Signature verification: reject unsigned/wrongly-signed entries
- **Property**: for all operations a, b: merge(apply(a), apply(b)) == merge(apply(b), apply(a))

### taba-solver
- Placement: deterministic, respects constraints
- Conflict detection: finds all conflicts, no false negatives
- Policy resolution: applies policies correctly
- **Property**: for all graph states g, nodes n: solve(g, n) on node_1 == solve(g, n) on node_2

### taba-node
- Local reconciliation: converges to desired state
- WAL: crash recovery preserves state
- Drift detection: detects all divergences

### taba-gossip
- Membership: join, leave, failure detection
- Convergence: all nodes agree on membership eventually
- **Property**: in absence of failures, membership converges within bounded time

### taba-erasure
- Encode/decode roundtrip
- Reconstruction from minimum shards
- Failure with too few shards
- **Property**: for all data d, k shards of n: decode(any_k_of(encode(d, n, k))) == d

### taba-security
- Capability enforcement: declared only, deny by default
- Signature operations: sign, verify, reject tampered
- Taint propagation: correct inheritance and narrowing
- **Property**: for all units without explicit capability c: access(c) == Denied

## Test data management

- Use `taba-test-harness` crate for shared fixtures, builders, and fakes
- Builder pattern for test data: `UnitBuilder::workload().with_capability("postgres").build()`
- No test data hardcoded in multiple places — centralize in harness
- Property tests generate their own data via `proptest` strategies
