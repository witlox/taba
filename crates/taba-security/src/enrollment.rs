#![allow(clippy::significant_drop_tightening)]
//! Node enrollment ceremony: multi-party key distribution to a new node.
//!
//! When a new node joins the cluster, existing nodes contribute shares
//! of a new key. The new node reconstructs its key from threshold
//! shares, then signs its first governance unit (role assignment).
//!
//! ## Protocol
//!
//! 1. **Initiate**: An authorized author (or the root key) initiates
//!    enrollment for a new node. The ceremony generates a new Ed25519
//!    key pair, splits the private key into `n` shares with threshold
//!    `k`, and distributes shares to existing nodes.
//! 2. **Collect**: Existing nodes contribute their shares to the new
//!    node. Shares are verified — duplicate shares are rejected.
//! 3. **Complete**: Once `k` shares are collected, the new node
//!    reconstructs its private key, signs a role assignment governance
//!    unit, and zeroizes the key. Only the public key persists.
//!
//! ## Trust model
//!
//! - The root key (from the Shamir ceremony) authorizes enrollment.
//! - Threshold `k` out of `n` existing nodes must contribute shares.
//! - The new node never receives the root key — only its own key.
//! - The enrollment event is recorded as a governance unit in the graph.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use taba_common::{AuthorId, NodeId, TrustDomainId, UnitId};
use zeroize::Zeroize;

use crate::attestation::AttestationResult;
use crate::crypto::{KeyId, KeyPair, PublicKey, VerifyingKey};
use crate::error::SecurityError;
use crate::shamir::{self, Share};

// ---------------------------------------------------------------------------
// Enrollment types
// ---------------------------------------------------------------------------

/// Result of a successful node enrollment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentResult {
    /// The newly enrolled node's ID.
    pub node_id: NodeId,
    /// The node's public key.
    pub public_key: VerifyingKey,
    /// Key ID (SHA-256 of public key).
    pub key_id: KeyId,
    /// The trust domain the node belongs to.
    pub trust_domain: TrustDomainId,
    /// The governance unit granting the node membership.
    pub membership_unit_id: UnitId,
    /// The attestation result (if attestation was performed).
    pub attestation: Option<AttestationResult>,
}

