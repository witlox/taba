# Attestation

Attestation proves a node's hardware/software integrity before it
joins the cluster. It is **optional** (A5): dev/small deployments
work without hardware attestation.

## Providers

### Software attestation (dev/test)

`SoftwareAttestation` generates a SHA-256 hash of the running
binary and OS information, bound to a nonce:

```
quote = SHA-256(binary_hash || os || arch || nonce)
signature = quote  (self-signed — intentionally weak)
```

**Weakness**: only proves the binary hasn't been modified at rest.
Does not prove the platform is trustworthy. A compromised system
can forge a quote.

**Use case**: dev, testing, CI. Clearly marked as dev-only.

### TPM attestation (production, feature-gated)

`TpmAttestation` uses TPM 2.0 to produce a cryptographically
signed quote of the platform's boot state:

- Uses the TPM's Attestation Identity Key (AIK)
- Quote includes the nonce (prevents replay)
- Signature is verifiable against the AIK public key
- Requires a TPM 2.0 chip and the `tss` feature

**Use case**: production, high-security environments.

## Verification

```rust
pub fn verify_software_attestation(
    result: &AttestationResult,
    expected_nonce: &[u8],
) -> Result<(), SecurityError>
```

Checks:
1. The quote matches the expected value (binary_hash + os + arch + nonce)
2. The signature matches the quote (self-signed for software)
3. The nonce matches (prevents replay)

Tampering with any field after attestation causes verification to fail.

## Attestation result

```rust
pub struct AttestationResult {
    pub node_id: NodeId,
    pub binary_hash: [u8; 32],
    pub os: String,
    pub arch: String,
    pub quote: Vec<u8>,
    pub signature: Vec<u8>,
    pub provider: AttestationProvider,
}
```

The `provider` field indicates whether software or TPM attestation
was used, allowing the enrollment ceremony to reject weak
attestation in production environments.
