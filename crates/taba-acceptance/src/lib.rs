//! BDD acceptance tests for taba.
//!
//! This crate runs the Gherkin feature files in `specs/features/`
//! using cucumber-rs. The `World` struct wires together all taba
//! crates into an in-memory test cluster.
//!
//! ## Running
//!
//! ```sh
//! # All scenarios (Tier 2)
//! cargo test -p taba-acceptance --test acceptance
//!
//! # @smoke only (Tier 1, fast)
//! TABA_BDD_FAST=1 cargo test -p taba-acceptance --test acceptance
//! ```
