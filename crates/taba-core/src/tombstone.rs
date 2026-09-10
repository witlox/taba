//! Tombstones for graph compaction (INV-G2).
//!
//! A [`Tombstone`] is a minimal record replacing a compacted unit in
//! the graph. It preserves provenance graph structure (identity,
//! references, termination reason) without carrying the full unit
//! content. The original content is retrievable from an archive
//! backend by digest, if one was configured.

use serde::{Deserialize, Serialize};

use taba_common::{AuthorId, ContentDigest, LogicalClock, UnitId};

// ---------------------------------------------------------------------------
// Tombstone
// ---------------------------------------------------------------------------

/// Minimal record replacing a compacted unit in the graph.
///
/// Preserves provenance graph structure without content. Compaction
/// eligibility is deterministic (INV-G1); tombstones are monotonic
/// and converge via CRDT. Governance units, active policies, and the
/// root ceremony chain are never compacted (INV-G3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    /// Original unit ID (preserved).
    pub unit_id: UnitId,
    /// Original author ID (preserved).
    pub author_id: AuthorId,
    /// Original unit type.
    pub unit_type: TombstoneUnitType,
    /// When the original unit was created (logical clock).
    pub created_at_lc: LogicalClock,
    /// When the unit was terminated or compacted (logical clock).
    pub terminated_at_lc: LogicalClock,
    /// Why the unit was terminated.
    pub termination_reason: TerminationReason,
    /// References: what the unit consumed or produced (preserves
    /// provenance graph, INV-D1).
    pub references: Vec<UnitId>,
    /// SHA-256 of the original unit content (for archive retrieval).
    pub original_digest: ContentDigest,
}

/// The type of unit a tombstone replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TombstoneUnitType {
    /// Replaces a workload unit.
    Workload,
    /// Replaces a data unit.
    Data,
    /// Replaces a policy unit.
    Policy,
}

/// Why a unit was terminated and became eligible for compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TerminationReason {
    /// Unit completed successfully (exit code 0).
    Completed,
    /// Unit failed (exit code non-zero after retry exhaustion).
    Failed,
    /// Bounded task deadline exceeded (INV-W2).
    DeadlineExceeded,
    /// Unit was superseded by a newer version.
    Superseded,
    /// Data retention period elapsed (INV-D2).
    RetentionExpired,
    /// Unit was drained during graceful shutdown.
    Drained,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tombstone_serialization_roundtrip() {
        let tombstone = Tombstone {
            unit_id: UnitId(uuid::Uuid::new_v4()),
            author_id: AuthorId(uuid::Uuid::new_v4()),
            unit_type: TombstoneUnitType::Workload,
            created_at_lc: LogicalClock(42),
            terminated_at_lc: LogicalClock(100),
            termination_reason: TerminationReason::Completed,
            references: vec![UnitId(uuid::Uuid::new_v4()), UnitId(uuid::Uuid::new_v4())],
            original_digest: ContentDigest("sha256:abc123def456".to_string()),
        };

        let json = serde_json::to_string(&tombstone).expect("serialize Tombstone");
        let decoded: Tombstone = serde_json::from_str(&json).expect("deserialize Tombstone");
        assert_eq!(tombstone, decoded);
    }

    #[test]
    fn test_termination_reason_variants() {
        let reasons = [
            TerminationReason::Completed,
            TerminationReason::Failed,
            TerminationReason::DeadlineExceeded,
            TerminationReason::Superseded,
            TerminationReason::RetentionExpired,
            TerminationReason::Drained,
        ];

        for reason in reasons {
            let json = serde_json::to_string(&reason).expect("serialize TerminationReason");
            let decoded: TerminationReason =
                serde_json::from_str(&json).expect("deserialize TerminationReason");
            assert_eq!(reason, decoded);
        }
    }

    #[test]
    fn test_tombstone_unit_type_variants() {
        let types = [
            TombstoneUnitType::Workload,
            TombstoneUnitType::Data,
            TombstoneUnitType::Policy,
        ];

        for unit_type in types {
            let json = serde_json::to_string(&unit_type).expect("serialize TombstoneUnitType");
            let decoded: TombstoneUnitType =
                serde_json::from_str(&json).expect("deserialize TombstoneUnitType");
            assert_eq!(unit_type, decoded);
        }
    }

    #[test]
    fn test_tombstone_preserves_references() {
        let refs = vec![
            UnitId(uuid::Uuid::new_v4()),
            UnitId(uuid::Uuid::new_v4()),
            UnitId(uuid::Uuid::new_v4()),
        ];
        let tombstone = Tombstone {
            unit_id: UnitId(uuid::Uuid::new_v4()),
            author_id: AuthorId(uuid::Uuid::new_v4()),
            unit_type: TombstoneUnitType::Data,
            created_at_lc: LogicalClock(10),
            terminated_at_lc: LogicalClock(50),
            termination_reason: TerminationReason::RetentionExpired,
            references: refs.clone(),
            original_digest: ContentDigest("sha256:xyz".to_string()),
        };

        assert_eq!(tombstone.references, refs);
        assert_eq!(tombstone.references.len(), 3);
    }
}
