//! Shamir key ceremony, solo bootstrap, and trust governance types.
//!
//! This module defines the types and traits for taba's cryptographic
//! root of trust. In M2, only Tier 0 solo bootstrap is implemented;
//! the full Shamir ceremony (Tiers 1-3) is deferred to M6 (Hardened).
//!
//! # Timestamp resolution (A002)
//!
//! The spec data-models reference a `Timestamp` type that was renamed
//! to [`DualClockEvent`] (design decision A002). This module applies
//! the resolution consistently:
//! - `Author.created_at`, `TrustDomain.established_at`,
//!   `KeyRevocation.revoked_at`, `CeremonyState` timestamps → [`DualClockEvent`]
//! - `TrustDomain.expires_at` → [`Option<WallTime>`]

use serde::{Deserialize, Serialize};
use taba_common::{
    AuthorId, CeremonyId, DualClockEvent, NodeId, TrustDomainId, UnitId, Version, WallTime,
};

use crate::crypto::{KeyId, KeyPair, PublicKey, VerifyingKey};
use crate::error::SecurityError;

// ===========================================================================
// Trust governance types (from data-models)
// ===========================================================================

/// An authenticated identity with scoped authority to create units.
///
/// Zero access by default — all scopes are explicit (INV-S5). No two
/// distinct authors may have identical scope tuples for state-producing
/// unit types (INV-S8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    /// Unique identifier for this author.
    pub id: AuthorId,
    /// The author's Ed25519 public key.
    pub public_key: PublicKey,
    /// Human-readable display name.
    pub display_name: String,
    /// When this author identity was created (dual clock — logical for
    /// ordering, wall time for compliance).
    pub created_at: DualClockEvent,
    /// Whether this author's key has been revoked.
    pub revoked: bool,
    /// If revoked, the revocation details.
    pub revocation: Option<KeyRevocation>,
}

/// A trust domain — an authorization boundary scoping author permissions.
///
/// Itself a governance unit, created through multi-party agreement
/// (INV-S6, INV-S10). Minimum 2 distinct author signatures required for
/// creation. Cross-domain role inheritance is disabled by default
/// (INV-S6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustDomain {
    /// The unique identifier for this trust domain.
    pub id: TrustDomainId,
    /// Human-readable name.
    pub name: String,
    /// The governance unit that defines this domain.
    pub governance_unit_id: UnitId,
    /// Authors who co-signed the domain creation (minimum 2, INV-S10).
    pub founding_signers: Vec<AuthorId>,
    /// When this domain was established (dual clock).
    pub established_at: DualClockEvent,
    /// When this domain expires (wall-clock deadline for compliance,
    /// INV-T2). `None` means the domain does not expire.
    pub expires_at: Option<WallTime>,
    /// Whether cross-domain role inheritance is enabled (default: no, INV-S6).
    pub allows_cross_domain_roles: bool,
}

// ===========================================================================
// Key revocation (from data-models)
// ===========================================================================

/// Revocation record for a compromised or retired key.
///
/// Propagated via priority gossip. Units signed after the revocation
/// timestamp are rejected (INV-S3). Units signed before remain valid
/// (causal model — no retroactive rejection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRevocation {
    /// The author whose key is revoked.
    pub author_id: AuthorId,
    /// The revoked public key.
    pub revoked_key: PublicKey,
    /// When the key was revoked (dual clock — logical for ordering,
    /// wall time for compliance). Units signed after this are invalid.
    pub revoked_at: DualClockEvent,
    /// Reason for revocation.
    pub reason: RevocationReason,
    /// Who authorized the revocation.
    pub authorized_by: AuthorId,
    /// Version for ordering multiple revocations of the same author.
    pub version: Version,
}

/// Reason a key was revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RevocationReason {
    /// Key material was compromised.
    Compromised,
    /// Author has left the organization.
    Departed,
    /// Key rotation — replaced by a new key.
    Rotated {
        /// The new public key replacing this one.
        replacement: PublicKey,
    },
    /// Administrative revocation.
    Administrative {
        /// Details of the administrative action.
        details: String,
    },
}

// ===========================================================================
// Shamir ceremony types (from data-models)
// ===========================================================================

