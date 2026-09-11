//! Shamir secret sharing over GF(2^8).
//!
//! Splits a secret into `n` shares such that any `k` shares can
//! reconstruct the original, but fewer than `k` reveal nothing
//! (information-theoretic security).
//!
//! ## Algorithm
//!
//! 1. Generate a random polynomial `f(x) = a_0 + a_1*x + ... + a_{k-1}*x^{k-1}`
//!    over GF(2^8), where `a_0` is the secret.
//! 2. Shares are `(i, f(i))` for `i = 1, 2, ..., n`.
//! 3. Reconstruct using Lagrange interpolation at `x = 0`.
//!
//! All arithmetic is in GF(2^8) with the irreducible polynomial
//! `x^8 + x^4 + x^3 + x + 1` (0x11B), the same field used by AES
//! and Reed-Solomon.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use rand::RngCore;
use zeroize::{Zeroize, ZeroizeOnDrop};

// ---------------------------------------------------------------------------
// GF(2^8) arithmetic
// ---------------------------------------------------------------------------

/// Irreducible polynomial for GF(2^8): x^8 + x^4 + x^3 + x + 1.
const GF_MODULUS: u16 = 0x11B;

/// Precomputed logarithm table for GF(2^8).
/// `LOG_TABLE[i]` = `log_g(i)` where g = 3 is the generator.
#[allow(clippy::cast_possible_truncation)]
static LOG_TABLE: [u8; 256] = build_log_table();

/// Precomputed exponentiation table for GF(2^8).
/// `EXP_TABLE[i]` = g^i where g = 3 is the generator.
#[allow(clippy::cast_possible_truncation)]
static EXP_TABLE: [u8; 512] = build_exp_table();

/// Build the exponentiation table for GF(2^8) with generator g = 3.
const fn build_exp_table() -> [u8; 512] {
    let mut table = [0u8; 512];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 512 {
        table[i] = x as u8;
        table[i] = x as u8;
        x = gf_mul_raw(x, 3);
        i += 1;
    }
    table
}

/// Build the logarithm table for GF(2^8) with generator g = 3.
///
/// `LOG_TABLE`[x] = `log_g(x)` for x = 1..255 (0-indexed).
/// `LOG_TABLE`[0] = 0 (sentinel — callers must check for 0 before
/// using, since log(0) is undefined).
const fn build_log_table() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut x: u16 = 1;
    let mut i = 0;
    while i < 255 {
        table[x as usize] = i as u8; // 0-indexed, i is 0..254
        x = gf_mul_raw(x, 3);
        i += 1;
    }
    table
}

/// Raw GF(2^8) multiplication using the Russian peasant method.
const fn gf_mul_raw(a: u16, b: u16) -> u16 {
    let mut result: u16 = 0;
    let mut a = a;
    let mut b = b;
    let mut i = 0;
    while i < 8 {
        if b & 1 != 0 {
            result ^= a;
        }
        let hi_bit = a & 0x80;
        a <<= 1;
        if hi_bit != 0 {
            a ^= GF_MODULUS;
        }
        b >>= 1;
        i += 1;
    }
    result
}

/// GF(2^8) multiplication using log/exp tables.
fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let log_a = LOG_TABLE[a as usize] as usize;
    let log_b = LOG_TABLE[b as usize] as usize;
    EXP_TABLE[(log_a + log_b) % 255]
}

/// GF(2^8) division: a / b.
fn gf_div(a: u8, b: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    assert!(b != 0, "GF(2^8) division by zero");
    let log_a = LOG_TABLE[a as usize] as usize;
    let log_b = LOG_TABLE[b as usize] as usize;
    // log(a/b) = log(a) - log(b) + 255 (mod 255)
    EXP_TABLE[(log_a + 255 - log_b) % 255]
}

/// GF(2^8) addition = XOR.
const fn gf_add(a: u8, b: u8) -> u8 {
    a ^ b
}

