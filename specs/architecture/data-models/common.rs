//! Common types shared across all taba crates.
//!
//! This module defines the foundational newtypes, identifiers, and configuration
//! primitives used throughout the system. Every crate in the workspace depends
//! on these types. Newtypes enforce domain semantics at the type level --
//! a `NodeId` cannot be accidentally used where an `AuthorId` is expected.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Identity newtypes
// ---------------------------------------------------------------------------

/// Globally unique, immutable identifier for a unit.
/// Assigned at creation time and never changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub Uuid);

/// Identity of a node in the taba cluster.
/// Deterministically derived: NodeId = SHA-256(Ed25519_public_key_bytes),
/// truncated to 128 bits, encoded as UUID v8. This derivation is platform-
/// independent and must produce identical results on all architectures.
/// Used as tiebreaker in partition resolution (lexicographic ordering, INV-C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub Uuid);

/// Identity of an authenticated author with scoped authority.
/// Bound to an Ed25519 key pair and scoped by (unit type, trust domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorId(pub Uuid);

/// Identity of a trust domain -- an authorization boundary.
/// Trust domains are themselves governance units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrustDomainId(pub Uuid);

/// Identity of a taba cluster.
/// Bound into signature context to prevent cross-cluster replay (INV-S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClusterId(pub Uuid);

/// Identity of a Shamir ceremony instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CeremonyId(pub Uuid);

/// Identity of an erasure-coded shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ShardId(pub Uuid);

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

/// Wall-clock timestamp for duration-based operations (retention, compliance).
/// NOT used for causal ordering — use LogicalClock for that (INV-T2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WallTime {
    /// Milliseconds since Unix epoch.
    pub millis: u64,
}

/// Lamport-style logical clock for causal ordering (INV-T1).
/// Monotonically increasing per node. On inter-node communication:
/// `local = max(local, remote) + 1`.
/// Authoritative for ordering, key revocation, signature validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogicalClock(pub u64);

impl LogicalClock {
    /// Advance the clock on local event.
    pub fn tick(&mut self) { self.0 += 1; }
    /// Sync with a remote clock (max + 1).
    pub fn sync(&mut self, remote: LogicalClock) {
        self.0 = self.0.max(remote.0) + 1;
    }
}

/// Every event records this triple (INV-T2).
/// Logical clock is authoritative for ordering.
/// Wall time + timezone are authoritative for retention/compliance.
/// Wall time + timezone are informational for human display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualClockEvent {
    pub logical_clock: LogicalClock,
    pub wall_time: WallTime,
    pub timezone: String,
}

/// Legacy compatibility alias. Use DualClockEvent for new code.
pub type Timestamp = DualClockEvent;

/// Clock quality reported by the node (INV-N1 capability).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ClockQuality {
    /// NTP-synced (seconds accuracy).
    Ntp,
    /// PTP-synced (microseconds accuracy).
    Ptp,
    /// GPS-disciplined (nanoseconds accuracy).
    Gps,
    /// No synchronization.
    Unsync,
}

// ---------------------------------------------------------------------------
// Fixed-point arithmetic
// ---------------------------------------------------------------------------

/// Fixed-point parts-per-million value for deterministic solver arithmetic.
///
/// All solver scoring and placement calculations use this type instead of
/// floating-point to guarantee identical results across platforms (INV-C3, A2).
/// Scale factor: 10^6. A value of 1_000_000 represents 1.0.
/// Division rounds toward zero (Rust integer division default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ppm(pub u64);

/// Signed fixed-point ppm for calculations that may go negative
/// (e.g., score deltas, cost differences).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SignedPpm(pub i64);

// ---------------------------------------------------------------------------
// Version tracking
// ---------------------------------------------------------------------------

/// Monotonically increasing version number for policy supersession chains
/// and solver version gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version(pub u64);

/// Validity window for bounded tasks and delegation tokens.
/// Optional on services (omitted = valid indefinitely per INV-W1).
/// Set on bounded tasks (auto-terminate on deadline per INV-W2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidityWindow {
    /// Logical clock range for causal validity (authoritative).
    pub lc_range: Option<(LogicalClock, LogicalClock)>,
    /// Wall-time deadline for human/compliance purposes.
    pub wall_time_deadline: Option<WallTime>,
}

/// Identity of a delegation token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DelegationTokenId(pub Uuid);

/// Identity of a policy unit (used for promotion policy dedup).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PolicyId(pub Uuid);

/// SHA256 content hash for artifact integrity and dedup (INV-A1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest(pub String);

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Cluster-wide configuration parameters.
/// Loaded from TOML on node startup and propagated via gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Unique identifier for this cluster.
    pub cluster_id: ClusterId,

    /// Resilience percentage for erasure coding.
    /// k = ceil(N * (1 - resilience_pct / 100)) per INV-R4.
    pub resilience_pct: u8,

    /// Maximum memory (bytes) for the active graph per node (INV-R6).
    /// Auto-compaction triggers at 80% of this limit.
    pub graph_memory_limit_bytes: u64,

    /// Shamir ceremony parameters.
    pub shamir_total_shares: u8,
    pub shamir_threshold: u8,

    /// Maximum depth for hierarchical data units.
    /// Enforced at 16 levels per domain model.
    pub max_data_hierarchy_depth: u8,

    /// Gossip protocol tuning.
    pub gossip_interval: Duration,
    pub gossip_suspicion_timeout: Duration,

    /// Number of independent witnesses required before declaring a node failed (INV-R3).
    pub witness_count: u8,

    /// Maximum spawn depth for bounded tasks (INV-W3, default 4).
    pub max_spawn_depth: u8,

    /// Optional revocation grace window in logical clock delta (INV-S3).
    /// None = pure causal model (default).
    pub revocation_grace_window: Option<u64>,

    /// Fleet command rate limit: minimum logical clock delta between
    /// commands of the same type (F-A314).
    pub fleet_command_rate_limit_lc: u64,
}

/// Per-node local configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Directory for WAL storage.
    pub wal_dir: String,

    /// Bind address for peer communication.
    pub listen_addr: String,

    /// Seed nodes for initial gossip bootstrap.
    pub seed_nodes: Vec<String>,

    /// Whether TPM attestation is required on this node (A5).
    pub require_tpm: bool,

    /// Environment tag for this node (env:dev, env:test, env:prod).
    pub environment: Option<String>,

    /// Custom freeform tags (INV-N4).
    pub custom_tags: Vec<(String, String)>,

    /// Archive backend configuration (None = no archival, tombstones only).
    pub archive_backend: Option<ArchiveBackendConfig>,
}

/// Configuration for the archive backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ArchiveBackendConfig {
    /// Local filesystem path.
    LocalPath { path: String },
    /// S3-compatible object store.
    S3 { endpoint: String, bucket: String, region: String },
}
