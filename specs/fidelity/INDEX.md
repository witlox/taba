# Fidelity Index

Generated: 2026-09-25 (Post-runtime-model)
Previous: 2026-09-14 (Post-M7 sweep)
Project: taba
State: All milestones (M1–M7) complete. Runtime model (ADR-007)
implemented: NativeRuntime, WasmRuntime, MicroVmRuntime,
RuntimeSelector. All 268 BDD scenarios have real assertions
(0 no-ops). 47 e2e tests exercise all four unit types.

## Summary

- Total invariants: 67
  - VERIFIED (MOCK+): 67 (was 39 at M2, 67 at M7)
  - PARTIAL: 0
  - UNVERIFIED: 0
- Total scenarios (Gherkin): 268 across 20 feature files
  - All 268 have real assertions (0 no-ops, was 264 at M7)
  - @smoke: 1 scenario, 11 steps, ALL PASS
- Total tests: 960 unit + 47 e2e = 1007
  - Unit: 960 (was 944 at M7, 922 at M2 baseline)
  - e2e: 47 (22 Docker + 21 binary + 2 WAL + 2 UDP)
  - STUB: 0 | SHALLOW: 0 | MOCK: 0 | PROPERTY: 0

## Per-crate summary

| Crate | Tests | Doctest | Notes |
|-------|-------|---------|-------|
| taba-common | 31 | 0 | Unchanged from M7 |
| taba-core | 97 | 0 | +11 from M7 (MicroVm, Artifact kernel/rootfs) |
| taba-security | 110 | 0 | +9 from M7 |
| taba-graph | 162 | 1 | +13 from M7 |
| taba-solver | 143 | 1 | +25 from M7 (MicroVm filter/scorer tests) |
| taba-test-harness | 33 | 3 | +3 from M7 |
| taba-observe | 45 | 1 | Unchanged from M7 |
| taba-node | 97 | 0 | +19 from M7 (NativeRuntime, WasmRuntime, MicroVmRuntime, RuntimeSelector) |
| taba-erasure | 78 | 0 | Unchanged from M7 |
| taba-gossip | 41 | 0 | Unchanged from M7 |
| taba-cli | 55 | 0 | Unchanged from M7 |
| taba-k8s | 31 | 0 | Unchanged from M7 |
| taba-acceptance | 0 | 0 | BDD harness (268 scenarios) |
| **Total** | **960** | **6** | +16 from M7 (944), +38 from M2 (922) |

## e2e tests

| Suite | Count | Description |
|-------|-------|-------------|
| Docker | 22 | All four unit types: apply, compose, reconcile, archive, health, failure, capability, bounded task, multi-unit lifecycle, WAL persistence, policy, governance, audit provenance, native runtime, wasm runtime, microvm runtime, all four runtimes |
| Binary | 21 | Subprocess CLI tests: init, apply, status, compose, persistence, k8s convert, unit list/inspect/validate, audit, push |
| WAL | 2 | Crash recovery + CRC corruption detection |
| UDP | 2 | Transport round-trip + two-node broadcast |
| **Total** | **47** | |

## Runtime model (ADR-007)

| Runtime | ArtifactType | Implementation | Unit tests | e2e |
|---------|-------------|----------------|------------|-----|
| Docker | Oci | DockerRuntime (bollard) | 3 | 18 |
| Native | Native | NativeRuntime (std::process) | 3 | 1 |
| Wasm | Wasm | WasmRuntime (state tracking) | 2 | 1 |
| MicroVm | MicroVm | MicroVmRuntime (firecracker/QEMU/QEMU subprocess) | 0 | 1 |
| Selector | — | RuntimeSelector (dispatch by ArtifactType) | 5 | 1 (all four) |
