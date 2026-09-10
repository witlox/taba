//! Auto-discovery of node capabilities (INV-N1).
//!
//! The [`CapabilityDiscoverer`] trait probes the system for available
//! runtimes, hardware, OS features, and reports a capability set.
//! Capabilities are cached locally and re-probed on `taba refresh`
//! or fleet-wide refresh command.

use std::sync::Mutex;

use taba_core::{NodeCapabilitySet, PrivilegeLevel, RuntimeCapability};

use crate::error::NodeError;

// ---------------------------------------------------------------------------
// CapabilityDiscoverer trait
// ---------------------------------------------------------------------------

/// Auto-discovers node capabilities on startup (INV-N1).
///
/// Probes the system for available runtimes, hardware, OS features,
/// and reports a capability set. Cached locally. Re-probed on
/// `taba refresh` or fleet-wide refresh command.
pub trait CapabilityDiscoverer: Send + Sync {
    /// Probe the system and return discovered capabilities.
    ///
    /// Probes: Docker/Podman socket, K8s API, wasmtime/wasmer,
    /// GPU (nvidia-smi), TPM, OS/arch, privilege level.
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if a critical probe fails
    ///   (e.g., OS/arch cannot be determined).
    async fn discover(&self) -> Result<NodeCapabilitySet, NodeError>;

    /// Re-probe capabilities (invalidate cache).
    ///
    /// # Errors
    ///
    /// - [`NodeError::ReconciliationFailed`] if a critical probe fails.
    async fn refresh(&self) -> Result<NodeCapabilitySet, NodeError>;
}

// ---------------------------------------------------------------------------
// DefaultCapabilityDiscoverer
// ---------------------------------------------------------------------------

/// Default implementation of [`CapabilityDiscoverer`].
///
/// Probes the system for OS, architecture, privilege level, and
/// available runtimes. For M3:
/// - Docker: probes the Docker socket via `bollard::Docker::connect_with_defaults`
/// - OS/arch: uses `std::env::consts`
/// - Privilege: checks if running as root (UID 0 on Unix)
/// - GPU, TPM, K8s: stubbed with comments (future phases)
///
/// Thread-safe via a [`Mutex`] cache.
#[derive(Debug)]
pub struct DefaultCapabilityDiscoverer {
    /// Cached capability set.
    cached: Mutex<Option<NodeCapabilitySet>>,
}

impl Default for DefaultCapabilityDiscoverer {
    fn default() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }
}

impl DefaultCapabilityDiscoverer {
    /// Creates a new `DefaultCapabilityDiscoverer` with no cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Probes the system for capabilities and returns a fresh
    /// [`NodeCapabilitySet`].
    ///
    /// This is the actual probe logic — `discover()` caches the
    /// result, `refresh()` calls this and re-caches.
    fn probe() -> NodeCapabilitySet {
        let os = std::env::consts::OS.to_string();
        let arch = std::env::consts::ARCH.to_string();

        // Detect privilege level.
        // On Unix, check if running as root (UID 0).
        #[cfg(unix)]
        let privilege = if is_root() {
            PrivilegeLevel::Root
        } else {
            PrivilegeLevel::User
        };
        #[cfg(not(unix))]
        let privilege = PrivilegeLevel::User;

        // Probe Docker socket.
        let docker_available = bollard::Docker::connect_with_defaults().is_ok();

        // Build runtime capabilities.
        let mut runtimes = Vec::new();
        if docker_available {
            runtimes.push(RuntimeCapability::Oci);
        }
        // K8s: stubbed — probe kubeconfig in a future phase.
        // Wasm: stubbed — probe wasmtime in a future phase.
        // Native is always available.
        runtimes.push(RuntimeCapability::Native);

        NodeCapabilitySet {
            arch,
            os,
            privilege,
            runtimes,
            ports_privileged: privilege == PrivilegeLevel::Root,
            storage: vec!["local".to_string()],
            environment: None,
            author_affinity: None,
            clock_quality: taba_common::ClockQuality::Ntp,
            timezone: local_timezone(),
            custom_tags: Vec::new(),
        }
    }
}

