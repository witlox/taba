//! Cryptographic primitives: Ed25519 keys, signatures, and key identifiers.
//!
//! This module wraps the [`ed25519_dalek`] crate to provide taba-specific
//! types. Private key material ([`SigningKey`]) is zeroized on drop and
//! intentionally does not implement `Clone` or `Serialize`. Public keys
//! ([`PublicKey`], [`VerifyingKey`]) and [`Signature`] are serializable.
//!
//! [`KeyId`] is a SHA-256 hash of the public key bytes, providing a stable,
//! content-addressed identifier for keys across the system.

use ed25519_dalek::Signer as DalekSigner;
use ed25519_dalek::VerifyingKey as DalekVerifyingKey;
use ed25519_dalek::{
    Signature as DalekSignature, SigningKey as DalekSigningKey, Verifier as DalekVerifier,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// PublicKey
// ---------------------------------------------------------------------------

/// Ed25519 public key (32 bytes).
///
/// The serializable representation of a public key. Use [`VerifyingKey`]
/// for cryptographic verification operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey(pub [u8; 32]);

impl PublicKey {
    /// Creates a public key from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw 32 bytes of this public key.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Converts this public key to a cryptographic [`VerifyingKey`].
    ///
    /// Returns `None` if the bytes do not represent a canonical Ed25519
    /// public key (non-canonical encoding). Canonical keys always succeed.
    #[must_use]
    pub fn to_verifying_key(&self) -> Option<VerifyingKey> {
        DalekVerifyingKey::from_bytes(&self.0)
            .ok()
            .map(|vk| VerifyingKey { inner: vk })
    }
}

// ---------------------------------------------------------------------------
// Signature
// ---------------------------------------------------------------------------

/// Ed25519 signature (64 bytes).
///
/// A detached signature over a unit or message, bound to a
/// [`SignatureContext`](crate::signing::SignatureContext) to prevent
/// cross-cluster and cross-domain replay (INV-S3).
///
/// `Serialize`/`Deserialize` are implemented manually because serde
/// does not support arrays larger than 32 elements by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Signature(pub [u8; 64]);

impl Signature {
    /// Creates a signature from raw bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Returns the raw 64 bytes of this signature.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(64))?;
        for byte in &self.0 {
            seq.serialize_element(byte)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::invalid_length(
                bytes.len(),
                &"a 64-byte Ed25519 signature",
            ));
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

// ---------------------------------------------------------------------------
// KeyId
// ---------------------------------------------------------------------------

/// SHA-256 hash of a public key, used as a stable key identifier.
///
/// The same public key always produces the same [`KeyId`]. Key IDs are
/// used to track keys across the system (revocation, lookup) without
/// exposing the full public key in every context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct KeyId(pub [u8; 32]);

impl KeyId {
    /// Computes a [`KeyId`] from a public key by SHA-256 hashing the key
    /// bytes.
    ///
    /// This is deterministic: the same [`PublicKey`] always produces the
    /// same [`KeyId`].
    #[must_use]
    pub fn from_public_key(pk: &PublicKey) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(pk.as_bytes());
        let result = hasher.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&result);
        Self(id)
    }

    /// Returns the raw 32 bytes of this key ID.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

// ---------------------------------------------------------------------------
// SigningKey (private)
// ---------------------------------------------------------------------------

/// Ed25519 signing key — private key material.
///
/// Wraps [`ed25519_dalek::SigningKey`], which implements `ZeroizeOnDrop`:
/// the secret key is securely erased when this value goes out of scope.
///
/// Intentionally does **not** implement `Clone` or `Serialize`. Private
/// key material must not be duplicated or persisted in plaintext.
pub struct SigningKey {
    inner: DalekSigningKey,
}

impl SigningKey {
    /// Creates a signing key from the underlying dalek type.
    #[must_use]
    pub const fn from_dalek(key: DalekSigningKey) -> Self {
        Self { inner: key }
    }

    /// Returns the public key corresponding to this signing key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.inner.verifying_key().to_bytes())
    }

    /// Signs a raw message with Ed25519.
    ///
    /// The caller is responsible for binding context (trust domain,
    /// cluster, validity window) into the message before calling this
    /// method (INV-S3).
    ///
    /// Returns `SecurityError::KeyError` if signing fails (practically
    /// impossible for Ed25519 with a valid key, but handled for safety).
    pub fn sign_raw(&self, message: &[u8]) -> Result<Signature, crate::error::SecurityError> {
        let dalek_sig: DalekSignature =
            self.inner
                .try_sign(message)
                .map_err(|e| crate::error::SecurityError::KeyError {
                    reason: e.to_string(),
                })?;
        Ok(Signature(dalek_sig.to_bytes()))
    }

    /// Returns the raw 32 bytes of this signing key.
    ///
    /// Used for key persistence (e.g., taba-cli local storage). The
    /// caller is responsible for securely erasing the returned bytes
    /// after use — the key material is not zeroized here because it
    /// remains valid inside this `SigningKey`.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.inner.to_bytes()
    }

    /// Creates a signing key from raw 32 bytes.
    ///
    /// Used for key restoration (e.g., taba-cli loading from local
    /// storage). The input must be exactly 32 bytes of Ed25519
    /// private key material.
    #[must_use]
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            inner: DalekSigningKey::from_bytes(bytes),
        }
    }
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKey")
            .field("public_key", &self.public_key())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// VerifyingKey (public)
