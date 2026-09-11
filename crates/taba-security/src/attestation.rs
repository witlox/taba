//! Platform attestation: TPM and software attestation providers.
//!
//! Attestation proves a node's hardware/software integrity before it
//! joins the cluster. TPM attestation (A5) is optional and
//! feature-gated — when unavailable, a software attestation can be
//! used (weaker, dev only).
//!
//! ## Trust model
//!
//! - TPM: cryptographically attests to the platform's boot state.
//!   The quote is signed by the TPM's attestation key (AIK).
//!   Requires a TPM 2.0 chip and the `tss` feature.
//! - Software: generates a hash of the running binary + OS info.
//!   Weaker than TPM but suitable for dev/testing. No hardware
//!   requirement.

use serde::{Deserialize, Serialize};

use taba_common::NodeId;

use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// Attestation types
// ---------------------------------------------------------------------------

/// Result of a platform attestation.
///
/// Contains the platform state, the attestation quote, and the
/// attestation signature. The quote is verified by the joining
/// node's peers before it is accepted into the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResult {
    /// The node being attested.
    pub node_id: NodeId,
    /// Hash of the running binary (SHA-256).
    pub binary_hash: [u8; 32],
    /// Operating system identifier.
    pub os: String,
    /// Architecture identifier.
    pub arch: String,
    /// The attestation quote (opaque bytes — TPM quote or software hash).
    pub quote: Vec<u8>,
    /// Signature over the quote (by TPM AIK or node key).
    pub signature: Vec<u8>,
    /// Attestation provider type.
    pub provider: AttestationProvider,
}

/// Type of attestation provider used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AttestationProvider {
    /// TPM 2.0 hardware attestation.
    Tpm,
    /// Software-based attestation (dev/test only).
    Software,
}

// ---------------------------------------------------------------------------
// AttestationProvider trait
// ---------------------------------------------------------------------------

/// Provides platform attestation for node join.
///
/// Implementations:
/// - [`SoftwareAttestation`]: generates a hash of the running binary
///   and OS info. No hardware requirement. Suitable for dev/testing.
/// - `TpmAttestation` (feature-gated): uses TPM 2.0 to produce a
///   cryptographically signed quote of the platform's boot state.
pub trait AttestationProviderTrait: Send + Sync {
    /// Perform attestation with the given nonce.
    ///
    /// The nonce prevents replay attacks — the caller generates a
    /// random nonce, the attestation provider includes it in the
    /// quote, and the verifier checks it matches.
    ///
    /// # Errors
    ///
    /// - [`SecurityError::KeyError`] if attestation cannot be performed
    ///   (e.g., TPM not available, binary hash failed).
    fn attest(&self, node_id: &NodeId, nonce: &[u8]) -> Result<AttestationResult, SecurityError>;

    /// Returns the type of this attestation provider.
    fn provider_type(&self) -> AttestationProvider;
}

// ---------------------------------------------------------------------------
// SoftwareAttestation
// ---------------------------------------------------------------------------

/// Software-based attestation provider (dev/test only).
///
/// Generates a SHA-256 hash of the running binary and OS info.
/// No hardware requirement. Weaker than TPM attestation because:
/// - No secure boot chain verification
/// - No hardware-rooted key
/// - Hash can be forged by a compromised system
#[derive(Debug, Clone, Default)]
pub struct SoftwareAttestation {
    /// Optional override for the binary hash (for testing).
    binary_hash_override: Option<[u8; 32]>,
}

impl SoftwareAttestation {
    /// Creates a new software attestation provider.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            binary_hash_override: None,
        }
    }

    /// Creates a software attestation with a specific binary hash
    /// (for testing only).
    #[must_use]
    pub const fn with_binary_hash(hash: [u8; 32]) -> Self {
        Self {
            binary_hash_override: Some(hash),
        }
    }
}

impl AttestationProviderTrait for SoftwareAttestation {
    fn attest(&self, node_id: &NodeId, nonce: &[u8]) -> Result<AttestationResult, SecurityError> {
        use sha2::{Digest, Sha256};

        // Compute binary hash (or use override).
        let binary_hash = self.binary_hash_override.unwrap_or_else(|| {
            // In a real implementation, this would hash the running
            // binary. For M6, we use a placeholder hash of /proc/self/exe
            // or just a constant. The override is for testing.
            let mut hasher = Sha256::new();
            hasher.update(b"taba-node-binary");
            hasher.update(nonce);
            let result = hasher.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        });

        // Generate quote: binary_hash || os || arch || nonce
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();

        let mut hasher = Sha256::new();
        hasher.update(binary_hash);
        hasher.update(&os);
        hasher.update(&arch);
        hasher.update(nonce);
        let quote = hasher.finalize().to_vec();

        // Signature: in software attestation, the "signature" is just
        // the quote itself (no hardware key). This is intentionally weak.
        let signature = quote.clone();

        Ok(AttestationResult {
            node_id: *node_id,
            binary_hash,
            os,
            arch,
            quote,
            signature,
            provider: AttestationProvider::Software,
        })
    }

