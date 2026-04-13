// taba-node: Local node operations — reconciliation, WAL, health, and
// operational mode management.
//
// This crate owns what happens on a single node: converging actual state
// toward desired state (reconciliation), persisting mutations to the WAL,
// reporting health, and managing operational mode transitions.

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct NodeId(/* opaque */);
pub struct UnitId(/* opaque */);
pub struct Unit(/* opaque */);

/// A placement assigned to this node by the solver.
pub struct LocalPlacement {
    pub unit: UnitId,
    pub desired_state: RuntimeState,
}

/// State of a unit as seen by the local reconciler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Note: this is RUNTIME state (what's running on this node), NOT lifecycle
/// state (Declared -> Composed -> Placed etc. defined in core::UnitState).
pub enum RuntimeState {
    /// Placement received, not yet started.
    Pending,
    /// Unit is starting (container, microVM, Wasm, etc.).
    Starting,
    /// Unit is running and healthy.
    Running,
    /// Unit is draining (preparing to stop).
    Draining,
    /// Unit has stopped.
    Stopped,
    /// Unit failed to start or crashed.
    Failed,
}

/// The operational mode of this node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationalMode {
    /// All operations permitted.
    Normal,
    /// Authoring/composition/placement frozen. Drain and evacuation only.
    /// Entered when erasure threshold exceeded (INV-R4), memory limit
    /// exceeded (INV-R6), or operator-triggered.
    Degraded { reason: DegradedReason },
    /// Gradual re-coding underway. Placement throttled. Auto-transitions
    /// to Normal when recovery complete.
    Recovery,
}

/// Why the node entered degraded mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradedReason {
    /// Erasure threshold exceeded (INV-R4).
    ErasureThresholdExceeded,
    /// Graph memory limit exceeded (INV-R6).
    MemoryLimitExceeded,
    /// WAL corruption or disk full (FM-07).
    WalFailure,
    /// Operator-triggered.
    OperatorTriggered,
}

/// Drift between desired and actual state for a single unit.
pub struct Drift {
    pub unit: UnitId,
    pub desired: RuntimeState,
    pub actual: RuntimeState,
}

/// WAL entry types (INV-C4, DL-008).
pub enum WalEntry {
    /// Unit verified and merged into graph.
    Merged(Unit),
    /// Unit verified but references not yet satisfied. Includes the list
    /// of missing reference IDs.
    Pending(Unit, Vec<UnitId>),
    /// A previously pending unit promoted to active after references arrived.
    Promoted(UnitId),
}

/// WAL replay position (for crash recovery).
pub struct WalPosition(pub u64);

