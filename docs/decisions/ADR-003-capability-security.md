# ADR-003: Capability-based security with fail-closed conflict resolution

## Status

Accepted

## Context

Current infrastructure security is layered after the fact: container isolation
(weak), namespace isolation (administrative), network policy (bolt-on), RBAC
(separate system), secrets management (separate system), mTLS (sidecar). It's
defense-in-depth by accident, not design. The default is effectively "can reach
anything unless explicitly denied."

## Decision

taba uses a capability-based security model where:
- The default is zero access — units can only reach what they explicitly declare
- Declarations that are compatible compose automatically (no policy needed)
- Declarations that conflict require an explicit policy unit to resolve
- No implicit resolution of security conflicts, ever
- Taint propagation is structural: sensitive data classifications propagate
  through the composition graph unless explicitly declassified by policy
- Every policy resolution creates an auditable artifact

The role model enforces this: authors have scoped authority (unit type × trust
domain), and scope conflicts at the author level also fail closed.

## Consequences

### Positive
- Security is structural, not layered
- Blast radius of compromise is limited by capability declarations
- No lateral movement by default
- Audit trail for every security decision
- Policy surface area proportional to actual security complexity

### Negative
- More upfront work for authors (must declare all capabilities)
- Simple deployments that "just work" in K8s may require explicit policy in taba
- Policy resolution requires human judgment — can't be fully automated

### Risks
- Overly restrictive defaults may frustrate users into workarounds
- Policy proliferation if declarations are too granular
- The fail-closed model means a missing policy blocks deployment entirely

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| RBAC (like K8s) | Familiar | Doesn't compose, doesn't propagate taints | Insufficient for the unit model |
| Allow-by-default + deny lists | Easier onboarding | Insecure default, easy to miss denials | Violates security-first design |
| Policy-as-code only (OPA) | Powerful, flexible | External system, not integrated with unit model | Can complement but not replace capability model |

## References

- `docs/vision/SYSTEM_VISION.md` § Security model
- `docs/vision/SYSTEM_VISION.md` § Role model
