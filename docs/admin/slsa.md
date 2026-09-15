# SLSA Provenance

taba supports [SLSA (Supply-chain Levels for Software Artifacts)](https://slsa.dev/)
build provenance verification. Trust domains can require a minimum
SLSA level for workload units.

## SLSA levels

| Level | Description | taba enforcement |
|-------|-------------|------------------|
| 0 | No provenance | No requirement (default) |
| 1 | Build process documented | Provenance exists |
| 2 | Hosted build service | Builder identified, source repo + digest |
| 3 | Hardened build platform | Builder signature verified |

## Provenance structure

```rust
pub struct SlsaProvenance {
    pub builder: String,           // e.g., "github-actions/v3"
    pub source_repo: String,       // e.g., "github.com/acme/service"
    pub source_digest: String,     // e.g., "sha256:abc123..."
    pub slsa_level: u8,            // 1, 2, or 3
    pub build_timestamp: u64,      // Wall time millis
    pub builder_signature: Vec<u8>, // Ed25519 signature over payload
    pub build_parameters: Vec<(String, String)>,
}
```

## Verification

The `ProvenanceVerifier` trait checks:

1. **Structural validation**: all required fields present
2. **SLSA level check**: `provenance.slsa_level >= min_level`
3. **Source digest comparison**: if expected digest is provided,
   it must match exactly
4. **Builder signature verification**: if the builder is in the
   trusted list, its Ed25519 signature is verified against the
   builder's public key

### Default behavior

- **No trusted builders**: all builders are trusted without
  signature verification (progressive disclosure — SLSA level 1
  doesn't require signatures)
- **With trusted builders**: only builders in the trusted list
  are accepted; their signatures are verified

## Signing

```rust
pub fn sign_provenance(
    provenance: &mut SlsaProvenance,
    signing_key: &SigningKey,
) -> Result<(), SecurityError>
```

Signs over the JSON serialization of the provenance without the
`builder_signature` field. Any modification after signing
(including a single byte change) causes verification to fail.

## Integration

The solver calls `ProvenanceVerifier::verify` before placing a
workload unit. If the trust domain requires a minimum SLSA level
and the unit's provenance is below that level, placement is
blocked (fail-closed per INV-S2).

## K8s migration

The `taba-k8s` converter can generate SLSA provenance placeholders:

```sh
taba-k8s convert deployment.yaml --provenance
```

The generated units contain commented-out `[provenance]` sections
for filling in build system details.
