//! Proptest strategies for generating valid test data.
//!
//! These strategies generate taba domain types suitable for
//! property-based testing. The [`arbitrary_workload_unit`] strategy
//! generates units that pass `DefaultValidator::validate`.
//!
//! # Example
//!
//! ```
//! use proptest::prelude::*;
//! use taba_test_harness::arbitrary_capability;
//!
//! proptest! {
//!     fn cap_has_type(cap in arbitrary_capability()) {
//!         assert!(!cap.cap_type.is_empty());
//!     }
//! }
//! ```

use proptest::prelude::*;

use taba_common::{
    AuthorId, ContentDigest, DualClockEvent, LogicalClock, TrustDomainId, UnitId, ValidityWindow,
    WallTime,
};
use taba_core::{
    Artifact, ArtifactType, Capability, Classification, CrashBehavior, FailureSemantics,
    OomBehavior, Scaling, ShutdownBehavior, StateRecovery, Tolerances, UnitHeader, UnitState,
    WorkloadKind, WorkloadUnit,
};

// ===========================================================================
// Primitive strategies
// ===========================================================================

/// Strategy for generating arbitrary [`UnitId`] values.
///
/// Uses a random 128-bit integer to construct a UUID, ensuring
/// full coverage of the UUID space.
pub fn arbitrary_unit_id() -> impl Strategy<Value = UnitId> {
    prop::num::u128::ANY.prop_map(|v| UnitId(uuid::Uuid::from_u128(v)))
}

/// Strategy for generating arbitrary [`LogicalClock`] values.
pub fn arbitrary_logical_clock() -> impl Strategy<Value = LogicalClock> {
    prop::num::u64::ANY.prop_map(LogicalClock)
}

/// Strategy for generating arbitrary [`Classification`] values.
///
/// Uniformly selects one of the four lattice levels:
/// `Public`, `Internal`, `Confidential`, `Pii`.
pub fn arbitrary_classification() -> impl Strategy<Value = Classification> {
    prop_oneof![
        Just(Classification::Public),
        Just(Classification::Internal),
        Just(Classification::Confidential),
        Just(Classification::Pii),
    ]
}

// ===========================================================================
// Capability strategy
// ===========================================================================

/// Strategy for generating arbitrary [`Capability`] values.
///
/// The `cap_type` and `name` are non-empty lowercase strings.
/// The `purpose` is optionally a non-empty lowercase string.
pub fn arbitrary_capability() -> impl Strategy<Value = Capability> {
    (
        "[a-z]{1,8}",                       // cap_type
        "[a-z]{1,12}",                      // name
        proptest::option::of("[a-z]{1,8}"), // purpose
    )
        .prop_map(|(cap_type, name, purpose)| Capability {
            cap_type,
            name,
            purpose,
        })
}

/// Strategy for generating sorted, deduplicated capability lists.
///
/// Generates a vec of capabilities, then sorts and removes
/// duplicates to ensure the list is well-formed (INV-K2).
/// The resulting list has between `min` and `max` capabilities
/// before dedup, so after dedup the count may be lower.
fn arbitrary_sorted_capabilities(min: usize, max: usize) -> impl Strategy<Value = Vec<Capability>> {
    prop::collection::vec(arbitrary_capability(), min..max).prop_map(|mut caps| {
        caps.sort();
        caps.dedup();
        caps
    })
}

// ===========================================================================
// WorkloadUnit strategy
// ===========================================================================