/// A share of the Shamir-split root key.
///
/// The root key is the root of all authority in a taba cluster. Shares
/// are never aggregated in memory — each participant holds their share
/// and the ceremony reconstructs the key only at completion, then
/// immediately zeroizes it.
///
/// Tier 1 (Phase 1): basic — start → add shares → complete with witness.
/// Tier 2 (Phase 3): password-protected — each share encrypted with Argon2id.
/// Tier 3 (Phase 5): offline two-factor — seed code + password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShamirShare {
    /// Index of this share (1-based).
    pub index: u8,
    /// The share data. Zeroed from memory after use.
    pub data: Vec<u8>,
    /// The ceremony this share belongs to.
    pub ceremony_id: CeremonyId,
    /// Whether this share is password-protected (Tier 2+).
    pub encrypted: bool,
}

/// State machine for the Shamir key ceremony.
///
/// Ceremony events are recorded as governance units in the graph.
///
/// Tier 1 flow: `Created` → `CollectingShares` → `Complete`.
/// Failure at any point → `Failed` (ceremony must restart).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CeremonyState {
    /// Ceremony has been initiated.
    Created {
        /// Unique identifier for this ceremony.
        ceremony_id: CeremonyId,
        /// Total number of shares to generate.
        total_shares: u8,
        /// Minimum shares required for reconstruction.
        threshold: u8,
        /// Who initiated the ceremony.
        initiator: AuthorId,
        /// When the ceremony was started (dual clock).
        started_at: DualClockEvent,
    },
    /// Shares are being collected from participants.
    CollectingShares {
        /// Unique identifier for this ceremony.
        ceremony_id: CeremonyId,
        /// Shares received so far (count, not content — shares are never
        /// aggregated in memory).
        shares_received: u8,
        /// Total expected.
        total_shares: u8,
        /// Threshold for completion.
        threshold: u8,
    },
    /// Ceremony completed successfully. Root key reconstructed and used.
    Complete {
        /// Unique identifier for this ceremony.
        ceremony_id: CeremonyId,
        /// When the ceremony completed (dual clock).
        completed_at: DualClockEvent,
        /// Witnesses who observed the ceremony.
        witnesses: Vec<NodeId>,
        /// The governance unit created by the ceremony (seeds the graph).
        root_governance_unit: UnitId,
    },
    /// Ceremony failed and must be restarted.
    Failed {
        /// Unique identifier for this ceremony.
        ceremony_id: CeremonyId,
        /// Why the ceremony failed.
        reason: String,
        /// When the failure occurred (dual clock).
        failed_at: DualClockEvent,
    },
}

// ===========================================================================
// CeremonyManager trait (DEFERRED to M6)
// ===========================================================================

/// Manages the Shamir Tier 1 key ceremony (DL-005).
///
/// The ceremony is the pre-graph bootstrap: it produces the root key
/// whose public half signs the first governance unit, seeding the
/// composition graph.
///
/// Protocol: start → `add_share` (repeated) → complete with witness.
/// Ceremony events are recorded as governance units in the graph.
///
/// # Not implemented until M6 (Hardened).
///
/// All methods of [`DefaultCeremonyManager`] return
/// [`SecurityError::CeremonyError`]. Use [`SoloBootstrap`] (Tier 0)
/// for single-node initialization.
pub trait CeremonyManager {
    /// Start a new Shamir ceremony with the given threshold and total shares.
    ///
    /// Default: 5 shares, threshold 3. The ceremony enters `Collecting`
    /// state.
    ///
    /// Returns [`SecurityError::CeremonyError`] if a ceremony is already
    /// in progress or parameters are invalid (threshold > total,
    /// threshold < 2).
    fn start(
        &self,
        total_shares: u8,
        threshold: u8,
    ) -> impl std::future::Future<Output = Result<CeremonyId, SecurityError>> + Send;

    /// Add a share to an in-progress ceremony.
    ///
    /// Shares are verified as they arrive. Duplicate shares from the same
    /// holder are rejected.
    ///
    /// Returns the updated ceremony state (how many shares received vs.
    /// threshold).
    fn add_share(
        &self,
        ceremony: &CeremonyId,
        share: ShamirShare,
    ) -> impl std::future::Future<Output = Result<CeremonyState, SecurityError>> + Send;

