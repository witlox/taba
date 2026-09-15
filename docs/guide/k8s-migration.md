# K8s Migration

taba includes a K8s manifest converter (`taba-k8s`) that reads
existing Kubernetes YAML and generates taba unit declarations.

## Quick start

```sh
# Convert a single deployment
taba-k8s convert deployment.yaml --output-dir ./units

# Apply the generated units
taba apply units/web-server.taba.toml
```

## Supported resources

| K8s Resource | taba Unit | Mapping |
|--------------|-----------|---------|
| Deployment | WorkloadUnit (service) | Container image → artifact, replicas → scaling |
| StatefulSet | WorkloadUnit (service) | + state recovery (require-quorum), PVC references |
| DaemonSet | WorkloadUnit (service) | + one-per-node constraint, node selector |
| Pod | WorkloadUnit (service) | Direct pod (no controller, no auto-scaling) |
| Service | WorkloadUnit (provides) | Network capability from ports |
| ConfigMap | DataUnit | Internal classification, text/plain schema |
| Secret | DataUnit | Confidential classification, encrypted_at_rest |
| NetworkPolicy | PolicyUnit | Pod selector → conflict units, ingress/egress → resolution |
| Role | GovernanceUnit (RoleAssignment) | Rules → unit_type_scope, namespace → trust_domain_scope |
| ClusterRole | GovernanceUnit (RoleAssignment) | Cluster-wide scope |
| RoleBinding | GovernanceUnit (RoleAssignment) | Binds Role to subjects |
| ClusterRoleBinding | GovernanceUnit (RoleAssignment) | Binds ClusterRole to subjects, cluster-wide |

## Unmappable resources

The converter surfaces resources it cannot map, with a reason and
suggestion:

| Resource | Reason | Suggestion |
|----------|--------|------------|
| Ingress | taba has no ingress concept | Map the service backend directly |
| PersistentVolume | No direct taba equivalent | Use a data unit with storage_requirements |
| PersistentVolumeClaim | No direct taba equivalent | Use a data unit with storage_requirements |
| CRDs | Unbounded, no generic mapping | Write a custom taba unit declaration |
| Job | Bounded task (manual) | Set kind = "bounded-task", add [deadline] |
| CronJob | Bounded task (manual) | Set kind = "bounded-task", add [deadline] |
| HPA | Scaling triggers (manual) | Add triggers to [scaling] section |
| Namespace | No taba equivalent needed | — |
| ServiceAccount | No taba equivalent needed | — |

## Multi-document YAML

The converter supports `---` separated multi-document YAML:

```sh
taba-k8s convert multi-resource.yaml --output-dir ./units
```

Each resource is converted independently. The conversion report
shows what was generated, skipped, and unmappable.

## Conversion report

```sh
taba-k8s convert deployment.yaml
```

```
=== K8s → taba Conversion Report ===

Generated: 1 units
  ✓ web-server

Unmappable: 1 resources
  ✗ Ingress/web-ingress: taba has no ingress concept.
    → Map the service backend directly.

Total processed: 2
```

## Provenance placeholders

```sh
taba-k8s convert deployment.yaml --provenance
```

Adds SLSA provenance placeholders to generated units, for filling
in build system details (builder, source repo, digest, SLSA level).
