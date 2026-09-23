#![allow(clippy::all, clippy::pedantic, dead_code, unused)]

//! Step definition modules for taba BDD acceptance tests.
//!
//! - `common.rs` — Shared steps used by 2+ features (background,
//!   solver evaluation, unit submission, assertions). Also contains
//!   broad catch-all regex patterns for any remaining steps.
//! - `smoke.rs` — Real assertions for the @smoke scenario.
//! - `unit_authoring.rs` — Real step definitions for unit-authoring.feature.
//! - Each remaining feature file gets real step definitions for
//!   steps unique to that feature.

pub mod ceremony;
pub mod common;
pub mod compaction;
pub mod compliance_audit;
pub mod composition;
pub mod conflict_resolution;
pub mod cross_domain;
pub mod data_lineage;
pub mod data_retention;
pub mod environment_progression;
pub mod network_partition;
pub mod node_lifecycle;
pub mod observability;
pub mod operational_modes;
pub mod placement;
pub mod recovery;
pub mod runtime_matching;
pub mod security_enforcement;
pub mod smoke;
pub mod spawned_tasks;
pub mod trust_domain;
pub mod unit_authoring;
