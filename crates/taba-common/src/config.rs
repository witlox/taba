//! Configuration types for taba clusters and nodes.
//!
//! These structures are loaded from TOML on node startup and propagated
//! via gossip (INV-A7: human-readable config). The types here are plain
//! data definitions with `Default` impls — actual TOML parsing is
//! deferred until the required dependencies are available.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::ClusterId;

/// Cluster-wide configuration parameters.
/// Loaded from TOML on node startup and propagated via gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Unique identifier for this cluster.
    pub cluster_id: ClusterId,

    /// Resilience percentage for erasure coding.
    /// `k = ceil(N * (1 - resilience_pct / 100))` per INV-R4.
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

    /// Gossip protocol tuning (DL-016). See `gossip::GossipParams` for full
    /// docs.
    ///
    /// Default: `interval=500ms`, `suspicion_timeout=5s`, `witness_count=2`,
    /// `indirect_probe_count=3`, `retransmit_multiplier=4`,
    /// `max_piggyback_entries=8`, `suspicion_multiplier=4`. Effective
    /// suspicion timeout auto-scales:
    /// `max(base, suspicion_mult × ceil(log2(N)) × interval)`.
    pub gossip_interval: Duration,

    /// Suspicion timeout base value. Auto-scales with cluster size.
    pub gossip_suspicion_timeout: Duration,

    /// Number of independent witnesses required before declaring a node
    /// failed (INV-R3). Default: 2.
    pub witness_count: u8,

    /// Peers asked for indirect probes on direct ping failure. Default: 3.
    pub indirect_probe_count: u8,

    /// Piggyback rounds = `retransmit_multiplier × log2(N)`. Default: 4.
    pub retransmit_multiplier: u8,

    /// Maximum membership changes piggybacked per message. Default: 8.
    pub max_piggyback_entries: u8,

    /// Suspicion timeout multiplier for auto-scaling with cluster size.
    /// Default: 4.
    pub suspicion_multiplier: u8,

    /// Maximum spawn depth for bounded tasks (INV-W3, default 4).
    pub max_spawn_depth: u8,

    /// Optional revocation grace window in logical clock delta (INV-S3).
    /// `None` = pure causal model (default).
    pub revocation_grace_window: Option<u64>,

    /// Fleet command rate limit: minimum logical clock delta between
    /// commands of the same type (F-A314).
    pub fleet_command_rate_limit_lc: u64,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            // Nil UUID — operator must assign a real cluster ID.
            cluster_id: ClusterId(Uuid::nil()),
            resilience_pct: 33,
            graph_memory_limit_bytes: 1_073_741_824, // 1 GiB
            shamir_total_shares: 5,
            shamir_threshold: 3,
            max_data_hierarchy_depth: 16,
            gossip_interval: Duration::from_millis(500),
            gossip_suspicion_timeout: Duration::from_secs(5),
            witness_count: 2,
            indirect_probe_count: 3,
            retransmit_multiplier: 4,
            max_piggyback_entries: 8,
            suspicion_multiplier: 4,
            max_spawn_depth: 4,
            revocation_grace_window: None,
            fleet_command_rate_limit_lc: 1000,
        }
    }
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

    /// Environment tag for this node (`env:dev`, `env:test`, `env:prod`).
    pub environment: Option<String>,

    /// Custom freeform tags (INV-N4).
    pub custom_tags: Vec<(String, String)>,

    /// Archive backend configuration (`None` = no archival, tombstones only).
    pub archive_backend: Option<ArchiveBackendConfig>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            wal_dir: "./wal".to_string(),
            listen_addr: "0.0.0.0:7946".to_string(),
            seed_nodes: Vec::new(),
            require_tpm: false,
            environment: None,
            custom_tags: Vec::new(),
            archive_backend: None,
        }
    }
}

/// Configuration for the archive backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ArchiveBackendConfig {
    /// Local filesystem path.
    LocalPath { path: String },
    /// S3-compatible object store.
    S3 {
        endpoint: String,
        bucket: String,
        region: String,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_config_default() {
        let config = ClusterConfig::default();

        assert_eq!(config.resilience_pct, 33);
        assert_eq!(config.graph_memory_limit_bytes, 1_073_741_824);
        assert_eq!(config.shamir_total_shares, 5);
        assert_eq!(config.shamir_threshold, 3);
        assert_eq!(config.max_data_hierarchy_depth, 16);
        assert_eq!(config.gossip_interval, Duration::from_millis(500));
        assert_eq!(config.gossip_suspicion_timeout, Duration::from_secs(5));
        assert_eq!(config.witness_count, 2);
        assert_eq!(config.indirect_probe_count, 3);
        assert_eq!(config.retransmit_multiplier, 4);
        assert_eq!(config.max_piggyback_entries, 8);
        assert_eq!(config.suspicion_multiplier, 4);
        assert_eq!(config.max_spawn_depth, 4);
        assert_eq!(config.revocation_grace_window, None);
        assert_eq!(config.fleet_command_rate_limit_lc, 1000);
        // cluster_id defaults to nil — operator must assign a real one.
        assert_eq!(config.cluster_id, ClusterId(Uuid::nil()));
    }

    #[test]
    fn test_node_config_default() {
        let config = NodeConfig::default();

        assert_eq!(config.wal_dir, "./wal");
        assert_eq!(config.listen_addr, "0.0.0.0:7946");
        assert!(config.seed_nodes.is_empty());
        assert!(!config.require_tpm);
        assert!(config.environment.is_none());
        assert!(config.custom_tags.is_empty());
        assert!(config.archive_backend.is_none());
    }

    #[test]
    fn test_archive_backend_config_variants() {
        let local = ArchiveBackendConfig::LocalPath {
            path: "/var/taba/archive".to_string(),
        };
        let s3 = ArchiveBackendConfig::S3 {
            endpoint: "https://s3.example.com".to_string(),
            bucket: "taba-archive".to_string(),
            region: "us-east-1".to_string(),
        };

        // Both variants serialize to JSON
        let local_json = serde_json::to_string(&local).expect("serialize LocalPath");
        let s3_json = serde_json::to_string(&s3).expect("serialize S3");
        assert!(local_json.contains("LocalPath"));
        assert!(s3_json.contains("S3"));
    }

    #[test]
    fn test_cluster_config_serialization_roundtrip() {
        // ClusterConfig does not derive PartialEq, so we compare the JSON
        // strings before and after round-tripping.
        let config = ClusterConfig::default();
        let json1 = serde_json::to_string(&config).expect("serialize ClusterConfig");
        let decoded: ClusterConfig =
            serde_json::from_str(&json1).expect("deserialize ClusterConfig");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize ClusterConfig");
        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_node_config_serialization_roundtrip() {
        // NodeConfig does not derive PartialEq, so we compare the JSON
        // strings before and after round-tripping.
        let config = NodeConfig {
            wal_dir: "/data/wal".to_string(),
            listen_addr: "10.0.0.1:7946".to_string(),
            seed_nodes: vec!["10.0.0.2:7946".to_string(), "10.0.0.3:7946".to_string()],
            require_tpm: true,
            environment: Some("env:prod".to_string()),
            custom_tags: vec![("arch".to_string(), "arm64".to_string())],
            archive_backend: Some(ArchiveBackendConfig::LocalPath {
                path: "/archive".to_string(),
            }),
        };
        let json1 = serde_json::to_string(&config).expect("serialize NodeConfig");
        let decoded: NodeConfig = serde_json::from_str(&json1).expect("deserialize NodeConfig");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize NodeConfig");
        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }
}
