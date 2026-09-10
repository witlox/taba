//! Unit signing with Ed25519 and context binding (INV-S3).
//!
//! Signatures bind: `Sign(key, SHA-256(unit_bytes || trust_domain_id ||
//! cluster_id || validity_window))`. This prevents replay across trust
//! domains or clusters. The [`Signer`] trait defines the interface; the
//! [`DefaultSigner`] implementation uses [`ed25519_dalek`] under the hood.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taba_common::{ClusterId, TrustDomainId, ValidityWindow};
use taba_core::Unit;

use crate::crypto::{KeyPair, PublicKey, Signature};
use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// SignatureContext
// ---------------------------------------------------------------------------

/// Context bound into every signature to prevent cross-cluster and
/// cross-domain replay attacks (INV-S3).
///
/// The signature covers `SHA-256(unit_bytes || trust_domain_id ||
/// cluster_id || validity_window)`. If any field differs, the signature
/// is invalid for that context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureContext {
    /// The trust domain this signature is valid within.
    pub trust_domain_id: TrustDomainId,
    /// The cluster this signature is valid within.
    pub cluster_id: ClusterId,
    /// Time window during which this signature is valid.
    pub validity_window: ValidityWindow,
}

// ---------------------------------------------------------------------------
// SignedUnit
// ---------------------------------------------------------------------------

/// A unit wrapped with its cryptographic signature and verification context.
///
/// This is the form in which units exist in the composition graph.
/// Verification is a synchronous gate before merge (INV-S3).
///
/// Generic over `T` so it can wrap an owned [`Unit`] or a reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedUnit<T> {
    /// The unit payload.
    pub unit: T,
    /// The signature over `(unit || context)`.
    pub signature: Signature,
    /// The context bound into the signature.
    pub context: SignatureContext,
    /// The public key of the signing author.
    pub signer: PublicKey,
}

// ---------------------------------------------------------------------------
// Signer trait
// ---------------------------------------------------------------------------

/// Signs units with Ed25519 and context binding.
///
/// Signatures bind: `Sign(key, SHA-256(unit || trust_domain_id ||
/// cluster_id || validity_window))` per INV-S3. This prevents replay
/// across trust domains or clusters.
///
/// The signing key MUST correspond to the author declared in the unit
/// header. Callers are responsible for ensuring this correspondence.
pub trait Signer {
    /// Produce a detached signature over a unit with full context binding.
    ///
    /// The signature covers the SHA-256 of `(unit content || trust_domain_id
    /// || cluster_id || validity_window)`. Returns
    /// [`SecurityError::KeyError`] if the signing key is unavailable or
    /// has been revoked.
    fn sign(
        &self,
        unit: &Unit,
        trust_domain: &TrustDomainId,
        cluster: &ClusterId,
        validity: &ValidityWindow,
    ) -> Result<Signature, SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultSigner
// ---------------------------------------------------------------------------

/// Default implementation of [`Signer`] using Ed25519.
///
/// Holds a [`KeyPair`] and signs by hashing `(unit_bytes || context_bytes)`
/// with SHA-256, then Ed25519-signing the hash. The unit is serialized with
/// `serde_json::to_vec` and the context is deterministically encoded.
#[derive(Debug)]
pub struct DefaultSigner {
    key_pair: KeyPair,
}

impl DefaultSigner {
    /// Creates a new signer with the given key pair.
    #[must_use]
    pub const fn new(key_pair: KeyPair) -> Self {
        Self { key_pair }
    }

    /// Returns a reference to the signer's public key.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        self.key_pair.public_key()
    }
}

impl Signer for DefaultSigner {
    fn sign(
        &self,
        unit: &Unit,
        trust_domain: &TrustDomainId,
        cluster: &ClusterId,
        validity: &ValidityWindow,
    ) -> Result<Signature, SecurityError> {
        let payload = signing_payload(unit, trust_domain, cluster, Some(validity));
        self.key_pair.signing_key().sign_raw(&payload)
    }
}

impl Default for DefaultSigner {
    fn default() -> Self {
        Self::new(KeyPair::generate())
    }
}

// ---------------------------------------------------------------------------
// Deterministic encoding (shared with verification)
// ---------------------------------------------------------------------------

