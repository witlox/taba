//! Health check orchestration for workloads (INV-O3, progressive).
//!
//! The [`HealthCheckOrchestrator`] trait manages health checks for
//! running workloads. Health checks are progressive:
//!
//! 1. OS-level process monitoring (default, no declaration)
//! 2. HTTP probe (declared via [`HealthCheckType::Http`])
//! 3. TCP connect (declared via [`HealthCheckType::Tcp`])
//! 4. Custom command (declared via [`HealthCheckType::Command`])
//!
//! For M3: HTTP and TCP checks are implemented using `tokio::net`.
//! OS-level and command checks are simulated.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use taba_common::UnitId;
use taba_core::{HealthCheck, HealthCheckType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::NodeError;

// ---------------------------------------------------------------------------
// HealthCheckResult
// ---------------------------------------------------------------------------

/// Result of a single health check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthCheckResult {
    /// The unit that was checked.
    pub unit: UnitId,
    /// Whether the unit is healthy.
    pub healthy: bool,
    /// Human-readable description of the result.
    pub message: String,
}

// ---------------------------------------------------------------------------
// HealthCheckOrchestrator trait
// ---------------------------------------------------------------------------

/// Orchestrates health checks for workloads (INV-O3, progressive).
///
/// Default: OS-level process monitoring (is process alive?).
/// Declared: HTTP probe, TCP connect, or custom command.
pub trait HealthCheckOrchestrator: Send + Sync {
    /// Register a health check for a running workload.
    fn register(&self, unit_id: &UnitId, check: &HealthCheck);

    /// Unregister a health check (unit terminated).
    fn unregister(&self, unit_id: &UnitId);

    /// Run all registered health checks.
    ///
    /// Returns results for each check. Failed checks are reported
    /// to the graph as health status events.
    async fn run_checks(&self) -> Result<Vec<(UnitId, HealthCheckResult)>, NodeError>;
}

// ---------------------------------------------------------------------------
// DefaultHealthCheckOrchestrator
// ---------------------------------------------------------------------------

/// Default implementation of [`HealthCheckOrchestrator`].
///
/// Stores health checks in a [`HashMap`] behind a [`Mutex`]. For M3:
/// - HTTP checks: connect to `127.0.0.1:port` and send a GET request.
///   2xx response = healthy.
/// - TCP checks: connect to `127.0.0.1:port`. Success = healthy.
/// - Command checks: simulated (always returns healthy for M3).
/// - OS-level (no check registered): simulated (always returns healthy).
///
/// Thread-safe via interior mutability.
#[derive(Debug, Default)]
pub struct DefaultHealthCheckOrchestrator {
    checks: Mutex<HashMap<UnitId, HealthCheck>>,
}

impl DefaultHealthCheckOrchestrator {
    /// Creates a new empty `DefaultHealthCheckOrchestrator`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs an HTTP health check against `127.0.0.1:port/path`.
    ///
    /// 2xx response = healthy. Non-2xx or connection failure =
    /// unhealthy.
    async fn run_http_check(
        &self,
        unit_id: UnitId,
        path: &str,
        port: u16,
        timeout: Duration,
    ) -> HealthCheckResult {
        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .unwrap_or_else(|_| ([127, 0, 0, 1], port).into());

        let connect_result =
            tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await;

        match connect_result {
            Err(_) => HealthCheckResult {
                unit: unit_id,
                healthy: false,
                message: "HTTP health check timed out".to_string(),
            },
            Ok(Err(e)) => HealthCheckResult {
                unit: unit_id,
                healthy: false,
                message: format!("HTTP health check connection failed: {e}"),
            },
            Ok(Ok(mut stream)) => {
                // Send a minimal HTTP GET request.
                let request =
                    format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
                if stream.write_all(request.as_bytes()).await.is_err() {
                    return HealthCheckResult {
                        unit: unit_id,
                        healthy: false,
                        message: "HTTP health check: failed to send request".to_string(),
                    };
                }

                // Read response.
                let mut response = Vec::new();
                let _ = stream.read_to_end(&mut response).await;

                // Parse status line: "HTTP/1.0 200 OK"
                let response_str = String::from_utf8_lossy(&response);
                let status_code = response_str
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .and_then(|code| code.parse::<u16>().ok());

                match status_code {
                    Some(code) if (200..300).contains(&code) => HealthCheckResult {
                        unit: unit_id,
                        healthy: true,
                        message: format!("HTTP health check: {code} OK"),
                    },
                    Some(code) => HealthCheckResult {
                        unit: unit_id,
                        healthy: false,
                        message: format!("HTTP health check: non-2xx status {code}"),
                    },
                    None => HealthCheckResult {
                        unit: unit_id,
                        healthy: false,
                        message: "HTTP health check: failed to parse status".to_string(),
                    },
                }
            }
        }
    }

