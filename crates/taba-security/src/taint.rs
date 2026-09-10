//! Taint computation via provenance graph traversal (INV-S4, INV-S7, INV-S9).
//!
//! Taint is computed at query time by traversing the provenance graph —
//! not cached at merge time. This makes taint eventually consistent
//! across nodes. Multi-input workloads inherit the union (most
//! restrictive) of all input classifications.
//!
//! Declassification policies (taint removal) require multi-party signing:
//! minimum 2 distinct authors — one with policy scope, one with
//! data-steward scope (INV-S9).

use std::collections::{HashMap, HashSet};

use taba_common::{AuthorId, UnitId};
use taba_core::Unit;
use taba_core::data::Classification;

use crate::error::SecurityError;

// ---------------------------------------------------------------------------
// TaintComputer trait
// ---------------------------------------------------------------------------

/// Computes data classification by traversing the provenance graph.
///
/// Taint is computed at query time, not cached at merge time (INV-S4).
/// Multi-input workloads inherit the union (most restrictive) of all
/// input classifications.
pub trait TaintComputer {
    /// Compute the effective classification of a data unit.
    ///
    /// Traverses the provenance graph from the given data unit back
    /// through all producing workloads and their inputs. The result is
    /// the join (most restrictive) of all input classifications, unless
    /// explicit declassification policies exist along the path.
    ///
    /// Enforces INV-S7: children can narrow freely but widen only with
    /// policy. Enforces INV-S9: declassification requires multi-party
    /// signing.
    ///
    /// Returns [`SecurityError::BrokenProvenance`] if the provenance
    /// chain is incomplete (references to units not yet in the local
    /// graph).
    fn compute_taint(&self, data_unit: &UnitId) -> Result<Classification, SecurityError>;

    /// Check whether a declassification policy along a provenance path
    /// is valid (multi-party signed per INV-S9).
    ///
    /// Returns [`SecurityError::DeclassificationDenied`] if the policy
    /// lacks the required signatures (minimum 2 distinct authors).
    fn validate_declassification(&self, policy_unit: &UnitId) -> Result<(), SecurityError>;
}

// ---------------------------------------------------------------------------
// DefaultTaintComputer
// ---------------------------------------------------------------------------

/// Default implementation of [`TaintComputer`].
///
/// For M2 (single-node, in-memory), holds a [`HashMap`] of units for
/// synchronous taint traversal. In production, this would be backed by
/// the in-memory graph (which implements `UnitStore`), but the
/// `UnitStore` trait is async while taint computation is an in-memory
/// query that must be synchronous (INV-S4).
#[derive(Debug, Clone, Default)]
pub struct DefaultTaintComputer {
    /// In-memory unit store for taint traversal.
    units: HashMap<UnitId, Unit>,
    /// Signers of each declassification policy (INV-S9).
    /// Maps policy unit ID to the set of distinct author IDs that signed it.
    declassification_signers: HashMap<UnitId, HashSet<AuthorId>>,
}

impl DefaultTaintComputer {
    /// Creates a new empty taint computer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            units: HashMap::new(),
            declassification_signers: HashMap::new(),
        }
    }

    /// Adds a unit to the in-memory store for taint traversal.
    pub fn add_unit(&mut self, unit: Unit) {
        self.units.insert(unit.header().id, unit);
    }

    /// Registers the signers of a declassification policy (INV-S9).
    ///
    /// A valid declassification requires at least 2 distinct signers.
    pub fn add_declassification_signers(&mut self, policy_id: UnitId, signers: HashSet<AuthorId>) {
        self.declassification_signers.insert(policy_id, signers);
    }
}

impl TaintComputer for DefaultTaintComputer {
    fn compute_taint(&self, data_unit: &UnitId) -> Result<Classification, SecurityError> {
        // Look up the data unit in the local store.
        let unit = self
            .units
            .get(data_unit)
            .ok_or_else(|| SecurityError::BrokenProvenance {
                unit: *data_unit,
                reason: "data unit not found in local store".to_string(),
            })?;

        // Extract the DataUnit variant.
        let Unit::Data(data) = unit else {
            return Err(SecurityError::BrokenProvenance {
                unit: *data_unit,
                reason: "unit is not a data unit".to_string(),
            });
        };

        // Base case: no provenance → own classification.
        let Some(provenance) = &data.provenance else {
            return Ok(data.classification);
        };

        // Recursive case: join (max) of own classification and all inputs.
        let mut result = data.classification;
        for input_id in &provenance.inputs {
            let input_taint = self.compute_taint(input_id)?;
            result = result.union(input_taint);
        }

        Ok(result)
    }

