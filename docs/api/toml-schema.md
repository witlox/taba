# Unit Declaration TOML Schema (DL-015)

The unit declaration format uses TOML for human authoring (A7). The schema
follows progressive disclosure: simple cases are simple, complexity is available
but never mandatory. One file = one unit.

**Key principles:**
- As simple as a Dockerfile for simple cases
- Richness available but not mandatory
- Defaults are the progressive disclosure path (INV-E3, INV-N5, INV-O3)
- Names scoped to trust domain (not globally unique)
- Duration strings: "50ms", "30s", "2h", "7y"
- One file per unit — multi-unit bundles are a directory convention

---

## Level 0 — Minimal workload (3 lines)

```toml
[unit]
name = "hello-web"
image = "hello:latest"
```

Everything else defaults:
- `type = "workload"` (inferred from `image`)
- `kind = "service"` (default)
- Trust domain: current (from `taba init`)
- Author: current (from keypair)
- Capabilities: none needed, none provided
- Health: OS-level process monitoring (INV-O3)
- Placement on failure: from environment (INV-N5: dev=leave-dead, prod=auto-replace)
- Scaling: min=1, max=1

---

## Level 1 — Capabilities

```toml
[unit]
name = "api-server"
image = "api:v2.1.0"
digest = "sha256:abc123..."

[needs]
postgres = { type = "storage" }
redis = { type = "storage", purpose = "cache" }

[provides]
http-api = { type = "network", purpose = "user-facing" }

[scaling]
min = 2
max = 10

[[scaling.triggers]]
metric = "cpu_ppm"
threshold = 700_000  # 70%
direction = "up"
```

### Capability shorthand

Capabilities are declared as tables under `[needs]` and `[provides]`. Each
key is the capability name. Value is a table with `type` (required) and
optional `purpose` qualifier (INV-K2).

```toml
# Full form:
postgres = { type = "storage", purpose = "primary" }

# Minimal form (type only):
postgres = { type = "storage" }
```

---

## Level 2 — Behavioral contracts

```toml
[unit]
name = "payment-processor"
image = "payments:v3.0.0"
kind = "service"

[tolerates]
max_latency = "50ms"
consistency = "strong"

[failure]
on_oom = "backoff-inputs"
on_crash = { restart_with_backoff = 3 }
on_shutdown = { drain = "30s" }

[recovery]
strategy = "replay-from-offset"
stream = "payments-events"

[health]
type = "http"
path = "/healthz"
port = 8080
interval = "10s"
timeout = "2s"
```

### Failure semantics

| Field | Values | Default |
|-------|--------|---------|
| `on_oom` | `"backoff-inputs"`, `"restart"`, `"fail-permanent"` | `"restart"` |
| `on_crash` | `{ restart_with_backoff = N }`, `"unexpected"` | `{ restart_with_backoff = 3 }` |
| `on_shutdown` | `{ drain = "duration" }`, `"immediate"` | `{ drain = "30s" }` |

### Health check types (progressive, INV-O3)

| Type | Fields | Default |
|------|--------|---------|
| `"http"` | `path`, `port`, `interval`, `timeout` | — |
| `"tcp"` | `port`, `interval`, `timeout` | — |
| `"command"` | `command`, `interval`, `timeout` | — |
| (omitted) | — | OS-level process monitoring |

### Recovery strategies

| Strategy | Fields | Default |
|----------|--------|---------|
| `"stateless"` | — | (default) |
| `"replay-from-offset"` | `stream`, `offset` (optional) | — |
| `"require-quorum"` | `min_peers` | — |

---

## Level 3 — Data unit

```toml
[unit]
name = "customer-profiles"
type = "data"

[schema]
format = "json-schema"
definition = "schemas/customer.json"

[classification]
level = "pii"

[retention]
mode = "persistent"
duration = "7y"
legal_basis = "GDPR Art. 6(1)(b)"
mandatory = true

[storage]
encrypted_at_rest = true
jurisdictions = ["EU", "CH"]

[provides]
customer-data = { type = "dataset", purpose = "analytics" }
```

### Classification levels (INV-S4, INV-S7)

Lattice: `"public"` < `"internal"` < `"confidential"` < `"pii"`

### Retention modes (INV-D2, INV-D4)

| Mode | Behavior | Default |
|------|----------|---------|
| `"persistent"` | Governed by `duration` + `legal_basis`. Tombstoned on expiry. | (default) |
| `"ephemeral"` | Auto-removed when producing bounded task terminates. Ref check determines tombstone vs full remove. | — |
| `"local-only"` | Never enters graph. Requires policy for classification > public (INV-D5). | — |

