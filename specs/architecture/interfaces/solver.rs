// taba-solver: Deterministic composition solver, conflict detection,
// placement scoring, and cycle detection.
//
// The solver is a pure function: graph snapshot + membership -> placements.
// All arithmetic is fixed-point ppm (10^6, u64). No floating-point anywhere
// in this crate (INV-C3, DL-004). Composition result is order-independent
// (INV-C6).

// ---------------------------------------------------------------------------
// Placeholder types
// ---------------------------------------------------------------------------

pub struct UnitId(/* opaque */);
pub struct Unit(/* opaque */);
pub struct NodeId(/* opaque */);
pub struct Capability(/* opaque */);
pub struct TrustDomainId(/* opaque */);

/// Point-in-time graph snapshot (from `graph::Graph::snapshot`).
pub struct GraphSnapshot(/* opaque */);

/// Current cluster membership (from `gossip::MembershipView`).
pub struct MembershipSnapshot(/* opaque */);

/// Fixed-point arithmetic at parts-per-million scale (10^6).
/// All solver scoring uses this type. Division rounds toward zero.
/// No floating-point conversions are permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ppm(pub u64);

/// A placement decision: which unit goes on which node.
pub struct Placement {
    pub unit: UnitId,
    pub node: NodeId,
    pub score: Ppm,
}

/// The full result of a solver run.
pub struct SolverResult {
    /// Units successfully placed.
    pub placements: Vec<Placement>,
    /// Units that could not be placed (unresolved conflicts, no capable node).
    pub unplaceable: Vec<(UnitId, SolverError)>,
    /// Detected conflicts requiring policy resolution.
    pub conflicts: Vec<Conflict>,
}

/// A detected conflict between units.
pub struct Conflict {
    /// The units involved in the conflict.
    pub units: Vec<UnitId>,
    /// The capability that triggered the conflict.
    pub capability: Capability,
    /// Whether a policy exists but is insufficient, or no policy exists at all.
    pub status: ConflictStatus,
}

pub enum ConflictStatus {
    /// No policy unit resolves this conflict.
    Unresolved,
    /// A policy exists but has been revoked without replacement.
    Revoked,
    /// Multiple non-revoked policies claim to resolve this conflict (INV-C7 violation).
    Ambiguous,
}

/// A cycle in recovery dependencies.
pub struct RecoveryCycle {
    /// The unit IDs forming the cycle, in dependency order.
    pub chain: Vec<UnitId>,
}

/// The supersession state of a conflict tuple's policy chain.
pub struct SupersessionChain {
    pub conflict_units: Vec<UnitId>,
    pub capability: Capability,
    /// Ordered from oldest to newest. Last entry is the active policy (if not revoked).
    pub policies: Vec<UnitId>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

pub enum SolverError {
    /// No node can satisfy the unit's capability needs.
    NoCapableNode { unit: UnitId, unmet: Vec<Capability> },
    /// Unresolved security conflict blocks composition (fail closed, INV-S2).
    UnresolvedConflict { conflict: Conflict },
    /// Circular recovery dependency detected (INV-K5).
    CyclicDependency { cycle: RecoveryCycle },
    /// Tolerance declarations cannot be met (latency, resource, etc.).
    ToleranceViolation { unit: UnitId, reason: String },
    /// Policy supersession chain is broken.
    BrokenSupersession { reason: String },
    /// Internal error (should not happen in correct implementation).
    InternalError { reason: String },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// The core solver: computes placements from graph state and membership.
///
/// The solver is deterministic (INV-C3): given identical graph snapshot and
/// identical membership, any node produces identical placement decisions.
/// Composition is order-independent (INV-C6).
///
/// The solver is a pure function. It does not mutate the graph or membership.
/// It reads only from the snapshots provided. No I/O, no randomness, no
/// floating-point.
pub trait Solver {
    /// Compute all placements for the current graph state.
    ///
    /// Evaluates all units in the graph. For each unit:
    /// 1. Match capabilities (needs vs. provides) across all units
    /// 2. Check for security conflicts (fail closed per INV-S2)
    /// 3. Check for cyclic recovery dependencies (fail closed per INV-K5)
    /// 4. Score candidate nodes (using `PlacementScorer`)
    /// 5. Place on highest-scoring node
    ///
    /// Tiebreaker: lexicographically lowest NodeId wins (INV-C3).
    ///
    /// Returns the full result including successful placements, unplaceable
    /// units, and detected conflicts. Never panics — all error cases are
    /// represented in `SolverResult`.
    fn solve(
        &self,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> SolverResult;

    /// Re-evaluate placements affected by a specific set of changed units.
    ///
    /// Incremental version of `solve`. Only recomputes compositions that
    /// involve at least one of the changed units. The result is identical
    /// to what `solve` would produce for those units — this is an
    /// optimization, not a different algorithm.
    fn solve_incremental(
        &self,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
        changed: &[UnitId],
    ) -> SolverResult;
}

/// Detects and reports conflicts in the composition graph.
///
/// A conflict occurs when capability declarations are incompatible and no
/// policy resolves them, or when the policy chain is broken. The solver
/// calls this as part of `solve`, but it is a separate trait for testability.
pub trait ConflictDetector {
    /// Find all unresolved conflicts in the graph.
    ///
    /// Scans all capability need/provide pairs. A conflict exists when:
    /// - A need has no matching provide (missing capability)
    /// - A need matches a provide but purpose qualifiers conflict (INV-K2)
    /// - A need matches but security requirements are incompatible (INV-S2)
    ///   and no policy resolves the incompatibility
    ///
    /// Returns all conflicts found. An empty list means composition is clean.
    fn detect_conflicts(&self, graph: &GraphSnapshot) -> Vec<Conflict>;

