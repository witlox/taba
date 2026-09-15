//! Local key management: keypair generation, persistence, and config.
//!
//! The [`LocalAuth`] struct manages the local author's Ed25519 keypair
//! and node configuration (trust domain, cluster ID, node ID). State
//! is persisted to a local directory (`~/.taba/` by default, or
//! `--state-dir` override).
//!
//! ## Key storage format
//!
//! The private key is serialized as raw 32 bytes encoded as hex in a
//! file named `keypair`. The public key is derived from the private
//! key — it is not stored separately. [`serde`] is NOT used for the
//! private key; raw [`ed25519_dalek::SigningKey`] bytes are used
//! directly to avoid any intermediate serialization that might leak
//! key material.
//!
//! ## Config format
//!
//! Node configuration ([`LocalConfig`]) is stored as JSON in a file
//! named `config.json`.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taba_common::{ClusterId, NodeId, TrustDomainId, UnitId};
use taba_security::{KeyId, KeyPair, SigningKey, SoloBootstrapResult};

use crate::error::CliError;

/// Filename for the keypair (hex-encoded private key).
const KEYPAIR_FILENAME: &str = "keypair";

/// Filename for the node configuration (JSON).
const CONFIG_FILENAME: &str = "config.json";

/// Local configuration persisted to disk.
///
/// Contains the trust domain, cluster ID, and node ID assigned during
/// `taba init`. These values scope all units authored on this node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConfig {
    /// The trust domain this node belongs to.
    pub trust_domain: TrustDomainId,
    /// The cluster ID this node belongs to.
    pub cluster_id: ClusterId,
    /// The local node ID.
    pub node_id: NodeId,
}

/// Manages local author keys and state.
///
/// The state directory contains:
/// - `keypair` — hex-encoded Ed25519 private key (32 bytes)
/// - `config.json` — node configuration (trust domain, cluster ID, node ID)
///
/// Use [`LocalAuth::new`] for the default state directory (`~/.taba`),
/// or [`LocalAuth::with_state_dir`] for a custom location.
pub struct LocalAuth {
    /// State directory (default: ~/.taba).
    state_dir: PathBuf,
}

impl LocalAuth {
    /// Creates a new `LocalAuth` using the default state directory.
    ///
    /// The default directory is `$HOME/.taba`. If the `HOME`
    /// environment variable is not set, falls back to `./.taba`.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::Io`] if the state directory cannot be
    /// created.
    pub fn new() -> Result<Self, CliError> {
        let dir = std::env::var("HOME")
            .map_or_else(|_| PathBuf::from(".taba"), |h| Path::new(&h).join(".taba"));
        Self::with_state_dir(dir)
    }