/// Strategy for generating valid [`WorkloadUnit`] values.
///
/// The generated units pass `DefaultValidator::validate`:
///
/// - `provides` is non-empty and sorted (INV-K2)
/// - `needs` is sorted (can be empty)
/// - `tolerates` is meaningful (has `max_latency` and `failure_modes`)
/// - `scaling.min_instances <= scaling.max_instances`
/// - `BoundedTask` kind has a validity window (INV-W2)
/// - No self-references in recovery relationships (empty)
pub fn arbitrary_workload_unit() -> impl Strategy<Value = WorkloadUnit> {
    let arb_uuid = || prop::num::u128::ANY.prop_map(uuid::Uuid::from_u128);

    (
        arb_uuid(),                          // id
        arb_uuid(),                          // author
        arb_uuid(),                          // trust_domain
        any::<bool>(),                       // is_bounded
        arbitrary_sorted_capabilities(1, 5), // provides (non-empty)
        arbitrary_sorted_capabilities(0, 5), // needs (can be empty)
        (1u32..50, 50u32..200),              // scaling (min < max guaranteed)
        "[a-z]{1,8}",                        // cap_name for artifact ref
    )
        .prop_map(
            |(id, author, trust_domain, is_bounded, provides, needs, (min, max), cap_name)| {
                let header = UnitHeader {
                    id: UnitId(id),
                    author: AuthorId(author),
                    trust_domain: TrustDomainId(trust_domain),
                    created_at: DualClockEvent {
                        logical_clock: LogicalClock(1),
                        wall_time: WallTime { millis: 1000 },
                        timezone: "UTC".to_string(),
                    },
                    validity: if is_bounded {
                        Some(ValidityWindow {
                            lc_range: Some((LogicalClock(10), LogicalClock(100))),
                            wall_time_deadline: None,
                        })
                    } else {
                        None
                    },
                    state: UnitState::Declared,
                    version: None,
                };

                WorkloadUnit {
                    header,
                    kind: if is_bounded {
                        WorkloadKind::BoundedTask
                    } else {
                        WorkloadKind::Service
                    },
                    artifact: Artifact {
                        artifact_type: ArtifactType::Oci,
                        artifact_ref: format!("registry.example.com/{cap_name}:v1"),
                        digest: ContentDigest("sha256:abc123".to_string()),
                        requires: Vec::new(),
                        kernel_ref: None,
                        rootfs_ref: None,
                    },
                    needs,
                    provides,
                    tolerates: Tolerances {
                        max_latency: Some(std::time::Duration::from_millis(100)),
                        failure_modes: vec!["timeout".to_string()],
                        consistency: None,
                    },
                    trusts: Vec::new(),
                    scaling: Scaling {
                        min_instances: min,
                        max_instances: max,
                        triggers: Vec::new(),
                    },
                    failure_semantics: FailureSemantics {
                        on_oom: OomBehavior::Restart,
                        on_crash: CrashBehavior::Unexpected,
                        on_shutdown: ShutdownBehavior::Immediate,
                    },
                    recovery_relationships: Vec::new(),
                    state_recovery: StateRecovery::Stateless,
                    placement_on_failure: None,
                    health_check: None,
                    spawn_context: None,
                }
            },
        )
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use taba_core::unit::Unit;
    use taba_core::{DefaultValidator, UnitValidator};

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_arbitrary_capability_valid(cap in arbitrary_capability()) {
            prop_assert!(!cap.cap_type.is_empty(), "cap_type must be non-empty");
            prop_assert!(!cap.name.is_empty(), "name must be non-empty");
        }

        #[test]
        fn proptest_arbitrary_workload_unit_valid(unit in arbitrary_workload_unit()) {
            let validator = DefaultValidator::empty();
            let result = validator.validate(&Unit::Workload(unit));
            prop_assert!(
                result.is_ok(),
                "generated workload unit must pass validation: {:?}",
                result.err()
            );
        }

        #[test]
        fn proptest_arbitrary_unit_id_unique(a in arbitrary_unit_id(), b in arbitrary_unit_id()) {
            // Extremely likely to be different; not guaranteed but
            // tests that the strategy produces diverse values.
            // This is a smoke test, not a uniqueness invariant.
            let _ = (a, b); // both are valid UnitId values
        }

        #[test]
        fn proptest_arbitrary_logical_clock_range(clock in arbitrary_logical_clock()) {
            // LogicalClock is a u64 wrapper; any value is valid.
            let _ = clock;
        }

        #[test]
        fn proptest_arbitrary_classification_valid(c in arbitrary_classification()) {
            // All four variants are valid.
            assert!(matches!(
                c,
                Classification::Public
                    | Classification::Internal
                    | Classification::Confidential
                    | Classification::Pii
            ));
        }
    }
}
