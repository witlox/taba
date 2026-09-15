# ADR-006: Role model — one primitive parameterized by unit type scope and trust domain scope

## Status

Accepted

## Context

Traditional RBAC systems suffer from role explosion as organizations grow. K8s RBAC
is a separate system from workload management, leading to drift between what's
configured and what's enforced. The taba unit model needs a role system that's
consistent with its own design philosophy: typed, composable, auditable.

## Decision

There is one role primitive, parameterized by two dimensions:
1. **Unit type scope**: which types of units can this author create (workload, data,
   policy, governance)
2. **Trust domain scope**: in which trust domains can this author operate

This yields all necessary roles without RBAC explosion:
- Developer = workload author in their trust domain
- Security team = policy author across trust domains
- Data steward = data unit author in their trust domain
- Regulatory/QA = governance (certification) unit author
- Operator = all unit types in their trust domain + placement authority

Trust domains are themselves units, created through composition. Creating a trust
domain is a conflict (multiple parties claim authority) that requires explicit
policy resolution — the management ceremony IS the implementation.

Role complexity is proportional to: (number of unit types) × (number of trust
domains). Both grow with organizational reality, not platform abstraction tax.

## Consequences

### Positive
- No RBAC explosion — dimensionality is fixed at two
- Roles map to existing organizational functions (no new structures needed)
- Audit is trivial — every unit has an author, every author has a scope
- Trust domain governance uses the same model as everything else
- Small team: one person, all scopes. Large org: many scoped instances. Same model.

### Negative
- Trust domain boundaries must be well-defined (messy orgs = messy domains)
- "Who creates the first trust domain?" is a bootstrap problem
- Migration from existing RBAC systems requires mapping exercise

### Risks
- If trust domains are drawn wrong, security boundaries are wrong
- Overly broad scopes (someone with all types × all domains) defeats the purpose
- The meta-authority problem (who governs trust domain creation) needs the Shamir
  root key ceremony, which is operationally heavy

## References

- `docs/vision/SYSTEM_VISION.md` § Role model
- `docs/decisions/ADR-003-capability-security.md` (security model context)
