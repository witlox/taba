# Summary

[Introduction](README.md)

---

# User Guide

- [Getting Started](guide/getting-started.md)
- [Unit Authoring](guide/unit-authoring.md)
- [Composition & Placement](guide/composition.md)
- [Runtime Matching](guide/runtime-matching.md)
- [Status & Audit](guide/status-audit.md)
- [K8s Migration](guide/k8s-migration.md)

# Administration

- [Deployment](admin/deployment.md)
- [Configuration Reference](admin/configuration.md)
- [Operational Modes](admin/operational-modes.md)
- [WAL & Persistence](admin/wal.md)
- [Health & Reconciliation](admin/health.md)
- [Key Management](admin/key-management.md)
- [Shamir Ceremony](admin/ceremony.md)
- [Node Enrollment](admin/enrollment.md)
- [SLSA Provenance](admin/slsa.md)
- [Attestation](admin/attestation.md)

# Architecture

- [System Overview](architecture/overview.md)
- [Module Map](architecture/module-map.md)
- [Dependency Graph](architecture/dependency-graph.md)
- [Build Phases](architecture/build-phases.md)
- [Testing Strategy](architecture/testing-strategy.md)
- [Enforcement Map](architecture/enforcement-map.md)
- [Error Taxonomy](architecture/error-taxonomy.md)
- [Events](architecture/events.md)

# Security

- [Security Model](security/model.md)
- [STRIDE Analysis](security/stride-analysis.md)
- [Capability Enforcement](security/capabilities.md)
- [Taint Propagation](security/taint.md)
- [Delegation Tokens](security/delegation.md)
- [Zero-Access Default](security/zero-access.md)

# Operations

- [Troubleshooting](operations/troubleshooting.md)
- [Performance Tuning](operations/performance.md)
- [Adversary Findings](operations/findings.md)

# API Reference

- [CLI Reference](api/cli.md)
- [Unit Declaration Format (TOML)](api/toml-schema.md)
- [K8s Converter](api/k8s-converter.md)

# Decisions

- [Architecture Decision Records](decisions/index.md)
  - [ADR-001: Unit Model](decisions/ADR-001-unit-model.md)
  - [ADR-002: CRDT Graph](decisions/ADR-002-crdt-graph.md)
  - [ADR-003: Capability Security](decisions/ADR-003-capability-security.md)
  - [ADR-004: No Masters](decisions/ADR-004-no-masters.md)
  - [ADR-005: Erasure Coding](decisions/ADR-005-erasure-coding.md)
  - [ADR-006: Role Model](decisions/ADR-006-role-model.md)
  - [ADR-007: Runtime Model](decisions/ADR-007-runtime-model.md)