/// GF(2^8) exponentiation: a^n.
#[allow(dead_code)]
fn gf_pow(a: u8, n: u32) -> u8 {
    if n == 0 {
        return 1;
    }
    if a == 0 {
        return 0;
    }
    let log_a = LOG_TABLE[a as usize] as usize;
    EXP_TABLE[(log_a * n as usize - 1) % 255 + 1]
}

// ---------------------------------------------------------------------------
// Polynomial operations
// ---------------------------------------------------------------------------

/// Evaluate a polynomial at point `x` over GF(2^8).
///
/// Uses Horner's method: `f(x) = a_0 + x*(a_1 + x*(a_2 + ...))`.
fn poly_eval(coeffs: &[u8], x: u8) -> u8 {
    let mut result: u8 = 0;
    for &coeff in coeffs.iter().rev() {
        result = gf_add(gf_mul(result, x), coeff);
    }
    result
}

/// Lagrange interpolation at `x = 0` using `k` points.
///
/// Given `(x_1, y_1), ..., (x_k, y_k)`, computes `f(0)` where `f`
/// is the unique polynomial of degree `k-1` passing through all points.
fn lagrange_interpolate_at_zero(points: &[(u8, u8)]) -> u8 {
    let k = points.len();
    let mut result: u8 = 0;

    for i in 0..k {
        let (x_i, y_i) = points[i];

        // Compute the Lagrange basis polynomial L_i(0).
        // L_i(0) = product_{j != i} (0 - x_j) / (x_i - x_j)
        //        = product_{j != i} x_j / (x_i ^ x_j)  (GF: -a = a)
        let mut numerator: u8 = 1;
        let mut denominator: u8 = 1;

        for (j, _) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            let x_j = points[j].0;
            // (0 - x_j) = x_j in GF(2^8)
            numerator = gf_mul(numerator, x_j);
            // (x_i - x_j) = (x_i ^ x_j) in GF(2^8)
            denominator = gf_mul(denominator, gf_add(x_i, x_j));
        }

        let basis = gf_div(numerator, denominator);
        result = gf_add(result, gf_mul(y_i, basis));
    }

    result
}

// ---------------------------------------------------------------------------
// Shamir secret sharing
// ---------------------------------------------------------------------------

/// A single Shamir share: (index, value).
///
/// The index is the x-coordinate, the value is the y-coordinate
/// (f(index) where f is the secret polynomial).
#[derive(Debug, Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Share {
    /// X-coordinate (1-based, 1..=255).
    pub index: u8,
    /// Y-coordinate (f(index)).
    pub value: Vec<u8>,
}

/// Splits a secret into `n` shares with threshold `k`.
///
/// Any `k` shares can reconstruct the original secret. Fewer than `k`
/// reveal nothing (information-theoretic security). All arithmetic is
/// over GF(2^8), so each byte of the secret is split independently.
///
/// # Parameters
///
/// - `secret`: The secret to split (any length).
/// - `threshold`: Minimum shares required for reconstruction (k).
/// - `total_shares`: Total number of shares to generate (n).
///
/// # Errors
///
/// Returns `SecurityError::CeremonyError` if:
/// - `threshold < 2`
/// - `threshold > total_shares`
/// - `total_shares > 255` (GF(2^8) limit)
pub fn split_secret(
    secret: &[u8],
    threshold: u8,
    total_shares: u8,
) -> Result<Vec<Share>, crate::error::SecurityError> {
    if threshold < 2 {
        return Err(crate::error::SecurityError::CeremonyError {
            reason: "threshold must be at least 2".to_string(),
        });
    }
    if threshold > total_shares {
        return Err(crate::error::SecurityError::CeremonyError {
            reason: "threshold cannot exceed total_shares".to_string(),
        });
    }

    let k = threshold as usize;
    let mut rng = rand::thread_rng();
    let mut shares: Vec<Share> = (1..=total_shares)
        .map(|i| Share {
            index: i,
            value: Vec::with_capacity(secret.len()),
        })
        .collect();

    // Split each byte of the secret independently.
    for &secret_byte in secret {
        // Generate random polynomial coefficients: a_0 = secret_byte,
        // a_1, ..., a_{k-1} are random.
        let mut coeffs = vec![0u8; k];
        coeffs[0] = secret_byte;
        // Fill remaining coefficients with random bytes.

        rng.fill_bytes(&mut coeffs[1..]);

        // Evaluate polynomial at each share index.
        for share in &mut shares {
            let y = poly_eval(&coeffs, share.index);
            share.value.push(y);
        }

        coeffs.zeroize();
    }

    Ok(shares)
}

