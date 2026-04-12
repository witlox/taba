//! Security types: cryptographic identity, signing, trust governance, and Shamir ceremonies.
//!
//! This module defines the cryptographic primitives and trust management types
//! that underpin taba's zero-access-default, capability-based security model.
//! Every unit in the graph is signed (INV-S3), every author has scoped authority
//! (INV-S5), and the root of all trust originates from a Shamir key ceremony.

use serde::{Deserialize, Serialize};

use crate::common::{
    AuthorId, CeremonyId, ClusterId, NodeId, Timestamp, TrustDomainId, UnitId, ValidityWindow,
    Version,
};

// ---------------------------------------------------------------------------
// Cryptographic primitives
// ---------------------------------------------------------------------------

/// An Ed25519 key pair used for signing units and gossip messages.
/// Private key material must be zeroed after use (zeroize crate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    /// The public key (32 bytes, Ed25519).
    pub public_key: PublicKey,
    // NOTE: private key is NOT serialized. Held only in memory, zeroed on drop.
    // Represented here for architectural completeness.
}

/// An Ed25519 public key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

/// An Ed25519 signature over a unit or message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Signature(pub [u8; 64]);

/// Context bound into every signature to prevent cross-cluster and
/// cross-domain replay attacks (INV-S3).
///
/// Signature = Sign(key, hash(unit || trust_domain_id || cluster_id || validity_window))
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureContext {
    /// The trust domain this signature is valid within.
    pub trust_domain_id: TrustDomainId,
    /// The cluster this signature is valid within.
    pub cluster_id: ClusterId,
    /// Time window during which this signature is valid.
    pub validity_window: ValidityWindow,
}

/// A unit wrapped with its cryptographic signature and verification context.
/// This is the form in which units exist in the composition graph.
/// Verification is a synchronous gate before merge (INV-S3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedUnit<T> {
    /// The unit payload.
    pub unit: T,
    /// The signature over (unit || context).
    pub signature: Signature,
    /// The context bound into the signature.
    pub context: SignatureContext,
    /// The public key of the signing author.
    pub signer: PublicKey,
}

// ---------------------------------------------------------------------------
// Author and role management
// ---------------------------------------------------------------------------

/// An authenticated identity with scoped authority to create units.
/// Zero access by default -- all scopes are explicit (INV-S5).
/// No two distinct authors may have identical scope tuples (INV-S8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    /// Unique identifier for this author.
    pub id: AuthorId,
    /// The author's Ed25519 public key.
    pub public_key: PublicKey,
    /// Human-readable display name.
    pub display_name: String,
    /// When this author identity was created.
    pub created_at: Timestamp,
    /// Whether this author's key has been revoked.
    pub revoked: bool,
    /// If revoked, the revocation details.
    pub revocation: Option<KeyRevocation>,
}

// ---------------------------------------------------------------------------
// Trust domain governance
// ---------------------------------------------------------------------------

/// A trust domain is an authorization boundary scoping author permissions.
/// Itself a governance unit, created through multi-party agreement (INV-S6, INV-S10).
/// Minimum 2 distinct author signatures required for creation.
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
    /// When this domain was established.
    pub established_at: Timestamp,
    /// When this domain expires (if set).
    pub expires_at: Option<Timestamp>,
    /// Whether cross-domain role inheritance is enabled (default: no, INV-S6).
    pub allows_cross_domain_roles: bool,
}

// ---------------------------------------------------------------------------
// Shamir key ceremony
// ---------------------------------------------------------------------------

/// A share of the Shamir-split root key.
/// The root key is the root of all authority in a taba cluster.
///
/// Tier 1 (Phase 1): basic -- start -> add shares -> complete with witness.
/// Tier 2 (Phase 3): password-protected -- each share encrypted with Argon2id.
/// Tier 3 (Phase 5): offline two-factor -- seed code + password.
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
/// Ceremony events are recorded as governance units in the graph.
///
/// Tier 1 flow: Created -> CollectingShares -> Complete.
/// Failure at any point -> Failed (ceremony must restart).
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
        /// When the ceremony was started.
        started_at: Timestamp,
    },
    /// Shares are being collected from participants.
    CollectingShares {
        ceremony_id: CeremonyId,
        /// Shares received so far (count, not content -- shares are never aggregated in memory).
        shares_received: u8,
        /// Total expected.
        total_shares: u8,
        /// Threshold for completion.
        threshold: u8,
    },
    /// Ceremony completed successfully. Root key reconstructed and used.
    Complete {
        ceremony_id: CeremonyId,
        /// When the ceremony completed.
        completed_at: Timestamp,
        /// Witnesses who observed the ceremony.
        witnesses: Vec<NodeId>,
        /// The governance unit created by the ceremony (seeds the graph).
        root_governance_unit: UnitId,
    },
    /// Ceremony failed and must be restarted.
    Failed {
        ceremony_id: CeremonyId,
        /// Why the ceremony failed.
        reason: String,
        /// When the failure occurred.
        failed_at: Timestamp,
    },
}

// ---------------------------------------------------------------------------
// Key revocation
// ---------------------------------------------------------------------------

/// Revocation record for a compromised or retired key.
/// Propagated via priority gossip. Units signed after the revocation
/// timestamp are rejected (INV-S3). Units signed before remain valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRevocation {
    /// The author whose key is revoked.
    pub author_id: AuthorId,
    /// The revoked public key.
    pub revoked_key: PublicKey,
    /// When the key was revoked. Units signed after this are invalid.
    pub revoked_at: Timestamp,
    /// Reason for revocation.
    pub reason: RevocationReason,
    /// Who authorized the revocation.
    pub authorized_by: AuthorId,
    /// Version for ordering multiple revocations of the same author.
    pub version: Version,
}

/// Reason a key was revoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RevocationReason {
    /// Key material was compromised.
    Compromised,
    /// Author has left the organization.
    Departed,
    /// Key rotation -- replaced by a new key.
    Rotated {
        /// The new public key replacing this one.
        replacement: PublicKey,
    },
    /// Administrative revocation.
    Administrative { details: String },
}
