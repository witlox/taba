//! Signature verification — synchronous gate before merge (INV-S3).
//!
//! Verification checks three conditions per INV-S3:
//! 1. Signature is cryptographically valid against the author's public key
//! 2. Context (trust domain, cluster, validity window) matches the binding
//!    (implicitly verified — mismatched context produces a different hash)
//! 3. Author's key is not revoked in the local set (causal model)
//!
//! This is intentionally NOT async. Signature verification must complete
//! before a unit can enter the graph. No unit enters graph state before
//! verification completes.

use std::collections::HashMap;

use taba_common::{AuthorId, ClusterId, LogicalClock, TrustDomainId, ValidityWindow};
use taba_core::Unit;

use crate::crypto::{KeyId, PublicKey};
use crate::error::SecurityError;
use crate::signing::signing_payload;

// ---------------------------------------------------------------------------
// Verifier trait
// ---------------------------------------------------------------------------

/// Verifies unit signatures. Synchronous — blocks merge (INV-S3).
///
/// This is intentionally NOT async. Signature verification must complete
/// before a unit can enter the graph. No unit enters graph state before
/// verification completes.
///
/// Author scope check (INV-S5) is a SEPARATE gate, not part of
/// [`verify`](Self::verify). This separation allows scope and signature
/// to be tested independently. Both gates must pass before merge.
pub trait Verifier {
    /// Verify that a unit's signature is valid.
    ///
    /// Checks (per INV-S3):
    /// 1. Signature is cryptographically valid against the author's public key
    /// 2. Signature context (trust domain, cluster, validity window) matches
    ///    the binding in the signature
    /// 3. Author's key is not revoked in the local graph (causal model)
    ///
    /// Returns [`Ok`] if all checks pass. Returns the specific
    /// [`SecurityError`] variant describing the first failure.
    ///
    /// This method is synchronous and MUST NOT perform network I/O. All
    /// required key material and revocation state must be locally available.
    ///
    /// If the author's key is not locally available, returns
    /// [`SecurityError::KeyNotFound`] — the caller should buffer the unit
    /// and retry when keys arrive via gossip.
    fn verify(
        &self,
        unit: &Unit,
        signature: &crate::crypto::Signature,
        trust_domain: &TrustDomainId,
        cluster: &ClusterId,
        logical_clock: &LogicalClock,
        validity_window: Option<&ValidityWindow>,
    ) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultVerifier
// ---------------------------------------------------------------------------

/// Default implementation of [`Verifier`].
///
/// Holds a set of known public keys (keyed by author ID) and revocation
/// status (keyed by key ID). Verification is synchronous and uses only
/// locally available state — no network I/O.
///
/// # Causal model
///
/// Revocation is a graph operation. Once a revocation governance unit is
/// merged into a node's local graph, that node rejects any subsequent
/// units from the revoked author. The `revoked_at` field is a logical
/// clock value: units whose creation logical clock is `>= revoked_at`
/// are rejected.
///
/// # Fail-closed
///
/// If the author's key is unknown, [`SecurityError::KeyNotFound`] is
/// returned. The caller should buffer the unit and retry when keys
/// arrive via gossip.
#[derive(Debug, Clone)]
pub struct DefaultVerifier {
    /// Mapping from author ID to (public key, key ID).
    author_keys: HashMap<AuthorId, (PublicKey, KeyId)>,
    /// Mapping from key ID to revocation timestamp (logical clock value).
    /// Presence in this map means the key is revoked.
    revocations: HashMap<KeyId, u64>,
}

impl Default for DefaultVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultVerifier {
    /// Creates a new empty verifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            author_keys: HashMap::new(),
            revocations: HashMap::new(),
        }
    }

    /// Adds a known public key for an author, with optional revocation
    /// status.
    ///
    /// If `revoked_at` is `Some(lc)`, the key is immediately considered
    /// revoked at logical clock `lc`. If `None`, the key is active.
    ///
    /// **Deviation from interface spec**: the interface defines
    /// `add_key(pk, revoked_at)` without an author ID. An `author_id`
    /// parameter is added here because the verifier must map unit
    /// authors (from `Unit::header().author`) to their public keys.
    pub fn add_key(&mut self, author_id: AuthorId, pk: PublicKey, revoked_at: Option<u64>) {
        let key_id = KeyId::from_public_key(&pk);
        self.author_keys.insert(author_id, (pk, key_id));
        if let Some(lc) = revoked_at {
            self.revocations.insert(key_id, lc);
        }
    }

    /// Marks a key as revoked. The key is revoked at logical clock 0
    /// (earliest possible time), meaning all units from this author are
    /// rejected.
    ///
    /// If the key was already revoked, this is a no-op.
    pub fn revoke(&mut self, key_id: &KeyId) {
        self.revocations.entry(*key_id).or_insert(0);
    }

    /// Returns `true` if the author's key has been revoked in the local
    /// set (causal model, INV-S3).
    ///
    /// This checks whether a revocation entry exists for the author's
    /// key — it does NOT compare clocks. The causal model means
    /// revocation takes effect when the revocation governance unit is
    /// merged into the local graph.
    #[must_use]
    pub fn is_revoked(&self, author_id: &AuthorId) -> bool {
        self.author_keys
            .get(author_id)
            .and_then(|(_, key_id)| self.revocations.get(key_id))
            .is_some()
    }
}

