#![allow(
    clippy::unused_async,
    clippy::needless_pass_by_ref_mut,
    clippy::used_underscore_binding,
    clippy::too_many_arguments,
    clippy::match_same_arms,
    clippy::all,
    clippy::pedantic,
    dead_code,
    unused
)]
//! Cucumber test runner for taba acceptance tests.
//!
//! Reads feature files from `../../specs/features/` and executes
//! step definitions.
//!
//! Set `TABA_BDD_FAST=1` to run only `@smoke` tagged scenarios.

use cucumber::World;
use std::sync::Arc;

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct TabaWorld {
    pub graph: Arc<taba_graph::DefaultGraph>,
    pub solver: taba_solver::DefaultSolver,
    pub trail_recorder: taba_observe::DefaultDecisionTrailRecorder,
    pub runtime: taba_node::SimulatedRuntime,
    pub health: taba_node::DefaultHealthReporter,
    pub mode: taba_node::DefaultModeManager,
    pub scope_checker: taba_security::DefaultScopeChecker,
    pub verifier: taba_security::DefaultVerifier,
    pub key_pair: taba_security::KeyPair,
    pub trust_domain: taba_common::TrustDomainId,
    pub cluster_id: taba_common::ClusterId,
    pub node_id: taba_common::NodeId,
    pub author_id: taba_common::AuthorId,
    pub units: std::collections::BTreeMap<String, taba_core::Unit>,
    pub last_solver_result: Option<taba_solver::SolverResult>,
    pub last_graph_error: Option<taba_graph::GraphError>,
    pub last_snapshot: Option<taba_graph::GraphSnapshot>,
    pub authors:
        std::collections::BTreeMap<String, (taba_common::AuthorId, taba_security::KeyPair)>,
    pub trust_domains: std::collections::BTreeMap<String, taba_common::TrustDomainId>,
    pub logical_clock: taba_common::LogicalClock,
    pub signed_units: std::collections::BTreeSet<String>,
    pub node_caps:
        std::collections::BTreeMap<String, (taba_common::NodeId, taba_core::NodeCapabilitySet)>,
    pub membership: taba_solver::MembershipSnapshot,
    pub alerts: Vec<String>,
    pub events: Vec<String>,
    pub ceremony_id: Option<String>,
    pub ceremony_state: Option<String>,
    pub ceremony_shares_received: u32,
    pub ceremony_threshold: u32,
    pub ceremony_total_shares: u32,
    pub ceremony_holders: std::collections::BTreeSet<String>,
    pub ceremony_expected_fp: Option<String>,
    pub ceremony_pk: Option<String>,
    pub ceremony_error: Option<String>,
    pub placement_on_node: std::collections::BTreeMap<String, taba_common::NodeId>,
}

impl TabaWorld {
    #[allow(clippy::too_many_lines)]
    fn new() -> Self {
        let key_pair = taba_security::KeyPair::generate();
        let public_key = *key_pair.public_key();
        let node_id = taba_common::NodeId(uuid::Uuid::new_v4());
        let author_id = taba_common::AuthorId(uuid::Uuid::new_v4());
        let trust_domain = taba_common::TrustDomainId(uuid::Uuid::new_v4());
        let cluster_id = taba_common::ClusterId(uuid::Uuid::new_v4());

        let mut verifier = taba_security::DefaultVerifier::new();
        verifier.add_key(author_id, public_key, None);
        let graph = std::sync::Arc::new(taba_graph::DefaultGraph::new(1_073_741_824));

        let membership = taba_solver::MembershipSnapshot::single_node(
            node_id,
            taba_test_harness::NodeCapabilitySetBuilder::new().build(),
        );

        Self {
            graph,
            solver: taba_solver::DefaultSolver::new(),
            trail_recorder: taba_observe::DefaultDecisionTrailRecorder::new(),
            runtime: taba_node::SimulatedRuntime::new(),
            health: taba_node::DefaultHealthReporter::new(node_id, 1_073_741_824),
            mode: taba_node::DefaultModeManager::new(),
            scope_checker: taba_security::DefaultScopeChecker::new(),
            verifier,
            key_pair,
            trust_domain,
            cluster_id,
            node_id,
            author_id,
            units: std::collections::BTreeMap::new(),
            last_solver_result: None,
            last_graph_error: None,
            last_snapshot: None,
            authors: std::collections::BTreeMap::new(),
            trust_domains: std::collections::BTreeMap::new(),
            logical_clock: taba_common::LogicalClock(0),
            signed_units: std::collections::BTreeSet::new(),
            node_caps: std::collections::BTreeMap::new(),
            membership,
            alerts: Vec::new(),
            events: Vec::new(),
            ceremony_id: None,
            ceremony_state: None,
            ceremony_shares_received: 0,
            ceremony_threshold: 0,
            ceremony_total_shares: 0,
            ceremony_holders: std::collections::BTreeSet::new(),
            ceremony_expected_fp: None,
            ceremony_pk: None,
            ceremony_error: None,
            placement_on_node: std::collections::BTreeMap::new(),
        }
    }

    pub fn register_author(&mut self, name: &str) {
        let kp = taba_security::KeyPair::generate();
        let pk = *kp.public_key();
        let aid = taba_common::AuthorId(uuid::Uuid::new_v4());
        self.verifier.add_key(aid, pk, None);
        self.authors.insert(name.to_string(), (aid, kp));
    }

    pub fn author_id_by_name(&self, name: &str) -> taba_common::AuthorId {
        self.authors.get(name).map_or(self.author_id, |(id, _)| *id)
    }

    pub fn register_trust_domain(&mut self, name: &str) {
        let td = taba_common::TrustDomainId(uuid::Uuid::new_v4());
        self.trust_domains.insert(name.to_string(), td);
    }

    pub fn trust_domain_id_by_name(&self, name: &str) -> taba_common::TrustDomainId {
        self.trust_domains
            .get(name)
            .copied()
            .unwrap_or(self.trust_domain)
    }

    pub const fn tick(&mut self) {
        self.logical_clock.0 += 1;
    }

    pub fn add_alert(&mut self, alert: &str) {
        self.alerts.push(alert.to_string());
    }

    pub fn add_event(&mut self, event: &str) {
        self.events.push(event.to_string());
    }

    pub fn reset_errors(&mut self) {
        self.last_graph_error = None;
    }

    pub fn unit_by_name(&self, name: &str) -> Option<&taba_core::Unit> {
        self.units.get(name)
    }

    pub fn unit_id_by_name(&self, name: &str) -> Option<taba_common::UnitId> {
        self.units.get(name).map(|u| u.header().id)
    }

    pub fn store_unit(&mut self, name: &str, unit: taba_core::Unit) {
        self.units.insert(name.to_string(), unit);
    }
}

mod steps;

#[tokio::main]
async fn main() {
    let fast = std::env::var("TABA_BDD_FAST").is_ok();

    let feature_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/features");

    if !feature_dir.exists() {
        eprintln!("Feature directory not found: {}", feature_dir.display());
        std::process::exit(1);
    }

    if fast {
        TabaWorld::filter_run(&feature_dir, |_feature, _rule, scenario| {
            scenario.tags.iter().any(|t| t == "smoke")
        })
        .await;
    } else {
        TabaWorld::run(&feature_dir).await;
    }
}