    fn provider_type(&self) -> AttestationProvider {
        AttestationProvider::Software
    }
}

/// Verifies a software attestation result.
///
/// Checks that:
/// 1. The quote is the SHA-256 of (`binary_hash` || os || arch || nonce)
/// 2. The signature matches the quote
///
/// # Errors
///
/// - [`SecurityError::InvalidSignature`] if the attestation is invalid.
pub fn verify_software_attestation(
    result: &AttestationResult,
    expected_nonce: &[u8],
) -> Result<(), SecurityError> {
    use sha2::{Digest, Sha256};

    // Recompute the quote.
    let mut hasher = Sha256::new();
    hasher.update(result.binary_hash);
    hasher.update(&result.os);
    hasher.update(&result.arch);
    hasher.update(expected_nonce);
    let expected_quote = hasher.finalize();

    // Check quote matches.
    if result.quote != expected_quote.as_slice() {
        return Err(SecurityError::InvalidSignature {
            reason: "attestation quote does not match expected value".to_string(),
        });
    }

    // Check signature matches quote (software attestation is self-signed).
    if result.signature != result.quote {
        return Err(SecurityError::InvalidSignature {
            reason: "attestation signature does not match quote".to_string(),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_software_attestation_roundtrip() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let nonce = b"test-nonce-12345";
        let provider = SoftwareAttestation::new();

        let result = provider
            .attest(&node_id, nonce)
            .expect("attestation should succeed");

        assert_eq!(result.node_id, node_id);
        assert_eq!(result.provider, AttestationProvider::Software);
        assert!(!result.quote.is_empty());

        verify_software_attestation(&result, nonce).expect("verification should succeed");
    }

    #[test]
    fn test_software_attestation_wrong_nonce() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let provider = SoftwareAttestation::new();

        let result = provider
            .attest(&node_id, b"correct-nonce")
            .expect("attestation should succeed");

        let verify_result = verify_software_attestation(&result, b"wrong-nonce");
        assert!(
            verify_result.is_err(),
            "verification with wrong nonce should fail"
        );
    }

    #[test]
    fn test_software_attestation_tampered_quote() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let nonce = b"test-nonce";
        let provider = SoftwareAttestation::new();

        let mut result = provider
            .attest(&node_id, nonce)
            .expect("attestation should succeed");

        // Tamper with the quote.
        result.quote[0] ^= 0xFF;

        let verify_result = verify_software_attestation(&result, nonce);
        assert!(
            verify_result.is_err(),
            "verification of tampered quote should fail"
        );
    }

    #[test]
    fn test_software_attestation_tampered_signature() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let nonce = b"test-nonce";
        let provider = SoftwareAttestation::new();

        let mut result = provider
            .attest(&node_id, nonce)
            .expect("attestation should succeed");

        // Tamper with the signature.
        result.signature[0] ^= 0xFF;

        let verify_result = verify_software_attestation(&result, nonce);
        assert!(
            verify_result.is_err(),
            "verification of tampered signature should fail"
        );
    }

    #[test]
    fn test_with_binary_hash_override() {
        let node_id = NodeId(uuid::Uuid::new_v4());
        let nonce = b"test";
        let custom_hash = [0xAB; 32];
        let provider = SoftwareAttestation::with_binary_hash(custom_hash);

        let result = provider
            .attest(&node_id, nonce)
            .expect("attestation should succeed");

        assert_eq!(result.binary_hash, custom_hash);
    }

    #[test]
    fn test_provider_type() {
        assert_eq!(
            SoftwareAttestation::new().provider_type(),
            AttestationProvider::Software
        );
    }

    #[test]
    fn test_attestation_result_serialization_roundtrip() {
        let result = AttestationResult {
            node_id: NodeId(uuid::Uuid::new_v4()),
            binary_hash: [1u8; 32],
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            quote: vec![2u8; 32],
            signature: vec![3u8; 64],
            provider: AttestationProvider::Software,
        };

        let json = serde_json::to_string(&result).expect("serialize");
        let decoded: AttestationResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(result.node_id, decoded.node_id);
        assert_eq!(result.binary_hash, decoded.binary_hash);
        assert_eq!(result.os, decoded.os);
        assert_eq!(result.provider, decoded.provider);
    }
}
