#![allow(clippy::unused_async, clippy::needless_pass_by_ref_mut)]
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
}

impl TabaWorld {
    fn new() -> Self {
        let key_pair = taba_security::KeyPair::generate();
        let public_key = *key_pair.public_key();
        let node_id = taba_common::NodeId(uuid::Uuid::new_v4());
        let author_id = taba_common::AuthorId(uuid::Uuid::new_v4());
        let trust_domain = taba_common::TrustDomainId(uuid::Uuid::new_v4());
        let cluster_id = taba_common::ClusterId(uuid::Uuid::new_v4());

        let mut verifier = taba_security::DefaultVerifier::new();
        verifier.add_key(author_id, public_key, None);

        Self {
            graph: Arc::new(taba_graph::DefaultGraph::new(1_073_741_824)),
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
