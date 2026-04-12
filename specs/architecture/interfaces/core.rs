// taba-core: Core unit model, capability matching, and abstract storage.
//
// This crate defines the foundational types and traits that all other crates
// depend on. It owns the Unit type hierarchy, capability algebra, and the
// abstract storage contract that taba-graph implements.

use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// Placeholder types — defined here for trait signatures only.
// Actual definitions live in the data-models spec.
// ---------------------------------------------------------------------------

/// Globally unique, immutable identifier for a unit.
pub struct UnitId(/* opaque */);

/// Typed, self-describing, signed entity. The core primitive.
pub struct Unit(/* opaque */);

/// The four unit kinds: Workload, Data, Policy, Governance.
pub enum UnitKind {
    Workload,
    Data,
    Policy,
    Governance,
}

/// Typed capability tuple: (type, name, optional purpose qualifier).
/// Sorted lexicographically for deterministic matching (INV-K2).
pub struct Capability(/* opaque */);

/// A matched pair: a declared need satisfied by a declared provide.
pub struct CapabilityMatch(/* opaque */);

/// Identifier for an author with scoped unit-creation authority.
pub struct AuthorId(/* opaque */);

/// Identifier for a trust domain boundary.
pub struct TrustDomainId(/* opaque */);

/// Snapshot of the composition graph at a point in time.
pub struct GraphSnapshot(/* opaque */);

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by taba-core operations.
pub enum CoreError {
    /// Unit failed structural validation (missing fields, invalid type, etc.).
    MalformedUnit { reason: String },
    /// Author scope does not permit creating this unit type in this trust domain.
    ScopeViolation { author: AuthorId, unit_kind: UnitKind, domain: TrustDomainId },
    /// Capability list is not well-formed (duplicates, invalid type, etc.).
    InvalidCapability { reason: String },
    /// Referenced unit does not exist in the store.
    UnitNotFound { id: UnitId },
    /// Storage backend error.
    StoreError { reason: String },
}

// ---------------------------------------------------------------------------
// Traits
// ---------------------------------------------------------------------------

/// Validates that a unit is well-formed before it enters the graph.
///
/// This is a structural check only — it does NOT verify signatures (that is
/// `security::Verifier`) or check graph-level constraints (that is
/// `graph::MergePolicy`). Think of it as the unit's type-checker.
pub trait UnitValidator {
    /// Check that the unit is structurally valid.
    ///
    /// Validates:
    /// - All required fields present for the unit's kind
    /// - Capability lists are well-formed and sorted (INV-K2)
    /// - Data unit hierarchy depth <= 16 (domain-model constraint)
    /// - Recovery relationships contain no self-references
    /// - Scaling parameters are internally consistent (min <= max)
    /// - Retention declarations parse correctly
    ///
    /// Returns `Ok(())` on success, `CoreError::MalformedUnit` on failure.
    fn validate(&self, unit: &Unit) -> Result<(), CoreError>;

    /// Check that the author's scope permits creating this unit.
    ///
    /// Enforces INV-S5: authors cannot create units outside their
    /// (type scope x trust domain scope). Also enforces INV-S8: no two
    /// distinct authors share identical scope tuples.
    ///
    /// Returns `CoreError::ScopeViolation` if the author lacks authority.
    fn validate_author_scope(
        &self,
        author: &AuthorId,
        unit: &Unit,
        domain: &TrustDomainId,
    ) -> Result<(), CoreError>;
}

/// Matches capability needs to capability provides.
///
/// Implements the typed capability algebra described in INV-K2. Matching is
/// deterministic: capability lists are sorted lexicographically by
/// (type, name, purpose) before comparison.
pub trait CapabilityMatcher {
    /// Attempt to match all needs of `consumer` against provides of `provider`.
    ///
    /// Returns the set of successful matches. Unmatched needs are returned as
    /// the second element. If any need has a purpose qualifier that does not
    /// match the provide's purpose, it is treated as unmatched (triggers
    /// conflict requiring policy per INV-K2).
    ///
    /// This is a pure function: no I/O, no side effects, deterministic.
    fn match_capabilities(
        &self,
        consumer: &Unit,
        provider: &Unit,
    ) -> (Vec<CapabilityMatch>, Vec<Capability>);

    /// Check whether a single need is satisfiable by a single provide.
    ///
    /// Handles type compatibility (e.g., "postgres-compatible" satisfies
    /// "postgres") and purpose qualifier matching. Returns `true` if the
    /// provide satisfies the need, `false` otherwise.
    fn is_satisfiable(&self, need: &Capability, provide: &Capability) -> bool;

    /// Given a set of providers, find all possible satisfiers for a given need.
    ///
    /// Returns providers sorted by match specificity (exact match first, then
    /// compatible matches). Deterministic ordering for solver reproducibility.
    fn find_providers(
        &self,
        need: &Capability,
        providers: &[Unit],
    ) -> Vec<UnitId>;
}

/// Abstract CRUD storage for units.
///
/// This trait decouples the core unit logic from the CRDT graph implementation.
/// The concrete implementation lives in taba-graph. In tests, this can be
/// backed by an in-memory map.
///
/// All mutating operations are async because the backing store may involve
/// WAL writes or network I/O.
pub trait UnitStore {
    /// Retrieve a unit by its ID.
    ///
    /// Returns `CoreError::UnitNotFound` if no unit with that ID exists in
    /// the store. Does NOT return archived or compacted units.
    async fn get(&self, id: &UnitId) -> Result<Unit, CoreError>;

    /// Check whether a unit exists in the store (including pending state).
    async fn contains(&self, id: &UnitId) -> Result<bool, CoreError>;

    /// Insert a unit into the store.
    ///
    /// The unit MUST have passed `UnitValidator::validate` and
    /// `security::Verifier::verify` before calling this method. This method
    /// does not re-validate — it trusts the caller.
    ///
    /// If the unit has unsatisfied references, it enters the pending queue
    /// (causal buffering per INV-C4). Otherwise it is immediately active.
    ///
    /// Returns `CoreError::StoreError` on persistence failure.
    async fn insert(&self, unit: Unit) -> Result<(), CoreError>;

    /// Remove a unit from the active store (mark for archival).
    ///
    /// The unit is not physically deleted — it transitions to archived state.
    /// Archived units are eligible for compaction. Returns
    /// `CoreError::UnitNotFound` if the unit does not exist.
    async fn archive(&self, id: &UnitId) -> Result<(), CoreError>;

    /// List all active (non-archived, non-pending) units of a given kind.
    async fn list_by_kind(&self, kind: UnitKind) -> Result<Vec<Unit>, CoreError>;

    /// List all units currently in the pending queue awaiting causal delivery.
    async fn list_pending(&self) -> Result<Vec<Unit>, CoreError>;
}