    /// Check the policy supersession chain for a specific conflict tuple.
    ///
    /// Verifies INV-C7: exactly zero or one non-revoked policy per conflict
    /// tuple. Reports broken chains, ambiguous policies, and orphaned policies.
    fn check_supersession(
        &self,
        graph: &GraphSnapshot,
        conflict_units: &[UnitId],
        capability: &Capability,
    ) -> Result<SupersessionChain, SolverError>;
}

/// Scores candidate nodes for unit placement.
///
/// All arithmetic is fixed-point Ppm(u64). No floating-point. Division
/// rounds toward zero. Scores are comparable and deterministic (INV-C3).
pub trait PlacementScorer {
    /// Score a single candidate node for a specific unit.
    ///
    /// Considers:
    /// - Capability match quality (exact > compatible)
    /// - Node health (Active > Suspected per INV-R5)
    /// - Resource availability vs. unit tolerance declarations (INV-K3)
    /// - Scaling parameters (INV-K4)
    /// - Existing placements on the node (spread vs. pack)
    ///
    /// Returns Ppm(0) if the node cannot host the unit at all.
    /// Higher scores are better. Scores are deterministic given the same
    /// inputs.
    fn score(
        &self,
        unit: &Unit,
        node: &NodeId,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> Ppm;

    /// Score all candidate nodes for a unit, returning them sorted by score
    /// (descending). Ties broken by lexicographically lowest NodeId.
    fn rank_nodes(
        &self,
        unit: &Unit,
        graph: &GraphSnapshot,
        membership: &MembershipSnapshot,
    ) -> Vec<(NodeId, Ppm)>;
}

/// Detects circular recovery dependencies among units.
///
/// Recovery relationships declare dependency ordering on failure. Cycles
/// in these relationships are unresolvable without explicit policy (INV-K5).
pub trait CycleDetector {
    /// Find all cycles in recovery dependency declarations.
    ///
    /// Scans all units' recovery relationships and detects strongly connected
    /// components of size > 1.
    ///
    /// Returns all cycles found. Each cycle includes the full chain of
    /// unit IDs. An empty list means no circular dependencies exist.
    fn detect_cycles(&self, graph: &GraphSnapshot) -> Vec<RecoveryCycle>;

    /// Check whether adding a unit would introduce a cycle.
    ///
    /// Used as a pre-check before insertion. Does not mutate anything.
    /// Returns `Ok(())` if no cycle would be introduced, or the specific
    /// cycle that would result.
    fn would_cycle(
        &self,
        graph: &GraphSnapshot,
        new_unit: &Unit,
    ) -> Result<(), RecoveryCycle>;
}
