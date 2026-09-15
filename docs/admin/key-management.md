# Key Management

taba uses Ed25519 for all cryptographic operations. Key material
is zeroized on drop via the `zeroize` crate.

## Key lifecycle

### Generation

```sh
taba init    # Generates Ed25519 keypair
```

The private key is stored as hex in `~/.taba/keypair` with
`0600` (owner-only) permissions on Unix. On non-Unix systems,
`std::fs::write` is used as a fallback (no permission control).

### Key ID

`KeyId` = SHA-256 of the public key, truncated to 128 bits,
encoded as a UUID. This provides a stable, deterministic
identifier without exposing the full public key.

### Revocation

Keys are revoked via a governance unit (`KeyRevocationDef`).
Revocation follows a **causal model** (INV-S3):

- Units signed **before** the revocation timestamp remain valid
- Units signed **after** are rejected
- No retroactive rejection — the graph doesn't undo history

The revocation is propagated via **priority gossip** (double the
normal retransmit rounds) for rapid convergence.

### Revocation reasons

| Reason | Description |
|--------|-------------|
| `Compromised` | Key material was compromised |
| `Departed` | Author has left the organization |
| `Rotated` | Key rotation — replaced by a new public key |
| `Administrative` | Administrative revocation with details |

## Shamir secret sharing

The root key (root of all authority) is split into `n` shares
using Shamir secret sharing over GF(2^8):

- **Tier 0** (solo): Single key, no Shamir. `taba init` in one command.
- **Tier 1** (basic): `start → add shares → complete with witness`
- **Tier 2** (password-protected): Each share encrypted with Argon2id
- **Tier 3** (offline two-factor): Seed code + password

The ceremony produces the root key whose public half signs the
first governance unit, seeding the composition graph. The
reconstructed key is **immediately zeroized** — only the public
key persists.

## Node enrollment

New nodes join via a multi-party enrollment ceremony:

1. An authorized author initiates enrollment for a new node
2. A new Ed25519 key pair is generated, private key split into `n` shares
3. Existing nodes contribute shares to the new node
4. Once `k` shares are collected, the new node reconstructs its key
5. The key is zeroized; only the public key persists
6. A membership governance unit is created

Attestation can be required before enrollment:
- **Software attestation** (dev/test): SHA-256 of binary + OS info
- **TPM attestation** (production, feature-gated): TPM 2.0 quote