    /// Runs a TCP health check against `127.0.0.1:port`.
    ///
    /// Successful connection = healthy. Connection failure = unhealthy.
    async fn run_tcp_check(
        &self,
        unit_id: UnitId,
        port: u16,
        timeout: Duration,
    ) -> HealthCheckResult {
        let addr: SocketAddr = format!("127.0.0.1:{port}")
            .parse()
            .unwrap_or_else(|_| ([127, 0, 0, 1], port).into());

        let connect_result =
            tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await;

        match connect_result {
            Err(_) => HealthCheckResult {
                unit: unit_id,
                healthy: false,
                message: "TCP health check timed out".to_string(),
            },
            Ok(Err(e)) => HealthCheckResult {
                unit: unit_id,
                healthy: false,
                message: format!("TCP health check connection failed: {e}"),
            },
            Ok(Ok(_)) => HealthCheckResult {
                unit: unit_id,
                healthy: true,
                message: "TCP health check: connection succeeded".to_string(),
            },
        }
    }

    /// Runs a command health check (simulated for M3).
    ///
    /// For M3, this always returns healthy. A real implementation
    /// would execute the command and check for exit code 0.
    #[allow(clippy::unused_self)] // self will be used for real command execution in future phases
    fn run_command_check(&self, unit_id: UnitId, command: &str) -> HealthCheckResult {
        // M3: simulated. In a real implementation, this would:
        // 1. Spawn the command
        // 2. Wait for completion
        // 3. Check exit code (0 = healthy)
        HealthCheckResult {
            unit: unit_id,
            healthy: true,
            message: format!("Command health check (simulated): {command}"),
        }
    }

    /// Runs the OS-level process check (simulated for M3).
    ///
    /// For M3, this always returns healthy. A real implementation
    /// would check if the process is still alive.
    ///
    /// This is the default health check tier (INV-O3): when a unit has
    /// no declared check, OS-level process monitoring applies. The
    /// function is prepared for future wiring into `run_checks` once
    /// the reconciliation loop tracks all running units, not just
    /// those with registered checks.
    #[allow(dead_code)]
    #[allow(clippy::unused_self)] // self will be used for process monitoring in future phases
    fn run_os_level_check(&self, unit_id: UnitId) -> HealthCheckResult {
        // M3: simulated. In a real implementation, this would:
        // 1. Get the PID of the unit's process
        // 2. Check if the process is still running (e.g., kill(pid, 0))
        HealthCheckResult {
            unit: unit_id,
            healthy: true,
            message: "OS-level health check (simulated): process alive".to_string(),
        }
    }
}

impl HealthCheckOrchestrator for DefaultHealthCheckOrchestrator {
    fn register(&self, unit_id: &UnitId, check: &HealthCheck) {
        self.checks
            .lock()
            .expect("health check mutex should not be poisoned")
            .insert(*unit_id, check.clone());
    }

    fn unregister(&self, unit_id: &UnitId) {
        self.checks
            .lock()
            .expect("health check mutex should not be poisoned")
            .remove(unit_id);
    }

