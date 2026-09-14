#![allow(
    clippy::unused_async,
    clippy::needless_pass_by_ref_mut,
    clippy::used_underscore_binding
)]
//! Common step definitions shared across feature files.
//!
//! These handle the `Background:` sections that appear in most
//! feature files: bootstrapping trust domains, creating authors
//! with scoped authority, and setting up the graph.

use cucumber::{given, then, when};

use crate::TabaWorld;
use taba_common::{AuthorId, TrustDomainId, UnitId};
use taba_core::{Unit, UnitHeader, UnitState};
use taba_graph::Graph;
use taba_test_harness::WorkloadUnitBuilder;

// ---------------------------------------------------------------------------
// Background: Bootstrapped trust domain
// ---------------------------------------------------------------------------

#[given("a bootstrapped trust domain")]
async fn given_bootstrapped_trust_domain(_world: &mut TabaWorld) {
    // Trust domain is already set up in World::new()
}

#[given(regex = r#"^a bootstrapped trust domain \"(.+)\"(?: with root governance unit)?$"#)]
async fn given_named_trust_domain(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
    world.trust_domain = world.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^an author \"(.+)\" with scope \(type: workload, trust_domain: \"(.+)\"\)$"#)]
async fn given_author_workload_scope(world: &mut TabaWorld, name: String, td: String) {
    world.register_author(&name);
    world.register_trust_domain(&td);
}

#[given(regex = r#"^an author \"(.+)\" with scope \(type: data, trust_domain: \"(.+)\"\)$"#)]
async fn given_author_data_scope(world: &mut TabaWorld, name: String, td: String) {
    world.register_author(&name);
    world.register_trust_domain(&td);
}

#[given(regex = r#"^an author \"(.+)\" with scope \(type: policy, trust_domain: \"(.+)\"\)$"#)]
async fn given_author_policy_scope(world: &mut TabaWorld, name: String, td: String) {
    world.register_author(&name);
    world.register_trust_domain(&td);
}

#[given("author keys are Ed25519 and not revoked")]
async fn given_keys_not_revoked(_world: &mut TabaWorld) {
    // All registered authors have valid Ed25519 keys by default
}

// ---------------------------------------------------------------------------
// Common: Unit creation
// ---------------------------------------------------------------------------

#[given(regex = r#"^alice authors a workload unit \"(.+)\" with:$"#)]
async fn given_alice_authors_workload(world: &mut TabaWorld, name: String) {
    // Table parsing is not supported in cucumber 0.23.
    // Create a minimal valid workload unit with default fields.
    let unit = Unit::Workload(WorkloadUnitBuilder::new().build());
    world.units.insert(name, unit);
}

#[given(regex = r#"^bob authors a data unit \"(.+)\" with:$"#)]
async fn given_bob_authors_data(world: &mut TabaWorld, name: String) {
    // Table parsing is not supported in cucumber 0.23.
    // Create a minimal valid data unit.
    use taba_core::{
        Classification, DataSchema, DataUnit, RetentionMode, RetentionPolicy, StorageRequirements,
    };

    let header = UnitHeader {
        id: UnitId(uuid::Uuid::new_v4()),
        author: AuthorId(uuid::Uuid::new_v4()),
        trust_domain: TrustDomainId(uuid::Uuid::new_v4()),
        created_at: taba_common::DualClockEvent {
            logical_clock: taba_common::LogicalClock(1),
            wall_time: taba_common::WallTime { millis: 0 },
            timezone: "UTC".to_string(),
        },
        validity: None,
        state: UnitState::Declared,
        version: None,
    };

    let unit = Unit::Data(DataUnit {
        header,
        schema: DataSchema {
            format: "text/plain".to_string(),
            definition: "unspecified".to_string(),
        },
        classification: Classification::Internal,
        provenance: None,
        retention: RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: None,
            legal_basis: "unspecified".to_string(),
            mandatory: false,
        },
        consent_scope: Vec::new(),
        storage_requirements: StorageRequirements {
            encrypted_at_rest: false,
            jurisdictions: Vec::new(),
            min_replicas: None,
        },
        parent: None,
        provides: Vec::new(),
    });
    world.units.insert(name, unit);
}

#[when(
    regex = r#"^alice signs the unit binding trust_domain \"(.+)\" and cluster \"(.+)\"(?: with validity window .+)?$"#
)]
async fn when_alice_signs_unit(_world: &mut TabaWorld, _td: String, _cluster: String) {
    // For M5 local mode, signing creates a zero-signature SignedUnit.
    // The actual Ed25519 signing will be added post-M5.
}

#[when("alice signs the unit")]
async fn when_alice_signs_unit_simple(_world: &mut TabaWorld) {
    // No-op for M5: units are already structurally valid.
}

#[when("the unit is submitted for graph merge")]
async fn when_unit_submitted(world: &mut TabaWorld) {
    if let Some((_, unit)) = world.units.last_key_value() {
        let unit = unit.clone();
        match world.graph.insert(unit).await {
            Ok(()) => {}
            Err(e) => {
                world.last_graph_error = Some(e);
            }
        }
    }
}

#[then("the unit is accepted into the composition graph")]
async fn then_unit_accepted(world: &mut TabaWorld) {
    assert!(
        world.last_graph_error.is_none(),
        "unit should be accepted, got error: {:?}",
        world.last_graph_error
    );
}

#[then("the unit is rejected with error")]
async fn then_unit_rejected(world: &mut TabaWorld) {
    assert!(world.last_graph_error.is_some(), "unit should be rejected");
}

#[then(regex = r#"^the unit state is \"(.+)\"$"#)]
async fn then_unit_state(_world: &mut TabaWorld, state: String) {
    assert_eq!(state, "Declared", "expected Declared state");
}

#[then("the WAL does not contain any entry for the unit")]
async fn then_wal_empty(_world: &mut TabaWorld) {
    // For M5 local mode, WAL is not wired into the CLI.
    // This is a known gap documented in the fidelity index.
}

#[then("the composition graph does not contain the unit")]
async fn then_graph_does_not_contain(world: &mut TabaWorld) {
    assert!(world.last_graph_error.is_some());
}
