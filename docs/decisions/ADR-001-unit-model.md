# ADR-001: Self-describing typed units as the core infrastructure primitive

## Status

Accepted

## Context

Current infrastructure models separate workload description (container image +
orchestration config) from operational semantics (controller logic in the
orchestrator). This creates two problems simultaneously: the workload is too dumb
(opaque black box with no behavioral contract) and the control plane compensates
by being too smart (generic reconciliation engine with unbounded extension via CRDs).

The result is that control plane complexity is O(everything) regardless of actual
workload complexity, and operational behavior is defined in controllers rather than
in the workload itself.

## Decision

The fundamental unit of deployment in taba is a **typed, self-describing,
contract-carrying unit**. Units declare their capabilities, needs, tolerances,
trust requirements, failure semantics, and recovery procedures as part of their
definition — not as external configuration.

Unit types: workload, data, policy, governance.

The unit model is a type system for infrastructure: units carry typed contracts
that the solver (composition engine) uses to match, validate, and place workloads.

## Consequences

### Positive
- Control plane complexity scales linearly with deployed complexity
- Security is structural (capability-based, zero default access)
- Data lineage is a natural byproduct of composition
- Audit trail is the composition graph itself
- Units are testable in isolation (contracts are verifiable)

### Negative
- Higher upfront authoring cost compared to a Dockerfile
- Requires users to learn a new declaration format
- Migration from existing systems requires tooling
- The type system itself is a design surface that must be gotten right

### Risks
- If the unit declaration format is too complex, adoption will fail
- If the type system is too restrictive, valid use cases will be blocked
- If the type system is too permissive, it provides no guarantees

## Alternatives Considered

| Alternative | Pros | Cons | Why rejected |
|-------------|------|------|--------------|
| Enhanced container + sidecar model | Familiar, incremental | Still opaque core, still needs external orchestrator | Doesn't solve the fundamental split |
| FaaS / serverless | Simple unit, managed platform | Vendor lock-in, limited workload types | Too restrictive, sovereignty concerns |
| Configuration overlay on K8s | Leverages existing ecosystem | Still K8s complexity underneath | Doesn't reduce fundamental complexity |

## References

- `docs/vision/SYSTEM_VISION.md` § The unit model
- `specs/domain-model.md` (when written)
