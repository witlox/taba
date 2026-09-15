# CLI Reference

## `taba init`

Initialize a local node. Generates an Ed25519 keypair, trust
domain, and root governance unit.

```
taba init [--force] [--state-dir <DIR>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--force` | false | Re-initialize even if already initialized |
| `--state-dir` | `~/.taba` | State directory (keypair, config, graph) |

## `taba apply`

Parse a TOML unit declaration, fill in identity, validate, and
insert into the graph.

```
taba apply <FILE> [--dry-run] [--state-dir <DIR>] [--format <FORMAT>]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--dry-run` | false | Validate and display without inserting |
| `--state-dir` | `~/.taba` | State directory |
| `--format` | `table` | Output format: `table` or `json` |

## `taba unit list`

List all active (non-archived) units in the graph.

```
taba unit list [--state-dir <DIR>] [--format <FORMAT>]
```

## `taba unit inspect`

Show details of a specific unit.

```
taba unit inspect <ID> [--state-dir <DIR>] [--format <FORMAT>]
```

## `taba unit validate`

Validate a TOML file without inserting into the graph.

```
taba unit validate <FILE>
```

## `taba unit archive`

Archive a unit (soft-delete). Archived units are removed from the
active set but retained for provenance integrity. Governance units
cannot be archived (INV-G3).

```
taba unit archive <ID> [--state-dir <DIR>]
```

## `taba status`

Show graph stats, operational mode, and node information.

```
taba status [--state-dir <DIR>] [--format <FORMAT>]
```

## `taba compose`

Run the deterministic solver on the current graph. Shows
placements, unplaceable units, and detected conflicts.

```
taba compose [--state-dir <DIR>] [--format <FORMAT>]
```

## `taba audit provenance`

Show the provenance chain for a data unit.

```
taba audit provenance <UNIT-ID> [--state-dir <DIR>] [--format <FORMAT>]
```

## `taba audit trails`

List all decision trails.

```
taba audit trails [--state-dir <DIR>] [--format <FORMAT>]
```

## `taba push`

Push an artifact to the local peer cache. Computes SHA-256 and
stores content-addressed.

```
taba push <FILE> [--cache-dir <DIR>]
```

## `taba-k8s convert`

Convert K8s manifests to taba unit declarations.

```
taba-k8s convert <FILE> [--output-dir <DIR>] [--trust-domain <TD>] [--provenance]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--output-dir` | stdout | Directory for generated `.taba.toml` files |
| `--trust-domain` | — | Trust domain for generated units |
| `--provenance` | false | Generate SLSA provenance placeholders |