impl Verifier for DefaultVerifier {
    fn verify(
        &self,
        unit: &Unit,
        signature: &crate::crypto::Signature,
        trust_domain: &TrustDomainId,
        cluster: &ClusterId,
        logical_clock: &LogicalClock,
        validity_window: Option<&ValidityWindow>,
    ) -> Result<(), SecurityError> {
        let author_id = unit.header().author;

        // Step 0: Look up the author's public key.
        let (pk, key_id) =
            self.author_keys
                .get(&author_id)
                .copied()
                .ok_or(SecurityError::KeyNotFound {
                    key: KeyId([0u8; 32]),
                })?;

        // Convert to VerifyingKey for crypto operations.
        let verifying_key = pk
            .to_verifying_key()
            .ok_or_else(|| SecurityError::KeyError {
                reason: format!("author {author_id:?} has a non-canonical public key"),
            })?;

        // Step 1: Check revocation (causal model).
        if let Some(revoked_at) = self.revocations.get(&key_id) {
            if logical_clock.0 >= *revoked_at {
                return Err(SecurityError::KeyRevoked {
                    key: key_id,
                    revoked_at: *revoked_at,
                });
            }
        }

        // Step 2: Reconstruct the signing payload and verify.
        let payload = signing_payload(unit, trust_domain, cluster, validity_window);
        verifying_key.verify_raw(&payload, signature)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{ClusterId, ContentDigest, DualClockEvent, TrustDomainId, UnitId, WallTime};
    use taba_core::unit::{UnitHeader, UnitState, WorkloadKind, WorkloadUnit};
    use taba_core::{
        Artifact, ArtifactType, Capability, CrashBehavior, FailureSemantics, OomBehavior, Scaling,
        ShutdownBehavior, StateRecovery, Tolerances, TrustDeclaration, Unit,
    };

    use crate::crypto::KeyPair;
    use crate::signing::{DefaultSigner, Signer};

    /// Creates a minimal [`UnitHeader`] for testing.
    fn test_header() -> UnitHeader {
        UnitHeader {
            id: UnitId(uuid::Uuid::new_v4()),
            author: AuthorId(uuid::Uuid::new_v4()),
            trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
            created_at: DualClockEvent {
                logical_clock: LogicalClock(10),
                wall_time: WallTime { millis: 10_000 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: UnitState::Declared,
            version: None,
        }
    }

    /// Creates a minimal [`WorkloadUnit`] for testing.
    fn test_workload_unit() -> WorkloadUnit {
        WorkloadUnit {
            header: test_header(),
            kind: WorkloadKind::Service,
            artifact: Artifact {
                artifact_type: ArtifactType::Oci,
                artifact_ref: "registry.example.com/app:v1".to_string(),
                digest: ContentDigest("sha256:abc123".to_string()),
                requires: Vec::new(),
            },
            needs: Vec::new(),
            provides: vec![Capability::new("compute", "http")],
            tolerates: Tolerances {
                max_latency: Some(std::time::Duration::from_millis(100)),
                failure_modes: vec!["timeout".to_string()],
                consistency: None,
            },
            trusts: Vec::<TrustDeclaration>::new(),
            scaling: Scaling {
                min_instances: 1,
                max_instances: 3,
                triggers: Vec::new(),
            },
            failure_semantics: FailureSemantics {
                on_oom: OomBehavior::Restart,
                on_crash: CrashBehavior::Unexpected,
                on_shutdown: ShutdownBehavior::Immediate,
            },
            recovery_relationships: Vec::new(),
            state_recovery: StateRecovery::Stateless,
            placement_on_failure: None,
            health_check: None,
            spawn_context: None,
        }
    }

    /// Creates a test [`Unit`] with the given logical clock.
    fn test_unit_with_lc(lc: u64) -> Unit {
        let mut w = test_workload_unit();
        w.header.created_at.logical_clock = LogicalClock(lc);
        Unit::Workload(w)
    }

    fn test_validity() -> ValidityWindow {
        ValidityWindow {
            lc_range: Some((LogicalClock(1), LogicalClock(100))),
            wall_time_deadline: None,
        }
    }

    #[test]
    fn test_verify_valid_signature() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit_with_lc(10);
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, public_key, None);

        verifier
            .verify(
                &unit,
                &signature,
                &trust_domain,
                &cluster,
                &LogicalClock(10),
                Some(&validity),
            )
            .expect("valid signature should verify");
    }

    #[test]
    fn test_verify_invalid_signature() {
        let key_pair_a = KeyPair::generate();
        let key_pair_b = KeyPair::generate();
        let public_key_b = *key_pair_b.public_key();

        let signer = DefaultSigner::new(key_pair_a);
        let unit = test_unit_with_lc(10);
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        // Verifier has the wrong public key (B instead of A).
        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, public_key_b, None);

        let result = verifier.verify(
            &unit,
            &signature,
            &trust_domain,
            &cluster,
            &LogicalClock(10),
            Some(&validity),
        );
        assert!(
            matches!(result, Err(SecurityError::InvalidSignature { .. })),
            "verification with wrong key should fail with InvalidSignature, got: {result:?}"
        );
    }