    async fn run_checks(&self) -> Result<Vec<(UnitId, HealthCheckResult)>, NodeError> {
        let checks: Vec<(UnitId, HealthCheck)> = {
            let checks = self
                .checks
                .lock()
                .expect("health check mutex should not be poisoned");
            checks.iter().map(|(k, v)| (*k, v.clone())).collect()
        };

        let mut results = Vec::with_capacity(checks.len());

        for (unit_id, check) in checks {
            let result = match &check.check_type {
                HealthCheckType::Http { path, port } => {
                    self.run_http_check(unit_id, path, *port, check.timeout)
                        .await
                }
                HealthCheckType::Tcp { port } => {
                    self.run_tcp_check(unit_id, *port, check.timeout).await
                }
                HealthCheckType::Command { command } => self.run_command_check(unit_id, command),
                // HealthCheckType is #[non_exhaustive]; unknown variants
                // are treated as unhealthy so future additions never
                // silently pass.
                _ => HealthCheckResult {
                    unit: unit_id,
                    healthy: false,
                    message: "unknown health check type".to_string(),
                },
            };
            results.push((unit_id, result));
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn http_check(path: &str, port: u16) -> HealthCheck {
        HealthCheck {
            check_type: HealthCheckType::Http {
                path: path.to_string(),
                port,
            },
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(2),
        }
    }

    fn tcp_check(port: u16) -> HealthCheck {
        HealthCheck {
            check_type: HealthCheckType::Tcp { port },
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(2),
        }
    }

    fn command_check(cmd: &str) -> HealthCheck {
        HealthCheck {
            check_type: HealthCheckType::Command {
                command: cmd.to_string(),
            },
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
        }
    }

    #[tokio::test]
    async fn test_register_and_unregister() {
        let orch = DefaultHealthCheckOrchestrator::new();
        let id = UnitId(uuid::Uuid::new_v4());

        // Register.
        orch.register(&id, &tcp_check(8080));
        assert_eq!(orch.checks.lock().expect("checks mutex").len(), 1);

        // Unregister.
        orch.unregister(&id);
        assert_eq!(orch.checks.lock().expect("checks mutex").len(), 0);
    }

    #[tokio::test]
    async fn test_run_checks_no_checks() {
        let orch = DefaultHealthCheckOrchestrator::new();

        let results = orch.run_checks().await.expect("run_checks should succeed");

        assert!(results.is_empty(), "no checks registered → empty results");
    }

    #[tokio::test]
    async fn test_run_checks_tcp_healthy() {
        // Start a TCP listener on a random port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("local addr").port();

        // Spawn a task that accepts connections (keeps the port open).
        let accept_task = tokio::spawn(async move {
            loop {
                if listener.accept().await.is_err() {
                    break;
                }
            }
        });

        let orch = DefaultHealthCheckOrchestrator::new();
        let id = UnitId(uuid::Uuid::new_v4());
        orch.register(&id, &tcp_check(port));

        let results = orch.run_checks().await.expect("run_checks should succeed");

        assert_eq!(results.len(), 1);
        assert!(
            results[0].1.healthy,
            "TCP check should be healthy: {}",
            results[0].1.message
        );

        accept_task.abort();
    }

    #[tokio::test]
    async fn test_run_checks_tcp_unhealthy() {
        // Use a port that's very unlikely to be open.
        let orch = DefaultHealthCheckOrchestrator::new();
        let id = UnitId(uuid::Uuid::new_v4());
        orch.register(&id, &tcp_check(1));

        let results = orch.run_checks().await.expect("run_checks should succeed");

        assert_eq!(results.len(), 1);
        assert!(
            !results[0].1.healthy,
            "TCP check on closed port should be unhealthy"
        );
    }

    #[tokio::test]
    async fn test_run_checks_http_healthy() {
        // Start a minimal HTTP server.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let port = listener.local_addr().expect("local addr").port();

        let server_task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let response = "HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });

        let orch = DefaultHealthCheckOrchestrator::new();
        let id = UnitId(uuid::Uuid::new_v4());
        orch.register(&id, &http_check("/healthz", port));

        let results = orch.run_checks().await.expect("run_checks should succeed");

        assert_eq!(results.len(), 1);
        assert!(
            results[0].1.healthy,
            "HTTP check should be healthy: {}",
            results[0].1.message
        );

        server_task.abort();
    }

    #[tokio::test]
    async fn test_run_checks_command_simulated() {
        let orch = DefaultHealthCheckOrchestrator::new();
        let id = UnitId(uuid::Uuid::new_v4());
        orch.register(&id, &command_check("/usr/local/bin/check --alive"));

        let results = orch.run_checks().await.expect("run_checks should succeed");

        assert_eq!(results.len(), 1);
        assert!(
            results[0].1.healthy,
            "command check (simulated) should be healthy"
        );
        assert!(results[0].1.message.contains("simulated"));
    }

    #[tokio::test]
    async fn test_run_checks_multiple() {
        let orch = DefaultHealthCheckOrchestrator::new();

        let id1 = UnitId(uuid::Uuid::new_v4());
        let id2 = UnitId(uuid::Uuid::new_v4());
        let id3 = UnitId(uuid::Uuid::new_v4());

        orch.register(&id1, &tcp_check(1)); // unhealthy
        orch.register(&id2, &command_check("echo ok")); // simulated healthy
        orch.register(&id3, &tcp_check(1)); // unhealthy

        let results = orch.run_checks().await.expect("run_checks should succeed");

        assert_eq!(results.len(), 3);

        // At least one should be healthy (the command check).
        let healthy_count = results.iter().filter(|(_, r)| r.healthy).count();
        assert!(
            healthy_count >= 1,
            "at least one check should be healthy, got {healthy_count}"
        );
    }
}
