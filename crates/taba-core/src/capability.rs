//! Capability algebra: typed matching of needs to provides (INV-K2).
//!
//! Capabilities are tuples: `(type, name, purpose?)`. Matching is
//! typed — `"needs postgres"` matches `"provides postgres-compatible"`
//! but not `"provides redis"`. Purpose is an optional qualifier: if
//! declared, it must match (purpose mismatch triggers a conflict
//! requiring policy).
//!
//! Capability lists are sorted lexicographically by `(type, name,
//! purpose)` before matching to ensure determinism regardless of
//! declaration order (INV-K2).

use serde::{Deserialize, Serialize};

use taba_common::UnitId;

use crate::unit::Unit;

// ---------------------------------------------------------------------------
// Capability
// ---------------------------------------------------------------------------

/// A typed resource or service a unit needs or provides.
///
/// Sorted lexicographically by `(cap_type, name, purpose)` before
/// matching to ensure determinism regardless of declaration order
/// (INV-K2). The `Ord` derive implements this ordering.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Capability {
    /// The type of capability (e.g., `"storage"`, `"compute"`, `"network"`).
    pub cap_type: String,
    /// The name of the capability (e.g., `"postgres-compatible"`, `"http"`).
    pub name: String,
    /// Optional purpose qualifier. If declared, must match during
    /// composition. Purpose mismatch triggers a conflict requiring
    /// policy (INV-K2, DL-010).
    pub purpose: Option<String>,
}

impl Capability {
    /// Creates a new capability with the given type, name, and no purpose.
    #[must_use]
    pub fn new(cap_type: &str, name: &str) -> Self {
        Self {
            cap_type: cap_type.to_string(),
            name: name.to_string(),
            purpose: None,
        }
    }

    /// Creates a new capability with the given type, name, and purpose.
    #[must_use]
    pub fn with_purpose(cap_type: &str, name: &str, purpose: &str) -> Self {
        Self {
            cap_type: cap_type.to_string(),
            name: name.to_string(),
            purpose: Some(purpose.to_string()),
        }
    }
}

/// Result of matching a `needs` capability against a `provides` capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityMatch {
    /// The unit that declared the need.
    pub needer: UnitId,
    /// The specific capability needed.
    pub need: Capability,
    /// The unit that provides the capability.
    pub provider: UnitId,
    /// The specific capability provided.
    pub provided: Capability,
}

// ---------------------------------------------------------------------------
// CapabilityMatcher trait
// ---------------------------------------------------------------------------

/// Matches capability needs to capability provides.
///
/// Implements the typed capability algebra described in INV-K2.
/// Matching is deterministic: capability lists are sorted
/// lexicographically by `(type, name, purpose)` before comparison.
///
/// This is a pure trait — implementations must have no I/O, no side
/// effects, and produce identical output for identical input on any
/// node (INV-C3).
pub trait CapabilityMatcher {
    /// Attempt to match all needs of `consumer` against provides of `provider`.
    ///
    /// Returns the set of successful matches. Unmatched needs are
    /// returned as the second element. If any need has a purpose
    /// qualifier that does not match the provide's purpose, it is
    /// treated as unmatched (triggers conflict requiring policy per
    /// INV-K2).
    ///
    /// This is a pure function: no I/O, no side effects, deterministic.
    #[must_use]
    fn match_capabilities(
        &self,
        consumer: &Unit,
        provider: &Unit,
    ) -> (Vec<CapabilityMatch>, Vec<Capability>);

    /// Check whether a single need is satisfiable by a single provide.
    ///
    /// Handles type compatibility (e.g., `"postgres-compatible"`
    /// satisfies `"postgres"`) and purpose qualifier matching. Returns
    /// `true` if the provide satisfies the need, `false` otherwise.
    #[must_use]
    fn is_satisfiable(&self, need: &Capability, provide: &Capability) -> bool;

    /// Given a set of providers, find all possible satisfiers for a given need.
    ///
    /// Returns provider [`UnitId`]s sorted by match specificity (exact
    /// match first, then compatible matches). Deterministic ordering
    /// for solver reproducibility (INV-C3).
    #[must_use]
    fn find_providers(&self, need: &Capability, providers: &[Unit]) -> Vec<UnitId>;
}

