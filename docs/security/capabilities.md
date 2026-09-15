# Capability Enforcement

taba enforces a **zero-access default** (INV-S1): units access
nothing unless explicitly declared and policy-approved.

## How it works

The `CapabilityEnforcer` trait checks that every capability a unit
exercises is in its granted set. Unknown units or missing
capabilities result in denial.

```rust
pub trait CapabilityEnforcer {
    fn check_access(&self, unit: &UnitId, capability: &Capability) -> Result<(), SecurityError>;
    fn check_all(&self, unit: &UnitId, capabilities: &[Capability]) -> Result<(), SecurityError>;
}
```

`check_all` short-circuits on the first denial.

## Capability matching

Capabilities are typed tuples: `(cap_type, name, purpose?)`.

- `cap_type`: the kind of resource (e.g., "storage", "network")
- `name`: the specific resource (e.g., "postgres-compatible")
- `purpose` (optional): a qualifier that must match during
  composition (INV-K2)

When `purpose` is declared on a need, the provide must have the
same purpose. When omitted, any provide of the same type+name is
acceptable.

Capability lists are sorted lexicographically before matching to
ensure determinism regardless of declaration order.

## Fail-closed

Ambiguous security decisions are denied, not guessed (INV-S2). If
the capability check is ambiguous or the policy is missing, access
is denied.
