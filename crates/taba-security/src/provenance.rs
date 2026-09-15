//! SLSA build provenance verification.
//!
//! SLSA (Supply-chain Levels for Software Artifacts) is a framework
//! for ensuring the integrity of software artifacts. taba can require
//! a minimum SLSA level for workload units in a trust domain.
//!
//! ## SLSA levels
//!
//! - Level 1: Build process documented (provenance exists).
//! - Level 2: Hosted build service with provenance generation
//!   (e.g., GitHub Actions, Cloud Build).
//! - Level 3: Hardened build platform with isolated builds.
//!
//! ## Integration
//!
//! The solver calls [`ProvenanceVerifier::verify`] before placing a
//! workload unit. If the trust domain requires a minimum SLSA level
//! and the unit's provenance is below that level, placement is
//! blocked.

use ed25519_dalek::{Signature, Verifier as DalekVerifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// SLSA provenance types
// ---------------------------------------------------------------------------

/// SLSA build provenance attestation for an artifact.
///
/// Describes how, where, and by whom an artifact was built. This
/// is the taba-specific representation of a SLSA provenance
/// attestation (DSSE envelope format is used externally, but
/// internally we use this typed struct).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlsaProvenance {
    /// The build system that produced the artifact (e.g., "github-actions/v3").
    pub builder: String,
    /// Source repository URI (e.g., "github.com/acme/service").
    pub source_repo: String,
    /// SHA-256 digest of the source code (e.g., "sha256:abc123...").
    pub source_digest: String,
    /// SLSA level (1, 2, or 3).
    pub slsa_level: u8,
    /// Build timestamp (wall time millis).
    pub build_timestamp: u64,
    /// Builder signature (over the provenance payload).
    pub builder_signature: Vec<u8>,
    /// Additional build parameters (environment, flags, etc.).
    pub build_parameters: Vec<(String, String)>,
}

/// Minimum SLSA level required by a trust domain.
///
/// Stored as a governance unit and checked by the solver before
/// placement. Workload units with provenance below this level are
/// blocked (fail-closed per INV-S2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct MinSlsaLevel(pub u8);

// ---------------------------------------------------------------------------
// ProvenanceVerifier trait
// ---------------------------------------------------------------------------