// ---------------------------------------------------------------------------
// DefaultCapabilityMatcher
// ---------------------------------------------------------------------------

/// Default implementation of [`CapabilityMatcher`].
///
/// A stateless, pure-function matcher. All methods are deterministic:
/// identical inputs always produce identical output. Safe to share
/// across threads (no interior mutability).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultCapabilityMatcher;

impl DefaultCapabilityMatcher {
    /// Creates a new default capability matcher.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns `true` if the provide is an exact match for the need
    /// (same `cap_type`, `name`, and `purpose`).
    fn is_exact_match(need: &Capability, provide: &Capability) -> bool {
        need.cap_type == provide.cap_type
            && need.name == provide.name
            && need.purpose == provide.purpose
    }
}

impl CapabilityMatcher for DefaultCapabilityMatcher {
    fn match_capabilities(
        &self,
        consumer: &Unit,
        provider: &Unit,
    ) -> (Vec<CapabilityMatch>, Vec<Capability>) {
        let consumer_id = consumer.header().id;
        let provider_id = provider.header().id;
        let needs = consumer.needs();
        let provider_caps = provider.provides();

        let mut matched = Vec::new();
        let mut unmatched = Vec::new();

        for need in needs {
            if let Some(provided) = provider_caps.iter().find(|p| self.is_satisfiable(need, p)) {
                matched.push(CapabilityMatch {
                    needer: consumer_id,
                    need: need.clone(),
                    provider: provider_id,
                    provided: provided.clone(),
                });
            } else {
                unmatched.push(need.clone());
            }
        }

        (matched, unmatched)
    }

    fn is_satisfiable(&self, need: &Capability, provide: &Capability) -> bool {
        // Type must match exactly.
        if need.cap_type != provide.cap_type {
            return false;
        }

        // Name must be compatible: exact match, or the provide is a
        // "{need}-compatible" variant (e.g., "postgres-compatible"
        // satisfies "postgres" per INV-K2).
        let name_compatible =
            need.name == provide.name || provide.name == format!("{}-compatible", need.name);
        if !name_compatible {
            return false;
        }

        // Purpose: if the need declares a purpose, the provide must
        // have the same purpose. If the need has no purpose, any
        // provide (with or without purpose) satisfies it.
        need.purpose
            .as_ref()
            .is_none_or(|need_purpose| provide.purpose.as_ref() == Some(need_purpose))
    }

