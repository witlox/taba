//! Artifact packaging model for workload units.
//!
//! An [`Artifact`] describes *what* a workload runs — its packaging format,
//! content reference, and digest. The solver matches the artifact's
//! [`ArtifactType`] against node [`RuntimeCapability`](crate::node_capability::RuntimeCapability)
//! to determine placement eligibility (INV-N2).

use serde::{Deserialize, Serialize};

use taba_common::ContentDigest;

// ---------------------------------------------------------------------------
// Artifact
// ---------------------------------------------------------------------------

/// A workload.s packaged executable and integrity metadata.
#[allow(clippy::doc_markdown)]
///
/// Runtime-agnostic: the solver matches [`ArtifactType`](crate::ArtifactType)
/// to node runtime capabilities (INV-N2). The digest enables
/// content-addressed peer-cache distribution and mandatory post-fetch
/// verification (INV-A1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Type of artifact (determines which runtime capability is needed).
    pub artifact_type: ArtifactType,
    /// Content reference (OCI image tag, binary URL, file path, etc.).
    pub artifact_ref: String,
    /// SHA-256 content hash for integrity and deduplication (INV-A1).
    pub digest: ContentDigest,
    /// Additional runtime requirements (e.g., `["windows", "dotnet-4.8"]`).
    pub requires: Vec<String>,
    /// Path to the kernel image for MicroVm workloads (INV-N6).
    ///
    /// `None` for Oci, Native, Wasm, and K8sManifest artifacts.
    /// `Some("path/to/vmlinux")` for MicroVm artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel_ref: Option<String>,
    /// Path to the root filesystem image for MicroVm workloads (INV-N6).
    ///
    /// `None` for Oci, Native, Wasm, and K8sManifest artifacts.
    /// `Some("path/to/rootfs.ext4")` for MicroVm artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rootfs_ref: Option<String>,
}

/// Artifact type — matched against node runtime capabilities by the solver.
///
/// Each variant maps to one or more [`RuntimeCapability`](crate::node_capability::RuntimeCapability)
/// values. The mapping is many-to-one: e.g., both `Oci` and `OciRootless`
/// can run an `Oci` artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ArtifactType {
    /// OCI container image (Docker, Podman).
    Oci,
    /// Native binary or package installer (MSI, RPM, DEB).
    Native,
    /// WebAssembly module.
    Wasm,
    /// Kubernetes manifest (pod spec).
    K8sManifest,
    /// [`MicroVm`] (Firecracker, cloud-hypervisor, QEMU).
    ///
    /// Requires `kernel_ref` and `rootfs_ref` on the [`Artifact`]
    /// (INV-N6). The solver matches this to
    /// [`RuntimeCapability::MicroVm`](crate::node_capability::RuntimeCapability::MicroVm).
    MicroVm,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_serialization_roundtrip() {
        let artifact = Artifact {
            artifact_type: ArtifactType::Oci,
            artifact_ref: "registry.example.com/app:v1".to_string(),
            digest: ContentDigest("sha256:abc123".to_string()),
            requires: vec!["linux".to_string(), "glibc-2.31".to_string()],
            kernel_ref: None,
            rootfs_ref: None,
        };

        let json = serde_json::to_string(&artifact).expect("serialize Artifact");
        let decoded: Artifact = serde_json::from_str(&json).expect("deserialize Artifact");
        assert_eq!(artifact, decoded);
    }

    #[test]
    fn test_artifact_type_all_variants() {
        let variants = [
            ArtifactType::Oci,
            ArtifactType::Native,
            ArtifactType::Wasm,
            ArtifactType::K8sManifest,
            ArtifactType::MicroVm,
        ];

        for variant in variants {
            let json = serde_json::to_string(&variant).expect("serialize ArtifactType");
            let decoded: ArtifactType =
                serde_json::from_str(&json).expect("deserialize ArtifactType");
            assert_eq!(variant, decoded);
        }
    }

    #[test]
    fn test_artifact_native_with_no_requires() {
        let artifact = Artifact {
            artifact_type: ArtifactType::Native,
            artifact_ref: "/usr/local/bin/daemon".to_string(),
            digest: ContentDigest("sha256:deadbeef".to_string()),
            requires: Vec::new(),
            kernel_ref: None,
            rootfs_ref: None,
        };

        assert!(artifact.requires.is_empty());
        assert_eq!(artifact.artifact_type, ArtifactType::Native);
    }

    #[test]
    fn test_artifact_microvm_with_kernel_rootfs() {
        let artifact = Artifact {
            artifact_type: ArtifactType::MicroVm,
            artifact_ref: "vmlinux-5.10".to_string(),
            digest: ContentDigest("sha256:kernel123".to_string()),
            requires: Vec::new(),
            kernel_ref: Some("/opt/vmlinux".to_string()),
            rootfs_ref: Some("/opt/rootfs.ext4".to_string()),
        };

        let json = serde_json::to_string(&artifact).expect("serialize MicroVm Artifact");
        let decoded: Artifact = serde_json::from_str(&json).expect("deserialize MicroVm Artifact");
        assert_eq!(artifact, decoded);
        assert!(decoded.kernel_ref.is_some());
        assert!(decoded.rootfs_ref.is_some());
    }
}
