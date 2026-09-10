//! Delegation token creation, validation, and revocation (INV-W4, INV-W4a).
//!
//! When a service is placed on a node, the author pre-signs a delegation
//! token scoping the node's spawn authority. The node uses this token to
//! sign spawned tasks — it never holds the author's private key.
//!
//! Spawned tasks inherit operational authority from the token but NOT
//! governance authority (INV-W4a): they cannot create policy units,
//! governance units, or initiate declassification.

use std::collections::HashMap;
use std::sync::Mutex;

use taba_common::{DelegationTokenId, LogicalClock, NodeId, TrustDomainId, UnitId};
use taba_core::DelegationToken;

use crate::crypto::{PublicKey, SigningKey, VerifyingKey};
use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// DelegationValidator trait
// ---------------------------------------------------------------------------

/// Validates delegation tokens for spawned task signing (INV-W4, INV-W4a).
///
/// When a service is placed on a node, the author pre-signs a delegation
/// token. The node uses this token to sign spawned tasks on behalf of
/// the author. The node never holds the author's private key.
///
/// INV-W4a: delegation grants operational authority only. Spawned tasks
/// CANNOT create policy units, governance units, or initiate
/// declassification.
pub trait DelegationValidator {
    /// Validate that a delegation token is valid for signing a spawned task.
    ///
    /// Checks:
    /// 1. Token's LC range covers the spawned task's creation LC
    /// 2. Spawn count has not exceeded `max_spawns`
    /// 3. Token has not been revoked
    /// 4. Token signature is valid (if the author's public key is known)
    ///
    /// Returns [`SecurityError::DelegationExpired`] if LC range exceeded.
    /// Returns [`SecurityError::DelegationSpawnLimitExceeded`] if count exceeded.
    /// Returns [`SecurityError::DelegationTokenForged`] if signature invalid or revoked.
    fn validate(
        &self,
        token: &DelegationToken,
        spawned_unit_lc: &LogicalClock,
    ) -> Result<(), SecurityError>;

    /// Check that a spawned task is not attempting governance operations.
    ///
    /// INV-W4a: spawned tasks cannot create policy units, governance
    /// units, or participate in multi-party declassification.
    ///
    /// `unit_type` should be one of `"policy"`, `"governance"`, or
    /// `"declassification"`. Returns
    /// [`SecurityError::DelegationGovernanceBlocked`] if the spawned task
    /// is attempting a governance operation.
    fn check_governance_block(
        &self,
        token: &DelegationToken,
        unit_type: &str,
    ) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultDelegationValidator
// ---------------------------------------------------------------------------

/// Default implementation of [`DelegationValidator`].
///
/// Holds known delegation tokens and their authors' public keys for
/// signature verification. Tokens are tracked by ID for revocation
/// checks (the stored copy reflects the latest revocation status).
#[derive(Debug, Default)]
pub struct DefaultDelegationValidator {
    /// Known tokens and their authors' public keys.
    /// Key: token ID, Value: (stored token, author public key).
    tokens: HashMap<DelegationTokenId, (DelegationToken, PublicKey)>,
}

impl DefaultDelegationValidator {
    /// Creates a new empty validator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Registers a delegation token with the author's public key.
    ///
    /// The stored copy is used for revocation checks. The public key is
    /// used for signature verification during [`validate`](DelegationValidator::validate).
    pub fn add_token(&mut self, token: DelegationToken, pk: PublicKey) {
        self.tokens.insert(token.id, (token, pk));
    }