    fn find_providers(&self, need: &Capability, providers: &[Unit]) -> Vec<UnitId> {
        let mut exact = Vec::new();
        let mut compatible = Vec::new();

        for provider in providers {
            let caps = provider.provides();

            // Check for an exact match first (highest specificity).
            let has_exact = caps.iter().any(|c| Self::is_exact_match(need, c));

            if has_exact {
                exact.push(provider.header().id);
            } else if caps.iter().any(|c| self.is_satisfiable(need, c)) {
                // Only compatible (non-exact) matches go here.
                compatible.push(provider.header().id);
            }
        }

        // Sort each category by UnitId for deterministic ordering (INV-C3).
        exact.sort_unstable();
        compatible.sort_unstable();

        // Exact matches first, then compatible matches.
        exact.extend(compatible);
        exact
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::{UnitHeader, WorkloadKind, WorkloadUnit};
    use taba_common::{DualClockEvent, LogicalClock, WallTime};

    /// Creates a minimal [`UnitHeader`] for testing.
    fn test_header() -> UnitHeader {
        UnitHeader {
            id: UnitId(uuid::Uuid::new_v4()),
            author: taba_common::AuthorId(uuid::Uuid::new_v4()),
            trust_domain: taba_common::TrustDomainId(uuid::Uuid::new_v4()),
            created_at: DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: crate::unit::UnitState::Declared,
            version: None,
        }
    }

    /// Creates a [`WorkloadUnit`] with the given needs and provides.
    fn workload_with(needs: Vec<Capability>, provides: Vec<Capability>) -> Unit {
        Unit::Workload(WorkloadUnit {
            header: test_header(),
            kind: WorkloadKind::Service,
            artifact: crate::artifact::Artifact {
                artifact_type: crate::artifact::ArtifactType::Oci,
                artifact_ref: "registry.example.com/app:v1".to_string(),
                digest: taba_common::ContentDigest("sha256:abc".to_string()),
                requires: Vec::new(),
            },
            needs,
            provides,
            tolerates: crate::contract::Tolerances {
                max_latency: None,
                failure_modes: vec!["timeout".to_string()],
                consistency: None,
            },
            trusts: Vec::new(),
            scaling: crate::contract::Scaling {
                min_instances: 1,
                max_instances: 3,
                triggers: Vec::new(),
            },
            failure_semantics: crate::contract::FailureSemantics {
                on_oom: crate::contract::OomBehavior::Restart,
                on_crash: crate::contract::CrashBehavior::Unexpected,
                on_shutdown: crate::contract::ShutdownBehavior::Immediate,
            },
            recovery_relationships: Vec::new(),
            state_recovery: crate::contract::StateRecovery::Stateless,
            placement_on_failure: None,
            health_check: None,
            spawn_context: None,
        })
    }

    #[test]
    fn test_capability_ordering() {
        // (type, name, purpose) sorted lexicographically
        let a = Capability::new("compute", "cpu");
        let b = Capability::new("compute", "cpu");
        let c = Capability::with_purpose("compute", "cpu", "batch");
        let d = Capability::new("storage", "redis");

        let mut caps = vec![d.clone(), c.clone(), b.clone(), a.clone()];
        caps.sort();

        // a == b < c (Some > None) < d (storage > compute)
        assert_eq!(caps, vec![a, b, c, d]);
    }

    #[test]
    fn test_capability_with_purpose_sorts_after_without() {
        let without = Capability::new("storage", "postgres");
        let with = Capability::with_purpose("storage", "postgres", "analytics");

        assert!(
            without < with,
            "Capability without purpose sorts before with"
        );
        assert!(
            with > without,
            "Capability with purpose sorts after without"
        );
    }

    #[test]
    fn test_capability_match_basic() {
        let need = Capability::new("storage", "postgres");
        let provide = Capability::new("storage", "postgres");
        let matcher = DefaultCapabilityMatcher::new();

        assert!(matcher.is_satisfiable(&need, &provide));
    }

    #[test]
    fn test_capability_match_purpose_mismatch() {
        let need = Capability::with_purpose("storage", "postgres", "analytics");
        let provide = Capability::new("storage", "postgres");
        let matcher = DefaultCapabilityMatcher::new();

        assert!(
            !matcher.is_satisfiable(&need, &provide),
            "need with purpose must not match provide without purpose"
        );
    }

    #[test]
    fn test_capability_match_type_compatible() {
        // "postgres-compatible" satisfies "postgres" (INV-K2)
        let need = Capability::new("storage", "postgres");
        let provide = Capability::new("storage", "postgres-compatible");
        let matcher = DefaultCapabilityMatcher::new();

        assert!(
            matcher.is_satisfiable(&need, &provide),
            "'postgres-compatible' should satisfy 'postgres'"
        );
    }

    #[test]
    fn test_capability_match_wrong_type() {
        let need = Capability::new("storage", "postgres");
        let provide = Capability::new("compute", "postgres");
        let matcher = DefaultCapabilityMatcher::new();

        assert!(
            !matcher.is_satisfiable(&need, &provide),
            "different cap_type should not satisfy"
        );
    }

    #[test]
    fn test_capability_match_wrong_name() {
        let need = Capability::new("storage", "postgres");
        let provide = Capability::new("storage", "redis");
        let matcher = DefaultCapabilityMatcher::new();

        assert!(
            !matcher.is_satisfiable(&need, &provide),
            "'redis' should not satisfy 'postgres'"
        );
    }

    #[test]
    fn test_find_providers_exact_match_first() {
        let need = Capability::new("storage", "postgres");

        // Provider A: exact match (name = "postgres")
        let provider_a = workload_with(Vec::new(), vec![Capability::new("storage", "postgres")]);

        // Provider B: compatible match (name = "postgres-compatible")
        let provider_b = workload_with(
            Vec::new(),
            vec![Capability::new("storage", "postgres-compatible")],
        );

        let matcher = DefaultCapabilityMatcher::new();

        // Exact match should sort first regardless of input order
        let result = matcher.find_providers(&need, &[provider_b.clone(), provider_a.clone()]);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            provider_a.header().id,
            "exact match should be first"
        );
        assert_eq!(
            result[1],
            provider_b.header().id,
            "compatible match should be second"
        );
    }

