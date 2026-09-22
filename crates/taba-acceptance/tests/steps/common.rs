#![allow(
    clippy::unused_async,
    clippy::needless_pass_by_ref_mut,
    clippy::used_underscore_binding,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused
)]
//! Common step definitions — all steps are now covered by
//! smoke.rs, critical.rs, or features.rs.

use cucumber::{given, then, when};
use std::collections::BTreeMap;

use crate::TabaWorld;

#[must_use]
pub const fn parse_table(_step: &cucumber::gherkin::Step) -> BTreeMap<String, String> {
    BTreeMap::new()
}