    /// Complete the ceremony once threshold shares are collected.
    ///
    /// Requires a witness node to co-sign the ceremony completion event.
    /// The reconstructed key signs the root governance unit, then is
    /// immediately zeroized. Only the public key persists.
    ///
    /// Returns [`SecurityError::CeremonyError`] if threshold not met,
    /// witness is invalid, or reconstruction fails.
    fn complete(
        &self,
        ceremony: &CeremonyId,
        witness_node: &KeyId,
    ) -> impl std::future::Future<Output = Result<VerifyingKey, SecurityError>> + Send;

    /// Cancel an in-progress ceremony. All collected shares are zeroized.
    ///
    /// Returns [`SecurityError::CeremonyError`] if the ceremony does not
    /// exist or is already complete.
    fn cancel(
        &self,
        ceremony: &CeremonyId,
    ) -> impl std::future::Future<Output = Result<(), SecurityError>> + Send;

    /// Query the current state of a ceremony.
    fn state(&self, ceremony: &CeremonyId) -> Result<CeremonyState, SecurityError>;
}

/// Default ceremony manager — **not implemented until M6 (Hardened)**.
///
/// All methods return [`SecurityError::CeremonyError`]. Use
/// [`DefaultSoloBootstrap`] for Tier 0 single-node initialization.
#[derive(Debug, Clone, Default)]
pub struct DefaultCeremonyManager;

impl CeremonyManager for DefaultCeremonyManager {
    fn start(
        &self,
        _total_shares: u8,
        _threshold: u8,
    ) -> impl std::future::Future<Output = Result<CeremonyId, SecurityError>> + Send {
        std::future::ready(Err(SecurityError::CeremonyError {
            reason: "Not implemented until M6 (Hardened).".to_string(),
        }))
    }

    fn add_share(
        &self,
        _ceremony: &CeremonyId,
        _share: ShamirShare,
    ) -> impl std::future::Future<Output = Result<CeremonyState, SecurityError>> + Send {
        std::future::ready(Err(SecurityError::CeremonyError {
            reason: "Not implemented until M6 (Hardened).".to_string(),
        }))
    }

    fn complete(
        &self,
        _ceremony: &CeremonyId,
        _witness_node: &KeyId,
    ) -> impl std::future::Future<Output = Result<VerifyingKey, SecurityError>> + Send {
        std::future::ready(Err(SecurityError::CeremonyError {
            reason: "Not implemented until M6 (Hardened).".to_string(),
        }))
    }

    fn cancel(
        &self,
        _ceremony: &CeremonyId,
    ) -> impl std::future::Future<Output = Result<(), SecurityError>> + Send {
        std::future::ready(Err(SecurityError::CeremonyError {
            reason: "Not implemented until M6 (Hardened).".to_string(),
        }))
    }

    fn state(&self, _ceremony: &CeremonyId) -> Result<CeremonyState, SecurityError> {
        Err(SecurityError::CeremonyError {
            reason: "Not implemented until M6 (Hardened).".to_string(),
        })
    }
}

// ===========================================================================
// Solo bootstrap (Tier 0)
// ===========================================================================

/// Tier 0 solo bootstrap: `taba init` in one command.
///
/// Generates a node key + author key + self-signed trust domain + root
/// governance unit. No Shamir, no shares, no witnesses. The developer
/// is immediately operational.
///
/// This is the simplest way to start a taba cluster. It produces a
/// single-key cluster where the developer holds all authority. For
/// production deployments, use the full Shamir ceremony
/// ([`CeremonyManager`]) — available in M6 (Hardened).
pub trait SoloBootstrap {
    /// Initialize a single-key cluster (Tier 0).
    ///
    /// Generates an Ed25519 key pair, creates a self-signed trust domain
    /// and root governance unit, and returns the key ID, public key,
    /// trust domain ID, and governance unit IDs.
    ///
    /// The private key is generated in memory and zeroized when this
    /// function returns — in a real deployment, the caller is
    /// responsible for persisting the private key (e.g., to a platform
    /// keystore). For M2, the caller gets the public parts only.
    fn solo_init(
        &self,
    ) -> impl std::future::Future<Output = Result<SoloBootstrapResult, SecurityError>> + Send;
}

/// Result of a successful solo bootstrap (Tier 0).
///
/// Contains the key ID, public key, trust domain ID, and governance
/// unit IDs needed to start operating immediately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoloBootstrapResult {
    /// SHA-256 hash of the public key — stable key identifier.
    pub key_id: KeyId,
    /// The Ed25519 verifying key (public key).
    pub public_key: VerifyingKey,
    /// The trust domain created by the bootstrap.
    pub trust_domain: TrustDomainId,
    /// The root governance unit created by the bootstrap.
    pub governance_unit_id: UnitId,
    /// The role assignment governance unit granting the developer
    /// authority over the new cluster.
    pub role_assignment_id: UnitId,
}