    /// Creates a new `LocalAuth` with the given state directory.
    ///
    /// The directory is created if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`CliError::Io`] if the directory cannot be created.
    pub fn with_state_dir(dir: PathBuf) -> Result<Self, CliError> {
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }
        Ok(Self { state_dir: dir })
    }

    /// Check if the node has been initialized.
    ///
    /// Returns `true` if both the keypair file and config file exist
    /// in the state directory.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.keypair_path().exists() && self.config_path().exists()
    }

    /// Returns the path to the keypair file.
    fn keypair_path(&self) -> PathBuf {
        self.state_dir.join(KEYPAIR_FILENAME)
    }

    /// Returns the path to the config file (`trust_domain`, `cluster_id`).
    fn config_path(&self) -> PathBuf {
        self.state_dir.join(CONFIG_FILENAME)
    }

    /// Returns the state directory path.
    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    /// Initialize: generate keypair, save to disk.
    ///
    /// Generates a new Ed25519 keypair, saves the private key as hex
    /// to the keypair file, and creates a node configuration with a
    /// fresh trust domain, cluster ID, and node ID.
    ///
    /// Returns the [`SoloBootstrapResult`] containing the key ID,
    /// public key, trust domain, and governance unit IDs.
    ///
    /// # Errors
    ///
    /// - [`CliError::Io`] if the keypair or config file cannot be
    ///   written.
    /// - [`CliError::Security`] if the generated public key is not a
    ///   canonical Ed25519 encoding (practically impossible).
    pub fn init(&self) -> Result<SoloBootstrapResult, CliError> {
        // Generate a new Ed25519 key pair.
        let key_pair = KeyPair::generate();
        let public_key = *key_pair.public_key();
        let signing_bytes = key_pair.signing_key().to_bytes();

        // Save the private key as hex.
        let hex_key = hex::encode(signing_bytes);
        // Write with restrictive permissions (0600 — owner only).
        // std::fs::write uses default permissions (0644 with umask 022),
        // which makes the private key readable by all users on the system.
        // Use OpenOptions to explicitly set 0600.
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(self.keypair_path())?
            .write_all(hex_key.as_bytes())?;

        // Derive the key ID and verifying key.
        let key_id = KeyId::from_public_key(&public_key);
        let verifying_key = public_key.to_verifying_key().ok_or_else(|| {
            CliError::Security(taba_security::SecurityError::KeyError {
                reason: "generated public key is not a canonical Ed25519 encoding".to_string(),
            })
        })?;

        // Create fresh trust domain, cluster, and node IDs.
        let trust_domain = TrustDomainId(uuid::Uuid::new_v4());
        let governance_unit_id = UnitId(uuid::Uuid::new_v4());
        let role_assignment_id = UnitId(uuid::Uuid::new_v4());
        let cluster_id = ClusterId(uuid::Uuid::new_v4());
        let node_id = NodeId(uuid::Uuid::new_v4());

        // Persist the node configuration.
        let config = LocalConfig {
            trust_domain,
            cluster_id,
            node_id,
        };
        self.save_config(&config)?;

        Ok(SoloBootstrapResult {
            key_id,
            public_key: verifying_key,
            trust_domain,
            governance_unit_id,
            role_assignment_id,
        })
    }

    /// Load the keypair from disk.
    ///
    /// Reads the hex-encoded private key from the keypair file and
    /// reconstructs a [`KeyPair`] using
    /// [`SigningKey::from_bytes`].
    ///
    /// # Errors
    ///
    /// - [`CliError::Io`] if the keypair file cannot be read.
    /// - [`CliError::InvalidInput`] if the hex is invalid or the key
    ///   is not exactly 32 bytes.
    pub fn load_keypair(&self) -> Result<KeyPair, CliError> {
        let hex_key = std::fs::read_to_string(self.keypair_path())?;
        let bytes = hex::decode(hex_key.trim()).map_err(|e| CliError::InvalidInput {
            reason: format!("invalid hex in keypair file: {e}"),
        })?;
        if bytes.len() != 32 {
            return Err(CliError::InvalidInput {
                reason: format!(
                    "keypair file must contain 32 bytes (64 hex chars), got {} bytes",
                    bytes.len()
                ),
            });
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let signing_key = SigningKey::from_bytes(&arr);
        Ok(KeyPair::from_signing_key(signing_key))
    }

    /// Load the local config (`trust_domain`, `cluster_id`, `node_id`).
    ///
    /// Reads and deserializes the `config.json` file from the state
    /// directory.
    ///
    /// # Errors
    ///
    /// - [`CliError::Io`] if the config file cannot be read.
    /// - [`CliError::Json`] if the config file is not valid JSON.
    pub fn load_config(&self) -> Result<LocalConfig, CliError> {
        let config_str = std::fs::read_to_string(self.config_path())?;
        let config: LocalConfig = serde_json::from_str(&config_str)?;
        Ok(config)
    }

    /// Save the local config.
    ///
    /// Serializes the config as pretty-printed JSON and writes it to
    /// `config.json` in the state directory.
    ///
    /// # Errors
    ///
    /// - [`CliError::Json`] if serialization fails.
    /// - [`CliError::Io`] if the file cannot be written.
    pub fn save_config(&self, config: &LocalConfig) -> Result<(), CliError> {
        let json = serde_json::to_string_pretty(config)?;
        std::fs::write(self.config_path(), json)?;
        Ok(())
    }
}