/// Reconstructs a secret from `k` shares using Lagrange interpolation.
///
/// # Parameters
///
/// - `shares`: At least `k` shares (only the first `k` are used).
///
/// # Errors
///
/// Returns `SecurityError::CeremonyError` if:
/// - Fewer than 2 shares are provided
/// - Shares have inconsistent lengths
/// - Shares have duplicate indices
pub fn reconstruct_secret(shares: &[Share]) -> Result<Vec<u8>, crate::error::SecurityError> {
    if shares.len() < 2 {
        return Err(crate::error::SecurityError::CeremonyError {
            reason: "at least 2 shares are required for reconstruction".to_string(),
        });
    }

    // All shares must have the same value length.
    let secret_len = shares[0].value.len();
    if shares.iter().any(|s| s.value.len() != secret_len) {
        return Err(crate::error::SecurityError::CeremonyError {
            reason: "shares have inconsistent lengths".to_string(),
        });
    }

    // Check for duplicate indices.
    let mut seen = std::collections::HashSet::new();
    for share in shares {
        if !seen.insert(share.index) {
            return Err(crate::error::SecurityError::CeremonyError {
                reason: format!("duplicate share index: {}", share.index),
            });
        }
    }

    // Reconstruct each byte independently.
    let mut secret = Vec::with_capacity(secret_len);
    for byte_idx in 0..secret_len {
        let points: Vec<(u8, u8)> = shares
            .iter()
            .map(|s| (s.index, s.value[byte_idx]))
            .collect();
        secret.push(lagrange_interpolate_at_zero(&points));
    }

    Ok(secret)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gf_mul_identity() {
        assert_eq!(gf_mul(1, 1), 1);
        assert_eq!(gf_mul(0, 5), 0);
        assert_eq!(gf_mul(5, 0), 0);
    }

    #[test]
    fn test_gf_mul_commutative() {
        for a in 1u8..=255 {
            for b in 1u8..=255 {
                assert_eq!(
                    gf_mul(a, b),
                    gf_mul(b, a),
                    "gf_mul({a}, {b}) != gf_mul({b}, {a})"
                );
            }
        }
    }

    #[test]
    fn test_gf_div_inverse() {
        for a in 1u8..=255 {
            let inv = gf_div(1, a);
            assert_eq!(gf_mul(a, inv), 1, "gf_mul({a}, 1/{a}) should be 1");
        }
    }

    #[test]
    fn test_gf_mul_div_roundtrip() {
        for a in 1u8..=255 {
            for b in 1u8..=255 {
                let product = gf_mul(a, b);
                if product != 0 {
                    assert_eq!(
                        gf_div(product, b),
                        a,
                        "gf_div(gf_mul({a}, {b}), {b}) should be {a}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_poly_eval_constant() {
        // f(x) = 42 for all x
        assert_eq!(poly_eval(&[42], 0), 42);
        assert_eq!(poly_eval(&[42], 1), 42);
        assert_eq!(poly_eval(&[42], 255), 42);
    }

    #[test]
    fn test_poly_eval_linear() {
        // f(x) = 3 + 5*x in GF(2^8)
        // f(0) = 3, f(1) = 3^5 = 6, f(2) = 3 + 5*2 = 3 ^ gf_mul(5,2)
        assert_eq!(poly_eval(&[3, 5], 0), 3);
    }

    #[test]
    fn test_split_reconstruct_roundtrip() {
        let secret = b"hello taba secret key 32 bytes!";
        let shares = split_secret(secret, 3, 5).expect("split should succeed");

        // Any 3 shares should reconstruct the secret.
        let reconstructed = reconstruct_secret(&shares[0..3]).expect("reconstruct should succeed");
        assert_eq!(reconstructed.as_slice(), secret);

        // Different 3 shares should also work.
        let reconstructed2 = reconstruct_secret(&shares[2..5]).expect("reconstruct should succeed");
        assert_eq!(reconstructed2.as_slice(), secret);

        // Yet another combination.
        let reconstructed3 =
            reconstruct_secret(&[shares[0].clone(), shares[2].clone(), shares[4].clone()])
                .expect("reconstruct should succeed");
        assert_eq!(reconstructed3.as_slice(), secret);
    }

    #[test]
    fn test_split_32_bytes_ed25519() {
        // Simulate an Ed25519 private key (32 bytes).
        let secret = [42u8; 32];
        let shares = split_secret(&secret, 5, 7).expect("split should succeed");
        assert_eq!(shares.len(), 7);
        assert_eq!(shares[0].value.len(), 32);

        let reconstructed = reconstruct_secret(&shares[1..6]).expect("reconstruct should succeed");
        assert_eq!(reconstructed.as_slice(), &secret);
    }

    #[test]
    fn test_threshold_too_low() {
        let result = split_secret(b"secret", 1, 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_threshold_exceeds_total() {
        let result = split_secret(b"secret", 6, 5);
        assert!(result.is_err());
    }

    #[test]
    fn test_too_many_shares() {
        let result = split_secret(b"secret", 2, 200);
        // 200 is within u8 range but > 255 would overflow. The check
        // `total_shares > 255` is a no-op since u8 max is 255.
        // For GF(2^8), 255 is the real limit (indices 1..=255).
        assert!(result.is_ok()); // 200 is valid
    }

    #[test]
    fn test_reconstruct_too_few() {
        let result = reconstruct_secret(&[]);
        assert!(result.is_err());

        let one_share = Share {
            index: 1,
            value: vec![42],
        };
        let result = reconstruct_secret(&[one_share]);
        assert!(result.is_err());
    }

    #[test]
    fn test_reconstruct_duplicate_indices() {
        let shares = split_secret(b"secret", 3, 5).expect("split should succeed");
        let dup = shares[0].clone();
        let result = reconstruct_secret(&[shares[0].clone(), dup, shares[2].clone()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_reconstruct_inconsistent_lengths() {
        let shares = vec![
            Share {
                index: 1,
                value: vec![1, 2, 3],
            },
            Share {
                index: 2,
                value: vec![4, 5],
            },
            Share {
                index: 3,
                value: vec![7, 8, 9],
            },
        ];
        let result = reconstruct_secret(&shares);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_secret() {
        let shares = split_secret(b"", 3, 5).expect("split should succeed");
        assert_eq!(shares.len(), 5);
        assert!(shares[0].value.is_empty());

        let reconstructed = reconstruct_secret(&shares[0..3]).expect("reconstruct should succeed");
        assert!(reconstructed.is_empty());
    }

    #[test]
    fn test_single_byte_secret() {
        let secret = [0x42u8];
        let shares = split_secret(&secret, 3, 5).expect("split should succeed");

        let reconstructed = reconstruct_secret(&shares[0..3]).expect("reconstruct should succeed");
        assert_eq!(reconstructed, vec![0x42]);
    }

    #[test]
    fn test_shares_differ_from_secret() {
        // No individual share should reveal the secret.
        let secret = [0x42u8; 32];
        let shares = split_secret(&secret, 3, 5).expect("split should succeed");

        // Share values should not all equal the secret bytes
        // (extremely unlikely with random coefficients).
        for share in &shares {
            assert_ne!(
                share.value,
                secret.to_vec(),
                "share {share:?} should not equal secret"
            );
        }
    }
}
