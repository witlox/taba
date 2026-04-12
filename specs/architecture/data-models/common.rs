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
/// Derived from the node's Ed25519 public key at join time.
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

/// Monotonic logical timestamp used throughout the system.
/// Combines wall-clock and a logical counter for causal ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp {
    /// Milliseconds since Unix epoch (wall clock).
    pub millis: u64,
    /// Logical counter for ordering events at the same millisecond.
    pub counter: u64,
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

/// Validity window for signatures and units.
/// A unit is valid only within this window (INV-S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ValidityWindow {
    /// Earliest time this unit/signature is valid.
    pub not_before: Timestamp,
    /// Latest time this unit/signature is valid.
    pub not_after: Timestamp,
}

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
}