    #[test]
    fn test_match_capabilities_returns_unmatched() {
        let need = Capability::new("storage", "redis");
        let provide = Capability::new("storage", "postgres");

        let consumer = workload_with(vec![need.clone()], Vec::new());
        let provider = workload_with(Vec::new(), vec![provide]);

        let matcher = DefaultCapabilityMatcher::new();
        let (results, rest) = matcher.match_capabilities(&consumer, &provider);

        assert!(results.is_empty(), "no matches expected");
        assert_eq!(rest, vec![need], "unmatched need should be returned");
    }

    #[test]
    fn test_match_capabilities_all_matched() {
        let need = Capability::new("storage", "postgres");
        let provide = Capability::new("storage", "postgres");

        let consumer = workload_with(vec![need.clone()], Vec::new());
        let provider = workload_with(Vec::new(), vec![provide.clone()]);

        let matcher = DefaultCapabilityMatcher::new();
        let (results, rest) = matcher.match_capabilities(&consumer, &provider);

        assert_eq!(results.len(), 1);
        assert!(rest.is_empty());
        assert_eq!(results[0].need, need);
        assert_eq!(results[0].provided, provide);
    }

    #[test]
    fn test_match_capabilities_purpose_filtering() {
        let need_exact = Capability::with_purpose("storage", "postgres", "analytics");
        let provide_with = Capability::with_purpose("storage", "postgres", "analytics");
        let provide_without = Capability::new("storage", "postgres");

        // need with purpose matches provide with same purpose
        let matcher = DefaultCapabilityMatcher::new();
        assert!(matcher.is_satisfiable(&need_exact, &provide_with));

        // need with purpose does NOT match provide without purpose
        assert!(!matcher.is_satisfiable(&need_exact, &provide_without));

        // need without purpose matches provide with purpose
        let need_none = Capability::new("storage", "postgres");
        assert!(matcher.is_satisfiable(&need_none, &provide_with));
    }

    // -- Property tests --------------------------------------------------

    use proptest::prelude::*;

    /// Strategy for generating a [`Capability`].
    fn arb_capability() -> impl Strategy<Value = Capability> {
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

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_capability_sorting_deterministic(
            caps in prop::collection::vec(arb_capability(), 0..20),
        ) {
            let mut sorted1 = caps.clone();
            sorted1.sort();

            let mut sorted2 = caps;
            // Reverse to perturb the input order, then re-sort.
            sorted2.reverse();
            sorted2.sort();

            prop_assert_eq!(sorted1, sorted2, "sorting must be deterministic regardless of input order");
        }

        #[test]
        fn proptest_capability_matching_deterministic(
            consumer_needs in prop::collection::vec(arb_capability(), 0..10),
            provider_provides in prop::collection::vec(arb_capability(), 0..10),
        ) {
            let consumer = workload_with(consumer_needs, Vec::new());
            let provider = workload_with(Vec::new(), provider_provides);
            let matcher = DefaultCapabilityMatcher::new();

            let result1 = matcher.match_capabilities(&consumer, &provider);
            let result2 = matcher.match_capabilities(&consumer, &provider);

            prop_assert_eq!(result1.0, result2.0, "matched capabilities must be identical");
            prop_assert_eq!(result1.1, result2.1, "unmatched capabilities must be identical");
        }
    }
}
