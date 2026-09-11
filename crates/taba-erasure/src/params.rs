//! Erasure coding parameters and fleet-size adaptation.
//!
//! [`ErasureParams`] describes a k-of-n Reed-Solomon configuration over
//! GF(2^8). The parameters adapt to fleet size: `k = N × (1 − R/100)`
//! where `R` is the configured resilience percentage (INV-R4). For
//! clusters larger than 128 nodes, shards are distributed to a
//! representative subset to stay within the GF(2^8) limit of 256
//! total shards.

use serde::{Deserialize, Serialize};

use crate::error::ErasureError;

// ---------------------------------------------------------------------------
// ErasureParams
// ---------------------------------------------------------------------------

/// Erasure coding parameters for the current cluster configuration.
///
/// k-of-n coding where parameters adapt to fleet size. `k` data shards
/// are required for reconstruction; `m = n − k` parity shards provide
/// redundancy. The cluster tolerates up to `m` simultaneous node
/// failures without data loss (INV-R4).
///
/// # GF(2^8) limits
///
/// The underlying Reed-Solomon code operates in GF(2^8), limiting
/// `total_shards` to 256. For clusters with more than 128 nodes,
/// [`compute_params`] distributes shards to a subset of nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ErasureParams {
    /// Total number of shards (n). Equals the number of active nodes
    /// participating in erasure coding (capped at 128 for large
    /// clusters).
    pub total_shards: u32,
    /// Minimum shards required for reconstruction (k).
    ///
    /// Computed as `floor(N × (1 − R/100))` using integer arithmetic.
    pub data_shards: u32,
    /// Number of parity shards (m = n − k). The cluster can tolerate
    /// this many simultaneous node failures without data loss.
    pub parity_shards: u32,
    /// The configured resilience percentage from `ClusterConfig`.
    ///
    /// `R = 33` means the cluster tolerates roughly one-third node
    /// failures.
    pub resilience_pct: u8,
}

