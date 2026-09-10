//! Health check declarations for progressive workload monitoring.
//!
//! If a workload unit declares no health check, the node uses OS-level
//! process monitoring (is the process alive?). If a [`HealthCheck`] is
//! declared, the node uses it. The node never skips health monitoring —
//! default is always active (INV-O3, progressive disclosure).

use std::time::Duration;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Health check
// ---------------------------------------------------------------------------

/// Health check declaration for progressive monitoring (INV-O3).
///
/// Default (`None` on the workload): OS-level process monitoring.
/// Declared: explicit check of the given [`HealthCheckType`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Type of health check.
    pub check_type: HealthCheckType,
    /// How often to run the check.
    pub interval: Duration,
    /// Maximum time to wait for a response.
    pub timeout: Duration,
}

/// Type of health check (progressive disclosure, INV-O3).
///
/// Progression: OS-level process monitoring (default, no declaration) →
/// HTTP probe → TCP probe → custom command. Each variant carries its
/// configuration inline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HealthCheckType {
    /// HTTP GET to a path. 2xx = healthy.
    Http {
        /// Path to GET (e.g., `/healthz`).
        path: String,
        /// Port to connect to.
        port: u16,
    },
    /// TCP connect to a port. Success = healthy.
    Tcp {
        /// Port to connect to.
        port: u16,
    },
    /// Execute a command. Exit 0 = healthy.
    Command {
        /// Command to execute.
        command: String,
    },
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_http_serialization_roundtrip() {
        let check = HealthCheck {
            check_type: HealthCheckType::Http {
                path: "/healthz".to_string(),
                port: 8080,
            },
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(2),
        };

        let json = serde_json::to_string(&check).expect("serialize HealthCheck");
        let decoded: HealthCheck = serde_json::from_str(&json).expect("deserialize HealthCheck");
        assert_eq!(check, decoded);
    }

    #[test]
    fn test_health_check_tcp_serialization_roundtrip() {
        let check = HealthCheck {
            check_type: HealthCheckType::Tcp { port: 5432 },
            interval: Duration::from_secs(10),
            timeout: Duration::from_millis(500),
        };

        let json = serde_json::to_string(&check).expect("serialize HealthCheck");
        let decoded: HealthCheck = serde_json::from_str(&json).expect("deserialize HealthCheck");
        assert_eq!(check, decoded);
    }

    #[test]
    fn test_health_check_command_serialization_roundtrip() {
        let check = HealthCheck {
            check_type: HealthCheckType::Command {
                command: "/usr/local/bin/check --alive".to_string(),
            },
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
        };

        let json = serde_json::to_string(&check).expect("serialize HealthCheck");
        let decoded: HealthCheck = serde_json::from_str(&json).expect("deserialize HealthCheck");
        assert_eq!(check, decoded);
    }

    #[test]
    fn test_health_check_type_equality() {
        assert_eq!(
            HealthCheckType::Tcp { port: 80 },
            HealthCheckType::Tcp { port: 80 }
        );
        assert_ne!(
            HealthCheckType::Tcp { port: 80 },
            HealthCheckType::Tcp { port: 81 }
        );
        assert_ne!(
            HealthCheckType::Tcp { port: 80 },
            HealthCheckType::Http {
                path: "/".to_string(),
                port: 80
            }
        );
    }
}
