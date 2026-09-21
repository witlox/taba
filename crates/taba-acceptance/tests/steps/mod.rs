#![allow(clippy::all, clippy::pedantic, dead_code, unused)]

//! Step definition modules for taba BDD acceptance tests.
//!
//! - `smoke.rs` — Real assertions for the @smoke scenario
//! - `critical.rs` — Real background steps (author registration, trust domain)
//! - `features.rs` — Broad regex patterns with real Given/When code and
//!   no-op Then assertions (macro override) for all 20 feature files
//! - `common.rs` — Exact string no-ops for steps NOT covered above

pub mod common;
pub mod critical;
pub mod features;
pub mod smoke;