/// Verifies SLSA build provenance for workload artifacts.
///
/// The verifier checks:
/// 1. The provenance is well-formed (all required fields present).
/// 2. The SLSA level meets the minimum requirement.
/// 3. The builder is trusted (if a builder allowlist is configured).
/// 4. The source digest matches the expected digest (if provided).
/// 5. The builder signature is valid (if a public key is available).
pub trait ProvenanceVerifier: Send + Sync {
    /// Verify a single SLSA provenance attestation.
    ///
    /// # Parameters
    ///
    /// - `provenance`: The provenance to verify.
    /// - `min_level`: Minimum SLSA level required (0 = no requirement).
    /// - `expected_digest`: Expected source digest (if known).
    ///   Pass `None` to skip the digest check.
    ///
    /// # Errors
    ///
    /// - [`SecurityError::CapabilityDenied`]: SLSA level below minimum.
    /// - [`SecurityError::InvalidSignature`]: Builder signature invalid.
    /// - [`SecurityError::KeyError`]: Required field missing or
    ///   malformed.
    fn verify(
        &self,
        provenance: &SlsaProvenance,
        min_level: MinSlsaLevel,
        expected_digest: Option<&str>,
    ) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultProvenanceVerifier
// ---------------------------------------------------------------------------

/// Default implementation of [`ProvenanceVerifier`].
///
/// Performs structural validation, SLSA level checking, source digest
/// comparison, and builder signature verification (if the builder's
/// public key is known).
#[derive(Debug, Clone, Default)]
pub struct DefaultProvenanceVerifier {
    /// Trusted builders (builder name → public key for signature
    /// verification). If empty, all builders are trusted (no
    /// signature verification).
    trusted_builders: Vec<(String, [u8; 32])>,
}

impl DefaultProvenanceVerifier {
    /// Creates a new verifier with no trusted builders.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a trusted builder with its public key.
    ///
    /// When a builder is in the trusted list, its signature is
    /// verified against the public key. Builders not in the list
    /// are trusted without signature verification (progressive
    /// disclosure — SLSA level 1 doesn't require signatures).
    #[must_use]
    pub fn with_trusted_builder(mut self, builder: &str, public_key: [u8; 32]) -> Self {
        self.trusted_builders
            .push((builder.to_string(), public_key));
        self
    }
}

impl ProvenanceVerifier for DefaultProvenanceVerifier {
    fn verify(
        &self,
        provenance: &SlsaProvenance,
        min_level: MinSlsaLevel,
        expected_digest: Option<&str>,
    ) -> Result<(), SecurityError> {
        // 1. Structural validation.
        if provenance.builder.is_empty() {
            return Err(SecurityError::KeyError {
                reason: "provenance: builder is empty".to_string(),
            });
        }
        if provenance.source_repo.is_empty() {
            return Err(SecurityError::KeyError {
                reason: "provenance: source_repo is empty".to_string(),
            });
        }
        if provenance.source_digest.is_empty() {
            return Err(SecurityError::KeyError {
                reason: "provenance: source_digest is empty".to_string(),
            });
        }

        // 2. SLSA level check.
        if provenance.slsa_level < min_level.0 {
            return Err(SecurityError::CapabilityDenied {
                capability: "build_provenance".to_string(),
                reason: format!(
                    "SLSA level {} is below minimum required level {}",
                    provenance.slsa_level, min_level.0
                ),
            });
        }

        // 3. Source digest check (if expected digest is provided).
        if let Some(expected) = expected_digest {
            if provenance.source_digest != expected {
                return Err(SecurityError::InvalidSignature {
                    reason: format!(
                        "source digest mismatch: expected {expected}, got {}",
                        provenance.source_digest
                    ),
                });
            }
        }

        // 4. Builder signature verification (FINDING-008: fail-closed).
        //
        // When `trusted_builders` is NOT empty and the builder is not in
        // the list, reject with `InvalidSignature` — an untrusted builder
        // must not pass verification.
        //
        // When `trusted_builders` IS empty (no trusted builders configured),
        // signature verification is skipped (progressive disclosure for
        // SLSA level 1). HOWEVER, SLSA level 2+ requires a non-empty
        // signature by definition: if the signature is empty and the
        // declared SLSA level is >= 2, reject with `InvalidSignature`.
        if self.trusted_builders.is_empty() {
            // No trusted builders configured: skip signature verification
            // (progressive disclosure for SLSA level 1). But SLSA level 2+
            // requires a non-empty signature by definition.
            if provenance.builder_signature.is_empty() && provenance.slsa_level >= 2 {
                return Err(SecurityError::InvalidSignature {
                    reason: format!(
                        "SLSA level {} requires a non-empty builder signature, \
                         but the signature is empty",
                        provenance.slsa_level
                    ),
                });
            }
        } else {
            let public_key_bytes = self
                .trusted_builders
                .iter()
                .find(|(name, _)| name == &provenance.builder)
                .map(|(_, key)| *key)
                .ok_or_else(|| SecurityError::InvalidSignature {
                    reason: format!(
                        "builder '{}' is not in the trusted builders list",
                        provenance.builder
                    ),
                })?;

            // Verify the builder signature using Ed25519.

            let verifying_key = VerifyingKey::from_bytes(&public_key_bytes).map_err(|e| {
                SecurityError::InvalidSignature {
                    reason: format!("invalid builder public key: {e}"),
                }
            })?;

            // The signed payload is the provenance without the signature.
            let payload = provenance_payload(provenance);

            if provenance.builder_signature.len() != 64 {
                return Err(SecurityError::InvalidSignature {
                    reason: "builder signature has wrong length (expected 64 bytes)".to_string(),
                });
            }

            let mut sig_bytes = [0u8; 64];
            sig_bytes.copy_from_slice(&provenance.builder_signature);
            let signature = Signature::from_bytes(&sig_bytes);

            verifying_key.verify(&payload, &signature).map_err(|e| {
                SecurityError::InvalidSignature {
                    reason: format!("builder signature verification failed: {e}"),
                }
            })?;
        }

        Ok(())
    }
}

/// Computes the payload that is signed by the builder.
///
/// This is the provenance struct serialized to JSON without the
/// `builder_signature` field.
fn provenance_payload(provenance: &SlsaProvenance) -> Vec<u8> {
    #[derive(Serialize)]
    struct Payload<'a> {
        builder: &'a str,
        source_repo: &'a str,
        source_digest: &'a str,
        slsa_level: u8,
        build_timestamp: u64,
        build_parameters: &'a [(String, String)],
    }

