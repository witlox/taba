//! Data governance: classification lattice, provenance, retention, consent.
//!
//! Data units carry structural governance metadata — classification,
//! provenance chains, retention policies, and consent scopes. Taint
//! propagation follows the classification lattice: if input data is
//! classified, output inherits that classification unless explicit
//! policy declassifies (INV-S4, INV-S7).

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, UnitId};

// ---------------------------------------------------------------------------
// Classification lattice
// ---------------------------------------------------------------------------

/// Data classification lattice: `Public < Internal < Confidential < Pii`.
///
/// Taint propagation follows this lattice (INV-S4). Multi-input
/// workloads inherit the union (most restrictive) of all input
/// classifications — computed at query time by traversing the
/// provenance graph (INV-S7, DL-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Classification {
    /// Unrestricted data.
    Public = 0,
    /// Organization-internal only.
    Internal = 1,
    /// Sensitive business data.
    Confidential = 2,
    /// Personally identifiable information — most restrictive.
    Pii = 3,
}

impl Classification {
    /// Returns the most restrictive (highest) of two classifications.
    ///
    /// This is the lattice join operation. Multi-input workloads
    /// inherit the union of all input classifications (INV-S4).
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        std::cmp::max(self, other)
    }
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Provenance chain for a data unit (INV-D1).
///
/// Links back to the producing workload and input data units.
/// References to units not yet in the local graph are marked pending
/// (causal buffering per INV-C4). Provenance is verified at query
/// time, not merge time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// The workload unit that produced this data.
    pub produced_by: UnitId,
    /// The input data units consumed by the producing workload.
    pub inputs: Vec<UnitId>,
    /// When this data was produced (dual clock — logical for ordering,
    /// wall time for compliance).
    pub produced_at: DualClockEvent,
    /// Policies in effect at production time.
    pub governing_policies: Vec<UnitId>,
}

// ---------------------------------------------------------------------------
// Retention
// ---------------------------------------------------------------------------

/// Retention policy for a data unit (INV-D2, INV-D4).
///
/// Determines lifecycle: persistent, ephemeral, or local-only.
/// Expired data units are eligible for compaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    /// Retention mode (persistent / ephemeral / local-only).
    pub mode: RetentionMode,
    /// How long to retain this data (wall time, for persistent mode).
    pub duration: Option<std::time::Duration>,
    /// Legal basis for retention (e.g., `"GDPR Art. 6(1)(f)"`).
    pub legal_basis: String,
    /// Whether retention is mandatory (must keep) or permissive (may delete).
    pub mandatory: bool,
}

/// Data retention mode (progressive disclosure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RetentionMode {
    /// Default. Governed by duration + legal basis. Tombstoned on expiry.
    Persistent,
    /// Auto-removed when producing bounded task terminates.
    /// Reference check: has refs → tombstone, no refs → full remove (INV-D4).
    Ephemeral,
    /// Never enters graph. Node-local scratch only.
    /// Requires policy for classification > Public (INV-D5).
    LocalOnly,
}

// ---------------------------------------------------------------------------
// Consent
// ---------------------------------------------------------------------------

/// What a data unit may be used for.
///
/// Expressed as purpose qualifiers on provided capabilities. A
/// capability with a declared purpose must match during composition
/// (INV-K2). Purpose mismatch triggers a conflict requiring policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentScope {
    /// The permitted purpose (e.g., `"analytics"`, `"personalization"`, `"billing"`).
    pub purpose: Purpose,
    /// Whether consent was explicit or derived.
    pub consent_type: ConsentType,
}

/// A named purpose for data usage.
///
/// Not a classification — a purpose is a capability qualifier
/// (e.g., `"analytics"` is a purpose, `Pii` is a classification).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Purpose(pub String);

/// How consent was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConsentType {
    /// User explicitly consented.
    Explicit,
    /// Consent derived from a legal basis.
    LegalBasis,
    /// Consent inherited from parent data unit.
    Inherited,
}

// ---------------------------------------------------------------------------
// Schema & storage
// ---------------------------------------------------------------------------

/// Typed schema declaration for a data unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSchema {
    /// Schema format (e.g., `"json-schema"`, `"protobuf"`, `"avro"`).
    pub format: String,
    /// The schema definition (inline or reference).
    pub definition: String,
}

/// Storage requirements for a data unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageRequirements {
    /// Whether data must be encrypted at rest.
    pub encrypted_at_rest: bool,
    /// Jurisdiction constraints (e.g., `"EU"`, `"CH"`).
    pub jurisdictions: Vec<String>,
    /// Minimum replication factor (if applicable beyond erasure coding).
    pub min_replicas: Option<u32>,
}

// ---------------------------------------------------------------------------
// Hierarchy
// ---------------------------------------------------------------------------

/// Position of a data unit in its hierarchy (INV-D3).
///
/// Children exist only where constraints diverge from parent.
/// Maximum depth: 16 levels (enforced at graph merge). Narrowing
/// (child more restrictive) is always allowed; widening (child less
/// restrictive) requires explicit policy (INV-S7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataHierarchy {
    /// This unit's position in the hierarchy (0 = root).
    pub depth: u8,
    /// Direct children of this data unit.
    pub children: Vec<UnitId>,
    /// Whether this unit narrows or widens parent constraints (INV-S7).
    pub constraint_relation: ConstraintRelation,
}