/// Deterministically encodes a signature context into bytes.
///
/// The encoding is: `trust_domain_id bytes (16) || cluster_id bytes (16) ||
/// validity_window JSON (if present)`. This must be deterministic and stable
/// across nodes so that signers and verifiers compute identical payloads.
pub(crate) fn encode_context(
    trust_domain_id: &TrustDomainId,
    cluster_id: &ClusterId,
    validity_window: Option<&ValidityWindow>,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 + 64);
    bytes.extend_from_slice(trust_domain_id.0.as_bytes());
    bytes.extend_from_slice(cluster_id.0.as_bytes());
    if let Some(vw) = validity_window {
        let vw_json = serde_json::to_vec(vw)
            .expect("ValidityWindow serialization must not fail for well-formed types");
        bytes.extend_from_slice(&vw_json);
    }
    bytes
}

/// Computes the deterministic signing payload:
/// `SHA-256(serialized_unit || encoded_context)`.
///
/// Both the signer and verifier call this function with identical
/// arguments to produce the same hash. Any mismatch in the unit
/// content or context renders the signature invalid.
pub(crate) fn signing_payload(
    unit: &Unit,
    trust_domain: &TrustDomainId,
    cluster: &ClusterId,
    validity_window: Option<&ValidityWindow>,
) -> Vec<u8> {
    let unit_bytes =
        serde_json::to_vec(unit).expect("Unit serialization must not fail for well-formed types");
    let context_bytes = encode_context(trust_domain, cluster, validity_window);
    let mut hasher = Sha256::new();
    hasher.update(&unit_bytes);
    hasher.update(&context_bytes);
    hasher.finalize().to_vec()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{
        AuthorId, ClusterId, ContentDigest, DualClockEvent, LogicalClock, TrustDomainId, UnitId,
        ValidityWindow, WallTime,
    };
    use taba_core::unit::{UnitHeader, UnitState, WorkloadKind, WorkloadUnit};
    use taba_core::{
        Artifact, ArtifactType, Capability, FailureSemantics, OomBehavior, Scaling,
        ShutdownBehavior, StateRecovery, Tolerances, TrustDeclaration,
    };

    use crate::verification::{DefaultVerifier, Verifier};

    /// Creates a minimal [`UnitHeader`] for testing.
    fn test_header() -> UnitHeader {
        UnitHeader {
            id: UnitId(uuid::Uuid::new_v4()),
            author: AuthorId(uuid::Uuid::new_v4()),
            trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
            created_at: DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
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
                on_crash: taba_core::CrashBehavior::Unexpected,
                on_shutdown: ShutdownBehavior::Immediate,
            },
            recovery_relationships: Vec::new(),
            state_recovery: StateRecovery::Stateless,
            placement_on_failure: None,
            health_check: None,
            spawn_context: None,
        }
    }

    /// Creates a test [`Unit`] (workload variant).
    fn test_unit() -> Unit {
        Unit::Workload(test_workload_unit())
    }

    /// Creates a test [`ValidityWindow`].
    fn test_validity() -> ValidityWindow {
        ValidityWindow {
            lc_range: Some((LogicalClock(1), LogicalClock(100))),
            wall_time_deadline: None,
        }
    }

    #[test]
    fn test_sign_and_verify_unit() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit();
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
                &unit.header().created_at.logical_clock,
                Some(&validity),
            )
            .expect("verification should succeed");
    }

    #[test]
    fn test_sign_with_wrong_key_fails() {
        let key_pair_a = KeyPair::generate();
        let key_pair_b = KeyPair::generate();
        let public_key_b = *key_pair_b.public_key();

        let signer = DefaultSigner::new(key_pair_a);
        let unit = test_unit();
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        // Verify with the wrong key (B instead of A)
        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, public_key_b, None);

        let result = verifier.verify(
            &unit,
            &signature,
            &trust_domain,
            &cluster,
            &unit.header().created_at.logical_clock,
            Some(&validity),
        );
        assert!(
            matches!(result, Err(SecurityError::InvalidSignature { .. })),
            "verification with wrong key should fail with InvalidSignature, got: {result:?}"
        );
    }

    #[test]
    fn test_sign_tampered_unit_fails() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit();
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        // Tamper: modify the unit's state
        let mut tampered = unit.clone();
        if let Unit::Workload(ref mut w) = tampered {
            w.header.state = UnitState::Running;
        }

        let mut verifier = DefaultVerifier::new();
        verifier.add_key(unit.header().author, public_key, None);

        let result = verifier.verify(
            &tampered,
            &signature,
            &trust_domain,
            &cluster,
            &unit.header().created_at.logical_clock,
            Some(&validity),
        );
        assert!(
            matches!(result, Err(SecurityError::InvalidSignature { .. })),
            "tampered unit should fail verification, got: {result:?}"
        );
    }

    #[test]
    fn test_sign_different_context_different_signature() {
        let key_pair = KeyPair::generate();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit();
        let trust_domain_a = TrustDomainId(uuid::Uuid::new_v4());
        let trust_domain_b = TrustDomainId(uuid::Uuid::new_v4());
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let sig_a = signer
            .sign(&unit, &trust_domain_a, &cluster, &validity)
            .expect("signing A should succeed");
        let sig_b = signer
            .sign(&unit, &trust_domain_b, &cluster, &validity)
            .expect("signing B should succeed");

        assert_ne!(
            sig_a, sig_b,
            "same unit signed with different trust domains must produce different signatures"
        );
    }

    #[test]
    fn test_signed_unit_serialize_roundtrip() {
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signer = DefaultSigner::new(key_pair);

        let unit = test_unit();
        let trust_domain = unit.header().trust_domain;
        let cluster = ClusterId(uuid::Uuid::new_v4());
        let validity = test_validity();

        let signature = signer
            .sign(&unit, &trust_domain, &cluster, &validity)
            .expect("signing should succeed");

        let signed_unit = SignedUnit {
            unit: unit.clone(),
            signature,
            context: SignatureContext {
                trust_domain_id: trust_domain,
                cluster_id: cluster,
                validity_window: validity,
            },
            signer: public_key,
        };

        let json = serde_json::to_string(&signed_unit).expect("serialize SignedUnit");
        let decoded: SignedUnit<Unit> =
            serde_json::from_str(&json).expect("deserialize SignedUnit");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize SignedUnit");

        assert_eq!(json, json2, "JSON must be identical after round-trip");
        assert_eq!(decoded.unit.header().id, unit.header().id);
        assert_eq!(decoded.signature, signature);
        assert_eq!(decoded.signer, public_key);
    }

    // -- Property tests -----------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_sign_verify_roundtrip(_seed in any::<u8>()) {
            let key_pair = KeyPair::generate();
            let public_key = *key_pair.public_key();
            let signer = DefaultSigner::new(key_pair);

            let unit = test_unit();
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
                    &unit.header().created_at.logical_clock,
                    Some(&validity),
                )
                .expect("sign-then-verify must succeed for any key pair");
        }

        #[test]
        fn proptest_signature_different_for_different_contexts(
            td1 in any::<u128>(),
            td2 in any::<u128>(),
            cl1 in any::<u128>(),
            cl2 in any::<u128>(),
        ) {
            // Only test when the contexts are actually different.
            prop_assume!(td1 != td2 || cl1 != cl2);

            let key_pair = KeyPair::generate();
            let signer = DefaultSigner::new(key_pair);

            let unit = test_unit();
            let trust_domain1 = TrustDomainId(uuid::Uuid::from_u128(td1));
            let trust_domain2 = TrustDomainId(uuid::Uuid::from_u128(td2));
            let cluster1 = ClusterId(uuid::Uuid::from_u128(cl1));
            let cluster2 = ClusterId(uuid::Uuid::from_u128(cl2));
            let validity = test_validity();

            let sig1 = signer
                .sign(&unit, &trust_domain1, &cluster1, &validity)
                .expect("signing 1 should succeed");
            let sig2 = signer
                .sign(&unit, &trust_domain2, &cluster2, &validity)
                .expect("signing 2 should succeed");

            prop_assert_ne!(
                sig1, sig2,
                "signatures for different contexts must differ"
            );
        }
    }
}
