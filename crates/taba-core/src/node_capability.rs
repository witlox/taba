//! Node capabilities and resource snapshots (INV-N1 through INV-N5).
//!
//! A [`NodeCapabilitySet`] describes what a node *can* do (static,
//! auto-discovered). A [`ResourceSnapshot`] describes what a node
//! *has* available (dynamic, periodically reported). The solver treats
//! capabilities as hard constraints (binary match, INV-N2) and
//! resources as soft constraints (ranking, INV-N3).

use serde::{Deserialize, Serialize};

use taba_common::{AuthorId, ClockQuality, LogicalClock, NodeId, Ppm};

// ---------------------------------------------------------------------------
// Node capability set
// ---------------------------------------------------------------------------

/// Complete capability set for a node.
///
/// Capabilities are auto-discovered at startup and cached locally
/// (INV-N1). Cached capabilities are authoritative until re-probed via
/// `taba refresh` or a fleet-wide `refresh-capabilities` operational
/// command. Custom freeform tags (INV-N4) are treated identically to
/// auto-discovered capabilities for solver matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapabilitySet {
    /// CPU architecture (e.g., `"x86_64"`, `"aarch64"`).
    pub arch: String,
    /// Operating system (e.g., `"linux"`, `"darwin"`).
    pub os: String,
    /// Privilege level of the node.
    pub privilege: PrivilegeLevel,
    /// Runtime capabilities the node can execute (INV-N2).
    pub runtimes: Vec<RuntimeCapability>,
    /// Whether the node can bind privileged ports (< 1024).
    pub ports_privileged: bool,
    /// Available storage backends (e.g., `"ssd"`, `"hdd"`, `"nvme"`).
    pub storage: Vec<String>,
    /// Environment tag (e.g., `"env:dev"`, `"env:prod"`), if any.
    pub environment: Option<String>,
    /// Author affinity: in dev, a node may be bound to a specific
    /// author's workloads (INV-E1). `None` in production.
    pub author_affinity: Option<AuthorId>,
    /// Clock quality reported by the node (INV-N1).
    pub clock_quality: ClockQuality,
    /// IANA timezone string (e.g., `"America/New_York"`).
    pub timezone: String,
    /// Custom freeform tags (key, value) — treated identically to
    /// auto-discovered capabilities for solver matching (INV-N4).
    pub custom_tags: Vec<(String, String)>,
}

/// Privilege level of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrivilegeLevel {
    /// Root / administrator privileges.
    Root,
    /// Unprivileged user.
    User,
}

/// Runtime capability that a node can execute (INV-N2).
///
/// The solver treats these as hard constraints: a workload requiring
#[allow(clippy::doc_markdown)]
/// `Oci` cannot be placed on a node without `Oci` or `OciRootless`.
/// No fallback, no approximation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RuntimeCapability {
    /// Docker/Podman with root daemon.
    Oci,
    /// Rootless Docker/Podman (userspace).
    OciRootless,
    /// Kubernetes API access (schedule pods).
    K8s,
    /// WebAssembly runtime (wasmtime/wasmer).
    Wasm,
    /// Native binary/package execution.
    Native,
    /// MicroVM execution (Firecracker, cloud-hypervisor, QEMU).
    ///
    /// Nodes with this capability can run [`ArtifactType::MicroVm`](crate::ArtifactType::MicroVm)
    /// workloads. The VM monitor binary (firecracker, cloud-hypervisor,
    /// or qemu-system-*) must be installed and accessible.
    MicroVm,
}

// ---------------------------------------------------------------------------
// Resource snapshot
// ---------------------------------------------------------------------------

