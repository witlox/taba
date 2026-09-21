#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    dead_code,
    unused
)]
//! Real step definitions for critical BDD scenarios.
//!
//! Background steps (author registration, trust domain) and
//! classification lattice assertion (INV-S7). Other critical
//! scenarios use no-op step definitions from common.rs —
//! scope checking and signature verification are tested in
//! unit tests (taba-graph, taba-security).

use cucumber::given;
use std::collections::BTreeMap;

use crate::TabaWorld;
use taba_common::UnitId;
use taba_core::{
    Classification, DataSchema, DataUnit, RetentionMode, RetentionPolicy, StorageRequirements,
    Unit, UnitState,
};
use taba_test_harness::DataUnitBuilder;

fn parse_table(step: &cucumber::gherkin::Step) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Some(table) = &step.table {
        for row in &table.rows {
            if row.len() >= 2 {
                map.insert(row[0].trim().to_string(), row[1].trim().to_string());
            }
        }
    }
    map
}

fn parse_duration(s: &str) -> std::time::Duration {
    let s = s.trim().to_lowercase();
    if let Some((n, unit)) = s.split_once(' ') {
        let n: u64 = n.parse().unwrap_or(365);
        match unit {
            "second" | "seconds" | "sec" | "secs" => std::time::Duration::from_secs(n),
            "minute" | "minutes" | "min" | "mins" => std::time::Duration::from_secs(n * 60),
            "hour" | "hours" | "h" => std::time::Duration::from_secs(n * 3600),
            "day" | "days" | "d" => std::time::Duration::from_secs(n * 86_400),
            "week" | "weeks" | "w" => std::time::Duration::from_secs(n * 604_800),
            "month" | "months" => std::time::Duration::from_secs(n * 2_592_000),
            "year" | "years" | "y" => std::time::Duration::from_secs(n * 31_536_000),
            _ => std::time::Duration::from_secs(86_400),
        }
    } else {
        std::time::Duration::from_secs(86_400)
    }
}

// === Background steps (used by ALL scenarios) ===

#[given(regex = r#"^an author "([^"]+)" with scope \(type: ([\w-]+), trust_domain: "([^"]+)"\)$"#)]
async fn given_author_with_scope(
    world: &mut TabaWorld,
    name: String,
    scope_type: String,
    td_name: String,
) {
    use taba_core::{RoleAssignment, UnitTypeScope};

    world.register_author(&name);
    world.register_trust_domain(&td_name);
    world.trust_domain = world.trust_domain_id_by_name(&td_name);

    let author_id = world.author_id_by_name(&name);
    let td = world.trust_domain_id_by_name(&td_name);

    let unit_type_scope = match scope_type.as_str() {
        "workload" => vec![UnitTypeScope::Workload],
        "data" => vec![UnitTypeScope::Data],
        "policy" => vec![UnitTypeScope::Policy],
        "governance" => vec![UnitTypeScope::Governance],
        "data-steward" => vec![UnitTypeScope::Data, UnitTypeScope::Policy],
        _ => vec![UnitTypeScope::Workload],
    };

    let ra = RoleAssignment {
        header: taba_core::UnitHeader {
            id: UnitId(uuid::Uuid::new_v4()),
            author: world.author_id,
            trust_domain: td,
            created_at: taba_common::DualClockEvent {
                logical_clock: world.logical_clock,
                wall_time: taba_common::WallTime { millis: 0 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: UnitState::Declared,
            version: None,
        },
        assignee: author_id,
        unit_type_scope,
        trust_domain_scope: vec![td],
    };

    world.scope_checker.add_assignment(ra);
}

#[given(regex = r#"^a bootstrapped trust domain "([^"]+)"(?: with root governance unit)?$"#)]
async fn given_bootstrapped_td(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
    world.trust_domain = world.trust_domain_id_by_name(&name);
}

#[given(regex = r#"^a bootstrapped trust domain "([^"]+)" \(Tier \d+, .*\)$"#)]
async fn given_bootstrapped_tier_td(world: &mut TabaWorld, name: String) {
    world.register_trust_domain(&name);
    world.trust_domain = world.trust_domain_id_by_name(&name);
}

#[given("author keys are Ed25519 and not revoked")]
async fn given_keys_not_revoked(_world: &mut TabaWorld) {}

// === Classification lattice (INV-S7) ===

#[given(regex = r#"^bob authors a data unit "([^"]+)" with:$"#)]
async fn given_bob_authors_data(
    world: &mut TabaWorld,
    name: String,
    step: &cucumber::gherkin::Step,
) {
    let table = parse_table(step);
    let author = world.author_id_by_name("bob");

    let mut builder = DataUnitBuilder::new().with_author(author);

    if let Some(class_str) = table.get("classification") {
        builder = builder.with_classification(match class_str.trim() {
            "public" => Classification::Public,
            "internal" => Classification::Internal,
            "confidential" => Classification::Confidential,
            "PII" | "pii" => Classification::Pii,
            _ => Classification::Internal,
        });
    }

    if let Some(retention_str) = table.get("retention") {
        let parts: Vec<&str> = retention_str.split(',').map(|s| s.trim()).collect();
        let duration_str = parts.first().unwrap_or(&"365 days");
        let legal_basis = parts
            .iter()
            .find_map(|p| p.strip_prefix("legal_basis:").map(|s| s.trim().to_string()))
            .unwrap_or_else(|| "consent".to_string());

        builder = builder.with_retention(RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: Some(parse_duration(duration_str)),
            legal_basis,
            mandatory: false,
        });
    }

    if let Some(storage_str) = table.get("storage") {
        let mut encrypted = false;
        let mut jurisdictions = Vec::new();
        for part in storage_str.split(',') {
            let part = part.trim();
            if part.starts_with("encryption:") {
                encrypted = true;
            }
            if let Some(j) = part.strip_prefix("jurisdiction:") {
                jurisdictions.push(j.trim().to_string());
            }
        }
        builder = builder.with_storage_requirements(StorageRequirements {
            encrypted_at_rest: encrypted,
            jurisdictions,
            min_replicas: None,
        });
    }

    if let Some(schema_str) = table.get("schema") {
        builder = builder.with_schema(DataSchema {
            format: "json".to_string(),
            definition: schema_str.clone(),
        });
    }

    let unit = builder.build();
    world.store_unit(&name, Unit::Data(unit));
}

use cucumber::then;

#[then(regex = r#"^the unit classification is positioned at level (\d+) in the lattice .*$"#)]
async fn then_classification_level(world: &mut TabaWorld, expected_level: u8) {
    let (_, unit) = world.units.last_key_value().expect("should have a unit");

    if let Unit::Data(d) = unit {
        let actual = match d.classification {
            Classification::Public => 1,
            Classification::Internal => 2,
            Classification::Confidential => 3,
            Classification::Pii => 4,
            _ => 2,
        };
        assert_eq!(
            actual, expected_level,
            "classification should be at level {expected_level}, got {actual}"
        );
    } else {
        panic!("expected a data unit, got {:?}", unit.kind());
    }
}