impl std::fmt::Debug for LocalAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalAuth")
            .field("state_dir", &self.state_dir)
            .field("initialized", &self.is_initialized())
            .finish()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_creates_files() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");

        assert!(!auth.is_initialized(), "should not be initialized yet");

        let result = auth.init().expect("init should succeed");

        assert!(auth.is_initialized(), "should be initialized after init");
        assert!(
            tmp.path().join(KEYPAIR_FILENAME).exists(),
            "keypair file should exist"
        );
        assert!(
            tmp.path().join(CONFIG_FILENAME).exists(),
            "config file should exist"
        );
        assert_ne!(result.key_id, KeyId([0u8; 32]), "key ID should be non-zero");
        assert_ne!(
            result.trust_domain,
            TrustDomainId(uuid::Uuid::nil()),
            "trust domain should be non-nil"
        );
    }

    #[test]
    fn test_is_initialized_false() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");
        assert!(
            !auth.is_initialized(),
            "empty dir should report not initialized"
        );
    }

    #[test]
    fn test_is_initialized_true() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");
        auth.init().expect("init");
        assert!(
            auth.is_initialized(),
            "after init, should report initialized"
        );
    }

    #[test]
    fn test_load_keypair() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");

        let result = auth.init().expect("init");
        let loaded = auth.load_keypair().expect("load keypair");

        // The loaded public key should match the init result's public key.
        let loaded_pk = *loaded.public_key();
        let init_pk = result.public_key.to_public_key();
        assert_eq!(
            loaded_pk, init_pk,
            "loaded public key should match the init public key"
        );
    }

    #[test]
    fn test_load_config() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");

        let result = auth.init().expect("init");
        let config = auth.load_config().expect("load config");

        assert_eq!(config.trust_domain, result.trust_domain);
        assert_ne!(
            config.cluster_id,
            ClusterId(uuid::Uuid::nil()),
            "cluster ID should be non-nil"
        );
        assert_ne!(
            config.node_id,
            NodeId(uuid::Uuid::nil()),
            "node ID should be non-nil"
        );
    }

    #[test]
    fn test_with_state_dir() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let custom = tmp.path().join("custom-state");
        let auth = LocalAuth::with_state_dir(custom.clone()).expect("auth");

        assert!(custom.exists(), "custom state dir should be created");
        assert_eq!(auth.state_dir(), custom);
    }

    #[test]
    fn test_load_keypair_not_found() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");
        assert!(
            auth.load_keypair().is_err(),
            "loading without init should fail"
        );
    }

    #[test]
    fn test_load_config_not_found() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let auth = LocalAuth::with_state_dir(tmp.path().to_path_buf()).expect("auth");
        assert!(
            auth.load_config().is_err(),
            "loading config without init should fail"
        );
    }

    #[test]
    fn test_init_and_load_roundtrip() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let dir = tmp.path().to_path_buf();

        // First instance: init
        let auth1 = LocalAuth::with_state_dir(dir.clone()).expect("auth1");
        let result1 = auth1.init().expect("init");
        let keypair1 = auth1.load_keypair().expect("load keypair 1");
        let pk1 = *keypair1.public_key();

        // Second instance: load from the same dir
        let auth2 = LocalAuth::with_state_dir(dir).expect("auth2");
        let keypair2 = auth2.load_keypair().expect("load keypair 2");
        let pk2 = *keypair2.public_key();

        // Public keys should match
        assert_eq!(pk1, pk2, "public keys should match after roundtrip");
        assert_eq!(
            KeyId::from_public_key(&pk1),
            result1.key_id,
            "key ID should match"
        );
    }
}
