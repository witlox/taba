//! Minimal K8s resource types for deserialization.
//!
//! These are hand-written structs covering only the fields taba
//! needs. Full Kubernetes API types are not required — the converter
//! only reads the fields that map to taba concepts.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Common K8s objects
// ---------------------------------------------------------------------------

/// A K8s manifest (can be any resource).
#[derive(Debug, Clone, Deserialize)]
pub struct K8sManifest {
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    pub kind: String,
    pub metadata: ObjectMeta,
    #[serde(default)]
    pub spec: serde_yaml::Value,
    /// Top-level data (`ConfigMap`, `Secret`).
    #[serde(default)]
    pub data: serde_yaml::Value,
}

/// K8s object metadata.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ObjectMeta {
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub annotations: std::collections::BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Workload specs (Deployment, StatefulSet, DaemonSet)
// ---------------------------------------------------------------------------

/// Pod template spec (shared by Deployment, `StatefulSet`, `DaemonSet`).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PodTemplateSpec {
    #[serde(default)]
    pub spec: PodSpec,
}

/// Pod spec.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PodSpec {
    #[serde(default)]
    pub containers: Vec<Container>,
    #[serde(default)]
    #[serde(rename = "initContainers")]
    pub init_containers: Vec<Container>,
    #[serde(default)]
    pub volumes: Vec<Volume>,
    #[serde(default)]
    #[serde(rename = "nodeSelector")]
    pub node_selector: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    #[serde(rename = "restartPolicy")]
    pub restart_policy: Option<String>,
}

/// A container (the taba workload equivalent).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Container {
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub ports: Vec<ContainerPort>,
    #[serde(default)]
    pub env: Vec<EnvVar>,
    #[serde(default)]
    pub resources: ResourceRequirements,
    #[serde(default)]
    #[serde(rename = "volumeMounts")]
    pub volume_mounts: Vec<VolumeMount>,
    #[serde(default)]
    #[serde(rename = "livenessProbe")]
    pub liveness_probe: Option<Probe>,
    #[serde(default)]
    #[serde(rename = "readinessProbe")]
    pub readiness_probe: Option<Probe>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub command: Vec<String>,
}

/// Container port.
#[derive(Debug, Clone, Deserialize)]
pub struct ContainerPort {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "containerPort")]
    pub container_port: u16,
    #[serde(default)]
    pub protocol: Option<String>,
}

/// Environment variable.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvVar {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Resource requirements.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ResourceRequirements {
    #[serde(default)]
    pub requests: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub limits: std::collections::BTreeMap<String, String>,
}

/// Volume mount.
#[derive(Debug, Clone, Deserialize)]
pub struct VolumeMount {
    pub name: String,
    pub mount_path: String,
    #[serde(default)]
    pub read_only: bool,
}

/// Volume.
#[derive(Debug, Clone, Deserialize)]
pub struct Volume {
    pub name: String,
    #[serde(default, rename = "configMap")]
    pub config_map: Option<ConfigMapVolumeSource>,
    #[serde(default)]
    pub secret: Option<SecretVolumeSource>,
    #[serde(default, rename = "emptyDir")]
    pub empty_dir: Option<serde_yaml::Value>,
    #[serde(default, rename = "persistentVolumeClaim")]
    pub persistent_volume_claim: Option<PvcVolumeSource>,
}

/// `ConfigMap` volume source.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigMapVolumeSource {
    pub name: String,
}

/// Secret volume source.
#[derive(Debug, Clone, Deserialize)]
pub struct SecretVolumeSource {
    #[serde(rename = "secretName")]
    pub secret_name: String,
}

/// PVC volume source.
#[derive(Debug, Clone, Deserialize)]
pub struct PvcVolumeSource {
    #[serde(rename = "claimName")]
    pub claim_name: String,
}

// ---------------------------------------------------------------------------
// Health checks
// ---------------------------------------------------------------------------