/// Dynamic resource snapshot reported by a node (INV-N3).
///
/// Resources are periodically reported and used by the solver for
/// soft-constraint ranking (best-fit). Unlike capabilities, resources
/// change constantly and are not used for hard filtering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// The node reporting this snapshot.
    pub node_id: NodeId,
    /// Logical clock at which this snapshot was taken.
    pub logical_clock: LogicalClock,
    /// Total memory in bytes.
    pub memory_total_bytes: u64,
    /// Available memory in bytes.
    pub memory_available_bytes: u64,
    /// Number of CPU cores.
    pub cpu_cores: u32,
    /// CPU load as parts-per-million (0 = idle, `1_000_000` = fully loaded).
    pub cpu_load_ppm: Ppm,
    /// Available disk space in bytes.
    pub disk_available_bytes: u64,
    /// Number of available GPUs.
    pub gpu_available: u32,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_capability_set_serialization_roundtrip() {
        let caps = NodeCapabilitySet {
            arch: "x86_64".to_string(),
            os: "linux".to_string(),
            privilege: PrivilegeLevel::Root,
            runtimes: vec![RuntimeCapability::Oci, RuntimeCapability::Wasm],
            ports_privileged: true,
            storage: vec!["ssd".to_string(), "nvme".to_string()],
            environment: Some("env:prod".to_string()),
            author_affinity: None,
            clock_quality: ClockQuality::Ntp,
            timezone: "UTC".to_string(),
            custom_tags: vec![("gpu".to_string(), "a100".to_string())],
        };

        let json = serde_json::to_string(&caps).expect("serialize NodeCapabilitySet");
        let decoded: NodeCapabilitySet =
            serde_json::from_str(&json).expect("deserialize NodeCapabilitySet");
        assert_eq!(caps, decoded);
    }

    #[test]
    fn test_node_capability_set_dev_with_author_affinity() {
        let caps = NodeCapabilitySet {
            arch: "aarch64".to_string(),
            os: "darwin".to_string(),
            privilege: PrivilegeLevel::User,
            runtimes: vec![RuntimeCapability::Native],
            ports_privileged: false,
            storage: vec!["ssd".to_string()],
            environment: Some("env:dev".to_string()),
            author_affinity: Some(AuthorId(uuid::Uuid::new_v4())),
            clock_quality: ClockQuality::Unsync,
            timezone: "America/New_York".to_string(),
            custom_tags: Vec::new(),
        };

        assert_eq!(caps.privilege, PrivilegeLevel::User);
        assert!(caps.author_affinity.is_some());
        assert_eq!(caps.environment.as_deref(), Some("env:dev"));
    }

    #[test]
    fn test_resource_snapshot_serialization_roundtrip() {
        let snapshot = ResourceSnapshot {
            node_id: NodeId(uuid::Uuid::new_v4()),
            logical_clock: LogicalClock(42),
            memory_total_bytes: 16 * 1024 * 1024 * 1024, // 16 GiB
            memory_available_bytes: 8 * 1024 * 1024 * 1024, // 8 GiB
            cpu_cores: 8,
            cpu_load_ppm: Ppm(250_000),                     // 25%
            disk_available_bytes: 100 * 1024 * 1024 * 1024, // 100 GiB
            gpu_available: 2,
        };

        let json = serde_json::to_string(&snapshot).expect("serialize ResourceSnapshot");
        let decoded: ResourceSnapshot =
            serde_json::from_str(&json).expect("deserialize ResourceSnapshot");
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn test_privilege_level_variants() {
        assert_ne!(PrivilegeLevel::Root, PrivilegeLevel::User);

        let json = serde_json::to_string(&PrivilegeLevel::Root).expect("serialize");
        let decoded: PrivilegeLevel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(PrivilegeLevel::Root, decoded);
    }

    #[test]
    fn test_runtime_capability_all_variants() {
        let caps = [
            RuntimeCapability::Oci,
            RuntimeCapability::OciRootless,
            RuntimeCapability::K8s,
            RuntimeCapability::Wasm,
            RuntimeCapability::Native,
        ];

        for cap in caps {
            let json = serde_json::to_string(&cap).expect("serialize RuntimeCapability");
            let decoded: RuntimeCapability =
                serde_json::from_str(&json).expect("deserialize RuntimeCapability");
            assert_eq!(cap, decoded);
        }
    }
}