    /// Marks a registered token as revoked by ID.
    ///
    /// If the token is not registered, this is a no-op.
    pub fn revoke(&mut self, token_id: &DelegationTokenId) {
        if let Some((token, _)) = self.tokens.get_mut(token_id) {
            token.revoked = true;
        }
    }
}

impl DelegationValidator for DefaultDelegationValidator {
    fn validate(
        &self,
        token: &DelegationToken,
        spawned_unit_lc: &LogicalClock,
    ) -> Result<(), SecurityError> {
        // Check 1: LC range covers the spawned task's creation LC.
        let (lc_start, lc_end) = &token.valid_lc_range;
        if spawned_unit_lc < lc_start || spawned_unit_lc > lc_end {
            return Err(SecurityError::DelegationExpired {
                reason: format!(
                    "spawned task LC {spawned_unit_lc:?} is outside token range \
                     ({lc_start:?}, {lc_end:?})"
                ),
            });
        }

        // Check 2: Spawn count has not exceeded max_spawns.
        if token.current_spawns >= token.max_spawns {
            return Err(SecurityError::DelegationSpawnLimitExceeded {
                reason: format!(
                    "spawn count {} has reached or exceeded maximum {}",
                    token.current_spawns, token.max_spawns
                ),
            });
        }

        // Check 3: Token has not been revoked.
        // Use the stored copy (which may have been revoked after the caller
        // received their copy).
        let stored = self.tokens.get(&token.id);
        let effective_token = stored.map_or(token, |(t, _)| t);
        if effective_token.revoked {
            return Err(SecurityError::DelegationTokenForged {
                reason: "delegation token has been revoked".to_string(),
            });
        }

        // Check 4: Signature is valid (if the author's public key is known).
        if let Some((_, pk)) = stored {
            let Some(verifying_key) = pk.to_verifying_key() else {
                return Err(SecurityError::DelegationTokenForged {
                    reason: "author public key is non-canonical".to_string(),
                });
            };
            verify_token_signature(&verifying_key, token).map_err(|e| match e {
                SecurityError::InvalidSignature { reason } => {
                    SecurityError::DelegationTokenForged {
                        reason: format!("token signature verification failed: {reason}"),
                    }
                }
                other => other,
            })?;
        }

        Ok(())
    }