/// K8s probe (liveness or readiness).
#[derive(Debug, Clone, Deserialize)]
pub struct Probe {
    #[serde(default, rename = "httpGet")]
    pub http_get: Option<HttpGetProbe>,
    #[serde(default, rename = "tcpSocket")]
    pub tcp_socket: Option<TcpSocketProbe>,
    #[serde(default)]
    pub exec: Option<ExecProbe>,
    #[serde(default, rename = "initialDelaySeconds")]
    pub initial_delay_seconds: Option<u32>,
    #[serde(default, rename = "periodSeconds")]
    pub period_seconds: Option<u32>,
    #[serde(default, rename = "timeoutSeconds")]
    pub timeout_seconds: Option<u32>,
}

/// HTTP GET probe.
#[derive(Debug, Clone, Deserialize)]
pub struct HttpGetProbe {
    #[serde(default)]
    pub path: Option<String>,
    pub port: serde_yaml::Value,
}

/// TCP socket probe.
#[derive(Debug, Clone, Deserialize)]
pub struct TcpSocketProbe {
    pub port: serde_yaml::Value,
}

/// Exec probe.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecProbe {
    pub command: Vec<String>,
}

// ---------------------------------------------------------------------------
// Deployment / StatefulSet / DaemonSet spec
// ---------------------------------------------------------------------------

/// Deployment spec.
#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentSpec {
    #[serde(default)]
    pub replicas: Option<u32>,
    #[serde(default)]
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
}

/// `StatefulSet` spec.
#[derive(Debug, Clone, Deserialize)]
pub struct StatefulSetSpec {
    #[serde(default)]
    pub replicas: Option<u32>,
    #[serde(default)]
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
    #[serde(default)]
    #[serde(rename = "volumeClaimTemplates")]
    pub volume_claim_templates: Vec<PvcSpec>,
}

/// `DaemonSet` spec.
#[derive(Debug, Clone, Deserialize)]
pub struct DaemonSetSpec {
    #[serde(default)]
    pub selector: LabelSelector,
    pub template: PodTemplateSpec,
}

/// Label selector.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LabelSelector {
    #[serde(default)]
    #[serde(rename = "matchLabels")]
    pub match_labels: std::collections::BTreeMap<String, String>,
}

/// PVC spec.
#[derive(Debug, Clone, Deserialize)]
pub struct PvcSpec {
    pub metadata: ObjectMeta,
    pub spec: PvcSpecBody,
}

/// PVC spec body.
#[derive(Debug, Clone, Deserialize)]
pub struct PvcSpecBody {
    #[serde(default)]
    #[serde(rename = "accessModes")]
    pub access_modes: Vec<String>,
    #[serde(default)]
    pub resources: ResourceRequirements,
    #[serde(default)]
    #[serde(rename = "storageClassName")]
    pub storage_class_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Service spec
// ---------------------------------------------------------------------------

/// Service spec.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceSpec {
    #[serde(default, rename = "type")]
    pub type_: Option<String>,
    #[serde(default)]
    pub ports: Vec<ServicePort>,
    #[serde(default)]
    pub selector: std::collections::BTreeMap<String, String>,
}

/// Service port.
#[derive(Debug, Clone, Deserialize)]
pub struct ServicePort {
    #[serde(default)]
    pub name: Option<String>,
    pub port: u16,
    #[serde(default)]
    #[serde(rename = "targetPort")]
    pub target_port: Option<serde_yaml::Value>,
    #[serde(default)]
    pub protocol: Option<String>,
}

// ---------------------------------------------------------------------------
// ConfigMap / Secret
// ---------------------------------------------------------------------------

/// `ConfigMap` data.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ConfigMapData {
    #[serde(default)]
    pub data: std::collections::BTreeMap<String, String>,
}

/// Secret data.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SecretData {
    #[serde(default, rename = "type")]
    pub type_: Option<String>,
    #[serde(default)]
    pub data: std::collections::BTreeMap<String, String>,
}
