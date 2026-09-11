//! Error types and identifiers for the erasure coding crate.
//!
//! [`ErasureError`] is the single error type returned by all erasure
//! operations. [`ShardGroupId`] identifies a set of shards that encode
//! the same underlying data.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// ShardGroupId
// ---------------------------------------------------------------------------

/// Identifier for a shard group — all shards encoding the same data.
///
/// A new [`Uuid`] is generated for each distinct encoding operation.
/// Shards within the same group carry indices `0..n-1` and can be
/// reassembled by the [`crate::coding::ErasureCoder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShardGroupId(pub Uuid);

impl ShardGroupId {
    /// Generate a new random shard-group identifier.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ShardGroupId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ShardGroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// ErasureError
// ---------------------------------------------------------------------------

/// Errors that can occur during erasure coding, distribution, or
/// reconstruction.
///
/// All variants map to the categories defined in
/// `specs/architecture/error-taxonomy.md`.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ErasureError {
    /// Not enough shards to reconstruct (need `k`, have fewer).
    ///
    /// Maps to FM-02: the system enters degraded mode when
    /// reconstruction is impossible (INV-R4).
    #[error("insufficient shards: need {need}, have {have}")]
    InsufficientShards {
        /// Minimum number of shards required (`k`).
        need: u32,
        /// Number of shards actually available.
        have: u32,
    },

    /// Shard data is corrupted (checksum mismatch).
    ///
    /// The corrupted shard must be fetched from a healthy peer.
    #[error("corrupt shard {group} at index {index}")]
    CorruptShard {
        /// The shard group containing the corrupt shard.
        group: ShardGroupId,
        /// Index of the corrupt shard within the group.
        index: u32,
    },

    /// Encoding parameters are invalid (`k == 0`, `m == 0`, out of
    /// GF(2^8) range, etc.).
    #[error("invalid erasure parameters: {reason}")]
    InvalidParams {
        /// Human-readable explanation of why the parameters are invalid.
        reason: String,
    },

    /// No nodes available to distribute shards to.
    #[error("no available nodes for shard distribution")]
    NoAvailableNodes,

    /// Reconstruction circuit breaker tripped — queue depth exceeded
    /// the configured threshold (INV-R1, FM-13).
    ///
    /// New reconstruction jobs are refused until an operator resets
    /// the circuit breaker.
    #[error("circuit breaker tripped: queue depth {queue_depth} exceeds threshold {threshold}")]
    CircuitBreakerTripped {
        /// Current queue depth when the breaker tripped.
        queue_depth: usize,
        /// Configured maximum queue depth.
        threshold: usize,
    },

    /// Network or storage error during shard transfer.
    #[error("shard transfer error from node {node:?}: {reason}")]
    TransferError {
        /// The node from which the transfer failed.
        node: taba_common::NodeId,
        /// Human-readable reason for the failure.
        reason: String,
    },

    /// Re-coding failed — insufficient healthy shards for the new
    /// parameters, or reconstruction of the original data failed.
    #[error("recoding failed: {reason}")]
    RecodingFailed {
        /// Human-readable explanation of the failure.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_group_id_new_unique() {
        let a = ShardGroupId::new();
        let b = ShardGroupId::new();
        assert_ne!(a, b, "two random ShardGroupIds should differ");
    }

    #[test]
    fn test_shard_group_id_default_is_new() {
        let a = ShardGroupId::default();
        // Default should produce a valid (non-nil) UUID.
        assert_ne!(a.0, Uuid::nil());
    }

    #[test]
    fn test_shard_group_id_display() {
        let id = ShardGroupId::new();
        let s = format!("{id}");
        assert_eq!(s, id.0.to_string());
    }

    #[test]
    fn test_shard_group_id_ordering() {
        let low = ShardGroupId(Uuid::from_u128(1));
        let high = ShardGroupId(Uuid::from_u128(2));
        assert!(low < high);
    }

    #[test]
    fn test_shard_group_id_serde_roundtrip() {
        let id = ShardGroupId::new();
        let json = serde_json::to_string(&id).expect("serialize ShardGroupId");
        let decoded: ShardGroupId = serde_json::from_str(&json).expect("deserialize ShardGroupId");
        assert_eq!(id, decoded);
    }

    #[test]
    fn test_all_error_variants_display() {
        let group = ShardGroupId::new();
        let node = taba_common::NodeId(Uuid::new_v4());

        let insufficient = ErasureError::InsufficientShards { need: 5, have: 3 };
        assert!(
            insufficient.to_string().contains("need 5"),
            "InsufficientShards display: {insufficient}"
        );
        assert!(
            insufficient.to_string().contains("have 3"),
            "InsufficientShards display: {insufficient}"
        );

        let corrupt = ErasureError::CorruptShard { group, index: 7 };
        assert!(
            corrupt.to_string().contains(&group.to_string()),
            "CorruptShard display: {corrupt}"
        );
        assert!(
            corrupt.to_string().contains('7'),
            "CorruptShard display: {corrupt}"
        );

        let invalid = ErasureError::InvalidParams {
            reason: "k is zero".to_string(),
        };
        assert!(
            invalid.to_string().contains("k is zero"),
            "InvalidParams display: {invalid}"
        );

        let no_nodes = ErasureError::NoAvailableNodes;
        assert!(
            no_nodes.to_string().contains("no available nodes"),
            "NoAvailableNodes display: {no_nodes}"
        );

        let cb = ErasureError::CircuitBreakerTripped {
            queue_depth: 100,
            threshold: 50,
        };
        assert!(
            cb.to_string().contains("100"),
            "CircuitBreakerTripped display: {cb}"
        );
        assert!(
            cb.to_string().contains("50"),
            "CircuitBreakerTripped display: {cb}"
        );

        let transfer = ErasureError::TransferError {
            node,
            reason: "timeout".to_string(),
        };
        assert!(
            transfer.to_string().contains(&format!("{node:?}")),
            "TransferError display: {transfer}"
        );
        assert!(
            transfer.to_string().contains("timeout"),
            "TransferError display: {transfer}"
        );

        let recode = ErasureError::RecodingFailed {
            reason: "too few healthy shards".to_string(),
        };
        assert!(
            recode.to_string().contains("too few healthy shards"),
            "RecodingFailed display: {recode}"
        );
    }

    #[test]
    fn test_error_equality() {
        let err_a = ErasureError::NoAvailableNodes;
        let err_b = ErasureError::NoAvailableNodes;
        assert_eq!(err_a, err_b);

        let err_c = ErasureError::InsufficientShards { need: 3, have: 1 };
        let err_d = ErasureError::InsufficientShards { need: 3, have: 1 };
        assert_eq!(err_c, err_d);

        let err_e = ErasureError::InsufficientShards { need: 3, have: 2 };
        assert_ne!(err_c, err_e);
    }
}
