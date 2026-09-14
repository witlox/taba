//! K8s manifest reader that generates taba unit declarations.
//!
//! This crate reads Kubernetes manifests (YAML) and converts them
//! into taba unit declarations (TOML). It surfaces constructs that
//! cannot be mapped, allowing operators to decide how to handle them.
//!
//! ## Supported K8s resources
//!
//! | K8s Resource | taba Unit | Notes |
//! |--------------|-----------|-------|
//! | Deployment | `WorkloadUnit` (service) | Container image → artifact |
//! | `StatefulSet` | `WorkloadUnit` (service) | + state recovery |
//! | `DaemonSet` | `WorkloadUnit` (service) | + one-per-node constraint |
//! | Service | `WorkloadUnit` provides | Network capability |
//! | `ConfigMap` | `DataUnit` | Configuration data |
//! | Secret | `DataUnit` | classification=confidential |
//! | Pod | `WorkloadUnit` | Direct pod (no controller) |
//!
//! ## Unsupported (surfaced as conflicts)
//!
//! - CRDs (Custom Resource Definitions) — unbounded, no taba equivalent
//! - Jobs/CronJobs — mapped to bounded tasks if possible
//! - `HorizontalPodAutoscaler` — mapped to scaling triggers if possible
//! - Ingress — surfaced as unmappable (taba has no ingress concept)
//! - PersistentVolume/PersistentVolumeClaim — surfaced as data dependency

pub mod converter;
pub mod error;
pub mod k8s_types;
pub mod report;

pub use converter::{ConvertResult, K8sConverter};
pub use error::K8sConvertError;
pub use report::{ConversionReport, UnmappableResource};
