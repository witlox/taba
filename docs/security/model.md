# Security Model

taba's security model is built on six principles:

## 1. Zero-access default (INV-S1)

Units access nothing unless explicitly declared and policy-approved.
The `CapabilityEnforcer` trait checks that every capability a unit
exercises is in its granted set. Unknown units or missing
capabilities result in denial — there is no implicit grant.

```rust
pub trait CapabilityEnforcer {
    fn check_access(&self, unit: &UnitId, capability: &Capability) -> Result<(), SecurityError>;
    fn check_all(&self, unit: &UnitId, capabilities: &[Capability]) -> Result<(), SecurityError>;
}
```

## 2. Fail closed (INV-S2)

Security conflicts are never implicitly resolved. When the solver
detects incompatible security requirements between units and no
policy resolves the incompatibility, the composition is blocked.
Ambiguous policies are denied, not guessed.

## 3. Signed units (INV-S3)

Every unit in the graph is signed with Ed25519. Signatures bind
context: `Sign(key, hash(unit || trust_domain_id || cluster_id || validity_window))`.

This prevents:
- Cross-cluster replay (different cluster ID)
- Cross-domain replay (different trust domain)
- Post-revocation forgery (causal model: units signed after
  revocation are rejected, units signed before remain valid)

Signature verification is a **synchronous gate** before any unit
enters graph state. No unit is accepted without verification.

## 4. Capability-based with purpose qualifiers (INV-K2)

Capabilities are typed tuples: `(cap_type, name, purpose?)`.

- `cap_type`: the kind of resource (e.g., "storage", "network")
- `name`: the specific resource (e.g., "postgres-compatible")
- `purpose` (optional): a qualifier that must match during composition

When `purpose` is declared on a need, the provide must have the
same purpose. When omitted, any provide of the same type+name is
acceptable.

Capability lists are sorted lexicographically before matching to
ensure determinism regardless of declaration order (INV-K2).

## 5. Author scope enforcement (INV-S5, INV-S8)

Authors are parameterized by `(unit_type_scope × trust_domain_scope)`.

- **INV-S5**: Authors cannot create units outside their scope.
  A workload-scoped author cannot create policy units. An author
  scoped to trust domain A cannot create units in trust domain B.
- **INV-S8**: For state-producing types (workload, data), no two
  distinct authors may have identical scope tuples. For
  decision-making types (policy, governance), overlapping scopes
  are permitted (INV-S8a).

## 6. Taint propagation (INV-S4, INV-S7)

Data classification is propagated through the provenance graph at
**query time** (not cached at merge time). This makes taint
eventually consistent across nodes.

- **Single input**: output inherits the input's classification
- **Multiple inputs**: output inherits the **union** (most
  restrictive) of all input classifications
- **Declassification**: requires multi-party signing (INV-S9) —
  one policy-scoped signer + one data-steward-scoped signer
- **Hierarchy**: children can narrow freely (INV-S7), widening
  requires explicit policy

Classification lattice: `Public < Internal < Confidential < PII`
