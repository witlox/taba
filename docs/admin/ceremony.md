# Shamir Ceremony

The Shamir key ceremony is the pre-graph bootstrap: it produces
the root key whose public half signs the first governance unit,
seeding the composition graph.

## Tiers

| Tier | Name | Flow | Use case |
|------|------|------|----------|
| 0 | Solo | `taba init` (one command) | Dev, single-operator |
| 1 | Basic | `start → add shares → complete with witness` | Small teams |
| 2 | Password-protected | Each share encrypted with Argon2id | Production |
| 3 | Offline two-factor | Seed code + password | High-security production |

## Tier 1 ceremony protocol

### 1. Start

```rust
ceremony.start(total_shares: 5, threshold: 3)
```

- Validates: threshold ≥ 2, threshold ≤ total_shares, total ≤ 255
- Generates a new Ed25519 `KeyPair`
- Splits the private key using `shamir::split_secret` over GF(2^8)
- Returns a `CeremonyId` and the shares (for distribution)

### 2. Add shares

```rust
ceremony.add_share(ceremony_id, share)
```

- Rejects duplicate share indices
- Rejects shares from unknown ceremonies
- Returns updated `CeremonyState::CollectingShares { shares_received, threshold }`

### 3. Complete

```rust
ceremony.complete(ceremony_id, witness_node: &KeyId)
```

- Requires ≥ threshold shares
- Reconstructs the private key via `shamir::reconstruct_secret`
  (Lagrange interpolation at x = 0)
- Derives `VerifyingKey` from reconstructed key
- **Zeroizes the reconstructed private key immediately**
- Returns the `VerifyingKey` (public key only)

### 4. Cancel (if needed)

```rust
ceremony.cancel(ceremony_id)
```

- Zeroizes all stored share material
- Transitions to `Failed` state

## GF(2^8) arithmetic

All Shamir operations use GF(2^8) with the irreducible polynomial
`x^8 + x^4 + x^3 + x + 1` (0x11B), the same field used by AES
and Reed-Solomon.

- Precomputed log/exp tables with generator g = 3
- Multiplication via log/exp: `gf_mul(a, b) = EXP[LOG[a] + LOG[b] % 255]`
- Division via log/exp: `gf_div(a, b) = EXP[(LOG[a] - LOG[b] + 255) % 255]`
- Each byte of the secret is split independently

## Security properties

- **Information-theoretic**: fewer than `k` shares reveal nothing
- **Zero-knowledge**: shares are never aggregated in memory —
  each participant holds their share, the ceremony reconstructs
  only at completion, then immediately zeroizes
- **Byzantine-resistant**: duplicate shares are rejected;
  threshold must be ≥ 2

## Ceremony state

```
Created → CollectingShares → Complete
                 ↓
              Failed (cancel or error)
```

Ceremony events are recorded as governance units in the graph.
