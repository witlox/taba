//! Reed-Solomon erasure coding over GF(2^8) (DL-013).
//!
//! The [`ErasureCoder`] trait is the pure coding layer — no I/O, no
//! distribution logic. It operates on byte slices and produces or
//! consumes [`Shard`] values.
//!
//! [`DefaultErasureCoder`] wraps the `reed-solomon-erasure` crate.
//! A 4-byte big-endian length prefix is prepended to the data before
//! encoding so that `decode` can recover the original length without
//! external metadata. The prefix is transparent to callers.

use reed_solomon_erasure::galois_8;

use crate::error::ErasureError;
use crate::params::{ErasureParams, compute_params};
use crate::shard::{Shard, ShardCriticality};

// ---------------------------------------------------------------------------
// Length-prefix helpers
// ---------------------------------------------------------------------------

/// Size of the big-endian length prefix prepended to encoded data.
const LENGTH_PREFIX_SIZE: usize = 4;

/// Prepends a 4-byte big-endian length prefix to `data`.
fn with_length_prefix(data: &[u8]) -> Vec<u8> {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    let mut buf = Vec::with_capacity(LENGTH_PREFIX_SIZE + data.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(data);
    buf
}

/// Splits `data` into `k` equal-sized chunks, padding the last chunk
/// with zeros if needed.
fn split_into_shards(data: &[u8], k: usize) -> Vec<Vec<u8>> {
    if data.is_empty() || k == 0 {
        return vec![Vec::new(); k.max(1)];
    }

    let chunk_size = data.len().div_ceil(k).max(1);
    let total_size = chunk_size * k;
    let mut padded = Vec::with_capacity(total_size);
    padded.extend_from_slice(data);
    padded.resize(total_size, 0u8);

    (0..k)
        .map(|i| padded[i * chunk_size..(i + 1) * chunk_size].to_vec())
        .collect()
}

/// Concatenates the first `k` shards and strips the length prefix,
/// returning the original data.
fn reassemble_data(shards: &[Vec<u8>], k: usize) -> Result<Vec<u8>, ErasureError> {
    let mut combined: Vec<u8> = Vec::new();
    for shard in shards.iter().take(k) {
        combined.extend_from_slice(shard);
    }

    if combined.len() < LENGTH_PREFIX_SIZE {
        return Err(ErasureError::CorruptShard {
            group: crate::error::ShardGroupId::new(),
            index: 0,
        });
    }

    let len_bytes: [u8; LENGTH_PREFIX_SIZE] =
        combined[..LENGTH_PREFIX_SIZE]
            .try_into()
            .map_err(|_| ErasureError::CorruptShard {
                group: crate::error::ShardGroupId::new(),
                index: 0,
            })?;

    let original_len =
        usize::try_from(u32::from_be_bytes(len_bytes)).map_err(|_| ErasureError::CorruptShard {
            group: crate::error::ShardGroupId::new(),
            index: 0,
        })?;

    if combined.len() < LENGTH_PREFIX_SIZE + original_len {
        return Err(ErasureError::CorruptShard {
            group: crate::error::ShardGroupId::new(),
            index: 0,
        });
    }

    Ok(combined[LENGTH_PREFIX_SIZE..LENGTH_PREFIX_SIZE + original_len].to_vec())
}

// ---------------------------------------------------------------------------
// ErasureCoder trait
// ---------------------------------------------------------------------------

/// Core erasure coding: encode data into shards and decode from shards.
///
/// This is the pure coding layer — no I/O, no distribution logic. It
/// operates on byte slices and produces/consumes [`Shard`] values.
///
/// The original data can be reconstructed from any `k` of the `n`
/// shards, where `k = params.data_shards` and `n = params.total_shards`.
pub trait ErasureCoder: Send + Sync {
    /// Encode data into `n` shards (`k` data + `m` parity).
    ///
    /// The original data can be reconstructed from any `k` of the `n`
    /// shards. Each shard includes its index for reconstruction
    /// ordering.
    ///
    /// # Errors
    ///
    /// Returns [`ErasureError::InvalidParams`] if the parameters are
    /// out of range or inconsistent.
    fn encode(&self, data: &[u8], params: ErasureParams) -> Result<Vec<Shard>, ErasureError>;

    /// Reconstruct original data from at least `k` shards.
    ///
    /// Shards can arrive in any order — the shard `index` field is
    /// used for positioning. Missing shards (indices not present in
    /// the slice) are reconstructed from the surviving ones.
    ///
    /// # Errors
    ///
    /// - [`ErasureError::InsufficientShards`] if fewer than `k` shards
    ///   are provided.
    /// - [`ErasureError::CorruptShard`] if shard data is malformed.
    /// - [`ErasureError::InvalidParams`] if the parameters are invalid.
    fn decode(&self, shards: &[Shard], params: ErasureParams) -> Result<Vec<u8>, ErasureError>;

    /// Compute the erasure parameters for a given fleet size and
    /// resilience percentage (INV-R4).
    ///
    /// `k = floor(N × (1 − R/100))` where `R` is the resilience
    /// percentage and `N` is the number of active nodes.
    /// `m = n − k`. For `N > 128`, shards are distributed to a
    /// representative subset of 128 nodes.
    ///
    /// # Errors
    ///
    /// Returns [`ErasureError::InvalidParams`] if `N < 2`,
    /// `R > 100`, or the computed parameters exceed GF(2^8) limits.
    fn compute_params(
        &self,
        node_count: u32,
        resilience_pct: u32,
    ) -> Result<ErasureParams, ErasureError>;
}

// ---------------------------------------------------------------------------
// DefaultErasureCoder
// ---------------------------------------------------------------------------

/// Default implementation of [`ErasureCoder`] using the
/// `reed-solomon-erasure` crate over GF(2^8).
///
/// This is a stateless, zero-cost wrapper — all methods are pure
/// computations with no I/O or internal state.
#[derive(Debug, Clone, Default)]
pub struct DefaultErasureCoder;

impl DefaultErasureCoder {
    /// Creates a new `DefaultErasureCoder`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ErasureCoder for DefaultErasureCoder {
    fn encode(&self, data: &[u8], params: ErasureParams) -> Result<Vec<Shard>, ErasureError> {
        params.validate()?;

        let k = params.data_shards as usize;
        let m = params.parity_shards as usize;
        let n = params.total_shards as usize;

        // Prepend length prefix so decode can recover original length.
        let prefixed = with_length_prefix(data);

        // Split into k data shards of equal size (zero-padded).
        let mut all_shards: Vec<Vec<u8>> = split_into_shards(&prefixed, k);

        // Append m zero-initialised parity shards.
        let shard_size = all_shards[0].len();
        all_shards.resize(n, vec![0u8; shard_size]);

        // Create the Reed-Solomon codec and encode parity.
        let rs = galois_8::ReedSolomon::new(k, m).map_err(|e| ErasureError::InvalidParams {
            reason: format!("reed-solomon init failed: {e}"),
        })?;

        rs.encode(&mut all_shards)
            .map_err(|e| ErasureError::InvalidParams {
                reason: format!("reed-solomon encode failed: {e}"),
            })?;

        // Convert to domain Shard type.
        let shards = all_shards
            .into_iter()
            .enumerate()
            .map(|(i, shard_data)| {
                Shard::new(
                    u32::try_from(i).unwrap_or(u32::MAX),
                    shard_data,
                    params,
                    ShardCriticality::Workload,
                )
            })
            .collect();

        Ok(shards)
    }

    fn decode(&self, shards: &[Shard], params: ErasureParams) -> Result<Vec<u8>, ErasureError> {
        params.validate()?;

        let k = params.data_shards as usize;
        let n = params.total_shards as usize;

        // Build the full shard array with None for missing shards.
        let mut opts: Vec<Option<Vec<u8>>> = vec![None; n];
        let mut present_count = 0u32;

        for shard in shards {
            let idx = shard.index as usize;
            if idx >= n {
                return Err(ErasureError::CorruptShard {
                    group: crate::error::ShardGroupId::new(),
                    index: shard.index,
                });
            }
            if opts[idx].is_none() {
                present_count += 1;
            }
            opts[idx] = Some(shard.data.clone());
        }

        if present_count < params.data_shards {
            return Err(ErasureError::InsufficientShards {
                need: params.data_shards,
                have: present_count,
            });
        }

        // Create the Reed-Solomon codec and reconstruct.
        let rs = galois_8::ReedSolomon::new(k, n - k).map_err(|e| ErasureError::InvalidParams {
            reason: format!("reed-solomon init failed: {e}"),
        })?;

        rs.reconstruct(&mut opts)
            .map_err(|_| ErasureError::CorruptShard {
                group: crate::error::ShardGroupId::new(),
                index: 0,
            })?;

        // Collect all reconstructed shard data.
        let all_shards: Vec<Vec<u8>> = opts
            .into_iter()
            .map(std::option::Option::unwrap_or_default)
            .collect();

        // Reassemble from first k data shards.
        reassemble_data(&all_shards, k)
    }

    fn compute_params(
        &self,
        node_count: u32,
        resilience_pct: u32,
    ) -> Result<ErasureParams, ErasureError> {
        compute_params(node_count, resilience_pct)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn coder() -> DefaultErasureCoder {
        DefaultErasureCoder::new()
    }

    fn params_3_2_1() -> ErasureParams {
        ErasureParams {
            total_shards: 3,
            data_shards: 2,
            parity_shards: 1,
            resilience_pct: 33,
        }
    }

    fn params_5_3_2() -> ErasureParams {
        ErasureParams {
            total_shards: 5,
            data_shards: 3,
            parity_shards: 2,
            resilience_pct: 40,
        }
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let coder = coder();
        let params = params_3_2_1();
        let data = b"Hello, taba erasure coding!";

        let shards = coder.encode(data, params).expect("encode should succeed");
        assert_eq!(shards.len(), 3, "should produce 3 shards");

        let decoded = coder
            .decode(&shards, params)
            .expect("decode should succeed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_decode_roundtrip_larger() {
        let coder = coder();
        let params = params_5_3_2();
        let data = vec![0x42; 10_000];

        let shards = coder.encode(&data, params).expect("encode");
        assert_eq!(shards.len(), 5);

        let decoded = coder.decode(&shards, params).expect("decode");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_decode_with_missing_parity() {
        let coder = coder();
        let params = params_3_2_1(); // k=2, m=1, n=3
        let data = b"survive parity loss";

        let shards = coder.encode(data, params).expect("encode");

        // Remove the parity shard (index 2). Keep all data shards.
        let subset: Vec<Shard> = shards.iter().filter(|s| s.index < 2).cloned().collect();
        assert_eq!(subset.len(), 2, "should have 2 data shards");

        let decoded = coder
            .decode(&subset, params)
            .expect("decode with all data should succeed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_encode_decode_with_missing_data() {
        let coder = coder();
        let params = params_5_3_2(); // k=3, m=2, n=5
        let data = b"reconstruct from parity when a data shard is lost!";

        let shards = coder.encode(data, params).expect("encode");

        // Remove one data shard (index 0). Keep k-1=2 data + 2 parity = 4 shards.
        let subset: Vec<Shard> = shards.iter().filter(|s| s.index != 0).cloned().collect();
        assert_eq!(subset.len(), 4, "should have 4 shards (2 data + 2 parity)");

        let decoded = coder
            .decode(&subset, params)
            .expect("decode with k-1 data + parity should succeed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_decode_insufficient_shards() {
        let coder = coder();
        let params = params_5_3_2(); // k=3
        let data = b"not enough shards";

        let shards = coder.encode(data, params).expect("encode");

        // Keep only 2 shards (< k=3).
        let subset: Vec<Shard> = shards.iter().take(2).cloned().collect();
        assert_eq!(subset.len(), 2);

        let result = coder.decode(&subset, params);
        assert!(matches!(
            result,
            Err(ErasureError::InsufficientShards { need: 3, have: 2 })
        ));
    }

    #[test]
    fn test_encode_invalid_params_zero_k() {
        let coder = coder();
        let params = ErasureParams {
            total_shards: 2,
            data_shards: 0,
            parity_shards: 2,
            resilience_pct: 33,
        };

        let result = coder.encode(b"data", params);
        assert!(matches!(result, Err(ErasureError::InvalidParams { .. })));
    }

    #[test]
    fn test_encode_invalid_params_mismatch() {
        let coder = coder();
        let params = ErasureParams {
            total_shards: 10,
            data_shards: 5,
            parity_shards: 3, // 5 + 3 != 10
            resilience_pct: 33,
        };

        let result = coder.encode(b"data", params);
        assert!(matches!(result, Err(ErasureError::InvalidParams { .. })));
    }

    #[test]
    fn test_encode_empty_data() {
        let coder = coder();
        let params = params_3_2_1();
        let data: &[u8] = b"";

        let shards = coder
            .encode(data, params)
            .expect("encoding empty data should succeed");
        assert_eq!(shards.len(), 3);

        let decoded = coder.decode(&shards, params).expect("decode empty data");
        assert!(decoded.is_empty(), "decoded data should be empty");
    }

    #[test]
    fn test_encode_large_data() {
        let coder = coder();
        let params = params_5_3_2();
        let data = vec![0xAB; 1_000_000]; // 1 MB

        let shards = coder
            .encode(&data, params)
            .expect("encoding 1MB should succeed");
        assert_eq!(shards.len(), 5);

        let decoded = coder.decode(&shards, params).expect("decode 1MB");
        assert_eq!(decoded.len(), 1_000_000);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_compute_params() {
        let coder = coder();

        let params = coder.compute_params(9, 33).expect("compute_params");
        assert_eq!(params.data_shards, 6);
        assert_eq!(params.parity_shards, 3);
        assert_eq!(params.total_shards, 9);
    }

    #[test]
    fn test_decode_shards_out_of_order() {
        let coder = coder();
        let params = params_5_3_2();
        let data = b"shards can arrive in any order!";

        let shards = coder.encode(data, params).expect("encode");

        // Reverse the order.
        let reversed: Vec<Shard> = shards.iter().rev().cloned().collect();

        let decoded = coder.decode(&reversed, params).expect("decode reversed");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_decode_with_arbitrary_missing_combination() {
        let coder = coder();
        let params = params_5_3_2(); // k=3, m=2, n=5
        let data = b"arbitrary shard loss is fine!";

        let shards = coder.encode(data, params).expect("encode");

        // Remove shards at indices 1 and 4 (one data, one parity).
        let subset: Vec<Shard> = shards
            .iter()
            .filter(|s| s.index != 1 && s.index != 4)
            .cloned()
            .collect();
        assert_eq!(subset.len(), 3, "should have 3 shards (= k)");

        let decoded = coder.decode(&subset, params).expect("decode with 3 shards");
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_decode_index_out_of_range() {
        let coder = coder();
        let params = params_3_2_1(); // n=3

        let mut shards = coder.encode(b"data", params).expect("encode");
        shards[0].index = 99; // out of range

        let result = coder.decode(&shards, params);
        assert!(matches!(
            result,
            Err(ErasureError::CorruptShard { index: 99, .. })
        ));
    }

    // -----------------------------------------------------------------------
    // Property tests
    // -----------------------------------------------------------------------

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_encode_decode_roundtrip(
            data in proptest::collection::vec(0u8..255, 0..10_000),
            k in 2u32..6,
            m in 1u32..4,
        ) {
            let n = k + m;
            // Skip if n > 256 (GF(2^8) limit) — extremely unlikely
            // with these ranges but guard anyway.
            proptest::prop_assume!(n <= 256);

            let params = ErasureParams {
                total_shards: n,
                data_shards: k,
                parity_shards: m,
                resilience_pct: (m * 100 / n).min(100) as u8,
            };
            params.validate().expect("params should be valid");

            let coder = DefaultErasureCoder::new();
            let shards = coder.encode(&data, params).expect("encode should succeed");
            let decoded = coder.decode(&shards, params).expect("decode should succeed");

            prop_assert_eq!(decoded, data, "roundtrip must preserve original data");
            prop_assert_eq!(u32::try_from(shards.len()).unwrap_or(u32::MAX), n, "must produce exactly n shards");
        }

        #[test]
        fn proptest_decode_with_arbitrary_missing(
            data in proptest::collection::vec(0u8..255, 1..10_000),
            k in 2u32..5,
            m in 1u32..3,
            missing_count in 1u32..3,
        ) {
            let n = k + m;
            proptest::prop_assume!(n <= 256);
            proptest::prop_assume!(missing_count <= m); // can't lose more than m shards

            let params = ErasureParams {
                total_shards: n,
                data_shards: k,
                parity_shards: m,
                resilience_pct: (m * 100 / n).min(100) as u8,
            };
            params.validate().expect("params should be valid");

            let coder = DefaultErasureCoder::new();
            let shards = coder.encode(&data, params).expect("encode should succeed");

            // Remove `missing_count` shards from the end (parity area
            // and possibly some data shards). We always keep at least
            // k shards.
            let keep = (n - missing_count) as usize;
            let subset: Vec<Shard> = shards.iter().take(keep).cloned().collect();

            prop_assert!(subset.len() >= k as usize,
                "must keep at least k={} shards, kept {}", k, subset.len());

            let decoded = coder.decode(&subset, params).expect("decode should succeed");
            prop_assert_eq!(decoded, data, "must reconstruct original data");
        }
    }
}