/// Health status reported by a node.
pub struct HealthStatus {
    pub node: NodeId,
    pub mode: OperationalMode,
    pub units_running: u32,
    pub units_failed: u32,
    pub wal_size_bytes: u64,
    pub graph_memory_bytes: u64,
    pub graph_memory_limit_bytes: u64,
    /// Percentage of memory limit used (0-100).
    pub memory_pressure_pct: u8,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum NodeError {
    /// Reconciliation failed for a specific unit.
    ReconciliationFailed { unit: UnitId, reason: String },
    /// WAL write failed (disk full, corruption, etc.).
    WalWriteFailed { reason: String },
    /// WAL replay encountered a corrupt entry.
    WalCorrupted { position: WalPosition, reason: String },
    /// Mode transition is not permitted from current state.
    InvalidModeTransition { from: OperationalMode, to: OperationalMode },
    /// Node is in degraded mode; operation not permitted.
    DegradedModeRestriction { operation: String },
    /// Unit not found on this node.
    UnitNotFound { id: UnitId },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Reconciles desired state (from solver placements) with actual state
/// (what is running on this node).
///
/// Each node reconciles itself independently — there is no central
/// reconciliation loop. Drift is detected locally and corrected locally.
pub trait Reconciler {
    /// Reconcile all placements assigned to this node.
    ///
    /// Compares desired state (solver placements) against actual state
    /// (running processes). For each drift:
    /// - Desired=Running, Actual=Stopped -> start the unit
    /// - Desired=Stopped, Actual=Running -> drain then stop the unit
    /// - Desired=Running, Actual=Failed -> restart (respecting failure semantics)
    ///
    /// Does NOT reconcile during Degraded mode (only drain/evacuation
    /// permitted). Returns `NodeError::DegradedModeRestriction` if
    /// reconciliation is attempted in degraded mode.
    async fn reconcile(&self, placements: &[LocalPlacement]) -> Result<Vec<Drift>, NodeError>;

    /// Detect drift between desired and actual state without correcting it.
    ///
    /// Read-only inspection. Returns all units where desired != actual.
    async fn detect_drift(&self, placements: &[LocalPlacement]) -> Vec<Drift>;

    /// Drain a specific unit (graceful shutdown using declared on_shutdown).
    ///
    /// Transitions the unit to Draining state, waits for the declared
    /// drain timeout, then stops it. Respects the unit's failure semantics
    /// declaration.
    async fn drain(&self, unit: &UnitId) -> Result<(), NodeError>;

    /// Evacuate all units from this node (pre-leave or pre-degraded).
    ///
    /// Drains all running units. Used before `gossip::MembershipProtocol::leave`
    /// to give the system time to re-place units elsewhere.
    async fn evacuate(&self) -> Result<(), NodeError>;
}

/// Write-ahead log for local persistence of graph state.
///
/// WAL-before-effect (INV-C4): every mutation is persisted to WAL atomically
/// before its effects become visible to local queries. Mutations form a
/// partial (causal) order, not a total order.
///
/// Entry types: Merged, Pending, Promoted (DL-008).
pub trait WalManager {
    /// Append an entry to the WAL atomically.
    ///
    /// The entry is durable after this call returns — method returns only
    /// after fsync() or equivalent durability guarantee. If the write fails
    /// (disk full, I/O error), the node should enter degraded mode.
    ///
    /// Returns the WAL position of the appended entry.
    /// Returns `NodeError::WalWriteFailed` on I/O failure.
    async fn append(&self, entry: WalEntry) -> Result<WalPosition, NodeError>;

    /// Replay the WAL from a given position forward.
    ///
    /// Used on node restart to reconstruct in-memory graph state. Entries
    /// are replayed in WAL order. Corrupt entries cause replay to stop and
    /// return `NodeError::WalCorrupted` with the position of the bad entry.
    ///
    /// The callback is invoked for each entry in order.
    async fn replay(
        &self,
        from: WalPosition,
        callback: &mut dyn FnMut(WalEntry) -> Result<(), NodeError>,
    ) -> Result<WalPosition, NodeError>;

    /// Compact the WAL by removing entries older than the given position.
    ///
    /// Safe to call only after all entries up to `before` have been
    /// durably reflected in the graph. Frees disk space.
    async fn compact(&self, before: WalPosition) -> Result<u64, NodeError>;

    /// Get the current WAL size in bytes.
    fn size_bytes(&self) -> u64;

    /// Get the latest WAL position (most recent entry).
    fn latest_position(&self) -> WalPosition;
}

/// Reports local node health status.
///
/// Health information is consumed by the gossip protocol (heartbeats) and
/// by the solver (node scoring considers health). This trait is read-only.
pub trait HealthReporter {
    /// Produce the current health status snapshot.
    ///
    /// Includes operational mode, running/failed unit counts, WAL size,
    /// graph memory usage, and memory pressure percentage. This is the
    /// payload included in gossip heartbeats.
    fn report(&self) -> HealthStatus;

    /// Check whether the node is approaching memory limit.
    ///
    /// Returns `true` if memory usage exceeds 80% of limit (the
    /// auto-compaction trigger threshold per INV-R6).
    fn is_memory_pressured(&self) -> bool;

    /// Check whether the node should enter degraded mode.
    ///
    /// Returns `Some(reason)` if any degraded-mode trigger condition is met:
    /// - Erasure threshold exceeded (INV-R4)
    /// - Memory limit exceeded (INV-R6)
    /// - WAL failure detected
    ///
    /// Returns `None` if the node is healthy.
    fn should_degrade(&self) -> Option<DegradedReason>;
}

/// Manages operational mode transitions.
///
/// Mode transitions follow a state machine:
/// - Normal -> Degraded (any trigger condition)
/// - Normal -> Recovery (not valid — recovery is entered from Degraded)
/// - Degraded -> Recovery (when trigger condition is resolved, re-coding starts)
/// - Recovery -> Normal (when re-coding completes)
/// - Recovery -> Degraded (if a new trigger fires during recovery)
/// - Degraded -> Normal (only via operator override — not automatic)
pub trait ModeManager {
    /// Get the current operational mode.
    fn current_mode(&self) -> OperationalMode;

    /// Transition to a new operational mode.
    ///
    /// Validates the transition against the state machine. Returns
    /// `NodeError::InvalidModeTransition` if the transition is not
    /// permitted from the current state.
    ///
    /// Side effects of transitions:
    /// - To Degraded: freezes authoring/composition/placement
    /// - To Recovery: enables throttled placement, starts re-coding
    /// - To Normal: unfreezes all operations
    async fn transition(&self, to: OperationalMode) -> Result<(), NodeError>;

    /// Check whether a specific operation is permitted in the current mode.
    ///
    /// In Degraded mode, only drain and evacuation are permitted.
    /// In Recovery mode, placement is throttled.
    /// In Normal mode, everything is permitted.
    fn is_operation_permitted(&self, operation: &str) -> bool;
}

// ---------------------------------------------------------------------------
// New traits for analyst-session concepts
// ---------------------------------------------------------------------------

pub struct NodeCapabilitySet(/* opaque */);
pub struct ArtifactRef(/* opaque */);
pub struct ContentDigest(/* opaque */);
pub struct DelegationToken(/* opaque */);
pub struct HealthCheck(/* opaque */);
pub struct HealthCheckResult(/* opaque */);
pub struct LogicalClock(/* opaque */);

/// Auto-discovers node capabilities on startup (INV-N1).
///
/// Probes the system for available runtimes, hardware, OS features,
/// and reports a capability set. Cached locally. Re-probed on
/// `taba refresh` or fleet-wide refresh command.
pub trait CapabilityDiscoverer {
    /// Probe the system and return discovered capabilities.
    ///
    /// Probes: Docker/Podman socket, K8s API, wasmtime/wasmer,
    /// GPU (nvidia-smi), TPM, OS/arch, privilege level.
    /// Returns the full capability set.
    async fn discover(&self) -> Result<NodeCapabilitySet, NodeError>;

    /// Re-probe capabilities (invalidate cache).
    async fn refresh(&self) -> Result<NodeCapabilitySet, NodeError>;
}

/// Fetches workload artifacts with peer cache (INV-A1, INV-A2).
///
/// Fetch order: peer cache → external source. Digest verification
/// mandatory after fetch (INV-A1). Push mode for air-gapped environments.
pub trait ArtifactFetcher {
    /// Fetch an artifact by reference and expected digest.
    ///
    /// 1. Check peer cache (local, then peers via gossip inventory)
    /// 2. If miss, fetch from external source (artifact_ref URL/registry)
    /// 3. Verify SHA256 digest after fetch (INV-A1)
    /// 4. Cache locally for future peer requests
    ///
    /// Returns path to the fetched artifact.
    /// Returns error on: digest mismatch, all sources exhausted, network failure.
    async fn fetch(
        &self,
        artifact_ref: &ArtifactRef,
        expected_digest: &ContentDigest,
    ) -> Result<std::path::PathBuf, NodeError>;

    /// Push an artifact to the peer cache (air-gapped/dev mode).
    ///
    /// The artifact is content-addressed (SHA256) and distributed
    /// to peer nodes via P2P.
    async fn push(
        &self,
        artifact_path: &std::path::Path,
    ) -> Result<ContentDigest, NodeError>;

    /// Check if a specific artifact is in the local peer cache.
    fn is_cached(&self, digest: &ContentDigest) -> bool;
}

/// Spawns bounded tasks at runtime via delegation tokens (INV-W4).
///
/// The spawner uses a delegation token (pre-signed by the author at
/// placement time) to sign spawned tasks. It never holds the author's
/// private key.
pub trait TaskSpawner {
    /// Spawn a bounded task from a running service.
    ///
    /// 1. Validate delegation token (calls taba-security DelegationValidator)
    /// 2. Sign the spawned task unit using the delegation token
    /// 3. Insert the spawned task into the graph
    /// 4. Track spawn count against token limit
    ///
    /// Returns error if: token expired, spawn limit exceeded, governance
    /// block (INV-W4a), or spawn depth exceeded (INV-W3).
    async fn spawn(
        &self,
        token: &DelegationToken,
        task_unit: Unit, // the task to spawn
    ) -> Result<UnitId, NodeError>;
}

/// Orchestrates health checks for workloads (INV-O3, progressive).
///
/// Default: OS-level process monitoring (is process alive?).
/// Declared: HTTP probe, TCP connect, or custom command.
pub trait HealthCheckOrchestrator {
    /// Register a health check for a running workload.
    fn register(&self, unit_id: &UnitId, check: &HealthCheck);

    /// Unregister a health check (unit terminated).
    fn unregister(&self, unit_id: &UnitId);

    /// Run all registered health checks.
    ///
    /// Returns results for each check. Failed checks are reported
    /// to the graph as health status events.
    async fn run_checks(&self) -> Vec<(UnitId, HealthCheckResult)>;
}
