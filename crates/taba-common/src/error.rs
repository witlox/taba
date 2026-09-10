//! Error types for the taba-common crate.
//!
//! [`CommonError`] is the foundation error type, wrapped by most other crate
//! errors. It covers serialization failures, configuration issues, and I/O
//! errors. See `specs/architecture/error-taxonomy.md` for the full taxonomy.

use std::io;

use thiserror::Error;

/// Errors originating from taba-common operations.
///
/// Every variant is **Fatal** — the operation cannot succeed without
/// external intervention (configuration change, code fix, or I/O recovery).
#[derive(Debug, Error)]
pub enum CommonError {
    /// Serialization or deserialization failed.
    ///
    /// Carries a human-readable message describing the failure. The
    /// underlying error type varies by format (JSON, protobuf, etc.) and
    /// is not bound here to avoid leaking format-specific dependencies.
    #[error("serialization error: {message}")]
    Serialization { message: String },

    /// Configuration is invalid or cannot be parsed.
    ///
    /// Carries a `reason` string describing what is wrong and what the
    /// operator should fix.
    #[error("configuration error: {reason}")]
    Config { reason: String },

    /// An I/O error occurred (disk, network file, etc.).
    #[error("I/O error")]
    Io {
        #[source]
        source: io::Error,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn test_serialization_error_display() {
        let err = CommonError::Serialization {
            message: "unexpected EOF".to_string(),
        };
        assert_eq!(err.to_string(), "serialization error: unexpected EOF");
    }

    #[test]
    fn test_config_error_display() {
        let err = CommonError::Config {
            reason: "resilience_pct must be <= 100".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "configuration error: resilience_pct must be <= 100"
        );
    }

    #[test]
    fn test_io_error_display() {
        let err = CommonError::Io {
            source: io::Error::new(io::ErrorKind::NotFound, "file missing"),
        };
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn test_io_error_preserves_source() {
        let source = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let err = CommonError::Io {
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        // The source chain should preserve the original error
        assert!(err.source().is_some());
        let src = err.source().expect("source should be present");
        assert_eq!(src.to_string(), source.to_string());
    }
}
