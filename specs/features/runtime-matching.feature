@capabilities @placement
Feature: Runtime matching and node capabilities
  Nodes auto-discover their capabilities at startup and advertise them
  via gossip. Workloads declare artifact type and requirements. The solver
  matches artifact type to node runtime capabilities (hard constraint)
  and ranks by resource availability (soft constraint). Progressive
  disclosure: userspace install has narrower capabilities, system install
  has full capabilities.

  Background:
    Given a bootstrapped trust domain "acme" with root governance unit
    And the following nodes in the cluster:
      | node_id     | env  | privilege | runtimes                  | os    | arch    |
      | dev-laptop  | dev  | user      | oci-rootless, wasm        | linux | aarch64 |
      | dev-desktop | dev  | root      | oci, native, wasm         | linux | x86_64  |
      | ci-runner   | test | root      | oci, native               | linux | x86_64  |
      | prod-1      | prod | root      | oci, k8s                  | linux | x86_64  |
      | prod-2      | prod | root      | oci, k8s                  | linux | x86_64  |
      | win-server  | prod | root      | native                    | windows | x86_64 |

  # --- Basic runtime matching ---

  Scenario: OCI workload matches nodes with OCI or OCI-rootless runtime
    # INV-N2: capabilities are hard constraints
    Given a workload unit "web-api" with:
      | field          | value                  |
      | artifact.type  | oci                    |
      | artifact.ref   | acme/web-api:v1.0      |
      | artifact.digest| sha256:abc123          |
    When the solver evaluates placement for "web-api"
    Then "web-api" can be placed on nodes: dev-laptop (oci-rootless), dev-desktop (oci), ci-runner (oci), prod-1 (oci), prod-2 (oci)
    And "web-api" cannot be placed on "win-server" (no oci runtime)

  Scenario: Native Windows workload matches only Windows nodes
    Given a workload unit "sql-server" with:
      | field              | value                           |
      | artifact.type      | native                          |
      | artifact.ref       | https://dl.example.com/sql.msi  |
      | artifact.digest    | sha256:def456                   |
      | artifact.requires  | ["windows", "dotnet-4.8"]       |
    When the solver evaluates placement for "sql-server"
    Then "sql-server" can only be placed on "win-server" (os:windows + runtime:native)
    And all Linux nodes are excluded (os mismatch)

  Scenario: Wasm workload matches nodes with Wasm runtime
    Given a workload unit "edge-function" with:
      | field          | value                       |
      | artifact.type  | wasm                        |
      | artifact.ref   | acme/edge:v1.0.wasm         |
      | artifact.digest| sha256:789abc               |
    When the solver evaluates placement for "edge-function"
    Then "edge-function" can be placed on: dev-laptop (wasm), dev-desktop (wasm)
    And "edge-function" cannot be placed on: ci-runner, prod-1, prod-2, win-server (no wasm)

  Scenario: K8s manifest workload matches only K8s-capable nodes
    Given a workload unit "k8s-service" with:
      | field          | value                     |
      | artifact.type  | k8s-manifest              |
      | artifact.ref   | acme/k8s-service:v2.0     |
      | artifact.digest| sha256:k8s123             |
    When the solver evaluates placement for "k8s-service"
    Then "k8s-service" can be placed on: prod-1 (k8s), prod-2 (k8s)
    And all non-K8s nodes are excluded

  # --- Userspace vs system install ---

  Scenario: Privileged port requirement excludes userspace nodes
    Given a workload unit "http-gateway" with:
      | field          | value              |
      | artifact.type  | oci                |
      | artifact.ref   | acme/gateway:v1.0  |
      | needs          | ports:privileged   |
    When the solver evaluates placement for "http-gateway"
    Then "http-gateway" is excluded from "dev-laptop" (privilege:user, no ports:privileged)
    And "http-gateway" can be placed on nodes with privilege:root

  Scenario: Userspace node runs rootless containers
    Given a workload unit "dev-service" with artifact.type = "oci"
    And "dev-service" does NOT require privileged ports
    When the solver evaluates placement on "dev-laptop" (privilege:user)
    Then "dev-laptop" matches via runtime:oci-rootless
    And the node uses rootless Podman/Docker to execute the container
    And the workload runs without root privileges

  # --- Resource ranking (soft constraints) ---

  Scenario: Solver ranks nodes by resource availability after capability filter
    # INV-N3: resources are soft constraints, ranked with ppm arithmetic
    Given workload "compute-heavy" requires artifact.type = "oci" and resource hint memory >= 4gb
    And the following resource snapshots:
      | node_id     | memory.available | cpu.load |
      | ci-runner   | 2gb              | 0.8      |
      | prod-1      | 12gb             | 0.2      |
      | prod-2      | 8gb              | 0.4      |
    And "compute-heavy" has a promotion policy for env:prod
    When the solver evaluates placement
    Then all three nodes satisfy capability requirements (runtime:oci)
    And the solver ranks by resource fit: prod-1 (best), prod-2, ci-runner (worst)
    And "compute-heavy" is placed on prod-1 (most available memory, lowest load)

  # --- Auto-discovery ---

  Scenario: Node auto-discovers capabilities on startup
    Given a fresh Linux machine with Docker installed and a CUDA GPU
    When "taba init" is run in userspace
    Then the node auto-discovers:
      | capability       | value       | method        |
      | arch             | x86_64      | auto-detected |
      | os               | linux       | auto-detected |
      | privilege        | user        | auto-detected |
      | runtime:oci-rootless | present | probe: Docker rootless socket |
      | gpu:cuda         | present     | probe: nvidia-smi |
    And the node does NOT claim runtime:oci (not running as root with Docker daemon)
    And the node does NOT claim ports:privileged (running as user)
    And capabilities are cached locally and advertised via gossip

  Scenario: Node re-probes capabilities on taba refresh
    Given node "dev-desktop" was auto-discovered with runtime:oci and runtime:native
    And Docker has been uninstalled from "dev-desktop" since last probe
    When the operator runs "taba refresh" on "dev-desktop"
    Then the node re-probes all capabilities
    And runtime:oci is removed (Docker socket not found)
    And runtime:native remains (package manager still available)
    And updated capabilities are advertised via gossip
    And the solver re-evaluates placements affected by the capability change

  Scenario: Fleet-wide capability refresh via governance command
    Given an operator authors an OperationalCommand governance unit "refresh-all"
    And "refresh-all" specifies command type "refresh-capabilities"
    When the governance unit is signed and inserted into the graph
    Then the command propagates via gossip to all nodes
    And every node re-probes its capabilities
    And updated capabilities are advertised via gossip
    And the solver re-evaluates all placements

  # --- Custom tags ---

  Scenario: Custom freeform tags used for placement
    # INV-N4: custom tags treated identically to auto-discovered capabilities
    Given node "win-server" has custom tags in its config:
      | tag                  | value |
      | oracle-licensed      | true  |
      | rack                 | east-3|
      | datacenter           | us-east-1 |
    And workload "oracle-db" needs capability "oracle-licensed:true"
    When the solver evaluates placement for "oracle-db"
    Then "oracle-db" can only be placed on "win-server" (only node with oracle-licensed:true)
    And the custom tag is matched identically to an auto-discovered capability

  # --- Artifact integrity ---

  @security
  Scenario: Artifact digest verified after fetch
    # INV-A1: digest verification is mandatory
    Given workload "web-api" with artifact.digest = "sha256:abc123"
    And the node fetches the artifact from registry
    When the fetched artifact's SHA256 hash is computed
    Then if hash matches "sha256:abc123", execution proceeds
    And if hash does NOT match, the artifact is rejected
    And the node reports "artifact digest mismatch" to the graph
    And the workload is NOT started with the mismatched artifact

  # --- P2P artifact distribution ---

  @distribution
  Scenario: Peer cache avoids redundant external downloads
    # INV-A2: peer cache first, then external source
    Given "prod-1" has already fetched artifact "sha256:abc123" for "web-api"
    And "prod-1" advertises "sha256:abc123" in its peer cache inventory via gossip
    When "prod-2" needs to fetch artifact "sha256:abc123"
    Then "prod-2" checks peer cache first
    And discovers "prod-1" has the artifact
    And fetches from "prod-1" via P2P transfer
    And does NOT contact the external registry
    And verifies digest after fetch (INV-A1)

  Scenario: Peer cache miss falls back to external registry
    Given no node in the cluster has artifact "sha256:new789"
    When "prod-1" needs to fetch artifact "sha256:new789"
    Then "prod-1" checks peer cache (no match)
    And falls back to external source (registry URL from artifact.ref)
    And fetches from registry
    And caches the artifact locally for future peer requests
    And verifies digest after fetch (INV-A1)

  @distribution
  Scenario: Push mode for air-gapped environments
    Given "dev-desktop" builds artifact "acme/web-api:v2.0" locally with digest "sha256:local456"
    And the cluster has no external registry access (air-gapped)
    When the developer runs "taba push sha256:local456"
    Then the artifact is distributed to peer nodes via P2P
    And nodes receiving the artifact verify the digest (INV-A1)
    And the artifact becomes available in peer cache across the cluster
