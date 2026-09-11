//! Errors produced by CLI operations.

use thiserror::Error;

/// Errors produced by CLI operations.
#[derive(Debug, Error)]
pub enum CliError {
    /// Failed to connect to the taba-node daemon.
    #[error("connection failed: {reason}")]
    ConnectionFailed {
        /// Why the connection failed.
        reason: String,
    },

    /// User input is invalid (malformed TOML, missing fields, etc.).
    #[error("invalid input: {reason}")]
    InvalidInput {
        /// Why the input is invalid.
        reason: String,
    },

    /// Caller is not authorized to perform this operation.
    #[error("unauthorized: {reason}")]
    Unauthorized {
        /// Why the operation was denied.
        reason: String,
    },

    /// The server (local or remote) returned an error.
    #[error("server error: {reason}")]
    ServerError {
        /// The error from the server.
        reason: String,
    },

    /// I/O error (file not found, permission denied, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error.
    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// JSON serialization error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Security error from taba-security.
    #[error("security error: {0}")]
    Security(#[from] taba_security::SecurityError),

    /// Graph error from taba-graph.
    #[error("graph error: {0}")]
    Graph(#[from] taba_graph::GraphError),

    /// Core error from taba-core.
    #[error("core error: {0}")]
    Core(#[from] taba_core::CoreError),

    /// Observe error from taba-observe.
    #[error("observe error: {0}")]
    Observe(#[from] taba_observe::ObserveError),
}
