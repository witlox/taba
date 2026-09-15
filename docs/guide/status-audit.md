# Status & Audit

## Status

```sh
taba status
```

Shows:
- **Graph stats**: active units, pending units, archived units, memory usage, memory limit
- **Operational mode**: Normal, Degraded (with reason), or Recovery
- **Node info**: node ID, trust domain, cluster ID

### Output formats

```sh
taba status --format table    # Human-readable (default)
taba status --format json     # JSON for scripting
```

## Unit inspection

```sh
taba unit list                # All active units
taba unit inspect <id>        # Details of a specific unit
taba unit validate <file>     # Validate a TOML file without inserting
taba unit archive <id>        # Archive a unit (soft-delete)
```

## Audit

### Decision trails

Every solver run produces a decision trail (INV-O1): the inputs
(graph snapshot, membership), outputs (placements, conflicts), and
solver version. Trails are queryable.

```sh
taba audit trails             # List all decision trails
taba audit trails --format json
```

### Provenance

Data units carry provenance metadata (INV-D1): which workload
produced them, from what input data. The provenance chain is
structural — it emerges from the composition graph, not a separate
logging system.

```sh
taba audit provenance <data-unit-id>
```

Returns the full provenance path: all producing workloads and their
input data units, recursively, back to source data units with no
provenance (root data).

## Taint propagation

Taint is computed at query time (INV-S4), not cached at merge time.
This makes taint eventually consistent across nodes.

- **Single input**: output inherits the input's classification
- **Multiple inputs**: output inherits the **union** (most restrictive)
  of all input classifications
- **Declassification**: requires multi-party signing (INV-S9) — one
  policy-scoped signer + one data-steward-scoped signer

Classification lattice: `Public < Internal < Confidential < PII`