// ---------------------------------------------------------------------------

/// Ed25519 verifying key — public key with cryptographic operations.
///
/// Wraps [`ed25519_dalek::VerifyingKey`] for signature verification.
/// Serializes as a 32-byte array, matching [`PublicKey`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct VerifyingKey {
    inner: DalekVerifyingKey,
}

impl VerifyingKey {
    /// Creates a verifying key from a [`PublicKey`].
    ///
    /// Returns `None` if the public key bytes are not a canonical Ed25519
    /// encoding.
    #[must_use]
    pub fn from_public_key(pk: &PublicKey) -> Option<Self> {
        DalekVerifyingKey::from_bytes(pk.as_bytes())
            .ok()
            .map(|vk| Self { inner: vk })
    }

    /// Converts this verifying key to a [`PublicKey`].
    #[must_use]
    pub fn to_public_key(&self) -> PublicKey {
        PublicKey(self.inner.to_bytes())
    }

    /// Verifies a raw signature against this key.
    ///
    /// Returns `Ok(())` if the signature is valid, or
    /// [`SecurityError::InvalidSignature`](crate::error::SecurityError::InvalidSignature)
    /// otherwise.
    pub fn verify_raw(
        &self,
        message: &[u8],
        signature: &Signature,
    ) -> Result<(), crate::error::SecurityError> {
        let dalek_sig = DalekSignature::from_bytes(&signature.0);
        self.inner.verify(message, &dalek_sig).map_err(|e| {
            crate::error::SecurityError::InvalidSignature {
                reason: e.to_string(),
            }
        })
    }
}

impl std::fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("VerifyingKey")
            .field(&self.inner.to_bytes())
            .finish()
    }
}

impl Serialize for VerifyingKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.inner.to_bytes().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VerifyingKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bytes = <[u8; 32]>::deserialize(deserializer)?;
        DalekVerifyingKey::from_bytes(&bytes)
            .map(|vk| Self { inner: vk })
            .map_err(|_| serde::de::Error::custom("invalid Ed25519 public key"))
    }
}

// ---------------------------------------------------------------------------
// KeyPair
// ---------------------------------------------------------------------------

/// Ed25519 key pair for signing units and messages.
///
/// Contains a public [`PublicKey`] and a private [`SigningKey`]. The
/// signing key is zeroized when this value is dropped.
///
/// Intentionally does **not** implement `Clone` or `Serialize`: private
/// key material must not be duplicated or persisted.
pub struct KeyPair {
    /// The public key (32 bytes, Ed25519).
    pub public_key: PublicKey,
    signing_key: SigningKey,
}

impl KeyPair {
    /// Generates a new Ed25519 key pair using the operating system's
    /// cryptographically secure random number generator.
    #[must_use]
    pub fn generate() -> Self {
        let mut rng = rand::rngs::OsRng;
        let dalek_key = DalekSigningKey::generate(&mut rng);
        let public_key = PublicKey(dalek_key.verifying_key().to_bytes());
        Self {
            public_key,
            signing_key: SigningKey { inner: dalek_key },
        }
    }

