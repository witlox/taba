//! Solver types: composition resolution, placement, conflict detection, and scaling.
//!
//! The solver is the only "active" component in taba. It is a deterministic
//! function: given identical graph state and identical node membership, any
//! node produces identical results (INV-C3). All arithmetic uses fixed-point
//! Ppm -- no floating-point anywhere in solver paths (A2).
//!
//! Two classes of operations:
//! - Commutative (placement, scaling, health): resolve via CRDT merge.
//! - Non-commutative (policy, trust domain): safe because the role model
//!   prevents concurrent conflicting writers (A1).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::common::{NodeId, Ppm, SignedPpm, Timestamp, UnitId};
use crate::core::{
    Capability, CapabilityMatch, Classification, ConflictTuple, Scaling, UnitState,
};
use crate::gossip::MembershipView;
use crate::graph::CompositionGraph;

// ---------------------------------------------------------------------------
// Solver input / output
// ---------------------------------------------------------------------------

/// Complete input to a solver invocation.
/// Same input on any node must produce identical output (INV-C3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverInput {
    /// Snapshot of the composition graph (immutable during solver run).
    pub graph: CompositionGraph,
    /// Current cluster membership and node health.
    pub membership: MembershipView,
    /// Solver version -- all nodes must agree before producing placements (FM-12).
    pub solver_version: u64,
}

/// Complete output from a solver invocation.
/// Deterministic: identical SolverInput -> identical SolverOutput on every node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverOutput {
    /// Placement decisions for units that need (re)placement.
    pub placements: Vec<PlacementDecision>,
    /// Composition results: successful capability matches.
    pub compositions: Vec<CompositionResult>,
    /// Conflicts that require policy resolution before composition can proceed.
    pub conflicts: Vec<ConflictReport>,
    /// Scaling decisions derived from unit-declared parameters (INV-K4).
    pub scaling_decisions: Vec<ScalingDecision>,
    /// When this solver run was computed.
    pub computed_at: Timestamp,
    /// The solver version that produced this output.
    pub solver_version: u64,
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// Assignment of a unit to a specific node.
/// Deterministic: same graph + same nodes = same placement (INV-C3).
/// Partition tiebreaker: lexicographically lowest NodeId wins (INV-C3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecision {
    /// The unit being placed.
    pub unit_id: UnitId,
    /// The node this unit is assigned to.
    pub target_node: NodeId,
    /// Scoring breakdown for auditability.
    pub score: PlacementScore,
    /// Why this placement was chosen (for debugging and audit).
    pub reason: PlacementReason,
}

/// Scoring breakdown for a placement decision.
/// All values are in Ppm (fixed-point, 10^6 scale) for determinism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementScore {
    /// Weighted total score. Higher is better.
    pub total: Ppm,
    /// Resource availability score (CPU, memory, storage headroom).
    pub resource_score: Ppm,
    /// Latency/proximity score relative to dependencies.
    pub latency_score: Ppm,
    /// Affinity/anti-affinity score based on tolerance declarations.
    pub affinity_score: Ppm,
    /// Health score of the candidate node.
    pub health_score: Ppm,
    /// Penalty applied if the node is in Suspected state (INV-R5).
    pub suspected_penalty: Ppm,
}

/// Why a particular placement was selected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PlacementReason {
    /// Normal placement based on scoring.
    BestScore,
    /// Re-placement due to node failure.
    NodeFailure { failed_node: NodeId },
    /// Re-placement due to partition tiebreaker (INV-C3).
    PartitionTiebreak { winner: NodeId, loser: NodeId },
    /// Placement due to scaling event.
    ScaleUp,
}

// ---------------------------------------------------------------------------
// Composition resolution
// ---------------------------------------------------------------------------

/// Result of resolving capability matches between units.
/// Composition is independent of insertion order (INV-C6).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionResult {
    /// The set of units participating in this composition.
    pub participants: Vec<UnitId>,
    /// The capability matches that bind these units together.
    pub matches: Vec<CapabilityMatch>,
    /// Whether all needs are satisfied.
    pub fully_satisfied: bool,
    /// Capabilities still unmatched (empty if fully_satisfied).
    pub unmatched_needs: Vec<UnmatchedNeed>,
}

/// A capability need that could not be matched to any provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmatchedNeed {
    /// The unit that declared the need.
    pub unit_id: UnitId,
    /// The capability that could not be matched.
    pub capability: Capability,
}

// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------

/// A detected conflict requiring policy resolution.
/// The solver fails closed until explicit policy resolves it (INV-S2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    /// The conflict tuple identifying the conflicting units and capability.
    pub conflict: ConflictTuple,
    /// Human-readable description of the conflict.
    pub description: String,
    /// The type of conflict detected.
    pub conflict_type: ConflictType,
    /// Whether a policy already exists for this conflict.
    pub has_policy: bool,
}

/// Types of conflict the solver can detect.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConflictType {
    /// Incompatible security requirements between units.
    SecurityIncompatible,
    /// Ambiguous capability match (multiple providers, no disambiguation).
    AmbiguousMatch,
    /// Purpose mismatch on capability (INV-K2).
    PurposeMismatch,
    /// Classification conflict between data units (INV-S7).
    ClassificationConflict,
    /// Cyclic recovery dependencies (INV-K5).
    CyclicRecoveryDependency,
    /// Conflicting retention requirements (FM-10).
    RetentionConflict,
}

// ---------------------------------------------------------------------------
// Scaling
// ---------------------------------------------------------------------------

/// A scaling decision derived from unit-declared parameters (INV-K4).
/// The solver does not invent scaling logic -- it evaluates declared triggers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalingDecision {
    /// The unit to scale.
    pub unit_id: UnitId,
    /// Current instance count.
    pub current_instances: u32,
    /// Target instance count after scaling.
    pub target_instances: u32,
    /// Which trigger fired.
    pub trigger_name: String,
    /// The metric value that triggered the scaling (in Ppm).
    pub metric_value: Ppm,
    /// The threshold that was crossed (in Ppm).
    pub threshold: Ppm,
}
