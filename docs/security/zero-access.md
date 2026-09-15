# Zero-Access Default

taba's security model starts from **zero access**. Every
capability a unit needs must be explicitly declared and
policy-approved. There is no implicit grant, no default
permission, no "allow if not denied."

## How it works

1. A workload unit declares `needs` — capabilities it requires
2. The solver matches `needs` against `provides` from other units
3. If a match exists, the `CapabilityEnforcer` checks that the
   match is permitted by policy
4. If no policy exists, the match is **denied** (fail-closed,
   INV-S2)
5. Only after all checks pass does the unit enter `Composed` state

## Contrast with Kubernetes

In Kubernetes, pods are "allowed by default" — network policies,
RBAC, and admission controllers must explicitly **deny** access.
taba inverts this: everything is denied until explicitly allowed.

## Scope enforcement

Authors are parameterized by `(unit_type_scope × trust_domain_scope)`:
- An author scoped to `workload` in `acme-prod` cannot create
  policy units or units in `acme-staging`
- For state-producing types (workload, data), no two distinct
  authors may have identical scope tuples (INV-S8)
- For decision-making types (policy, governance), overlapping
  scopes are permitted (INV-S8a)
