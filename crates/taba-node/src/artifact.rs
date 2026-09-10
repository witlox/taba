//! Artifact fetching with digest verification (INV-A1, INV-A2).
//!
//! The [`ArtifactFetcher`] trait fetches workload artifacts by
//! reference and verifies their SHA-256 digest after fetch. For M3,
//! only local file fetching is supported — peer cache and remote
//! URL fetching are deferred to later phases.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use taba_common::ContentDigest;

use crate::error::NodeError;

// ---------------------------------------------------------------------------
// ArtifactRef
// ---------------------------------------------------------------------------

/// A reference to a workload artifact.
///
/// Can be a local file path (e.g., `/usr/local/bin/app`) or a URL
/// (e.g., `https://registry.example.com/app:v1`). For M3, only local
/// file paths are supported.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactRef(pub String);

impl ArtifactRef {
    /// Creates a new `ArtifactRef` from a string.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Returns `true` if this reference is a local file path.
    ///
    /// A reference is considered local if it doesn't start with a
    /// URL scheme (`http://`, `https://`, `oci://`, etc.).
    #[must_use]
    pub fn is_local(&self) -> bool {
        !self.0.contains("://")
    }

    /// Returns the reference as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ArtifactRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// ArtifactFetcher trait
// ---------------------------------------------------------------------------

/// Fetches workload artifacts with peer cache (INV-A1, INV-A2).
///
/// Fetch order: peer cache → external source. Digest verification
/// mandatory after fetch (INV-A1). Push mode for air-gapped
/// environments.
///
/// For M3, only local file fetching is supported. Peer cache and
/// remote URL fetching are deferred.
pub trait ArtifactFetcher: Send + Sync {
    /// Fetch an artifact by reference and expected digest.
    ///
    /// 1. Check local cache (M3: no peer cache)
    /// 2. If miss, fetch from source (M3: local file copy only)
    /// 3. Verify SHA-256 digest after fetch (INV-A1)
    /// 4. Cache locally for future requests
    ///
    /// Returns path to the fetched (cached) artifact.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalWriteFailed`] if the artifact cannot be
    ///   fetched (file not found, URL unsupported).
    /// - [`NodeError::ReconciliationFailed`] if the digest does not
    ///   match the expected value (INV-A1).
    async fn fetch(
        &self,
        artifact_ref: &ArtifactRef,
        expected_digest: &ContentDigest,
    ) -> Result<PathBuf, NodeError>;

    /// Push an artifact to the local cache (air-gapped/dev mode).
    ///
    /// The artifact is content-addressed (SHA-256) and stored in the
    /// cache directory. Returns the computed content digest.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalWriteFailed`] if the artifact cannot be
    ///   read or written to the cache.
    async fn push(&self, artifact_path: &Path) -> Result<ContentDigest, NodeError>;

    /// Check if a specific artifact is in the local cache.
    fn is_cached(&self, digest: &ContentDigest) -> bool;
}

// ---------------------------------------------------------------------------
// DefaultArtifactFetcher
// ---------------------------------------------------------------------------

/// Default implementation of [`ArtifactFetcher`].
///
/// Uses a local directory as the artifact cache. Artifacts are stored
/// by their SHA-256 digest (content-addressed). For M3, only local
/// file fetching is supported.
///
/// Thread-safe via interior mutability.
#[derive(Debug)]
pub struct DefaultArtifactFetcher {
    /// The cache directory.
    cache_dir: PathBuf,
}

impl DefaultArtifactFetcher {
    /// Creates a new `DefaultArtifactFetcher` with the given cache
    /// directory. The directory is created if it doesn't exist.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalWriteFailed`] if the cache directory cannot
    ///   be created.
    pub fn new(cache_dir: impl Into<PathBuf>) -> Result<Self, NodeError> {
        let cache_dir = cache_dir.into();
        std::fs::create_dir_all(&cache_dir).map_err(|e| NodeError::WalWriteFailed {
            reason: format!(
                "failed to create artifact cache dir {}: {e}",
                cache_dir.display()
            ),
        })?;
        Ok(Self { cache_dir })
    }

    /// Computes the SHA-256 digest of a file.
    fn compute_digest(path: &Path) -> Result<ContentDigest, NodeError> {
        let data = std::fs::read(path).map_err(|e| NodeError::WalWriteFailed {
            reason: format!("failed to read artifact {}: {e}", path.display()),
        })?;
        Ok(sha256_digest(&data))
    }

    /// Returns the cache path for a given digest.
    fn cache_path(&self, digest: &ContentDigest) -> PathBuf {
        self.cache_dir.join(&digest.0)
    }
}

/// Computes the SHA-256 digest of a byte slice and returns it as a
/// `ContentDigest` (hex string with `sha256:` prefix).
fn sha256_digest(data: &[u8]) -> ContentDigest {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    ContentDigest(format!("sha256:{}", hex_encode(&hash)))
}

/// Encodes a byte slice as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

impl ArtifactFetcher for DefaultArtifactFetcher {
    async fn fetch(
        &self,
        artifact_ref: &ArtifactRef,
        expected_digest: &ContentDigest,
    ) -> Result<PathBuf, NodeError> {
        // Check local cache first.
        if self.is_cached(expected_digest) {
            return Ok(self.cache_path(expected_digest));
        }

        // M3: only local file fetching is supported.
        if !artifact_ref.is_local() {
            return Err(NodeError::WalWriteFailed {
                reason: format!("remote artifact fetching not supported in M3: {artifact_ref}"),
            });
        }

        let source_path = Path::new(artifact_ref.as_str());
        if !source_path.exists() {
            return Err(NodeError::WalWriteFailed {
                reason: format!("artifact not found: {}", source_path.display()),
            });
        }

        // Compute digest of the source file.
        let actual_digest = Self::compute_digest(source_path)?;

        // Verify digest (INV-A1).
        if actual_digest != *expected_digest {
            return Err(NodeError::ReconciliationFailed {
                unit: taba_common::UnitId(uuid::Uuid::nil()),
                reason: format!(
                    "artifact digest mismatch: expected {}, got {} (INV-A1)",
                    expected_digest.0, actual_digest.0
                ),
            });
        }

        // Copy to cache.
        let cache_path = self.cache_path(expected_digest);
        std::fs::copy(source_path, &cache_path).map_err(|e| NodeError::WalWriteFailed {
            reason: format!(
                "failed to copy artifact to cache {}: {e}",
                cache_path.display()
            ),
        })?;

        Ok(cache_path)
    }

