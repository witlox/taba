//! Command-line interface for human operators of taba.
//!
//! This crate provides the `taba` binary — the primary user-facing
//! entry point for authoring units, managing composition, inspecting
//! status, and querying audit trails.
//!
//! ## Commands
//!
//! - `init` — Tier 0 solo bootstrap (key + trust domain + governance)
//! - `apply` — Sign and insert a unit TOML into the graph
//! - `unit` — Create, inspect, validate, list, archive units
//! - `status` — Node health, graph stats, operational mode
//! - `compose` — Run solver, inspect placements and conflicts
//! - `policy` — Create, supersede, list policies
//! - `promote` — Create promotion policy for a unit version
//! - `trust-domain` — Create, inspect, list members
//! - `audit` — Lineage query, provenance chain, decision trail
//! - `push` — Push artifact to local cache
//!
//! ## Modes
//!
//! For M5, the CLI operates in **local mode** — it uses the library
//! crates directly with an in-memory graph persisted to a local state
//! directory. A future **daemon mode** will connect to a running
//! taba-node via gRPC.

pub mod auth;
pub mod client;
pub mod commands;
pub mod error;
pub mod format;
pub mod parser;

pub use auth::{LocalAuth, LocalConfig};
pub use client::LocalClient;
pub use error::CliError;
pub use format::OutputFormat;
pub use parser::parse_unit;
