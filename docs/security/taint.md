# Taint Propagation

Data classification is propagated through the provenance graph at
**query time** (INV-S4), not cached at merge time. This makes
taint eventually consistent across nodes.

## Classification lattice

```
Public < Internal < Confidential < PII
```

- **Single input**: output inherits the input's classification
- **Multiple inputs**: output inherits the **union** (most
  restrictive) of all input classifications
- **Declassification**: requires multi-party signing (INV-S9) —
  one policy-scoped signer + one data-steward-scoped signer

## Hierarchy

Children can narrow freely (INV-S7) — a child data unit with
more restrictive constraints than its parent is always allowed.
Widening requires explicit policy.

## Implementation

The `TaintComputer` trait traverses the provenance graph:

```rust
pub trait TaintComputer {
    fn compute_taint(&self, data_unit: &UnitId) -> Result<Classification, SecurityError>;
    fn validate_declassification(&self, policy_unit: &UnitId) -> Result<(), SecurityError>;
}
```

`compute_taint` walks backward from the data unit through all
producing workloads and their inputs. `validate_declassification`
checks that the declassification policy has at least 2 distinct
signer scopes (INV-S9).