    fn validate_declassification(&self, policy_unit: &UnitId) -> Result<(), SecurityError> {
        let Some(signers) = self.declassification_signers.get(policy_unit) else {
            return Err(SecurityError::DeclassificationDenied {
                reason: "no signers found for declassification policy".to_string(),
            });
        };

        if signers.len() >= 2 {
            Ok(())
        } else {
            Err(SecurityError::DeclassificationDenied {
                reason: format!(
                    "declassification requires 2 distinct signers, got {} (INV-S9)",
                    signers.len()
                ),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{DualClockEvent, LogicalClock, UnitId, WallTime};
    use taba_core::data::{
        Classification, ConsentScope, DataSchema, Provenance, RetentionMode, RetentionPolicy,
        StorageRequirements,
    };
    use taba_core::unit::{UnitHeader, UnitState};
    use taba_core::{Capability, Unit};

    /// Creates a minimal [`UnitHeader`] for testing.
    fn test_header(id: UnitId) -> UnitHeader {
        UnitHeader {
            id,
            author: AuthorId(uuid::Uuid::new_v4()),
            trust_domain: taba_common::TrustDomainId(uuid::Uuid::new_v4()),
            created_at: DualClockEvent {
                logical_clock: LogicalClock(1),
                wall_time: WallTime { millis: 1000 },
                timezone: "UTC".to_string(),
            },
            validity: None,
            state: UnitState::Declared,
            version: None,
        }
    }

    /// Creates a [`DataUnit`] with the given classification and optional provenance.
    fn data_unit(
        id: UnitId,
        classification: Classification,
        provenance: Option<Provenance>,
    ) -> Unit {
        Unit::Data(taba_core::unit::DataUnit {
            header: test_header(id),
            schema: DataSchema {
                format: "json-schema".to_string(),
                definition: "{}".to_string(),
            },
            classification,
            provenance,
            retention: RetentionPolicy {
                mode: RetentionMode::Persistent,
                duration: Some(std::time::Duration::from_secs(86400)),
                legal_basis: "consent".to_string(),
                mandatory: false,
            },
            consent_scope: Vec::<ConsentScope>::new(),
            storage_requirements: StorageRequirements {
                encrypted_at_rest: false,
                jurisdictions: Vec::new(),
                min_replicas: None,
            },
            parent: None,
            provides: vec![Capability::new("storage", "dataset-x")],
        })
    }

    #[test]
    fn test_compute_taint_single_unit() {
        let id = UnitId(uuid::Uuid::new_v4());
        let unit = data_unit(id, Classification::Internal, None);

        let computer = DefaultTaintComputer::from_unit(unit);
        let result = computer.compute_taint(&id).expect("taint should compute");

        assert_eq!(
            result,
            Classification::Internal,
            "data unit with no provenance should have its own classification"
        );
    }

    #[test]
    fn test_compute_taint_multi_input() {
        // Input A: Public
        let id_a = UnitId(uuid::Uuid::new_v4());
        let unit_a = data_unit(id_a, Classification::Public, None);

        // Input B: PII (most restrictive)
        let id_b = UnitId(uuid::Uuid::new_v4());
        let unit_b = data_unit(id_b, Classification::Pii, None);

        // Output D: Internal, with provenance referencing A and B
        let id_d = UnitId(uuid::Uuid::new_v4());
        let provenance = Provenance {
            produced_by: UnitId(uuid::Uuid::new_v4()),
            inputs: vec![id_a, id_b],
            produced_at: DualClockEvent {
                logical_clock: LogicalClock(5),
                wall_time: WallTime { millis: 5000 },
                timezone: "UTC".to_string(),
            },
            governing_policies: Vec::new(),
        };
        let unit_d = data_unit(id_d, Classification::Internal, Some(provenance));

        let mut computer = DefaultTaintComputer::new();
        computer.add_unit(unit_a);
        computer.add_unit(unit_b);
        computer.add_unit(unit_d);

        let result = computer.compute_taint(&id_d).expect("taint should compute");

        // D is Internal, A is Public, B is PII → max(Internal, Public, PII) = PII
        assert_eq!(
            result,
            Classification::Pii,
            "multi-input taint should be the most restrictive (PII)"
        );
    }

    #[test]
    fn test_compute_taint_broken_provenance() {
        // Output D references a non-existent input.
        let missing_id = UnitId(uuid::Uuid::new_v4());
        let id_d = UnitId(uuid::Uuid::new_v4());
        let provenance = Provenance {
            produced_by: UnitId(uuid::Uuid::new_v4()),
            inputs: vec![missing_id],
            produced_at: DualClockEvent {
                logical_clock: LogicalClock(5),
                wall_time: WallTime { millis: 5000 },
                timezone: "UTC".to_string(),
            },
            governing_policies: Vec::new(),
        };
        let unit_d = data_unit(id_d, Classification::Public, Some(provenance));

        let mut computer = DefaultTaintComputer::new();
        computer.add_unit(unit_d);
        // missing_id is NOT added to the computer

        let result = computer.compute_taint(&id_d);
        assert!(
            matches!(result, Err(SecurityError::BrokenProvenance { unit, .. }) if unit == missing_id),
            "missing reference should fail with BrokenProvenance, got: {result:?}"
        );
    }

    #[test]
    fn test_validate_declassification_valid() {
        let policy_id = UnitId(uuid::Uuid::new_v4());
        let signers = HashSet::from([
            AuthorId(uuid::Uuid::new_v4()),
            AuthorId(uuid::Uuid::new_v4()),
        ]);

        let mut computer = DefaultTaintComputer::new();
        computer.add_declassification_signers(policy_id, signers);

        assert!(
            computer.validate_declassification(&policy_id).is_ok(),
            "declassification with 2 distinct signers should be valid"
        );
    }

    #[test]
    fn test_validate_declassification_insufficient_signers() {
        let policy_id = UnitId(uuid::Uuid::new_v4());
        let signers = HashSet::from([AuthorId(uuid::Uuid::new_v4())]);

        let mut computer = DefaultTaintComputer::new();
        computer.add_declassification_signers(policy_id, signers);

        let result = computer.validate_declassification(&policy_id);
        assert!(
            matches!(result, Err(SecurityError::DeclassificationDenied { .. })),
            "declassification with 1 signer should be denied (INV-S9), got: {result:?}"
        );
    }

    // -- Test helper extension -----------------------------------------------

    impl DefaultTaintComputer {
        /// Creates a taint computer from a single unit.
        fn from_unit(unit: Unit) -> Self {
            let mut computer = Self::new();
            computer.add_unit(unit);
            computer
        }
    }
}
