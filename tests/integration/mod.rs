//! End-to-end integration tests for taba.
//!
//! These tests exercise the full workflow through the CLI library
//! API (`LocalClient`, commands, parser) — not the binary, but the
//! same code path. They verify that all 12 crates work together
//! correctly.
//!
//! ## Test coverage
//!
//! 1. **Single-node**: init → apply → status → compose → audit
//! 2. **Multi-node**: two `LocalClients` sharing state via graph.json
//! 3. **K8s migration**: Deployment YAML → taba-k8s convert → apply

mod e2e;
mod k8s_migration;
mod multi_node;