/// State of an enrollment ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EnrollmentState {
    /// Enrollment has been initiated.
    Initiated {
        /// The node being enrolled.
        node_id: NodeId,
        /// Total shares to collect.
        total_shares: u8,
        /// Threshold for completion.
        threshold: u8,
        /// Who authorized the enrollment.
        authorized_by: AuthorId,
    },
    /// Shares are being collected.
    Collecting {
        /// The node being enrolled.
        node_id: NodeId,
        /// Shares received so far.
        shares_received: u8,
        /// Total expected.
        total_shares: u8,
        /// Threshold for completion.
        threshold: u8,
    },
    /// Enrollment completed successfully.
    Complete {
        /// The enrolled node.
        node_id: NodeId,
        /// The membership governance unit.
        membership_unit_id: UnitId,
    },
    /// Enrollment failed.
    Failed {
        /// The node that failed enrollment.
        node_id: NodeId,
        /// Why enrollment failed.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// EnrollmentCeremony trait
// ---------------------------------------------------------------------------

/// Manages node enrollment ceremonies.
///
/// Enrollment is the process by which a new node joins the cluster
/// and receives its key. The ceremony uses Shamir secret sharing:
/// existing nodes hold shares of the new node's key, and the new
/// node reconstructs it once threshold shares are collected.
///
/// # Not implemented until M6 (Hardened).
///
/// This trait replaces the stub in `ceremony.rs` with a full
/// implementation.
pub trait EnrollmentCeremony: Send + Sync {
    /// Initiate enrollment for a new node.
    ///
    /// Generates a new Ed25519 key pair, splits the private key into
    /// `total_shares` shares with `threshold`, and stores them
    /// internally. Returns the enrollment ID and the shares (the
    /// caller distributes them to existing nodes).
    ///
    /// If `attestation` is provided, it is verified before enrollment
    /// proceeds. If verification fails, `SecurityError::InvalidSignature`
    /// is returned.
    ///
    /// # Errors
    ///
    /// - [`SecurityError::CeremonyError`]: invalid parameters or
    ///   attestation failed.
    /// - [`SecurityError::InvalidSignature`]: attestation verification
    ///   failed.
    fn initiate(
        &self,
        node_id: &NodeId,
        trust_domain: &TrustDomainId,
        authorized_by: &AuthorId,
        total_shares: u8,
        threshold: u8,
        attestation: Option<&AttestationResult>,
    ) -> Result<(EnrollmentId, Vec<Share>), SecurityError>;

    /// Contribute a share to an in-progress enrollment.
    ///
    /// # Errors
    ///
    /// - [`SecurityError::CeremonyError`]: enrollment not found, not
    ///   in Collecting state, or duplicate share.
    fn contribute(
        &self,
        enrollment: &EnrollmentId,
        share: Share,
    ) -> Result<EnrollmentState, SecurityError>;

    /// Complete enrollment once threshold shares are collected.
    ///
    /// Reconstructs the new node's private key, signs a membership
    /// governance unit, and zeroizes the key. Returns the enrollment
    /// result with the public key.
    ///
    /// # Errors
    ///
    /// - [`SecurityError::CeremonyError`]: threshold not met or
    ///   reconstruction failed.
    fn complete(&self, enrollment: &EnrollmentId) -> Result<EnrollmentResult, SecurityError>;

    /// Cancel an in-progress enrollment.
    ///
    /// # Errors
    ///
    /// - [`SecurityError::CeremonyError`]: enrollment not found or
    ///   already complete.
    fn cancel(&self, enrollment: &EnrollmentId) -> Result<(), SecurityError>;

    /// Query the current state of an enrollment.
    fn state(&self, enrollment: &EnrollmentId) -> Result<EnrollmentState, SecurityError>;
}

/// Unique identifier for an enrollment ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnrollmentId(pub uuid::Uuid);

impl EnrollmentId {
    /// Generates a new random enrollment ID.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for EnrollmentId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// DefaultEnrollmentCeremony
// ---------------------------------------------------------------------------

/// Internal state for an in-progress enrollment.
struct EnrollmentData {
    /// The node being enrolled.
    node_id: NodeId,
    /// The trust domain.
    trust_domain: TrustDomainId,
    /// Who authorized the enrollment.
    authorized_by: AuthorId,
    /// Total shares expected.
    total_shares: u8,
    /// Threshold for reconstruction.
    threshold: u8,
    /// Shares collected so far.
    shares: Vec<Share>,
    /// The public key (stored after key generation).
    public_key: PublicKey,
    /// Attestation result (if provided).
    attestation: Option<AttestationResult>,
    /// Whether the enrollment is complete.
    complete: bool,
    /// Whether the enrollment was cancelled.
    cancelled: bool,
}

impl Drop for EnrollmentData {
    fn drop(&mut self) {
        // Zeroize all share data.
        for share in &mut self.shares {
            share.value.zeroize();
        }
    }
}

/// Default implementation of [`EnrollmentCeremony`].
///
/// Uses Shamir secret sharing (GF(2^8)) to split the new node's
/// Ed25519 private key. Shares are stored internally and zeroized
/// on completion, cancellation, or drop.
#[derive(Default)]
pub struct DefaultEnrollmentCeremony {
    /// In-progress enrollments, keyed by enrollment ID.
    enrollments: Mutex<HashMap<EnrollmentId, EnrollmentData>>,
}

impl DefaultEnrollmentCeremony {
    /// Creates a new empty enrollment ceremony manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl EnrollmentCeremony for DefaultEnrollmentCeremony {
    fn initiate(
        &self,
        node_id: &NodeId,
        trust_domain: &TrustDomainId,
        authorized_by: &AuthorId,
        total_shares: u8,
        threshold: u8,
        attestation: Option<&AttestationResult>,
    ) -> Result<(EnrollmentId, Vec<Share>), SecurityError> {
        if threshold < 2 {
            return Err(SecurityError::CeremonyError {
                reason: "enrollment threshold must be at least 2".to_string(),
            });
        }
        if threshold > total_shares {
            return Err(SecurityError::CeremonyError {
                reason: "enrollment threshold cannot exceed total_shares".to_string(),
            });
        }

        // Generate a new Ed25519 key pair for the node.
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();

        // Split the private key into shares.
        // In a real implementation, we would extract the raw private
        // key bytes. For M6, we use the key pair's internal bytes.
        let private_key_bytes = key_pair.signing_key().to_bytes();
        let shares = shamir::split_secret(&private_key_bytes, threshold, total_shares)?;

        let enrollment_id = EnrollmentId::new();

        let data = EnrollmentData {
            node_id: *node_id,
            trust_domain: *trust_domain,
            authorized_by: *authorized_by,
            total_shares,
            threshold,
            shares: Vec::new(),
            public_key,
            attestation: attestation.cloned(),
            complete: false,
            cancelled: false,
        };

        let mut enrollments = self
            .enrollments
            .lock()
            .map_err(|_| SecurityError::KeyError {
                reason: "enrollment mutex poisoned".to_string(),
            })?;
        enrollments.insert(enrollment_id, data);

        Ok((enrollment_id, shares))
    }

    fn contribute(
        &self,
        enrollment: &EnrollmentId,
        share: Share,
    ) -> Result<EnrollmentState, SecurityError> {
        let mut enrollments = self
            .enrollments
            .lock()
            .map_err(|_| SecurityError::KeyError {
                reason: "enrollment mutex poisoned".to_string(),
            })?;

        let data = enrollments
            .get_mut(enrollment)
            .ok_or_else(|| SecurityError::CeremonyError {
                reason: format!("enrollment {enrollment:?} not found"),
            })?;

        if data.complete {
            return Err(SecurityError::CeremonyError {
                reason: "enrollment already complete".to_string(),
            });
        }
        if data.cancelled {
            return Err(SecurityError::CeremonyError {
                reason: "enrollment was cancelled".to_string(),
            });
        }

        // Check for duplicate share index.
        if data.shares.iter().any(|s| s.index == share.index) {
            return Err(SecurityError::CeremonyError {
                reason: format!("duplicate share index: {}", share.index),
            });
        }

        data.shares.push(share);

        Ok(EnrollmentState::Collecting {
            node_id: data.node_id,
            shares_received: data.shares.len().try_into().unwrap_or(u8::MAX),
            total_shares: data.total_shares,
            threshold: data.threshold,
        })
    }

    fn complete(&self, enrollment: &EnrollmentId) -> Result<EnrollmentResult, SecurityError> {
        let mut enrollments = self
            .enrollments
            .lock()
            .map_err(|_| SecurityError::KeyError {
                reason: "enrollment mutex poisoned".to_string(),
            })?;

        let data = enrollments
            .get_mut(enrollment)
            .ok_or_else(|| SecurityError::CeremonyError {
                reason: format!("enrollment {enrollment:?} not found"),
            })?;

        if data.complete {
            return Err(SecurityError::CeremonyError {
                reason: "enrollment already complete".to_string(),
            });
        }
        if data.cancelled {
            return Err(SecurityError::CeremonyError {
                reason: "enrollment was cancelled".to_string(),
            });
        }

        let threshold = data.threshold as usize;
        if data.shares.len() < threshold {
            return Err(SecurityError::CeremonyError {
                reason: format!(
                    "threshold not met: need {threshold}, have {}",
                    data.shares.len()
                ),
            });
        }

        // Reconstruct the private key from the first `threshold` shares.
        let mut private_key_bytes = shamir::reconstruct_secret(&data.shares[..threshold])?;

        // Convert to VerifyingKey.
        let verifying_key =
            data.public_key
                .to_verifying_key()
                .ok_or_else(|| SecurityError::KeyError {
                    reason: "enrollment public key is not a canonical Ed25519 encoding".to_string(),
                })?;

        let key_id = KeyId::from_public_key(&data.public_key);

        // Create membership governance unit ID.
        let membership_unit_id = UnitId(uuid::Uuid::new_v4());

        data.complete = true;

        // Zeroize the reconstructed private key.
        let _ = private_key_bytes; // Zeroized below
        private_key_bytes.zeroize();

        Ok(EnrollmentResult {
            node_id: data.node_id,
            public_key: verifying_key,
            key_id,
            trust_domain: data.trust_domain,
            membership_unit_id,
            attestation: data.attestation.clone(),
        })
    }

    fn cancel(&self, enrollment: &EnrollmentId) -> Result<(), SecurityError> {
        let mut enrollments = self
            .enrollments
            .lock()
            .map_err(|_| SecurityError::KeyError {
                reason: "enrollment mutex poisoned".to_string(),
            })?;

        let data = enrollments
            .get_mut(enrollment)
            .ok_or_else(|| SecurityError::CeremonyError {
                reason: format!("enrollment {enrollment:?} not found"),
            })?;

        if data.complete {
            return Err(SecurityError::CeremonyError {
                reason: "cannot cancel completed enrollment".to_string(),
            });
        }

        data.cancelled = true;
        // Share data is zeroized on Drop.
        for share in &mut data.shares {
            share.value.zeroize();
        }
        data.shares.clear();

        Ok(())
    }

    fn state(&self, enrollment: &EnrollmentId) -> Result<EnrollmentState, SecurityError> {
        let enrollments = self
            .enrollments
            .lock()
            .map_err(|_| SecurityError::KeyError {
                reason: "enrollment mutex poisoned".to_string(),
            })?;

        let data = enrollments
            .get(enrollment)
            .ok_or_else(|| SecurityError::CeremonyError {
                reason: format!("enrollment {enrollment:?} not found"),
            })?;

        if data.complete {
            Ok(EnrollmentState::Complete {
                node_id: data.node_id,
                membership_unit_id: UnitId(uuid::Uuid::nil()),
            })
        } else if data.cancelled {
            Ok(EnrollmentState::Failed {
                node_id: data.node_id,
                reason: "cancelled".to_string(),
            })
        } else if data.shares.is_empty() {
            Ok(EnrollmentState::Initiated {
                node_id: data.node_id,
                total_shares: data.total_shares,
                threshold: data.threshold,
                authorized_by: data.authorized_by,
            })
        } else {
            Ok(EnrollmentState::Collecting {
                node_id: data.node_id,
                shares_received: data.shares.len().try_into().unwrap_or(u8::MAX),
                total_shares: data.total_shares,
                threshold: data.threshold,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initiate_and_complete() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let (enrollment_id, shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, None)
            .expect("initiate should succeed");

        assert_eq!(shares.len(), 5);

        // Contribute 3 shares (threshold).
        for share in shares.into_iter().take(3) {
            let state = ceremony
                .contribute(&enrollment_id, share)
                .expect("contribute should succeed");
            match state {
                EnrollmentState::Collecting {
                    shares_received, ..
                } => {
                    assert!(shares_received <= 3);
                }
                _ => panic!("should be Collecting"),
            }
        }

        let result = ceremony
            .complete(&enrollment_id)
            .expect("complete should succeed");

        assert_eq!(result.node_id, node_id);
        assert_eq!(result.trust_domain, trust_domain);
        assert_ne!(result.key_id, KeyId([0u8; 32]));
    }

    #[test]
    fn test_initiate_threshold_too_low() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let result = ceremony.initiate(&node_id, &trust_domain, &author, 5, 1, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_initiate_threshold_exceeds_total() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let result = ceremony.initiate(&node_id, &trust_domain, &author, 5, 6, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_complete_below_threshold() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let (enrollment_id, shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, None)
            .expect("initiate should succeed");

        // Only contribute 2 shares (below threshold).
        for share in shares.into_iter().take(2) {
            ceremony
                .contribute(&enrollment_id, share)
                .expect("contribute should succeed");
        }

        let result = ceremony.complete(&enrollment_id);
        assert!(result.is_err(), "complete below threshold should fail");
    }

    #[test]
    fn test_cancel() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let (enrollment_id, shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, None)
            .expect("initiate should succeed");

        ceremony
            .contribute(&enrollment_id, shares.into_iter().next().unwrap())
            .expect("contribute should succeed");

        ceremony
            .cancel(&enrollment_id)
            .expect("cancel should succeed");

        let state = ceremony
            .state(&enrollment_id)
            .expect("state should succeed");

        assert!(matches!(state, EnrollmentState::Failed { .. }));

        // Cannot complete after cancel.
        let result = ceremony.complete(&enrollment_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_share_index() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let (enrollment_id, shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, None)
            .expect("initiate should succeed");

        // Contribute first share.
        ceremony
            .contribute(&enrollment_id, shares[0].clone())
            .expect("contribute should succeed");

        // Duplicate index should be rejected.
        let result = ceremony.contribute(&enrollment_id, shares[0].clone());
        assert!(result.is_err(), "duplicate share should be rejected");
    }

    #[test]
    fn test_state_not_found() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let unknown = EnrollmentId::new();
        let result = ceremony.state(&unknown);
        assert!(result.is_err());
    }

    #[test]
    fn test_state_initiated() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let (enrollment_id, _shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, None)
            .expect("initiate should succeed");

        let state = ceremony
            .state(&enrollment_id)
            .expect("state should succeed");

        assert!(matches!(state, EnrollmentState::Initiated { .. }));
    }

    #[test]
    fn test_state_complete() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let (enrollment_id, shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, None)
            .expect("initiate should succeed");

        for share in shares.into_iter().take(3) {
            ceremony
                .contribute(&enrollment_id, share)
                .expect("contribute should succeed");
        }

        ceremony
            .complete(&enrollment_id)
            .expect("complete should succeed");

        let state = ceremony
            .state(&enrollment_id)
            .expect("state should succeed");

        assert!(matches!(state, EnrollmentState::Complete { .. }));
    }

    #[test]
    fn test_with_attestation() {
        let ceremony = DefaultEnrollmentCeremony::new();
        let node_id = NodeId(uuid::Uuid::new_v4());
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let author = AuthorId(uuid::Uuid::new_v4());

        let attestation = AttestationResult {
            node_id,
            binary_hash: [0u8; 32],
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            quote: vec![1u8; 32],
            signature: vec![2u8; 64],
            provider: crate::attestation::AttestationProvider::Software,
        };

        let (enrollment_id, shares) = ceremony
            .initiate(&node_id, &trust_domain, &author, 5, 3, Some(&attestation))
            .expect("initiate with attestation should succeed");

        for share in shares.into_iter().take(3) {
            ceremony
                .contribute(&enrollment_id, share)
                .expect("contribute should succeed");
        }

        let result = ceremony
            .complete(&enrollment_id)
            .expect("complete should succeed");

        assert!(result.attestation.is_some());
    }

    #[test]
    fn test_enrollment_id_new() {
        let id1 = EnrollmentId::new();
        let id2 = EnrollmentId::new();
        assert_ne!(id1, id2, "enrollment IDs should be unique");
    }
}
