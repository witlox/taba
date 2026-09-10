//! Security error type — all cryptographic, authorization, and ceremony failures.
//!
//! Every error that can occur during signing, verification, capability
//! enforcement, scope checking, taint computation, delegation validation,
//! or key ceremony is represented by a variant of [`SecurityError`]. The
//! variants are designed to be actionable: they describe what went wrong
//! and (where applicable) which key, author, or unit was involved.

use taba_common::{AuthorId, UnitId, ValidityWindow};

use crate::crypto::KeyId;

/// Errors produced by taba-security operations.
///
/// All variants are permanent (not retriable) unless explicitly noted.
/// Security errors fail closed (INV-S2): when ambiguous, the operation
/// is denied rather than allowed.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// Cryptographic signature is invalid (bad bytes, wrong key, etc.).
    #[error("invalid signature: {reason}")]
    InvalidSignature {
        /// Human-readable description of why the signature is invalid.
        reason: String,
    },

    /// Author's key was revoked before the unit's creation timestamp.
    ///
    /// `revoked_at` is a logical clock value. Any unit whose creation
    /// logical clock is `>= revoked_at` is rejected (causal model, INV-S3).
    #[error("key {key:?} revoked at logical clock {revoked_at}")]
    KeyRevoked {
        /// The key that was revoked.
        key: KeyId,
        /// Logical clock value at which the key was revoked.
        revoked_at: u64,
    },

    /// Signature is outside its validity window.
    #[error("signature for key {key:?} outside validity window")]
    ExpiredSignature {
        /// The key whose signature expired.
        key: KeyId,
        /// The validity window that was exceeded.
        window: ValidityWindow,
    },

    /// Author did not have valid scope at creation time (INV-S5).
    #[error("scope violation by author {author:?}: {reason}")]
    ScopeViolation {
        /// The author who violated scope.
        author: AuthorId,
        /// Human-readable description of the violation.
        reason: String,
    },

    /// Capability access denied — zero default, fail closed (INV-S1, INV-S2).
    #[error("capability denied: {capability} — {reason}")]
    CapabilityDenied {
        /// The capability that was requested.
        capability: String,
        /// Human-readable reason for denial.
        reason: String,
    },

    /// Taint traversal encountered a broken provenance chain (INV-D1).
    #[error("broken provenance for unit {unit:?}: {reason}")]
    BrokenProvenance {
        /// The unit with a broken provenance chain.
        unit: UnitId,
        /// Human-readable description of the break.
        reason: String,
    },

    /// Ceremony protocol violation.
    #[error("ceremony error: {reason}")]
    CeremonyError {
        /// Human-readable description of the ceremony failure.
        reason: String,
    },

    /// Key generation, lookup, or management failure.
    #[error("key error: {reason}")]
    KeyError {
        /// Human-readable description of the key error.
        reason: String,
    },

    /// Declassification requires multi-party signing (INV-S9).
    ///
    /// Minimum 2 distinct authors: one policy-scoped, one data-steward-scoped.
    #[error("declassification denied: {reason}")]
    DeclassificationDenied {
        /// Human-readable reason for denial.
        reason: String,
    },

    /// Author's public key not locally available. The caller should buffer
    /// the unit and retry when keys arrive via gossip.
    #[error("key not found: {key:?}")]
    KeyNotFound {
        /// The key that could not be found.
        key: KeyId,
    },

    /// Delegation token's logical clock range was exceeded.
    #[error("delegation expired: {reason}")]
    DelegationExpired {
        /// Human-readable description.
        reason: String,
    },

    /// Delegation token was used outside its authorized scope.
    #[error("delegation scope violation: {reason}")]
    DelegationScopeViolation {
        /// Human-readable description.
        reason: String,
    },

    /// Delegation token signature is invalid (forged or corrupted).
    #[error("delegation token forged: {reason}")]
    DelegationTokenForged {
        /// Human-readable description.
        reason: String,
    },

    /// Delegation token's spawn count limit was exceeded.
    #[error("delegation spawn limit exceeded: {reason}")]
    DelegationSpawnLimitExceeded {
        /// Human-readable description.
        reason: String,
    },

    /// Spawned task attempted a governance operation (INV-W4a).
    ///
    /// Spawned tasks cannot create policy units, governance units, or
    /// initiate declassification.
    #[error("delegation governance blocked: {reason}")]
    DelegationGovernanceBlocked {
        /// Human-readable description.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = SecurityError::InvalidSignature {
            reason: "bad bytes".to_string(),
        };
        assert_eq!(err.to_string(), "invalid signature: bad bytes");

        let err = SecurityError::CapabilityDenied {
            capability: "storage".to_string(),
            reason: "not granted".to_string(),
        };
        assert_eq!(err.to_string(), "capability denied: storage — not granted");

        let err = SecurityError::CeremonyError {
            reason: "quorum not met".to_string(),
        };
        assert_eq!(err.to_string(), "ceremony error: quorum not met");
    }
}