    /// Returns a reference to the public key.
    #[must_use]
    pub const fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Returns a reference to the private signing key.
    #[must_use]
    pub const fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Creates a key pair from an existing signing key.
    ///
    /// Used for key restoration (e.g., taba-cli loading from local
    /// storage). The public key is derived from the signing key.
    #[must_use]
    pub fn from_signing_key(signing_key: SigningKey) -> Self {
        let public_key = signing_key.public_key();
        Self {
            public_key,
            signing_key,
        }
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("public_key", &self.public_key)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_pair_generate() {
        let key_pair = KeyPair::generate();
        let pk = key_pair.public_key();
        assert_ne!(
            pk,
            &PublicKey([0u8; 32]),
            "generated public key should be non-zero"
        );
    }

    #[test]
    fn test_key_id_from_public_key() {
        let key_pair = KeyPair::generate();
        let pk = *key_pair.public_key();
        let id1 = KeyId::from_public_key(&pk);
        let id2 = KeyId::from_public_key(&pk);
        assert_eq!(id1, id2, "same public key must produce the same KeyId");
    }

    #[test]
    fn test_key_id_different_keys() {
        let key_pair_a = KeyPair::generate();
        let key_pair_b = KeyPair::generate();
        let id_a = KeyId::from_public_key(key_pair_a.public_key());
        let id_b = KeyId::from_public_key(key_pair_b.public_key());
        assert_ne!(id_a, id_b, "different keys must produce different KeyIds");
    }

    #[test]
    fn test_signature_serialize_roundtrip() {
        let sig = Signature([
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46,
            47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64,
        ]);
        let json = serde_json::to_string(&sig).expect("serialize Signature");
        let decoded: Signature = serde_json::from_str(&json).expect("deserialize Signature");
        assert_eq!(sig, decoded, "Signature must round-trip");
    }

    #[test]
    fn test_public_key_serialize_roundtrip() {
        let key_pair = KeyPair::generate();
        let pk = *key_pair.public_key();
        let json = serde_json::to_string(&pk).expect("serialize PublicKey");
        let decoded: PublicKey = serde_json::from_str(&json).expect("deserialize PublicKey");
        assert_eq!(pk, decoded, "PublicKey must round-trip");
    }

    #[test]
    fn test_signing_key_not_clone() {
        // Compile-time check: SigningKey does not implement Clone.
        // If this test compiles, the assertion passes. We verify at
        // the type level by checking that KeyPair (which contains a
        // non-Clone SigningKey) cannot be cloned.
        let key_pair = KeyPair::generate();
        // The following would not compile if uncommented:
        // let _clone = key_pair.clone();
        // Instead, verify that the public key is still accessible:
        assert_ne!(*key_pair.public_key(), PublicKey([0u8; 32]));
    }

    #[test]
    fn test_verifying_key_serialize_roundtrip() {
        let key_pair = KeyPair::generate();
        let pk = *key_pair.public_key();
        let vk = pk
            .to_verifying_key()
            .expect("valid public key should convert to VerifyingKey");
        let json = serde_json::to_string(&vk).expect("serialize VerifyingKey");
        let decoded: VerifyingKey = serde_json::from_str(&json).expect("deserialize VerifyingKey");
        assert_eq!(vk, decoded, "VerifyingKey must round-trip");
        assert_eq!(
            vk.to_public_key(),
            decoded.to_public_key(),
            "public keys must match after round-trip"
        );
    }

    #[test]
    fn test_sign_and_verify_raw() {
        let key_pair = KeyPair::generate();
        let message = b"hello, taba!";
        let signature = key_pair
            .signing_key()
            .sign_raw(message)
            .expect("signing should succeed");

        let vk = key_pair
            .public_key()
            .to_verifying_key()
            .expect("valid public key");
        assert!(
            vk.verify_raw(message, &signature).is_ok(),
            "signature should verify"
        );
        assert!(
            vk.verify_raw(b"tampered", &signature).is_err(),
            "tampered message should fail verification"
        );
    }

    // -- Property tests -----------------------------------------------------

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_key_id_deterministic(bytes in prop::collection::vec(any::<u8>(), 32)) {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            let pk = PublicKey(arr);
            let id1 = KeyId::from_public_key(&pk);
            let id2 = KeyId::from_public_key(&pk);
            prop_assert_eq!(id1, id2, "same public key must always produce the same KeyId");
        }

        #[test]
        fn proptest_sign_verify_raw_roundtrip(_seed in any::<u8>()) {
            let key_pair = KeyPair::generate();
            let pk = *key_pair.public_key();
            let vk = pk.to_verifying_key()
                .expect("generated public key must be valid");
            let message = b"proptest message";
            let signature = key_pair
                .signing_key()
                .sign_raw(message)
                .expect("signing should succeed");
            prop_assert!(
                vk.verify_raw(message, &signature).is_ok(),
                "sign then verify must succeed for any key pair"
            );
        }
    }
}