    fn check_governance_block(
        &self,
        _token: &DelegationToken,
        unit_type: &str,
    ) -> Result<(), SecurityError> {
        // INV-W4a: spawned tasks cannot create policy, governance, or
        // initiate declassification.
        match unit_type {
            "policy" | "governance" | "declassification" => {
                Err(SecurityError::DelegationGovernanceBlocked {
                    reason: format!("spawned tasks cannot create {unit_type} units (INV-W4a)"),
                })
            }
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// DelegationManager trait
// ---------------------------------------------------------------------------

/// Manages delegation token lifecycle.
///
/// Tokens are created by signing `(service_id || node_id || trust_domain ||
/// lc_range || max_spawns)` with the author's signing key. The node never
/// holds the author's private key — it uses the token to sign spawned
/// tasks on behalf of the author.
pub trait DelegationManager {
    /// Create a delegation token for a service placement.
    ///
    /// The author signs the token. The token binds: `service_id`, `node_id`,
    /// `trust_domain`, LC range, `max_spawns`.
    #[allow(clippy::too_many_arguments)]
    fn create_token(
        &self,
        signing_key: &SigningKey,
        service_id: &UnitId,
        node_id: &NodeId,
        trust_domain: &TrustDomainId,
        lc_start: &LogicalClock,
        lc_end: &LogicalClock,
        max_spawns: u32,
    ) -> Result<DelegationToken, SecurityError>;

    /// Revoke a delegation token by ID.
    ///
    /// After revocation, [`DelegationValidator::validate`] will reject
    /// the token.
    fn revoke_token(&self, token_id: &DelegationTokenId) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultDelegationManager
// ---------------------------------------------------------------------------

/// Default implementation of [`DelegationManager`].
///
/// Creates tokens by signing `(service_id || node_id || trust_domain ||
/// lc_range || max_spawns)` with the author's [`SigningKey`]. Tracks
/// tokens by ID for revocation.
///
/// Uses a [`Mutex`] for interior mutability (the trait takes `&self`)
/// and is `Sync` — safe for multi-threaded use.
#[derive(Debug, Default)]
pub struct DefaultDelegationManager {
    /// Known tokens, keyed by ID, for revocation tracking.
    tokens: Mutex<HashMap<DelegationTokenId, DelegationToken>>,
}

impl DefaultDelegationManager {
    /// Creates a new empty delegation manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }
}

impl DelegationManager for DefaultDelegationManager {
    fn create_token(
        &self,
        signing_key: &SigningKey,
        service_id: &UnitId,
        node_id: &NodeId,
        trust_domain: &TrustDomainId,
        lc_start: &LogicalClock,
        lc_end: &LogicalClock,
        max_spawns: u32,
    ) -> Result<DelegationToken, SecurityError> {
        let token_id = DelegationTokenId(uuid::Uuid::new_v4());

        // Sign: service_id || node_id || trust_domain || lc_start || lc_end || max_spawns
        let payload = delegation_payload(
            service_id,
            node_id,
            trust_domain,
            *lc_start,
            *lc_end,
            max_spawns,
        );
        let signature = signing_key.sign_raw(&payload)?;

        let token = DelegationToken {
            id: token_id,
            service_id: *service_id,
            node_id: *node_id,
            trust_domain: *trust_domain,
            valid_lc_range: (*lc_start, *lc_end),
            max_spawns,
            current_spawns: 0,
            author_signature: signature.0.to_vec(),
            revoked: false,
        };

        // Store the token for revocation tracking.
        self.tokens
            .lock()
            .map_err(|_| SecurityError::KeyError {
                reason: "delegation manager mutex poisoned".to_string(),
            })?
            .insert(token_id, token.clone());

        Ok(token)
    }

    fn revoke_token(&self, token_id: &DelegationTokenId) -> Result<(), SecurityError> {
        let mut tokens = self.tokens.lock().map_err(|_| SecurityError::KeyError {
            reason: "delegation manager mutex poisoned".to_string(),
        })?;

        match tokens.get_mut(token_id) {
            Some(token) => {
                token.revoked = true;
                Ok(())
            }
            None => Err(SecurityError::DelegationTokenForged {
                reason: format!("token {token_id:?} not found for revocation"),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding and verification helpers
// ---------------------------------------------------------------------------

/// Deterministically encodes delegation token fields into a signing payload.
///
/// The encoding is: `service_id (16) || node_id (16) || trust_domain (16) ||
/// lc_start (8, big-endian) || lc_end (8, big-endian) || max_spawns (4, big-endian)`.
pub(crate) fn delegation_payload(
    service_id: &UnitId,
    node_id: &NodeId,
    trust_domain: &TrustDomainId,
    lc_start: LogicalClock,
    lc_end: LogicalClock,
    max_spawns: u32,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + 16 + 16 + 8 + 8 + 4);
    bytes.extend_from_slice(service_id.0.as_bytes());
    bytes.extend_from_slice(node_id.0.as_bytes());
    bytes.extend_from_slice(trust_domain.0.as_bytes());
    bytes.extend_from_slice(&lc_start.0.to_be_bytes());
    bytes.extend_from_slice(&lc_end.0.to_be_bytes());
    bytes.extend_from_slice(&max_spawns.to_be_bytes());
    bytes
}

/// Verifies a delegation token's signature against the author's public key.
fn verify_token_signature(
    verifying_key: &VerifyingKey,
    token: &DelegationToken,
) -> Result<(), SecurityError> {
    let payload = delegation_payload(
        &token.service_id,
        &token.node_id,
        &token.trust_domain,
        token.valid_lc_range.0,
        token.valid_lc_range.1,
        token.max_spawns,
    );

    let signature = crate::crypto::Signature::from_bytes(
        token.author_signature.as_slice().try_into().map_err(|_| {
            SecurityError::DelegationTokenForged {
                reason: "author signature has wrong length".to_string(),
            }
        })?,
    );

    verifying_key.verify_raw(&payload, &signature)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    /// Creates a key pair, delegation manager, and validator for testing.
    fn test_setup() -> (
        KeyPair,
        DefaultDelegationManager,
        DefaultDelegationValidator,
    ) {
        let key_pair = KeyPair::generate();
        let manager = DefaultDelegationManager::new();
        let validator = DefaultDelegationValidator::new();
        (key_pair, manager, validator)
    }

    /// Creates typical delegation token parameters.
    fn test_params() -> (
        UnitId,
        NodeId,
        TrustDomainId,
        LogicalClock,
        LogicalClock,
        u32,
    ) {
        (
            UnitId(uuid::Uuid::new_v4()),
            NodeId(uuid::Uuid::new_v4()),
            TrustDomainId(uuid::Uuid::new_v4()),
            LogicalClock(10),
            LogicalClock(100),
            50,
        )
    }

    #[test]
    fn test_create_and_validate_token() {
        let (key_pair, manager, mut validator) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, max_spawns) = test_params();

        let token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        // Register the token with the validator (with the author's public key).
        validator.add_token(token.clone(), *key_pair.public_key());

        // Validate within the LC range.
        validator
            .validate(&token, &LogicalClock(50))
            .expect("token should be valid within its LC range");
    }

    #[test]
    fn test_validate_token_expired() {
        let (key_pair, manager, mut validator) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, max_spawns) = test_params();

        let token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        validator.add_token(token.clone(), *key_pair.public_key());

        // Validate with LC outside the range (200 > 100).
        let result = validator.validate(&token, &LogicalClock(200));
        assert!(
            matches!(result, Err(SecurityError::DelegationExpired { .. })),
            "token with LC outside range should fail with DelegationExpired, got: {result:?}"
        );

        // Also test before the range (5 < 10).
        let result = validator.validate(&token, &LogicalClock(5));
        assert!(
            matches!(result, Err(SecurityError::DelegationExpired { .. })),
            "token with LC before range should fail with DelegationExpired, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_token_revoked() {
        let (key_pair, manager, mut validator) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, max_spawns) = test_params();

        let token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        validator.add_token(token.clone(), *key_pair.public_key());

        // Revoke the token via the validator.
        validator.revoke(&token.id);

        // Validate should now fail.
        let result = validator.validate(&token, &LogicalClock(50));
        assert!(
            matches!(result, Err(SecurityError::DelegationTokenForged { .. })),
            "revoked token should fail with DelegationTokenForged, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_token_governance_block() {
        let (key_pair, manager, validator) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, max_spawns) = test_params();

        let token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        // Spawned task trying to create a policy unit → blocked.
        let result = validator.check_governance_block(&token, "policy");
        assert!(
            matches!(
                result,
                Err(SecurityError::DelegationGovernanceBlocked { .. })
            ),
            "spawned task creating policy should be blocked (INV-W4a), got: {result:?}"
        );

        // Governance unit → also blocked.
        let result = validator.check_governance_block(&token, "governance");
        assert!(
            matches!(
                result,
                Err(SecurityError::DelegationGovernanceBlocked { .. })
            ),
            "spawned task creating governance should be blocked (INV-W4a), got: {result:?}"
        );

        // Declassification → also blocked.
        let result = validator.check_governance_block(&token, "declassification");
        assert!(
            matches!(
                result,
                Err(SecurityError::DelegationGovernanceBlocked { .. })
            ),
            "spawned task initiating declassification should be blocked (INV-W4a), got: {result:?}"
        );

        // Non-governance type → allowed.
        let result = validator.check_governance_block(&token, "workload");
        assert!(
            result.is_ok(),
            "non-governance type should be allowed, got: {result:?}"
        );
    }

    #[test]
    fn test_revoke_token() {
        let (key_pair, manager, _validator) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, max_spawns) = test_params();

        let token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        // Revoke the token.
        manager
            .revoke_token(&token.id)
            .expect("revocation should succeed");

        // Revoking a non-existent token should fail.
        let unknown_id = DelegationTokenId(uuid::Uuid::new_v4());
        let result = manager.revoke_token(&unknown_id);
        assert!(
            matches!(result, Err(SecurityError::DelegationTokenForged { .. })),
            "revoking unknown token should fail, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_token_wrong_signature() {
        let (key_pair_a, _manager, mut validator) = test_setup();
        let (key_pair_b, manager, _validator2) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, max_spawns) = test_params();

        // Token signed by key A.
        let token = manager
            .create_token(
                key_pair_a.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                max_spawns,
            )
            .expect("token creation should succeed");

        // Validator has key B (wrong key).
        validator.add_token(token.clone(), *key_pair_b.public_key());

        let result = validator.validate(&token, &LogicalClock(50));
        assert!(
            matches!(result, Err(SecurityError::DelegationTokenForged { .. })),
            "token with wrong author key should fail with DelegationTokenForged, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_token_spawn_limit() {
        let (key_pair, manager, _validator) = test_setup();
        let (service_id, node_id, trust_domain, lc_start, lc_end, _) = test_params();

        let mut token = manager
            .create_token(
                key_pair.signing_key(),
                &service_id,
                &node_id,
                &trust_domain,
                &lc_start,
                &lc_end,
                1, // max_spawns = 1
            )
            .expect("token creation should succeed");

        // Set current_spawns to the limit.
        token.current_spawns = 1;

        // Validate with a fresh validator (no stored copy, so the passed
        // token's current_spawns is used).
        let validator = DefaultDelegationValidator::new();
        let result = validator.validate(&token, &LogicalClock(50));
        assert!(
            matches!(
                result,
                Err(SecurityError::DelegationSpawnLimitExceeded { .. })
            ),
            "token at spawn limit should fail with DelegationSpawnLimitExceeded, got: {result:?}"
        );
    }
}