impl ErasureParams {
    /// Validates that these parameters are within GF(2^8) limits and
    /// internally consistent.
    ///
    /// # Errors
    ///
    /// Returns [`ErasureError::InvalidParams`] if `data_shards` is zero,
    /// `parity_shards` is zero, `total_shards` does not equal
    /// `data_shards + parity_shards`, or `total_shards` exceeds 256.
    pub fn validate(&self) -> Result<(), ErasureError> {
        if self.data_shards == 0 {
            return Err(ErasureError::InvalidParams {
                reason: "data_shards must be > 0".to_string(),
            });
        }
        if self.parity_shards == 0 {
            return Err(ErasureError::InvalidParams {
                reason: "parity_shards must be > 0".to_string(),
            });
        }
        if self.data_shards + self.parity_shards != self.total_shards {
            return Err(ErasureError::InvalidParams {
                reason: format!(
                    "total_shards ({}) must equal data_shards ({}) + parity_shards ({})",
                    self.total_shards, self.data_shards, self.parity_shards
                ),
            });
        }
        if self.total_shards > 256 {
            return Err(ErasureError::InvalidParams {
                reason: format!(
                    "total_shards ({}) exceeds GF(2^8) limit of 256",
                    self.total_shards
                ),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// compute_params
// ---------------------------------------------------------------------------

/// Compute erasure parameters for a given fleet size and resilience
/// percentage (INV-R4).
///
/// The data-shard count is `k = floor(N × (1 − R/100))` (integer
/// arithmetic), and the parity-shard count is `m = n − k` where `n`
/// is the number of participating nodes.
///
/// For `N ≤ 128`, every node participates (`n = N`). For `N > 128`,
/// shards are distributed to a representative subset of 128 nodes to
/// stay comfortably within the GF(2^8) limit.
///
/// # Errors
///
/// Returns [`ErasureError::InvalidParams`] if:
/// - `node_count < 2` (need at least 2 nodes for erasure coding)
/// - `resilience_pct > 100`
/// - The computed `k` exceeds 255 (would leave no room for parity)
/// - The computed `n` exceeds 255 (GF(2^8) limit)
///
/// # Examples
///
/// ```
/// use taba_erasure::params::compute_params;
///
/// let params = compute_params(9, 33).unwrap();
/// assert_eq!(params.data_shards, 6);
/// assert_eq!(params.parity_shards, 3);
/// assert_eq!(params.total_shards, 9);
/// ```
pub fn compute_params(node_count: u32, resilience_pct: u32) -> Result<ErasureParams, ErasureError> {
    if node_count < 2 {
        return Err(ErasureError::InvalidParams {
            reason: format!("node_count ({node_count}) must be >= 2"),
        });
    }
    if resilience_pct > 100 {
        return Err(ErasureError::InvalidParams {
            reason: format!("resilience_pct ({resilience_pct}) must be <= 100"),
        });
    }

    // For N > 128, distribute to a representative subset of 128 nodes.
    // GF(2^8) supports up to 256 shards; capping at 128 leaves ample
    // headroom and matches the spec's "distribute to subset" clause.
    let n = if node_count <= 128 { node_count } else { 128 };

    // k = floor(N * (1 - R/100)) using integer arithmetic.
    // For N <= 128, k is based on n (= N). For N > 128, k is based
    // on the subset size n = 128.
    let k = n * (100 - resilience_pct) / 100;

    // k must be at least 1 and at most 255 (need at least 1 parity shard).
    if k == 0 {
        return Err(ErasureError::InvalidParams {
            reason: format!("computed data_shards is 0 for n={n}, resilience_pct={resilience_pct}"),
        });
    }
    if k > 255 {
        return Err(ErasureError::InvalidParams {
            reason: format!("computed data_shards ({k}) exceeds 255"),
        });
    }
    if n > 255 {
        return Err(ErasureError::InvalidParams {
            reason: format!("computed total_shards ({n}) exceeds 255"),
        });
    }

    let m = n - k;
    if m == 0 {
        return Err(ErasureError::InvalidParams {
            reason: format!(
                "computed parity_shards is 0 for n={n}, k={k} (resilience_pct={resilience_pct} too low?)"
            ),
        });
    }

    #[allow(clippy::cast_possible_truncation)]
    let resilience_u8 = resilience_pct as u8;

    let params = ErasureParams {
        total_shards: n,
        data_shards: k,
        parity_shards: m,
        resilience_pct: resilience_u8,
    };

    params.validate()?;
    Ok(params)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_params_3_nodes() {
        let params = compute_params(3, 33).expect("3 nodes, R=33 should be valid");
        assert_eq!(params.data_shards, 2);
        assert_eq!(params.parity_shards, 1);
        assert_eq!(params.total_shards, 3);
        assert_eq!(params.resilience_pct, 33);
    }

    #[test]
    fn test_compute_params_9_nodes() {
        let params = compute_params(9, 33).expect("9 nodes, R=33 should be valid");
        assert_eq!(params.data_shards, 6);
        assert_eq!(params.parity_shards, 3);
        assert_eq!(params.total_shards, 9);
    }

    #[test]
    fn test_compute_params_100_nodes() {
        let params = compute_params(100, 33).expect("100 nodes, R=33 should be valid");
        assert_eq!(params.data_shards, 67);
        assert_eq!(params.parity_shards, 33);
        assert_eq!(params.total_shards, 100);
    }

    #[test]
    fn test_compute_params_invalid_n() {
        assert!(compute_params(0, 33).is_err(), "N=0 should be invalid");
        assert!(compute_params(1, 33).is_err(), "N=1 should be invalid");
    }

    #[test]
    fn test_compute_params_invalid_r() {
        assert!(compute_params(10, 101).is_err(), "R=101 should be invalid");
        assert!(compute_params(10, 200).is_err(), "R=200 should be invalid");
    }

    #[test]
    fn test_compute_params_max_shards() {
        // N=256 > 128, so n is capped at 128 (representative subset).
        // k = 128 * 67 / 100 = 85, m = 128 - 85 = 43.
        // k + m = 128 <= 256 — within GF(2^8) limit.
        let params = compute_params(256, 33).expect("N=256 should succeed with subset");
        assert!(params.total_shards <= 256, "total_shards must be <= 256");
        assert_eq!(params.total_shards, 128, "N=256 should use subset of 128");
        assert_eq!(params.data_shards, 85);
        assert_eq!(params.parity_shards, 43);
    }

    #[test]
    fn test_compute_params_r_100_too_many_parity() {
        // R=100 means 100% of nodes are parity, k=0 — invalid.
        assert!(
            compute_params(10, 100).is_err(),
            "R=100 gives k=0, should be invalid"
        );
    }

    #[test]
    fn test_compute_params_r_0_all_data() {
        // R=0 means 0% parity, m=0 — invalid (no redundancy).
        assert!(
            compute_params(10, 0).is_err(),
            "R=0 gives m=0, should be invalid"
        );
    }

    #[test]
    fn test_compute_params_r_50() {
        // N=10, R=50 → k = 10*50/100 = 5, m = 5
        let params = compute_params(10, 50).expect("N=10, R=50 should be valid");
        assert_eq!(params.data_shards, 5);
        assert_eq!(params.parity_shards, 5);
        assert_eq!(params.total_shards, 10);
    }

    #[test]
    fn test_compute_params_resilience_pct_field() {
        let params = compute_params(10, 33).unwrap();
        assert_eq!(params.resilience_pct, 33);
    }

    #[test]
    fn test_erasure_params_validate_valid() {
        let params = ErasureParams {
            total_shards: 10,
            data_shards: 7,
            parity_shards: 3,
            resilience_pct: 33,
        };
        assert!(params.validate().is_ok());
    }

    #[test]
    fn test_erasure_params_validate_zero_data() {
        let params = ErasureParams {
            total_shards: 3,
            data_shards: 0,
            parity_shards: 3,
            resilience_pct: 33,
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn test_erasure_params_validate_zero_parity() {
        let params = ErasureParams {
            total_shards: 3,
            data_shards: 3,
            parity_shards: 0,
            resilience_pct: 33,
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn test_erasure_params_validate_mismatch() {
        let params = ErasureParams {
            total_shards: 10,
            data_shards: 7,
            parity_shards: 2, // 7 + 2 != 10
            resilience_pct: 33,
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn test_erasure_params_validate_too_many() {
        let params = ErasureParams {
            total_shards: 257,
            data_shards: 200,
            parity_shards: 57,
            resilience_pct: 33,
        };
        assert!(params.validate().is_err());
    }

    #[test]
    fn test_erasure_params_serde_roundtrip() {
        let params = ErasureParams {
            total_shards: 10,
            data_shards: 7,
            parity_shards: 3,
            resilience_pct: 33,
        };
        let json = serde_json::to_string(&params).expect("serialize ErasureParams");
        let decoded: ErasureParams =
            serde_json::from_str(&json).expect("deserialize ErasureParams");
        assert_eq!(params, decoded);
    }
}
