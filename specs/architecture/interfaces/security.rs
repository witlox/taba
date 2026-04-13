// taba-security: Signing, verification, capability enforcement, taint, key
// management, and Shamir ceremony.
//
// This crate owns all cryptographic and authorization logic. Signature
// verification is a synchronous gate (INV-S3). Taint is computed at query
// time (INV-S4). Ceremony follows the Tier 1 protocol (DL-005).

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct UnitId(/* opaque */);
pub struct Unit(/* opaque */);
pub struct AuthorId(/* opaque */);
pub struct TrustDomainId(/* opaque */);
pub struct ClusterId(/* opaque */);
pub struct Capability(/* opaque */);

/// Ed25519 signing key (private). Zeroized on drop.
pub struct SigningKey(/* opaque, zeroize */);

/// Ed25519 verification key (public).
pub struct VerifyingKey(/* opaque */);

/// Detached signature over a unit with context binding.
pub struct Signature(/* opaque */);

/// Key identifier — hash of the public key.
pub struct KeyId(/* opaque */);

/// Validity window for a signature: [not_before, not_after].
pub struct ValidityWindow {
    pub not_before: u64,
    pub not_after: u64,
}

/// Data classification in the ordered lattice:
/// Public < Internal < Confidential < PII.
pub enum Classification {
    Public,
    Internal,
    Confidential,
    Pii,
}

/// A share of a Shamir-split secret.
pub struct Share(/* opaque, zeroize */);

/// Identifier for an in-progress ceremony.
pub struct CeremonyId(/* opaque */);