/// Default implementation of [`SoloBootstrap`].
///
/// Generates an Ed25519 key pair using the operating system's CSPRNG,
/// derives the key ID, and creates a self-signed trust domain with a
/// root governance unit and role assignment.
#[derive(Debug, Clone, Default)]
pub struct DefaultSoloBootstrap;

impl DefaultSoloBootstrap {
    /// Creates a new solo bootstrap instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl SoloBootstrap for DefaultSoloBootstrap {
    fn solo_init(
        &self,
    ) -> impl std::future::Future<Output = Result<SoloBootstrapResult, SecurityError>> + Send {
        // Generate a new Ed25519 key pair.
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let key_id = KeyId::from_public_key(&public_key);

        // Convert to VerifyingKey for the result.
        let verifying_key = public_key
            .to_verifying_key()
            .ok_or_else(|| SecurityError::KeyError {
                reason: "generated public key is not a canonical Ed25519 encoding".to_string(),
            });

        let result = verifying_key.map(|verifying_key| {
            // Create a self-signed trust domain and root governance unit.
            let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
            let governance_unit_id = UnitId(uuid::Uuid::new_v4());
            let role_assignment_id = UnitId(uuid::Uuid::new_v4());

            SoloBootstrapResult {
                key_id,
                public_key: verifying_key,
                trust_domain,
                governance_unit_id,
                role_assignment_id,
            }
        });

        std::future::ready(result)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_solo_bootstrap() {
        let bootstrap = DefaultSoloBootstrap::new();
        let result = bootstrap
            .solo_init()
            .await
            .expect("solo bootstrap should succeed");

        // Key ID should be non-zero (SHA-256 of a non-zero public key).
        assert_ne!(result.key_id, KeyId([0u8; 32]), "key ID should be non-zero");

        // Trust domain should be a non-nil UUID.
        assert_ne!(
            result.trust_domain,
            TrustDomainId(uuid::Uuid::nil()),
            "trust domain should be non-zero"
        );

        // Governance unit and role assignment IDs should be non-nil and different.
        assert_ne!(
            result.governance_unit_id,
            UnitId(uuid::Uuid::nil()),
            "governance unit ID should be non-zero"
        );
        assert_ne!(
            result.role_assignment_id,
            UnitId(uuid::Uuid::nil()),
            "role assignment ID should be non-zero"
        );
        assert_ne!(
            result.governance_unit_id, result.role_assignment_id,
            "governance unit ID and role assignment ID should be different"
        );

        // The public key should round-trip to a PublicKey.
        let pk = result.public_key.to_public_key();
        assert_eq!(KeyId::from_public_key(&pk), result.key_id);
    }

    #[tokio::test]
    async fn test_ceremony_manager_not_implemented() {
        let manager = DefaultCeremonyManager;
        let ceremony_id = CeremonyId(uuid::Uuid::new_v4());
        let witness = KeyId([0u8; 32]);
        let share = ShamirShare {
            index: 1,
            data: vec![0u8; 32],
            ceremony_id,
            encrypted: false,
        };

        assert!(
            matches!(
                manager.start(5, 3).await,
                Err(SecurityError::CeremonyError { .. })
            ),
            "start() should return CeremonyError (not implemented until M6)"
        );
        assert!(
            matches!(
                manager.add_share(&ceremony_id, share).await,
                Err(SecurityError::CeremonyError { .. })
            ),
            "add_share() should return CeremonyError (not implemented until M6)"
        );
        assert!(
            matches!(
                manager.complete(&ceremony_id, &witness).await,
                Err(SecurityError::CeremonyError { .. })
            ),
            "complete() should return CeremonyError (not implemented until M6)"
        );
        assert!(
            matches!(
                manager.cancel(&ceremony_id).await,
                Err(SecurityError::CeremonyError { .. })
            ),
            "cancel() should return CeremonyError (not implemented until M6)"
        );
        assert!(
            matches!(
                manager.state(&ceremony_id),
                Err(SecurityError::CeremonyError { .. })
            ),
            "state() should return CeremonyError (not implemented until M6)"
        );
    }

    #[test]
    fn test_key_revocation_serialize_roundtrip() {
        let revocation = KeyRevocation {
            author_id: AuthorId(uuid::Uuid::new_v4()),
            revoked_key: PublicKey([1u8; 32]),
            revoked_at: DualClockEvent {
                logical_clock: taba_common::LogicalClock(42),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            reason: RevocationReason::Compromised,
            authorized_by: AuthorId(uuid::Uuid::new_v4()),
            version: Version(1),
        };

        let json1 = serde_json::to_string(&revocation).expect("serialize KeyRevocation");
        let decoded: KeyRevocation =
            serde_json::from_str(&json1).expect("deserialize KeyRevocation");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize KeyRevocation");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_author_serialize_roundtrip() {
        let author = Author {
            id: AuthorId(uuid::Uuid::new_v4()),
            public_key: PublicKey([2u8; 32]),
            display_name: "Test Author".to_string(),
            created_at: DualClockEvent {
                logical_clock: taba_common::LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            revoked: false,
            revocation: None,
        };

        let json1 = serde_json::to_string(&author).expect("serialize Author");
        let decoded: Author = serde_json::from_str(&json1).expect("deserialize Author");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize Author");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_trust_domain_serialize_roundtrip() {
        let domain = TrustDomain {
            id: TrustDomainId(uuid::Uuid::new_v4()),
            name: "production".to_string(),
            governance_unit_id: UnitId(uuid::Uuid::new_v4()),
            founding_signers: vec![
                AuthorId(uuid::Uuid::new_v4()),
                AuthorId(uuid::Uuid::new_v4()),
            ],
            established_at: DualClockEvent {
                logical_clock: taba_common::LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            expires_at: Some(WallTime {
                millis: 1_735_689_600_000,
            }),
            allows_cross_domain_roles: false,
        };

        let json1 = serde_json::to_string(&domain).expect("serialize TrustDomain");
        let decoded: TrustDomain = serde_json::from_str(&json1).expect("deserialize TrustDomain");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize TrustDomain");

        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    #[test]
    fn test_ceremony_state_variants() {
        let ceremony_id = CeremonyId(uuid::Uuid::new_v4());
        let initiator = AuthorId(uuid::Uuid::new_v4());
        let dual_clock = DualClockEvent {
            logical_clock: taba_common::LogicalClock(1),
            wall_time: WallTime { millis: 1000 },
            timezone: "UTC".to_string(),
        };

        // Created
        let created = CeremonyState::Created {
            ceremony_id,
            total_shares: 5,
            threshold: 3,
            initiator,
            started_at: dual_clock.clone(),
        };
        let json = serde_json::to_string(&created).expect("serialize Created");
        let decoded: CeremonyState = serde_json::from_str(&json).expect("deserialize Created");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize Created");
        assert_eq!(json, json2, "Created variant must round-trip");

        // CollectingShares
        let collecting = CeremonyState::CollectingShares {
            ceremony_id,
            shares_received: 2,
            total_shares: 5,
            threshold: 3,
        };
        let json = serde_json::to_string(&collecting).expect("serialize CollectingShares");
        let decoded: CeremonyState =
            serde_json::from_str(&json).expect("deserialize CollectingShares");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize CollectingShares");
        assert_eq!(json, json2, "CollectingShares variant must round-trip");

        // Complete
        let complete = CeremonyState::Complete {
            ceremony_id,
            completed_at: dual_clock.clone(),
            witnesses: vec![NodeId(uuid::Uuid::new_v4())],
            root_governance_unit: UnitId(uuid::Uuid::new_v4()),
        };
        let json = serde_json::to_string(&complete).expect("serialize Complete");
        let decoded: CeremonyState = serde_json::from_str(&json).expect("deserialize Complete");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize Complete");
        assert_eq!(json, json2, "Complete variant must round-trip");

        // Failed
        let failed = CeremonyState::Failed {
            ceremony_id,
            reason: "threshold not met".to_string(),
            failed_at: dual_clock,
        };
        let json = serde_json::to_string(&failed).expect("serialize Failed");
        let decoded: CeremonyState = serde_json::from_str(&json).expect("deserialize Failed");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize Failed");
        assert_eq!(json, json2, "Failed variant must round-trip");
    }
}
