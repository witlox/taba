# Node Enrollment

New nodes join the cluster via a multi-party enrollment ceremony.
Unlike the root key ceremony (which establishes the root of trust),
enrollment gives a **new node** its own Ed25519 key.

## Protocol

### 1. Initiate

An authorized author (or the root key) initiates enrollment:

```rust
enrollment.initiate(
    node_id: &NodeId,
    trust_domain: &TrustDomainId,
    authorized_by: &AuthorId,
    total_shares: u8,
    threshold: u8,
    attestation: Option<&AttestationResult>,
)
```

- If attestation is provided, it is verified before enrollment proceeds
- Generates a new Ed25519 `KeyPair`
- Splits the private key via `shamir::split_secret`
- Returns `EnrollmentId` and shares (for distribution to existing nodes)

### 2. Contribute

Existing nodes contribute shares:

```rust
enrollment.contribute(enrollment_id, share)
```

- Rejects duplicate share indices
- Rejects shares from unknown or completed enrollments
- Returns updated `EnrollmentState::Collecting { shares_received, threshold }`

### 3. Complete

```rust
enrollment.complete(enrollment_id)
```

- Requires ≥ threshold shares
- Reconstructs private key via `shamir::reconstruct_secret`
- Derives `VerifyingKey` (public key)
- **Zeroizes the reconstructed private key**
- Generates a membership governance unit ID
- Returns `EnrollmentResult { node_id, public_key, key_id, trust_domain, membership_unit_id, attestation }`

### 4. Cancel (if needed)

```rust
enrollment.cancel(enrollment_id)
```

Zeroizes all stored share material. Transitions to `Failed` state.

## Trust model

- The root key (from the Shamir ceremony) authorizes enrollment
- Threshold `k` out of `n` existing nodes must contribute shares
- The new node **never receives the root key** — only its own key
- The enrollment event is recorded as a governance unit in the graph

## Attestation

Attestation can be required before enrollment proceeds:

| Provider | Security | Use case |
|----------|----------|----------|
| `SoftwareAttestation` | Weak (SHA-256 of binary + OS info) | Dev, testing |
| `TpmAttestation` | Strong (TPM 2.0 quote, AIK-signed) | Production (feature-gated) |

Software attestation generates a SHA-256 hash of the running binary
and OS information, bound to a nonce. The verifier checks the quote
matches and the signature is valid. This is intentionally weak — it
only proves the binary hasn't been modified at rest, not that the
platform is trustworthy.

TPM attestation uses the TPM's Attestation Identity Key (AIK) to
sign a quote of the platform's boot state. This is cryptographically
verifiable and resistant to tampering.
