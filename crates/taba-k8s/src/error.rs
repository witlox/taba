//! Errors produced by the K8s-to-taba converter.

use thiserror::Error;

/// Errors produced during K8s manifest conversion.
#[derive(Debug, Error)]
pub enum K8sConvertError {
    /// YAML parsing failed.
    #[error("YAML parse error: {0}")]
    YamlParse(String),

    /// TOML serialization failed.
    #[error("TOML serialize error: {0}")]
    TomlSerialize(String),

    /// K8s resource is not supported.
    #[error("unsupported K8s resource: {kind}")]
    UnsupportedResource {
        /// The K8s resource kind (e.g., "Ingress", "`CustomResourceDefinition`").
        kind: String,
    },

    /// K8s resource is missing required fields.
    #[error("missing required field '{field}' in {kind}")]
    MissingField {
        /// The missing field name.
        field: String,
        /// The K8s resource kind.
        kind: String,
    },

    /// Container image is missing.
    #[error("container image is missing in {container}")]
    MissingImage {
        /// The container name.
        container: String,
    },

    /// I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