    async fn push(&self, artifact_path: &Path) -> Result<ContentDigest, NodeError> {
        // Compute digest.
        let digest = Self::compute_digest(artifact_path)?;

        // Copy to cache (if not already cached).
        let cache_path = self.cache_path(&digest);
        if !cache_path.exists() {
            std::fs::copy(artifact_path, &cache_path).map_err(|e| NodeError::WalWriteFailed {
                reason: format!(
                    "failed to copy artifact to cache {}: {e}",
                    cache_path.display()
                ),
            })?;
        }

        Ok(digest)
    }

    fn is_cached(&self, digest: &ContentDigest) -> bool {
        self.cache_path(digest).exists()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_ref_local() {
        assert!(ArtifactRef::new("/usr/local/bin/app").is_local());
        assert!(ArtifactRef::new("./bin/app").is_local());
        assert!(!ArtifactRef::new("https://example.com/app").is_local());
        assert!(!ArtifactRef::new("oci://registry/app:v1").is_local());
    }

    #[tokio::test]
    async fn test_fetch_local_file() {
        let tmp = tempfile::tempdir().expect("create temp dir");

        // Create a source artifact.
        let source = tmp.path().join("source.bin");
        let data = b"hello world";
        std::fs::write(&source, data).expect("write source");

        // Compute expected digest.
        let expected = sha256_digest(data);

        // Create fetcher with cache dir.
        let cache_dir = tmp.path().join("cache");
        let fetcher = DefaultArtifactFetcher::new(&cache_dir).expect("create fetcher");

        // Fetch should copy and verify.
        let fetched_path = fetcher
            .fetch(
                &ArtifactRef::new(source.to_string_lossy().to_string()),
                &expected,
            )
            .await
            .expect("fetch should succeed");

        // Verify the fetched file exists and has the right content.
        assert!(fetched_path.exists(), "fetched file should exist");
        let fetched_data = std::fs::read(&fetched_path).expect("read fetched");
        assert_eq!(fetched_data, data);

        // Verify it's in the cache.
        assert!(fetcher.is_cached(&expected));
    }

    #[tokio::test]
    async fn test_fetch_digest_mismatch() {
        let tmp = tempfile::tempdir().expect("create temp dir");

        // Create a source artifact.
        let source = tmp.path().join("source.bin");
        std::fs::write(&source, b"hello world").expect("write source");

        // Use a wrong expected digest.
        let wrong_digest = ContentDigest(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        );

        let cache_dir = tmp.path().join("cache");
        let fetcher = DefaultArtifactFetcher::new(&cache_dir).expect("create fetcher");

        let result = fetcher
            .fetch(
                &ArtifactRef::new(source.to_string_lossy().to_string()),
                &wrong_digest,
            )
            .await;

        assert!(
            result.is_err(),
            "fetch with wrong digest should fail (INV-A1)"
        );

        // Should not be cached.
        assert!(!fetcher.is_cached(&wrong_digest));
    }

    #[tokio::test]
    async fn test_push_creates_cache_entry() {
        let tmp = tempfile::tempdir().expect("create temp dir");

        // Create a source artifact.
        let source = tmp.path().join("source.bin");
        let data = b"push me";
        std::fs::write(&source, data).expect("write source");

        let cache_dir = tmp.path().join("cache");
        let fetcher = DefaultArtifactFetcher::new(&cache_dir).expect("create fetcher");

        // Push should compute digest and cache.
        let digest = fetcher.push(&source).await.expect("push should succeed");

        // Verify digest is correct.
        let expected = sha256_digest(data);
        assert_eq!(digest, expected);

        // Verify it's cached.
        assert!(fetcher.is_cached(&digest));
    }

    #[tokio::test]
    async fn test_is_cached_false() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let cache_dir = tmp.path().join("cache");
        let fetcher = DefaultArtifactFetcher::new(&cache_dir).expect("create fetcher");

        let unknown_digest = ContentDigest(
            "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        );

        assert!(!fetcher.is_cached(&unknown_digest));
    }

    #[tokio::test]
    async fn test_fetch_remote_unsupported() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let cache_dir = tmp.path().join("cache");
        let fetcher = DefaultArtifactFetcher::new(&cache_dir).expect("create fetcher");

        let digest = ContentDigest("sha256:abc".to_string());
        let result = fetcher
            .fetch(&ArtifactRef::new("https://example.com/app"), &digest)
            .await;

        assert!(result.is_err(), "remote fetching should fail in M3");
    }
}