/// Returns the local timezone string, or `"UTC"` if it cannot be
/// determined.
fn local_timezone() -> String {
    // For M3, we just return UTC. A real implementation would
    // check the TZ environment variable or /etc/timezone.
    std::env::var("TZ").unwrap_or_else(|_| "UTC".to_string())
}

/// Detects whether the current process is running as root (UID 0).
///
/// On Linux, reads `/proc/self/status` and checks the effective UID
/// from the `Uid:` line. On other Unix systems (or if `/proc` is
/// unavailable), falls back to checking the `USER` environment variable.
fn is_root() -> bool {
    // Try /proc/self/status first (Linux). The `Uid:` line has four
    // whitespace-separated fields: real, effective, saved, fsuid.
    // We check the effective UID (second field) for zero.
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(effective) = rest.split_whitespace().nth(1) {
                    return effective == "0";
                }
            }
        }
    }

    // Fallback: check the USER environment variable.
    std::env::var("USER").is_ok_and(|u| u == "root")
}

impl CapabilityDiscoverer for DefaultCapabilityDiscoverer {
    async fn discover(&self) -> Result<NodeCapabilitySet, NodeError> {
        let mut cached = self
            .cached
            .lock()
            .expect("capability cache mutex should not be poisoned");

        if let Some(ref caps) = *cached {
            return Ok(caps.clone());
        }

        let caps = Self::probe();
        *cached = Some(caps.clone());
        Ok(caps)
    }

    async fn refresh(&self) -> Result<NodeCapabilitySet, NodeError> {
        let mut cached = self
            .cached
            .lock()
            .expect("capability cache mutex should not be poisoned");

        // Invalidate cache and re-probe.
        *cached = None;
        let caps = Self::probe();
        *cached = Some(caps.clone());
        Ok(caps)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover_returns_capability_set() {
        let discoverer = DefaultCapabilityDiscoverer::new();
        let caps = discoverer
            .discover()
            .await
            .expect("discover should succeed");

        // OS and arch should be non-empty.
        assert!(!caps.os.is_empty(), "OS should be detected");
        assert!(!caps.arch.is_empty(), "arch should be detected");

        // Should have at least Native runtime.
        assert!(
            caps.runtimes.contains(&RuntimeCapability::Native),
            "Native runtime should always be available"
        );

        // Timezone should be set.
        assert!(!caps.timezone.is_empty());
    }

    #[tokio::test]
    async fn test_discover_os_arch() {
        let discoverer = DefaultCapabilityDiscoverer::new();
        let caps = discoverer
            .discover()
            .await
            .expect("discover should succeed");

        // Verify the detected OS and arch match std::env::consts.
        assert_eq!(caps.os, std::env::consts::OS);
        assert_eq!(caps.arch, std::env::consts::ARCH);
    }

    #[tokio::test]
    async fn test_discover_privilege() {
        let discoverer = DefaultCapabilityDiscoverer::new();
        let caps = discoverer
            .discover()
            .await
            .expect("discover should succeed");

        // Privilege should be Root or User.
        assert!(
            caps.privilege == PrivilegeLevel::Root || caps.privilege == PrivilegeLevel::User,
            "privilege should be Root or User, got {:?}",
            caps.privilege
        );

        // ports_privileged should match privilege.
        assert_eq!(
            caps.ports_privileged,
            caps.privilege == PrivilegeLevel::Root
        );
    }

    #[tokio::test]
    async fn test_refresh_clears_cache() {
        let discoverer = DefaultCapabilityDiscoverer::new();

        // First discover.
        let caps1 = discoverer.discover().await.expect("first discover");

        // Second discover should return cached (same instance).
        let caps2 = discoverer
            .discover()
            .await
            .expect("second discover (cached)");

        // Both should have the same OS and arch.
        assert_eq!(caps1.os, caps2.os);
        assert_eq!(caps1.arch, caps2.arch);

        // Refresh should re-probe.
        let caps3 = discoverer.refresh().await.expect("refresh");

        // Should still have the same OS and arch (re-probed).
        assert_eq!(caps3.os, caps1.os);
        assert_eq!(caps3.arch, caps1.arch);
    }
}
