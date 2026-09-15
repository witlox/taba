# CHANGELOG

All notable changes to taba are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added — M1: Types compile
- `taba-common`: Identity newtypes (`UnitId`, `NodeId`, `AuthorId`, `TrustDomainId`, `ClusterId`, `CeremonyId`, `ShardId`, `DelegationTokenId`, `PolicyId`), `WallTime`, `LogicalClock` with `tick()`/`sync()`, `DualClockEvent`, `ClockQuality`, `Ppm`/`SignedPpm` fixed-point arithmetic, `Version`, `ValidityWindow`, `ContentDigest`, `ClusterConfig`, `NodeConfig`, `ArchiveBackendConfig`, `CommonError`
- `taba-core`: `Unit` enum (Workload, Data, Policy, Governance), `UnitHeader`, `UnitState`, `WorkloadUnit`, `WorkloadKind`, `DataUnit`, `PolicyUnit`, `GovernanceUnit` (TrustDomainDef, RoleAssignment, Certification, OperationalCommand, PromotionGate, CrossDomainCapability, KeyRevocation), `Capability`, `CapabilityMatch`, `CapabilityMatcher` trait + `DefaultCapabilityMatcher`, `UnitValidator` trait + `DefaultValidator`, `UnitStore` trait, `CoreError`
- `taba-test-harness`: `WorkloadUnitBuilder`, `DataUnitBuilder`, `PolicyUnitBuilder`, `CapabilityBuilder`, `NodeCapabilitySetBuilder`, `InMemoryUnitStore`, proptest strategies

### Added — M2: Single-node compose
- `taba-security`: Ed25519 signing/verification, `SignatureContext`, `SignedUnit<T>`, `Signer`/`Verifier` traits, `ScopeChecker` trait + `DefaultScopeChecker`, `CapabilityEnforcer` trait + `DefaultCapabilityEnforcer`, `TaintComputer` trait + `DefaultTaintComputer`, `DelegationValidator`/`DelegationManager` traits, `SoloBootstrap` trait + `DefaultSoloBootstrap`, `ShamirShare`, `CeremonyState`, `KeyRevocation`, `RevocationReason`, `Author`, `TrustDomain`, `SecurityError`
- `taba-graph`: δ-state CRDT (`CompositionGraphData`, `GraphDelta`), `Graph` trait + `DefaultGraph`, `GraphQuery` trait, `GraphSnapshot`, `MergePolicy` trait + `DefaultMergePolicy`, `MemoryMonitor` trait + `DefaultMemoryMonitor`, `Compactor` trait + `DefaultCompactor`, `InMemoryWal`, `WalEntry`, `GraphError`
- `taba-solver`: `Solver` trait + `DefaultSolver`, `ConflictDetector` trait + `DefaultConflictDetector`, `PlacementScorer` trait + `DefaultPlacementScorer`, `CycleDetector` trait + `DefaultCycleDetector`, `CapabilityFilter` trait + `DefaultCapabilityFilter`, `ResourceRanker` trait + `DefaultResourceRanker`, `PromotionEvaluator` trait + `DefaultPromotionEvaluator`, `MembershipSnapshot`, `SolverError`

### Added — M3: Persistent
- `taba-observe`: `DecisionTrailRecorder`/`DecisionTrailQuery` traits + `DefaultDecisionTrailRecorder`, `SolverReplay` trait + `DefaultSolverReplayer`, `EventEmitter` trait + `DefaultEventEmitter`, `HealthAggregator` trait + `DefaultHealthAggregator`, `PrometheusExporter` trait + `DefaultPrometheusExporter`, `AlertDispatcher` trait + `DefaultAlertDispatcher`, `NodeMetrics`, `StructuredEvent`, `EventType`, `ObserveError`
- `taba-node`: `WalManager` trait + `DiskWalManager` (CRC32C framing, segment rotation, compaction), `RuntimeExecutor` trait + `SimulatedRuntime`/`DockerRuntime`, `Reconciler` trait + `DefaultReconciler`, `HealthReporter` trait + `DefaultHealthReporter`, `ModeManager` trait + `DefaultModeManager`, `CapabilityDiscoverer` trait + `DefaultCapabilityDiscoverer`, `ArtifactFetcher` trait + `DefaultArtifactFetcher`, `TaskSpawner` trait + `DefaultTaskSpawner`, `HealthCheckOrchestrator` trait + `DefaultHealthCheckOrchestrator`, `NodeError`, `WalEntry`/`WalEntryType` (proto), `WalConfig`