/// Relationship of a child data unit's constraints to its parent (INV-S7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConstraintRelation {
    /// Same constraints as parent (should not exist per INV-D3, but valid during authoring).
    Identical,
    /// More restrictive than parent — always allowed.
    Narrowed,
    /// Less restrictive than parent — requires explicit policy.
    Widened,
}

// ---------------------------------------------------------------------------
// Taint result
// ---------------------------------------------------------------------------

/// Result of computing taint propagation through the provenance graph (INV-S4).
///
/// Taint is computed at query time by traversing the provenance graph
/// (DL-007). Multi-input workloads inherit the union (most restrictive)
/// of all input classifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintResult {
    /// The unit whose taint was computed.
    pub unit_id: UnitId,
    /// The computed classification after taint propagation.
    pub effective_classification: Classification,
    /// The input units that contributed to this classification.
    pub contributing_inputs: Vec<UnitId>,
    /// Whether declassification policy was applied.
    pub declassified: bool,
    /// The policy that authorized declassification, if any.
    pub declassification_policy: Option<UnitId>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classification_ordering() {
        assert!(Classification::Public < Classification::Internal);
        assert!(Classification::Internal < Classification::Confidential);
        assert!(Classification::Confidential < Classification::Pii);

        // Full chain
        assert!(Classification::Public < Classification::Pii);
    }

    #[test]
    fn test_classification_lattice_union() {
        // max(a, b) gives the most restrictive
        assert_eq!(
            Classification::Public.union(Classification::Internal),
            Classification::Internal
        );
        assert_eq!(
            Classification::Internal.union(Classification::Public),
            Classification::Internal
        );
        assert_eq!(
            Classification::Confidential.union(Classification::Pii),
            Classification::Pii
        );
        assert_eq!(
            Classification::Pii.union(Classification::Pii),
            Classification::Pii
        );
    }

    #[test]
    fn test_classification_discriminant_values() {
        // The discriminant values matter for the lattice ordering.
        assert_eq!(Classification::Public as u8, 0);
        assert_eq!(Classification::Internal as u8, 1);
        assert_eq!(Classification::Confidential as u8, 2);
        assert_eq!(Classification::Pii as u8, 3);
    }

    #[test]
    fn test_retention_policy_serialization_roundtrip() {
        let policy = RetentionPolicy {
            mode: RetentionMode::Persistent,
            duration: Some(std::time::Duration::from_secs(86_400 * 365)),
            legal_basis: "GDPR Art. 6(1)(f)".to_string(),
            mandatory: true,
        };

        let json = serde_json::to_string(&policy).expect("serialize RetentionPolicy");
        let decoded: RetentionPolicy =
            serde_json::from_str(&json).expect("deserialize RetentionPolicy");
        assert_eq!(policy, decoded);
    }

    #[test]
    fn test_consent_scope_serialization_roundtrip() {
        let scope = ConsentScope {
            purpose: Purpose("analytics".to_string()),
            consent_type: ConsentType::Explicit,
        };

        let json = serde_json::to_string(&scope).expect("serialize ConsentScope");
        let decoded: ConsentScope = serde_json::from_str(&json).expect("deserialize ConsentScope");
        assert_eq!(scope, decoded);
    }

    #[test]
    fn test_data_schema_serialization_roundtrip() {
        let schema = DataSchema {
            format: "json-schema".to_string(),
            definition: "{\"type\":\"object\"}".to_string(),
        };

        let json = serde_json::to_string(&schema).expect("serialize DataSchema");
        let decoded: DataSchema = serde_json::from_str(&json).expect("deserialize DataSchema");
        assert_eq!(schema, decoded);
    }

    #[test]
    fn test_storage_requirements_serialization_roundtrip() {
        let req = StorageRequirements {
            encrypted_at_rest: true,
            jurisdictions: vec!["EU".to_string(), "CH".to_string()],
            min_replicas: Some(3),
        };

        let json = serde_json::to_string(&req).expect("serialize StorageRequirements");
        let decoded: StorageRequirements =
            serde_json::from_str(&json).expect("deserialize StorageRequirements");
        assert_eq!(req, decoded);
    }

    #[test]
    fn test_data_hierarchy_serialization_roundtrip() {
        let h = DataHierarchy {
            depth: 2,
            children: vec![UnitId(uuid::Uuid::new_v4())],
            constraint_relation: ConstraintRelation::Narrowed,
        };

        let json = serde_json::to_string(&h).expect("serialize DataHierarchy");
        let decoded: DataHierarchy =
            serde_json::from_str(&json).expect("deserialize DataHierarchy");
        assert_eq!(h, decoded);
    }

    #[test]
    fn test_constraint_relation_ordering() {
        // Not ordered by value, but equality works
        assert_eq!(ConstraintRelation::Narrowed, ConstraintRelation::Narrowed);
        assert_ne!(ConstraintRelation::Narrowed, ConstraintRelation::Widened);
    }
}