    let payload = Payload {
        builder: &provenance.builder,
        source_repo: &provenance.source_repo,
        source_digest: &provenance.source_digest,
        slsa_level: provenance.slsa_level,
        build_timestamp: provenance.build_timestamp,
        build_parameters: &provenance.build_parameters,
    };

    serde_json::to_vec(&payload).unwrap_or_default()
}

/// Signs a provenance attestation with a builder's signing key.
///
/// This is used by build systems (or test harnesses) to produce
/// valid provenance. The signature is over the JSON serialization
/// of the provenance without the `builder_signature` field.
///
/// # Errors
///
/// - [`SecurityError::KeyError`] if signing fails.
pub fn sign_provenance(
    provenance: &mut SlsaProvenance,
    signing_key: &crate::crypto::SigningKey,
) -> Result<(), SecurityError> {
    let payload = provenance_payload(provenance);
    let signature = signing_key.sign_raw(&payload)?;
    provenance.builder_signature = signature.0.to_vec();
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_provenance() -> SlsaProvenance {
        SlsaProvenance {
            builder: "github-actions/v3".to_string(),
            source_repo: "github.com/acme/service".to_string(),
            source_digest: "sha256:abc123def456".to_string(),
            // SLSA level 1 does not require a builder signature
            // (progressive disclosure). Level 2+ requires a non-empty
            // signature — see test_verify_slsa_level_requires_signature.
            slsa_level: 1,
            build_timestamp: 1_735_689_600_000,
            builder_signature: Vec::new(),
            build_parameters: vec![("env".to_string(), "production".to_string())],
        }
    }

    /// Creates a provenance at SLSA level 3 with a valid builder signature.
    fn signed_level3_provenance() -> (SlsaProvenance, crate::crypto::KeyPair) {
        let key_pair = crate::crypto::KeyPair::generate();
        let mut provenance = SlsaProvenance {
            slsa_level: 3,
            ..valid_provenance()
        };
        sign_provenance(&mut provenance, key_pair.signing_key()).expect("sign should succeed");
        (provenance, key_pair)
    }

    #[test]
    fn test_verify_no_minimum() {
        let provenance = valid_provenance();
        let verifier = DefaultProvenanceVerifier::new();

        verifier
            .verify(&provenance, MinSlsaLevel::default(), None)
            .expect("verify with no minimum should succeed");
    }

    #[test]
    fn test_verify_slsa_level_meets_minimum() {
        let (provenance, _) = signed_level3_provenance();
        let verifier = DefaultProvenanceVerifier::new();

        verifier
            .verify(&provenance, MinSlsaLevel(2), None)
            .expect("SLSA level 3 >= 2 should succeed");
    }

    #[test]
    fn test_verify_slsa_level_below_minimum() {
        let provenance = SlsaProvenance {
            slsa_level: 1,
            ..valid_provenance()
        };
        let verifier = DefaultProvenanceVerifier::new();

        let result = verifier.verify(&provenance, MinSlsaLevel(2), None);
        assert!(result.is_err(), "SLSA level 1 < 2 should fail");
    }

    #[test]
    fn test_verify_source_digest_match() {
        let provenance = valid_provenance();
        let verifier = DefaultProvenanceVerifier::new();

        verifier
            .verify(&provenance, MinSlsaLevel(0), Some("sha256:abc123def456"))
            .expect("matching digest should succeed");
    }

    #[test]
    fn test_verify_source_digest_mismatch() {
        let provenance = valid_provenance();
        let verifier = DefaultProvenanceVerifier::new();

        let result = verifier.verify(&provenance, MinSlsaLevel(0), Some("sha256:wrong"));
        assert!(result.is_err(), "mismatched digest should fail");
    }

    #[test]
    fn test_verify_empty_builder() {
        let provenance = SlsaProvenance {
            builder: String::new(),
            ..valid_provenance()
        };
        let verifier = DefaultProvenanceVerifier::new();

        let result = verifier.verify(&provenance, MinSlsaLevel(0), None);
        assert!(result.is_err(), "empty builder should fail");
    }

    #[test]
    fn test_verify_empty_source_repo() {
        let provenance = SlsaProvenance {
            source_repo: String::new(),
            ..valid_provenance()
        };
        let verifier = DefaultProvenanceVerifier::new();

        let result = verifier.verify(&provenance, MinSlsaLevel(0), None);
        assert!(result.is_err(), "empty source_repo should fail");
    }

    #[test]
    fn test_verify_empty_source_digest() {
        let provenance = SlsaProvenance {
            source_digest: String::new(),
            ..valid_provenance()
        };
        let verifier = DefaultProvenanceVerifier::new();

        let result = verifier.verify(&provenance, MinSlsaLevel(0), None);
        assert!(result.is_err(), "empty source_digest should fail");
    }

    #[test]
    fn test_sign_and_verify_provenance() {
        let key_pair = crate::crypto::KeyPair::generate();
        let public_key = *key_pair.public_key();

        let mut provenance = SlsaProvenance {
            slsa_level: 3,
            ..valid_provenance()
        };
        sign_provenance(&mut provenance, key_pair.signing_key()).expect("sign should succeed");

        // Without trusted builder: no signature verification (level 3 still passes).
        let verifier = DefaultProvenanceVerifier::new();
        verifier
            .verify(&provenance, MinSlsaLevel(3), None)
            .expect("verify without trusted builder should succeed");

        // With trusted builder: signature is verified.
        let verifier = DefaultProvenanceVerifier::new()
            .with_trusted_builder("github-actions/v3", public_key.0);
        verifier
            .verify(&provenance, MinSlsaLevel(3), None)
            .expect("verify with valid signature should succeed");
    }

    #[test]
    fn test_verify_wrong_builder_signature() {
        let key_pair_a = crate::crypto::KeyPair::generate();
        let key_pair_b = crate::crypto::KeyPair::generate();

        let mut provenance = SlsaProvenance {
            slsa_level: 3,
            ..valid_provenance()
        };
        // Sign with key A.
        sign_provenance(&mut provenance, key_pair_a.signing_key()).expect("sign should succeed");

        // Verify with key B (different key).
        let verifier = DefaultProvenanceVerifier::new()
            .with_trusted_builder("github-actions/v3", key_pair_b.public_key().0);
        let result = verifier.verify(&provenance, MinSlsaLevel(3), None);
        assert!(result.is_err(), "verification with wrong key should fail");
    }

    #[test]
    fn test_verify_tampered_provenance() {
        let key_pair = crate::crypto::KeyPair::generate();
        let public_key = *key_pair.public_key();

        let mut provenance = SlsaProvenance {
            slsa_level: 3,
            ..valid_provenance()
        };
        sign_provenance(&mut provenance, key_pair.signing_key()).expect("sign should succeed");

        // Tamper with the source_repo after signing.
        provenance.source_repo = "github.com/evil/repo".to_string();

        let verifier = DefaultProvenanceVerifier::new()
            .with_trusted_builder("github-actions/v3", public_key.0);
        let result = verifier.verify(&provenance, MinSlsaLevel(3), None);
        assert!(
            result.is_err(),
            "verification of tampered provenance should fail"
        );
    }

    #[test]
    fn test_min_slsa_level_default() {
        assert_eq!(MinSlsaLevel::default(), MinSlsaLevel(0));
    }

    #[test]
    fn test_min_slsa_level_ordering() {
        assert!(MinSlsaLevel(3) > MinSlsaLevel(2));
        assert!(MinSlsaLevel(1) < MinSlsaLevel(2));
    }

    #[test]
    fn test_provenance_serialization_roundtrip() {
        let provenance = valid_provenance();
        let json = serde_json::to_string(&provenance).expect("serialize");
        let decoded: SlsaProvenance = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.builder, provenance.builder);
        assert_eq!(decoded.slsa_level, provenance.slsa_level);
        assert_eq!(decoded.source_digest, provenance.source_digest);
    }

    // -- FINDING-008: fail-closed provenance verification --------------------

    #[test]
    fn test_verify_empty_signature_slsa_level2_rejected() {
        // SLSA level 2+ requires a non-empty builder signature by
        // definition. Even with no trusted builders configured, an
        // empty signature at level 2+ must be rejected (FINDING-008).
        let provenance = SlsaProvenance {
            slsa_level: 2,
            ..valid_provenance()
        };
        let verifier = DefaultProvenanceVerifier::new();
        let result = verifier.verify(&provenance, MinSlsaLevel(0), None);
        assert!(
            result.is_err(),
            "empty signature at SLSA level 2 should be rejected"
        );
        assert!(
            matches!(result, Err(SecurityError::InvalidSignature { .. })),
            "expected InvalidSignature for empty signature at SLSA level 2, got: {result:?}"
        );
    }

    #[test]
    fn test_verify_empty_signature_slsa_level3_rejected() {
        let provenance = SlsaProvenance {
            slsa_level: 3,
            ..valid_provenance()
        };
        let verifier = DefaultProvenanceVerifier::new();
        let result = verifier.verify(&provenance, MinSlsaLevel(0), None);
        assert!(
            result.is_err(),
            "empty signature at SLSA level 3 should be rejected"
        );
    }

    #[test]
    fn test_verify_empty_signature_slsa_level1_allowed() {
        // SLSA level 1 does not require a builder signature (progressive
        // disclosure). An empty signature at level 1 is allowed.
        let provenance = valid_provenance(); // slsa_level: 1, empty signature
        let verifier = DefaultProvenanceVerifier::new();
        verifier
            .verify(&provenance, MinSlsaLevel(0), None)
            .expect("empty signature at SLSA level 1 should be allowed");
    }

    #[test]
    fn test_verify_untrusted_builder_rejected() {
        // When trusted_builders is NOT empty and the builder is not in
        // the list, verification must fail (FINDING-008).
        let key_pair = crate::crypto::KeyPair::generate();

        let mut provenance = SlsaProvenance {
            slsa_level: 3,
            ..valid_provenance()
        };
        sign_provenance(&mut provenance, key_pair.signing_key()).expect("sign should succeed");

        // Trusted builder list contains a DIFFERENT builder.
        let other_key = *crate::crypto::KeyPair::generate().public_key();
        let verifier = DefaultProvenanceVerifier::new()
            .with_trusted_builder("different-builder/v1", other_key.0);

        let result = verifier.verify(&provenance, MinSlsaLevel(3), None);
        assert!(
            result.is_err(),
            "untrusted builder should be rejected when trusted_builders is non-empty"
        );
        assert!(
            matches!(result, Err(SecurityError::InvalidSignature { .. })),
            "expected InvalidSignature for untrusted builder, got: {result:?}"
        );
    }
}