/// State of a Shamir ceremony.
pub enum CeremonyState {
    Collecting { received: u8, threshold: u8 },
    Complete,
    Cancelled,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum SecurityError {
    /// Cryptographic signature is invalid (bad bytes, wrong key, etc.).
    InvalidSignature { reason: String },
    /// Author's key was revoked before the unit's creation timestamp.
    KeyRevoked { key: KeyId, revoked_at: u64 },
    /// Signature is outside its validity window.
    ExpiredSignature { key: KeyId, window: ValidityWindow },
    /// Author did not have valid scope at creation time.
    ScopeViolation { author: AuthorId, reason: String },
    /// Capability access denied (zero-default, INV-S1).
    CapabilityDenied { capability: String, reason: String },
    /// Taint traversal encountered a broken provenance chain.
    BrokenProvenance { unit: UnitId, reason: String },
    /// Ceremony protocol violation.
    CeremonyError { reason: String },
    /// Key generation or lookup failure.
    KeyError { reason: String },
    /// Declassification requires multi-party signing (INV-S9).
    DeclassificationDenied { reason: String },
    /// Author's public key not locally available. Caller should buffer the
    /// unit and retry when keys arrive via gossip.
    KeyNotFound { key: KeyId },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Signs units with Ed25519 and context binding.
///
/// Signatures bind: Sign(key, hash(unit || trust_domain_id || cluster_id ||
/// validity_window)) per INV-S3. This prevents replay across trust domains
/// or clusters.
pub trait Signer {
    /// Produce a detached signature over a unit with full context binding.
    ///
    /// The signature covers the hash of (unit content || trust_domain_id ||
    /// cluster_id || validity_window). The signing key MUST correspond to
    /// the author declared in the unit.
    ///
    /// Returns `SecurityError::KeyError` if the signing key is unavailable
    /// or has been revoked.
    fn sign(
        &self,
        unit: &Unit,
        trust_domain: &TrustDomainId,
        cluster: &ClusterId,
        validity: &ValidityWindow,
    ) -> Result<Signature, SecurityError>;
}

/// Verifies unit signatures. Synchronous — blocks merge (INV-S3).
///
/// This is intentionally NOT async. Signature verification must complete
/// before a unit can enter the graph. No unit enters graph state before
/// verification completes.
pub trait Verifier {
    /// Verify that a unit's signature is valid.
    ///
    /// Checks (per INV-S3):
    /// 1. Signature is cryptographically valid against the author's public key
    /// 2. Author had valid scope at the unit's creation timestamp
    /// 3. Author's key was not revoked before the unit's creation timestamp
    /// 4. Signature context (trust domain, cluster, validity window) matches
    ///
    /// Returns `Ok(())` if all checks pass. Returns the specific
    /// `SecurityError` variant describing the first failure.
    ///
    /// This method is synchronous and MUST NOT perform network I/O. All
    /// required key material and revocation state must be locally available.
    ///
    /// If the author's key is not locally available, returns
    /// `SecurityError::KeyNotFound` — the caller should buffer the unit
    /// and retry when keys arrive via gossip.
    fn verify(
        &self,
        unit: &Unit,
        signature: &Signature,
        trust_domain: &TrustDomainId,
        cluster: &ClusterId,
    ) -> Result<(), SecurityError>;
}

/// Runtime enforcement of declared vs. allowed capabilities.
///
/// Enforces INV-S1: a unit can only access capabilities it explicitly declared
/// AND that policy approved. Zero default. Fail closed on ambiguity (INV-S2).
pub trait CapabilityEnforcer {
    /// Check whether a unit is permitted to exercise a specific capability.
    ///
    /// Looks up the unit's declared needs, checks that a matching provide
    /// exists in the composition, and verifies that any required policy units
    /// approve the access. Purpose qualifiers must match if declared.
    ///
    /// Returns `SecurityError::CapabilityDenied` if access is not permitted.
    /// Fails closed: if the check is ambiguous or policy is missing, access
    /// is denied (INV-S2).
    fn check_access(
        &self,
        unit: &UnitId,
        capability: &Capability,
    ) -> Result<(), SecurityError>;

    /// Check whether a set of capabilities are all permitted for a unit.
    ///
    /// Short-circuits on the first denial. Returns the specific capability
    /// that was denied.
    fn check_all(
        &self,
        unit: &UnitId,
        capabilities: &[Capability],
    ) -> Result<(), SecurityError>;
}

/// Computes data classification by traversing the provenance graph.
///
/// Taint is computed at query time, not cached at merge time (INV-S4). This
/// makes taint eventually consistent across nodes. Multi-input workloads
/// inherit the union (most restrictive) of all input classifications.
pub trait TaintComputer {
    /// Compute the effective classification of a data unit.
    ///
    /// Traverses the provenance graph from the given data unit back through
    /// all producing workloads and their inputs. The result is the join
    /// (most restrictive) of all input classifications, unless explicit
    /// declassification policies exist along the path.
    ///
    /// Enforces INV-S7: children can narrow freely but widen only with policy.
    /// Enforces INV-S9: declassification requires multi-party signing.
    ///
    /// Returns `SecurityError::BrokenProvenance` if the provenance chain
    /// is incomplete (references to units not yet in the local graph).
    ///
    /// This is intentionally synchronous — taint traversal is in-memory only
    /// (the graph and provenance are local). No I/O justifies async here.
    ///
    /// For declassification policies encountered during traversal, also
    /// re-verifies that all signers are still active (not revoked as of
    /// query timestamp) per INV-S9.
    fn compute_taint(
        &self,
        data_unit: &UnitId,
    ) -> Result<Classification, SecurityError>;

    /// Check whether a declassification policy along a provenance path
    /// is valid (multi-party signed per INV-S9).
    ///
    /// Returns `SecurityError::DeclassificationDenied` if the policy lacks
    /// the required signatures (minimum 2 distinct authors: one policy-scoped,
    /// one data-steward-scoped).
    fn validate_declassification(
        &self,
        policy_unit: &UnitId,
    ) -> Result<(), SecurityError>;
}

/// Manages the Shamir Tier 1 key ceremony (DL-005).
///
/// The ceremony is the pre-graph bootstrap: it produces the root key whose
/// public half signs the first governance unit, seeding the composition graph.
///
/// Protocol: start -> add_share (repeated) -> complete with witness.
/// Ceremony events are recorded as governance units in the graph.
pub trait CeremonyManager {
    /// Start a new Shamir ceremony with the given threshold and total shares.
    ///
    /// Default: 5 shares, threshold 3. The ceremony enters Collecting state.
    /// All key material is zeroized on drop.
    ///
    /// Returns `SecurityError::CeremonyError` if a ceremony is already in
    /// progress or parameters are invalid (threshold > total, threshold < 2).
    async fn start(
        &self,
        total_shares: u8,
        threshold: u8,
    ) -> Result<CeremonyId, SecurityError>;

    /// Add a share to an in-progress ceremony.
    ///
    /// Shares are verified as they arrive. Duplicate shares from the same
    /// holder are rejected. The share is zeroized from the caller's memory
    /// after this call.
    ///
    /// Returns the updated ceremony state (how many shares received vs.
    /// threshold). Returns `SecurityError::CeremonyError` if the ceremony
    /// is not in Collecting state or the share is invalid.
    async fn add_share(
        &self,
        ceremony: &CeremonyId,
        share: Share,
    ) -> Result<CeremonyState, SecurityError>;

    /// Complete the ceremony once threshold shares are collected.
    ///
    /// Requires a witness node to co-sign the ceremony completion event.
    /// The reconstructed key signs the root governance unit, then is
    /// immediately zeroized. Only the public key persists.
    ///
    /// Returns `SecurityError::CeremonyError` if threshold not met, witness
    /// is invalid, or reconstruction fails.
    async fn complete(
        &self,
        ceremony: &CeremonyId,
        witness_node: &KeyId,
    ) -> Result<VerifyingKey, SecurityError>;

    /// Cancel an in-progress ceremony. All collected shares are zeroized.
    ///
    /// Returns `SecurityError::CeremonyError` if the ceremony does not
    /// exist or is already complete.
    async fn cancel(&self, ceremony: &CeremonyId) -> Result<(), SecurityError>;

    /// Query the current state of a ceremony.
    fn state(&self, ceremony: &CeremonyId) -> Result<CeremonyState, SecurityError>;
}

/// Manages Ed25519 key lifecycle: generation, revocation, and lookup.
///
/// Key material is zeroized on drop. Revocation is irreversible and propagated
/// via priority gossip. Revoked keys remain in the store for historical
/// signature verification (INV-S3 checks revocation timestamp).
pub trait KeyManager {
    /// Generate a new Ed25519 key pair for an author.
    ///
    /// The private key is stored securely (platform keystore where available).
    /// Returns the public key and a KeyId (hash of public key).
    ///
    /// Returns `SecurityError::KeyError` on generation failure.
    async fn generate(&self) -> Result<(KeyId, VerifyingKey), SecurityError>;

    /// Revoke a key by its ID. Revocation is timestamped and irreversible.
    ///
    /// After revocation, `Verifier::verify` will reject any unit whose
    /// creation timestamp is after the revocation timestamp.
    ///
    /// Returns `SecurityError::KeyError` if the key does not exist or is
    /// already revoked.
    async fn revoke(&self, key: &KeyId) -> Result<(), SecurityError>;

    /// Look up a public key by its ID.
    ///
    /// Returns the key and its revocation status (None = active,
    /// Some(timestamp) = revoked at that time).
    ///
    /// Returns `SecurityError::KeyError` if the key is unknown.
    fn lookup(&self, key: &KeyId) -> Result<(VerifyingKey, Option<u64>), SecurityError>;

    /// Check whether a key was valid (not revoked) at a specific timestamp.
    fn is_valid_at(&self, key: &KeyId, timestamp: u64) -> Result<bool, SecurityError>;

    /// Check whether a key is revoked in the local graph (causal model, INV-S3).
    /// Returns true if a revocation governance unit for this author has been
    /// merged into the local graph. Does NOT compare clocks.
    fn is_revoked_in_local_graph(&self, author: &AuthorId) -> bool;
}

// ---------------------------------------------------------------------------
// Delegation (INV-W4, INV-W4a)
// ---------------------------------------------------------------------------

pub struct DelegationTokenId(/* opaque */);
pub struct DelegationToken(/* opaque */);
pub struct LogicalClock(/* opaque */);

/// Validates delegation tokens for spawned task signing.
///
/// When a service is placed on a node, the author pre-signs a delegation
/// token. The node uses this token to sign spawned bounded tasks on behalf
/// of the author. The node never holds the author's private key.
///
/// INV-W4a: delegation grants operational authority only. Spawned tasks
/// CANNOT create policy units, governance units, or initiate declassification.
pub trait DelegationValidator {
    /// Validate that a delegation token is valid for signing a spawned task.
    ///
    /// Checks:
    /// 1. Token was signed by an author with valid scope
    /// 2. Token's LC range covers the spawned task's creation LC
    /// 3. Spawn count has not exceeded max_spawns
    /// 4. Token has not been revoked
    /// 5. Parent service is still active (not terminated)
    ///
    /// Returns `SecurityError::DelegationExpired` if LC range exceeded.
    /// Returns `SecurityError::DelegationSpawnLimitExceeded` if count exceeded.
    /// Returns `SecurityError::DelegationTokenForged` if signature invalid.
    fn validate(
        &self,
        token: &DelegationToken,
        spawned_unit_lc: &LogicalClock,
    ) -> Result<(), SecurityError>;

    /// Check that a spawned task is not attempting governance operations.
    ///
    /// INV-W4a: spawned tasks cannot create policy units, governance units,
    /// or participate in multi-party declassification.
    fn check_governance_block(
        &self,
        token: &DelegationToken,
        unit_type: &str, // "policy" | "governance" | "declassification"
    ) -> Result<(), SecurityError>;
}

/// Manages delegation token lifecycle.
pub trait DelegationManager {
    /// Create a delegation token for a service placement.
    ///
    /// The author signs the token. The token binds: service_id, node_id,
    /// trust_domain, LC range, max_spawns.
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
    fn revoke_token(&self, token_id: &DelegationTokenId) -> Result<(), SecurityError>;
}

pub struct NodeId(/* opaque */);

// ---------------------------------------------------------------------------
// Tier 0 solo ceremony
// ---------------------------------------------------------------------------

/// Tier 0 solo bootstrap: `taba init` in one command.
/// Generates node key + author key + self-signed trust domain + root
/// governance unit. No Shamir, no shares, no witnesses.
pub trait SoloBootstrap {
    /// Initialize a single-key cluster (Tier 0).
    ///
    /// Returns: (key_pair, trust_domain_governance_unit, root_role_assignment).
    /// The developer is immediately operational.
    async fn solo_init(&self) -> Result<SoloBootstrapResult, SecurityError>;
}

pub struct SoloBootstrapResult {
    pub key_id: KeyId,
    pub public_key: VerifyingKey,
    pub trust_domain: TrustDomainId,
    pub governance_unit_id: UnitId,
    pub role_assignment_id: UnitId,
}