### Added — M4: Multi-node
- `taba-gossip`: SWIM-based membership protocol, `MembershipProtocol`/`GossipTransport`/`MembershipView` traits, `InMemoryTransport`, signed gossip messages (`GossipMessage`, `GossipPayload`), 2-witness failure detection (DL-009), higher-incarnation-wins merge, `CapabilityAdvertiser`, `CrossDomainGossip` (stub), `FleetCommandService` (rate-limited), `GossipError`
- `taba-erasure`: Reed-Solomon over GF(2^8) via `reed-solomon-erasure` crate (DL-013), `ErasureCoder` trait + `DefaultErasureCoder`, `ShardManager` trait + `DefaultShardManager` + `InMemoryShardFetcher`, `ReconstructionScheduler` trait + `DefaultReconstructionScheduler` (priority queue, circuit breaker, backpressure), `Shard`, `ShardCriticality`, `ErasureParams`, `BackpressureState`, `ErasureError`

### Added — M5: Usable
- `taba-cli`: clap-based CLI (`init`, `apply`, `unit list/inspect/validate/archive`, `status`, `compose`, `audit provenance/trails`, `push`), TOML unit declaration parser (DL-015, all 6 levels), `LocalAuth` (keypair persistence with 0600 permissions), `LocalClient` (in-memory graph with JSON persistence), `OutputFormat` (table/JSON), `CliError`

### Added — M6: Hardened
- `taba-security` (advanced): Shamir secret sharing over GF(2^8) (`shamir.rs`), `AttestationProviderTrait` + `SoftwareAttestation`, `verify_software_attestation`, `EnrollmentCeremony` trait + `DefaultEnrollmentCeremony`, `SlsaProvenance`, `ProvenanceVerifier` trait + `DefaultProvenanceVerifier`, `sign_provenance`, `MinSlsaLevel`

### Added — M7: Migration
- `taba-k8s`: K8s manifest converter (`K8sConverter`), K8s type definitions, `ConversionReport`/`UnmappableResource`, CLI binary (`taba-k8s convert`), supports Deployment, StatefulSet, DaemonSet, Pod, Service, ConfigMap, Secret, NetworkPolicy, Role, ClusterRole, RoleBinding, ClusterRoleBinding

### Added — Post-M7 validation
- LICENSE (Apache-2.0)
- CI/CD: `.github/workflows/ci.yml` (Tier 1: fmt, clippy, deny, unit tests, BDD @smoke, integration), `.github/workflows/nightly.yml` (Tier 2: slow tests, full BDD), `.github/workflows/docs.yml` (mdbook → gh-pages)
- CONTRIBUTING.md
- rust-toolchain.toml
- deny.toml (fixed for cargo-deny 0.19)
- BDD acceptance test infrastructure (cucumber-rs 0.23, `TabaWorld`, common step definitions)
- End-to-end integration tests (15 tests: single-node, multi-node, K8s migration)
- Fidelity sweep #2 (all 14 crates audited, `specs/fidelity/INDEX.md` updated)
- Adversary implementation sweep (30 findings, 3 Critical resolved)
- OQ-005 resolved (K8s scope: NetworkPolicy→PolicyUnit, RBAC→GovernanceUnit)
- OQ-007 re-evaluated (benchmarks re-run, ~12-16% regression from security gates)
- WAL deadlock fix (re-entrant Mutex in `current_segment()`)
- CeremonyManager: replaced M2 stub with full Shamir secret sharing
- LocalClient: wired scope checker (INV-S8) and max spawn depth (INV-W3)
- Private key file permissions: 0600 on Unix (was world-readable)
- WAL compaction: write-then-delete (was delete-then-write, data loss on crash)
- Auth.rs: cfg-gated Unix-specific code for Windows compatibility
- mdbook documentation: 20+ pages covering user guide, administration,
  architecture, security, operations, API reference, and ADRs
