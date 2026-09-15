# Adversary Findings

## Spec review (pre-architect)

**Date**: 2026-04-12
**Status**: Complete — all critical/high resolved

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 5 | All resolved |
| High | 32 | All resolved |
| Medium | 23 | Tracked |
| Low | 4 | Tracked |

See `specs/findings/INDEX.md` for the full list.

## Architecture review

**Date**: 2026-04-13
**Status**: Complete — all critical/high resolved

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 4 | All resolved |
| High | 41 | All resolved |

## Implementation sweep (post-M7)

**Date**: 2026-09-15
**Status**: Complete — 3 Critical resolved, 6 High tracked

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 3 | All resolved (WAL compaction, unsigned units, key permissions) |
| High | 6 | Tracked (SLSA fail-open, WAL replay, spawn depth, scope checker, witness verification, TOML injection) |
| Medium | 12 | Tracked |
| Low | 8 | Tracked |
| Info | 1 | Noted |

See `specs/findings/adversary-implementation-sweep.md` for the full list.

### Resolved Critical findings

| ID | Title | Resolution |
|----|-------|------------|
| F-011 | Compaction deletes old segments before writing new ones | Write-then-delete: new segment is fsynced before old segments are removed |
| F-016 | Default graph accepts unsigned units | Documented: verifier is None by default in M5 local mode; must be explicitly enabled via `with_verifier()` |
| F-026 | Private key file world-readable | Use `OpenOptions::mode(0o600)` on Unix; non-Unix fallback via `std::fs::write` |

### Remaining High findings (tracked)

| ID | Title | Component |
|----|-------|-----------|
| F-008 | Empty builder_signature accepted by default — fail-open | taba-security/provenance.rs |
| F-012 | replay ignores the `from` parameter — replays from beginning | taba-node/wal.rs |
| F-017 | Spawn depth taken from unit's own declaration, not computed | taba-graph/graph.rs |
| F-018 | Scope checker not invoked for workload units (and None by default) | taba-graph/graph.rs |
| F-020 | declare_failed does not verify witnesses are known or distinct | taba-gossip/swim.rs |
| F-028 | TOML injection via K8s metadata — unsanitized interpolation | taba-k8s/converter.rs |