### Hierarchical data

```toml
[unit]
name = "eu-customers"
type = "data"
parent = "customer-profiles"

[classification]
level = "pii"

[storage]
jurisdictions = ["EU"]
# Inherits encrypted_at_rest from parent
# Narrows jurisdiction (always allowed per INV-S7)
```

---

## Level 4 — Policy unit

```toml
[unit]
name = "allow-analytics-access"
type = "policy"

[conflict]
units = ["customer-profiles", "analytics-pipeline"]
capability = "customer-data"

[resolution]
action = "allow"
rationale = "Analytics pipeline has DPA and processes within EU jurisdiction"
```

### Resolution actions

| Action | Behavior |
|--------|----------|
| `"allow"` | Allow the composition unconditionally |
| `"deny"` | Deny the composition |
| `"conditional"` | Allow under conditions: `conditions = ["condition1", "condition2"]` |

### Policy supersession

```toml
[unit]
name = "updated-analytics-policy"
type = "policy"
supersedes = "allow-analytics-access"

[conflict]
units = ["customer-profiles", "analytics-pipeline"]
capability = "customer-data"

[resolution]
action = "conditional"
conditions = ["processing within EU only", "audit trail enabled"]
rationale = "Updated per legal review 2026-03"
```

---

## Level 5 — Bounded task with spawning

```toml
[unit]
name = "nightly-etl"
kind = "bounded-task"
image = "etl:v1.2.0"

[deadline]
wall_time = "2h"

[spawn]
max_depth = 2
max_count = 10
```

### Bounded task fields

| Field | Type | Default |
|-------|------|---------|
| `kind` | `"bounded-task"` | `"service"` |
| `deadline.wall_time` | duration string | (required for bounded-task) |
| `deadline.lc_range` | integer (logical clock delta) | (optional) |
| `spawn.max_depth` | integer (1-4) | governed by INV-W3 default (4) |
| `spawn.max_count` | integer | (required if spawning) |

---

## Governance units

Governance units use `type = "governance"` with a `governance_type` field.

### Trust domain definition

```toml
[unit]
name = "production-domain"
type = "governance"
governance_type = "trust-domain"

[trust_domain]
description = "Production workloads for ACME Corp"
```

### Promotion gate (INV-E3)

```toml
[unit]
name = "prod-gate"
type = "governance"
governance_type = "promotion-gate"

[[transitions]]
from = "dev"
to = "test"
mode = "auto"

[[transitions]]
from = "test"
to = "prod"
mode = "human-approval"
```

### Cross-domain capability advertisement (INV-X5)

```toml
[unit]
name = "shared-data-api"
type = "governance"
governance_type = "cross-domain-capability"

[cross_domain]
provides = { type = "dataset", name = "shared-metrics", purpose = "monitoring" }
conditions = "Bilateral policy required. Read-only access."
```

---

## Artifact type shorthand

The `[unit]` section uses shorthand keys to infer artifact type:

| Key | Artifact type | Example |
|-----|---------------|---------|
| `image` | OCI container | `image = "myapp:v1.0"` |
| `binary` | Native binary | `binary = "bin/myapp"` |
| `wasm` | WebAssembly module | `wasm = "module.wasm"` |
| `k8s` | Kubernetes manifest | `k8s = "pod-spec.yaml"` |

Only one artifact key per unit. `digest` (SHA256) is always recommended
and required for production environments (INV-A1).

---

## Defaults summary

| Field | Default | Source |
|-------|---------|--------|
| `type` | `"workload"` | inferred from artifact key |
| `kind` | `"service"` | — |
| `trust_domain` | current | from `taba init` |
| `author` | current | from keypair |
| `scaling.min` | 1 | — |
| `scaling.max` | 1 | — |
| `health` | OS-level process monitoring | INV-O3 |
| `placement_on_failure` | env-derived (dev=leave-dead, else=auto-replace) | INV-N5 |
| `failure.on_oom` | `"restart"` | — |
| `failure.on_crash` | `{ restart_with_backoff = 3 }` | — |
| `failure.on_shutdown` | `{ drain = "30s" }` | — |
| `recovery.strategy` | `"stateless"` | — |
| `retention.mode` | `"persistent"` | — |
| `classification.level` | `"internal"` | — |

---

## File conventions

- Extension: `.taba.toml` (e.g., `api-server.taba.toml`)
- One file per unit
- Multi-unit deployments: directory of `.taba.toml` files
- Policy files can reference units by name (scoped to trust domain)
- Names must match `[a-z0-9][a-z0-9-]*` (lowercase, hyphens, no dots)