    #[test]
    fn test_verify_revoked_key() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signer = DefaultSigner::new(key_pair);

        // Unit created at LC 10.
        let unit = test_unit_with_lc(10);
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        // Key was revoked at LC 5 (before unit creation at LC 10).
        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, public_key, Some(5));

        let result = verifier.verify(
            &unit,
            &signature,
            &trust_domain,
            &cluster,
            &LogicalClock(10),
            Some(&validity),
        );
        assert!(
            matches!(result, Err(SecurityError::KeyRevoked { revoked_at: 5, .. })),
            "revoked key should fail with KeyRevoked, got: {result:?}"
        );
    }

    #[test]
    fn test_verify_context_mismatch() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit_with_lc(10);
        let trust_domain_a = unit.header().trust_domain;
        let trust_domain_b = TrustDomainId(uuid::Uuid::new_v4());
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        // Sign for domain A.
        let signature = signer
            .sign(&unit, &trust_domain_a, &cluster, &validity)
            .expect("signing should succeed");

        // Verify for domain B (mismatch).
        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, public_key, None);

        let result = verifier.verify(
            &unit,
            &signature,
            &trust_domain_b,
            &cluster,
            &LogicalClock(10),
            Some(&validity),
        );
        assert!(
            matches!(result, Err(SecurityError::InvalidSignature { .. })),
            "context mismatch should fail with InvalidSignature, got: {result:?}"
        );
    }

    #[test]
    fn test_verify_key_not_found() {
        let key_pair = KeyPair::generate();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit_with_lc(10);
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        // Verifier has no keys at all.
        let verifier = DefaultVerifier::new();

        let result = verifier.verify(
            &unit,
            &signature,
            &trust_domain,
            &cluster,
            &LogicalClock(10),
            Some(&validity),
        );
        assert!(
            matches!(result, Err(SecurityError::KeyNotFound { .. })),
            "unknown signer should fail with KeyNotFound, got: {result:?}"
        );
    }

    #[test]
    fn test_is_revoked_after_revoke() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let key_id = KeyId::from_public_key(&public_key);
        let author_id = AuthorId(uuid::Uuid::new_v4());

        let mut verifier = DefaultVerifier::new();
        verifier.add_key(author_id, public_key, None);

        assert!(
            !verifier.is_revoked(&author_id),
            "key should not be revoked before revoke()"
        );

        verifier.revoke(&key_id);

        assert!(
            verifier.is_revoked(&author_id),
            "key should be revoked after revoke()"
        );
    }
}
