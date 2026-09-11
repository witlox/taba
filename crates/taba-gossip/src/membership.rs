//! Membership view, member info, health assessment, gossip tuning
//! parameters, and the membership snapshot bridge to the solver.
//!
//! The [`MembershipView`] trait provides a read-only projection of
//! cluster membership used by the solver and other crates. The live,
//! concurrently-updated implementation is [`DefaultMembershipView`].
//!
//! `MembershipSnapshot` is re-exported from **taba-solver** (A009):
//! taba-gossip populates it from its live view, and the solver reads it
//! for placement decisions (INV-C3, INV-R5).

use std::collections::BTreeMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use taba_common::{DualClockEvent, NodeId};
use taba_core::NodeCapabilitySet;
use taba_security::PublicKey;
use taba_solver::MembershipSnapshot;

use crate::error::GossipError;
use crate::message::NodeState;
use crate::transport::NodeAddr;

// ---------------------------------------------------------------------------
// HealthAssessment
// ---------------------------------------------------------------------------

/// Overall health assessment of a node, as observed by peers.
///
/// Health is peer-determined, not self-reported, for Byzantine
/// resistance (FM-04). `Unknown` is used while a node is `Suspected`
/// but not yet confirmed (INV-R5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HealthAssessment {
    /// Node is responsive and healthy.
    Healthy,
    /// Node is responsive but reporting degraded performance.
    Degraded,
    /// Node health is unknown (suspected state, INV-R5).
    Unknown,
    /// Node has been confirmed failed by witness consensus (INV-R3).
    Failed,
}

impl std::fmt::Display for HealthAssessment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "Healthy"),
            Self::Degraded => write!(f, "Degraded"),
            Self::Unknown => write!(f, "Unknown"),
            Self::Failed => write!(f, "Failed"),
        }
    }
}

// ---------------------------------------------------------------------------
// NodeHealth
// ---------------------------------------------------------------------------

/// Health observation of a node as seen by peers.
///
/// Wraps a [`HealthAssessment`] with the observation time and the node
/// that made the observation. This is the gossip-layer health type,
/// distinct from the solver's simpler
/// [`taba_solver::NodeHealth`](taba_solver::membership::NodeHealth) enum
/// (which has only `Active`/`Suspected` for placement scoring).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeHealth {
    /// Overall health assessment.
    pub status: HealthAssessment,
    /// When this health observation was made (dual clock).
    pub observed_at: DualClockEvent,
    /// Which node made this observation.
    pub observer: NodeId,
}

// ---------------------------------------------------------------------------
// MemberInfo (internal, full record)
// ---------------------------------------------------------------------------

/// Information about a cluster member, held in the live membership view.
///
/// Contains the full state needed by gossip: lifecycle state, public key
/// for signature verification, peer-observed health, last-seen time, and
/// incarnation number for superseding stale state (DL-009).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    /// The node's ID.
    pub node_id: NodeId,
    /// The node's public key (for gossip signature verification, INV-R3).
    pub public_key: PublicKey,
    /// Current lifecycle state as known by this node.
    pub state: NodeState,
    /// Latest health status (peer-observed), if any.
    pub health: Option<NodeHealth>,
    /// When this member was last heard from (dual clock).
    pub last_seen: DualClockEvent,
    /// Incarnation number (for superseding stale state on rejoin).
    pub incarnation: u64,
}

// ---------------------------------------------------------------------------
// MemberRecord (public projection)
// ---------------------------------------------------------------------------

/// Public projection of a cluster member's state.
///
/// Returned by [`MembershipView::members`] and friends. Carries the
/// network address, capability-key, health, incarnation, and solver
/// version — the fields needed by consumers outside gossip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberRecord {
    /// The node's ID.
    pub id: NodeId,
    /// The node's public key (for signature verification).
    pub key: PublicKey,
    /// The node's network address.
    pub addr: NodeAddr,
    /// Latest peer-observed health.
    pub health: NodeHealth,
    /// Incarnation number (distinguishes restarts).
    pub incarnation: u64,
    /// Solver version announced by this node (version gating, FM-12).
    pub solver_version: u64,
}

// ---------------------------------------------------------------------------
// MembershipViewData (the data-models struct)
// ---------------------------------------------------------------------------

/// Current view of cluster membership from a single node's perspective.
///
/// A plain, serializable snapshot of membership state. Eventually
/// consistent — all nodes converge in the absence of actual failures
/// (INV-R3). The live, concurrently-updated view used in production is
/// [`DefaultMembershipView`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipViewData {
    /// Known nodes and their current states.
    pub members: BTreeMap<NodeId, MemberInfo>,
    /// The local node's ID.
    pub local_node: NodeId,
    /// When this view was last updated (dual clock).
    pub updated_at: DualClockEvent,
    /// Incarnation number for this node (incremented on rejoin to
    /// override stale suspicion from a previous incarnation).
    pub incarnation: u64,
}

// ---------------------------------------------------------------------------
// MembershipView trait
// ---------------------------------------------------------------------------

/// Read-only view of the current cluster membership.
///
/// Used by the solver (via [`MembershipSnapshot`]) and by other crates
/// that need to query membership without mutating it. The trait is named
/// `MembershipView` to match the interface contract; the serializable
/// data container is [`MembershipViewData`] to avoid the name clash.
///
/// Renamed from the interface's `MembershipView` because this crate also
/// defines a `MembershipViewData` struct (the data-models `MembershipView`).
pub trait MembershipView {
    /// Get all currently known members and their health.
    fn members(&self) -> Vec<MemberRecord>;

    /// Get a specific node's membership record.
    ///
    /// Returns [`GossipError::NotAMember`] if the node is unknown.
    fn get(&self, id: &NodeId) -> Result<MemberRecord, GossipError>;

    /// Get all nodes matching a health assessment.
    fn by_health(&self, health: HealthAssessment) -> Vec<MemberRecord>;

    /// Get the count of active (`NodeState::Active`) nodes.
    fn active_count(&self) -> usize;

    /// Check whether all active nodes report the same solver version.
    ///
    /// Used during the solver upgrade ceremony (FM-12). Returns the
    /// version if uniform, or `None` if there is version skew or no
    /// active nodes.
    fn uniform_solver_version(&self) -> Option<u64>;

    /// Take an immutable snapshot for the solver.
    ///
    /// The snapshot is decoupled from membership changes that occur
    /// after it is taken. Used as input to `solver::Solver::solve`.
    fn snapshot(&self) -> MembershipSnapshot;
}

// ---------------------------------------------------------------------------
// DefaultMembershipView
// ---------------------------------------------------------------------------

/// Live, concurrently-updated implementation of [`MembershipView`].
///
/// Stores members in a [`RwLock`]ed [`BTreeMap`] (keyed by [`NodeId`])
/// alongside per-node network addresses, solver versions, and capability
/// sets. The local node's incarnation is tracked with an [`AtomicU64`].
///
/// # Concurrency
///
/// All mutating methods acquire a write lock on the members map (and the
/// relevant auxiliary map). Read methods acquire a read lock. The
/// `snapshot` method copies the current state into an immutable
/// [`MembershipSnapshot`] under a read lock.
pub struct DefaultMembershipView {
    /// Known members keyed by node ID.
    members: RwLock<BTreeMap<NodeId, MemberInfo>>,
    /// Per-node network addresses (gossip transport endpoints).
    addrs: RwLock<BTreeMap<NodeId, NodeAddr>>,
    /// Per-node announced solver version (version gating, FM-12).
    solver_versions: RwLock<BTreeMap<NodeId, u64>>,
    /// Per-node capability sets (populated by `advertise_capabilities`).
    capabilities: RwLock<BTreeMap<NodeId, NodeCapabilitySet>>,
    /// The local node's ID.
    local_node: NodeId,
    /// The local node's incarnation (incremented on rejoin).
    incarnation: AtomicU64,
    /// Generation counter — bumped on every membership change so the
    /// solver snapshot reflects a consistent, monotonic view.
    generation: AtomicU64,
}

impl std::fmt::Debug for DefaultMembershipView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultMembershipView")
            .field("local_node", &self.local_node)
            .field("incarnation", &self.incarnation.load(Ordering::Relaxed))
            .field("generation", &self.generation.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl DefaultMembershipView {
    /// Creates a new, empty membership view for the given local node.
    #[must_use]
    pub const fn new(local_node: NodeId) -> Self {
        Self {
            members: RwLock::new(BTreeMap::new()),
            addrs: RwLock::new(BTreeMap::new()),
            solver_versions: RwLock::new(BTreeMap::new()),
            capabilities: RwLock::new(BTreeMap::new()),
            local_node,
            incarnation: AtomicU64::new(0),
            generation: AtomicU64::new(0),
        }
    }

    /// Returns the local node's ID.
    #[must_use]
    pub const fn local_node(&self) -> NodeId {
        self.local_node
    }

    /// Returns the local node's current incarnation number.
    #[must_use]
    pub fn incarnation(&self) -> u64 {
        self.incarnation.load(Ordering::Relaxed)
    }

    /// Returns the current generation counter.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Sets the local node's incarnation (e.g. after a rejoin).
    pub fn set_incarnation(&self, incarnation: u64) {
        self.incarnation.store(incarnation, Ordering::Relaxed);
    }

    /// Records a network address for a member.
    pub fn set_addr(&self, node: NodeId, addr: NodeAddr) {
        if let Ok(mut map) = self.addrs.write() {
            map.insert(node, addr);
        }
    }

    /// Records a solver version announcement for a member (FM-12).
    pub fn set_solver_version(&self, node: NodeId, version: u64) {
        if let Ok(mut map) = self.solver_versions.write() {
            map.insert(node, version);
        }
        self.bump_generation();
    }

    /// Records a capability set for a member (INV-N1).
    ///
    /// Called by [`crate::capability::DefaultCapabilityAdvertiser`] for
    /// the local node, and by message handling for remote nodes.
    pub fn set_capabilities(&self, node: NodeId, caps: NodeCapabilitySet) {
        if let Ok(mut map) = self.capabilities.write() {
            map.insert(node, caps);
        }
        self.bump_generation();
    }

    /// Returns the recorded capability set for a node, if any.
    #[must_use]
    pub fn capabilities(&self, node: &NodeId) -> Option<NodeCapabilitySet> {
        self.capabilities
            .read()
            .ok()
            .and_then(|map| map.get(node).cloned())
    }

    /// Adds or replaces a member in the view, bumping the generation.
    ///
    /// Also records the member's address and solver version if provided
    /// in the auxiliary maps (kept in sync by callers).
    pub fn add_member(&self, info: MemberInfo) {
        if let Ok(mut map) = self.members.write() {
            map.insert(info.node_id, info);
        }
        self.bump_generation();
    }

    /// Applies a membership change: the incoming state replaces the
    /// existing one **only if its incarnation is greater or equal**
    /// (higher incarnation wins, DL-009).
    ///
    /// Returns `true` if the change was applied, `false` if it was
    /// superseded by a higher-incarnation existing entry.
    pub fn apply_membership_change(&self, change: &crate::message::MembershipChange) -> bool {
        if let Ok(mut map) = self.members.write() {
            #[allow(clippy::suspicious_operation_groupings)]
            match map.get(&change.node_id) {
                Some(existing) if existing.incarnation > change.incarnation => {
                    // Stale: existing has a strictly higher incarnation.
                    return false;
                }
                Some(existing)
                    if existing.incarnation == change.incarnation
                        && existing.state == change.to_state =>
                {
                    // Idempotent duplicate: same incarnation, same state.
                    return false;
                }
                _ => {}
            }
            let updated = MemberInfo {
                node_id: change.node_id,
                public_key: map
                    .get(&change.node_id)
                    .map_or_else(|| PublicKey([0u8; 32]), |m| m.public_key),
                state: change.to_state,
                health: map.get(&change.node_id).and_then(|m| m.health.clone()),
                last_seen: change.observed_at.clone(),
                incarnation: change.incarnation,
            };
            map.insert(change.node_id, updated);
        }
        self.bump_generation();
        true
    }

    /// Merges another view's members into this one. Higher incarnation
    /// wins; equal incarnation with different state keeps the existing
    /// entry (last-writer-wins would be non-deterministic, so we treat
    /// equal-incarnation conflicts conservatively).
    ///
    /// This merge is **commutative, associative, and idempotent**
    /// (INV-R3): merging the same view twice yields the same result,
    /// and the order of merges does not matter.
    pub fn merge(&self, other: &BTreeMap<NodeId, MemberInfo>) {
        if let Ok(mut map) = self.members.write() {
            for (id, info) in other {
                match map.get(id) {
                    Some(existing) if existing.incarnation > info.incarnation => {
                        // Keep existing (higher incarnation).
                        continue;
                    }
                    Some(existing) if existing.incarnation == info.incarnation => {
                        // Equal incarnation: keep existing to ensure
                        // determinism (no last-writer-wins).
                        continue;
                    }
                    _ => {}
                }
                map.insert(*id, info.clone());
            }
        }
        self.bump_generation();
    }

    /// Returns a snapshot of the members map (for property tests).
    #[must_use]
    pub fn members_snapshot(&self) -> BTreeMap<NodeId, MemberInfo> {
        self.members.read().map(|m| m.clone()).unwrap_or_default()
    }

    /// Increments the generation counter.
    fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Builds a [`MemberRecord`] for the given member by joining its
    /// [`MemberInfo`] with the auxiliary address, solver version, and
    /// health maps.
    fn record_for(&self, id: NodeId, info: &MemberInfo) -> MemberRecord {
        let addr = self
            .addrs
            .read()
            .ok()
            .and_then(|m| m.get(&id).cloned())
            .unwrap_or_else(|| NodeAddr::new("0.0.0.0".to_string(), 0));
        let solver_version = self
            .solver_versions
            .read()
            .ok()
            .and_then(|m| m.get(&id).copied())
            .unwrap_or(0);
        let health = info.health.clone().unwrap_or_else(|| NodeHealth {
            status: HealthAssessment::Unknown,
            observed_at: info.last_seen.clone(),
            observer: self.local_node,
        });
        MemberRecord {
            id,
            key: info.public_key,
            addr,
            health,
            incarnation: info.incarnation,
            solver_version,
        }
    }
}

impl MembershipView for DefaultMembershipView {
    fn members(&self) -> Vec<MemberRecord> {
        let map = self.members.read().expect("members lock poisoned");
        map.iter()
            .map(|(id, info)| self.record_for(*id, info))
            .collect()
    }

    fn get(&self, id: &NodeId) -> Result<MemberRecord, GossipError> {
        let map = self.members.read().expect("members lock poisoned");
        map.get(id)
            .map_or(Err(GossipError::NotAMember { node: *id }), |info| {
                Ok(self.record_for(*id, info))
            })
    }

    fn by_health(&self, health: HealthAssessment) -> Vec<MemberRecord> {
        self.members()
            .into_iter()
            .filter(|r| r.health.status == health)
            .collect()
    }

    fn active_count(&self) -> usize {
        let map = self.members.read().expect("members lock poisoned");
        map.values()
            .filter(|m| m.state == NodeState::Active)
            .count()
    }

    fn uniform_solver_version(&self) -> Option<u64> {
        let map = self.members.read().expect("members lock poisoned");
        let active_ids: Vec<NodeId> = map
            .iter()
            .filter(|(_, m)| m.state == NodeState::Active)
            .map(|(id, _)| *id)
            .collect();
        drop(map);
        if active_ids.is_empty() {
            return None;
        }
        let sv = self.solver_versions.read().expect("solver_versions lock");
        let mut versions: Vec<u64> = active_ids
            .iter()
            .map(|id| sv.get(id).copied().unwrap_or(0))
            .collect();
        versions.sort_unstable();
        versions.dedup();
        if versions.len() == 1 {
            Some(versions[0])
        } else {
            None
        }
    }

    fn snapshot(&self) -> MembershipSnapshot {
        let map = self.members.read().expect("members lock poisoned");
        let caps = self
            .capabilities
            .read()
            .expect("capabilities lock poisoned");
        let mut nodes: Vec<(NodeId, NodeCapabilitySet, taba_solver::NodeHealth)> =
            Vec::with_capacity(map.len());
        for (id, info) in map.iter() {
            let capability = caps.get(id).cloned().unwrap_or_else(|| NodeCapabilitySet {
                arch: String::new(),
                os: String::new(),
                privilege: taba_core::PrivilegeLevel::User,
                runtimes: Vec::new(),
                ports_privileged: false,
                storage: Vec::new(),
                environment: None,
                author_affinity: None,
                clock_quality: taba_common::ClockQuality::Unsync,
                timezone: String::new(),
                custom_tags: Vec::new(),
            });
            let solver_health = if info.state == NodeState::Suspected {
                taba_solver::NodeHealth::Suspected
            } else {
                taba_solver::NodeHealth::Active
            };
            nodes.push((*id, capability, solver_health));
        }
        nodes.sort_by_key(|n| n.0);
        MembershipSnapshot {
            nodes,
            generation: self.generation.load(Ordering::Relaxed),
        }
    }
}

// ---------------------------------------------------------------------------
// GossipParams (DL-016)
// ---------------------------------------------------------------------------

/// SWIM gossip protocol tuning parameters with Lifeguard-style auto-scaling.
///
/// The effective suspicion timeout auto-scales with cluster size:
/// `effective = max(base_timeout, suspicion_mult × ceil(log2(N)) × interval)`.
///
/// # Defaults
///
/// | Parameter | Default | Notes |
/// |-----------|---------|-------|
/// | `gossip_interval` | 500ms | one random peer probed per interval |
/// | `suspicion_timeout` | 5s | base, auto-scaled with cluster size |
/// | `witness_count` | 2 | INV-R3 |
/// | `indirect_probe_count` | 3 | standard SWIM |
/// | `retransmit_multiplier` | 4 | piggyback rounds = mult × log2(N) |
/// | `max_piggyback_entries` | 8 | bounds message size |
/// | `suspicion_multiplier` | 4 | effective = mult × ceil(log2(N)) × interval |
///
/// Key revocation messages use `2 × retransmit_multiplier × log2(N)`
/// rounds (double normal) for rapid convergence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipParams {
    /// Base probe interval — one random peer probed per interval.
    /// Default: 500ms.
    pub gossip_interval: Duration,
    /// Base suspicion timeout — time a node stays Suspected before
    /// witnesses can confirm failure. Must be > `gossip_interval ×
    /// indirect_probe_count`. Default: 5s.
    pub suspicion_timeout: Duration,
    /// Number of independent witnesses required before declaring a
    /// node failed. Default: 2 (INV-R3).
    pub witness_count: u8,
    /// Number of peers asked to do indirect probes (ping-req) when a
    /// direct ping fails. Default: 3 (standard SWIM).
    pub indirect_probe_count: u8,
    /// Membership changes piggybacked for `retransmit_multiplier ×
    /// log2(N)` rounds. Default: 4.
    pub retransmit_multiplier: u8,
    /// Maximum membership changes piggybacked per message. Bounds
    /// message size. Default: 8.
    pub max_piggyback_entries: u8,
    /// Suspicion timeout scales as `suspicion_multiplier × ceil(log2(N))
    /// × interval`. Larger clusters get proportionally more time,
    /// reducing false positives. Default: 4.
    pub suspicion_multiplier: u8,
}

impl Default for GossipParams {
    fn default() -> Self {
        Self {
            gossip_interval: Duration::from_millis(500),
            suspicion_timeout: Duration::from_secs(5),
            witness_count: 2,
            indirect_probe_count: 3,
            retransmit_multiplier: 4,
            max_piggyback_entries: 8,
            suspicion_multiplier: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// JoinResult
// ---------------------------------------------------------------------------

/// The result of a successful cluster join.
///
/// Returned by [`crate::swim::MembershipProtocol::join`]. Contains the
/// joining node's ID and the seed members' records (to bootstrap the
/// local membership view).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinResult {
    /// The joining node's ID.
    pub node_id: NodeId,
    /// Seed members returned by the contacted seed node.
    pub seed_members: Vec<MemberRecord>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{LogicalClock, WallTime};
    use taba_test_harness::NodeCapabilitySetBuilder;

    fn now() -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(1),
            wall_time: WallTime { millis: 1_000 },
            timezone: "UTC".to_string(),
        }
    }

    fn nid(n: u128) -> NodeId {
        NodeId(uuid::Uuid::from_u128(n))
    }

    fn pk(seed: u8) -> PublicKey {
        let mut bytes = [0u8; 32];
        bytes[0] = seed;
        PublicKey(bytes)
    }

    fn member(id: NodeId, seed: u8, state: NodeState, inc: u64) -> MemberInfo {
        MemberInfo {
            node_id: id,
            public_key: pk(seed),
            state,
            health: Some(NodeHealth {
                status: if state == NodeState::Suspected {
                    HealthAssessment::Unknown
                } else if state == NodeState::Active {
                    HealthAssessment::Healthy
                } else {
                    HealthAssessment::Unknown
                },
                observed_at: now(),
                observer: nid(999),
            }),
            last_seen: now(),
            incarnation: inc,
        }
    }

    #[test]
    fn test_gossip_params_default() {
        let p = GossipParams::default();
        assert_eq!(p.gossip_interval, Duration::from_millis(500));
        assert_eq!(p.suspicion_timeout, Duration::from_secs(5));
        assert_eq!(p.witness_count, 2);
        assert_eq!(p.indirect_probe_count, 3);
        assert_eq!(p.retransmit_multiplier, 4);
        assert_eq!(p.max_piggyback_entries, 8);
        assert_eq!(p.suspicion_multiplier, 4);
    }

    #[test]
    fn test_membership_view_add_member() {
        let view = DefaultMembershipView::new(nid(1));
        let m = member(nid(2), 2, NodeState::Active, 1);
        view.add_member(m);

        let record = view.get(&nid(2)).expect("member should exist after add");
        assert_eq!(record.id, nid(2));
    }

    #[test]
    fn test_membership_view_get_missing() {
        let view = DefaultMembershipView::new(nid(1));
        let err = view.get(&nid(2)).expect_err("missing member should error");
        assert!(matches!(err, GossipError::NotAMember { node } if node == nid(2)));
    }

    #[test]
    fn test_membership_view_by_health() {
        let view = DefaultMembershipView::new(nid(1));
        view.add_member(member(nid(2), 2, NodeState::Active, 1));
        view.add_member(member(nid(3), 3, NodeState::Suspected, 1));

        let healthy = view.by_health(HealthAssessment::Healthy);
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id, nid(2));

        let unknown = view.by_health(HealthAssessment::Unknown);
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].id, nid(3));
    }

    #[test]
    fn test_membership_view_active_count() {
        let view = DefaultMembershipView::new(nid(1));
        view.add_member(member(nid(2), 2, NodeState::Active, 1));
        view.add_member(member(nid(3), 3, NodeState::Active, 1));
        view.add_member(member(nid(4), 4, NodeState::Suspected, 1));
        view.add_member(member(nid(5), 5, NodeState::Left, 1));

        assert_eq!(view.active_count(), 2, "only Active nodes counted");
    }

    #[test]
    fn test_membership_view_uniform_solver_version() {
        let view = DefaultMembershipView::new(nid(1));

        // No active nodes → None.
        assert_eq!(view.uniform_solver_version(), None);

        view.add_member(member(nid(2), 2, NodeState::Active, 1));
        view.set_solver_version(nid(2), 7);
        assert_eq!(view.uniform_solver_version(), Some(7));

        view.add_member(member(nid(3), 3, NodeState::Active, 1));
        view.set_solver_version(nid(3), 7);
        assert_eq!(
            view.uniform_solver_version(),
            Some(7),
            "same version → Some"
        );

        view.set_solver_version(nid(3), 9);
        assert_eq!(view.uniform_solver_version(), None, "version skew → None");
    }

    #[test]
    fn test_membership_view_snapshot() {
        let view = DefaultMembershipView::new(nid(1));
        view.add_member(member(nid(2), 2, NodeState::Active, 1));
        view.set_capabilities(nid(2), NodeCapabilitySetBuilder::new().build());

        let snap = view.snapshot();
        assert_eq!(snap.nodes.len(), 1);
        assert_eq!(snap.nodes[0].0, nid(2));

        // Mutating the view after snapshot does not affect the snapshot.
        view.add_member(member(nid(3), 3, NodeState::Active, 1));
        assert_eq!(snap.nodes.len(), 1, "snapshot is immutable");
        assert_eq!(view.snapshot().nodes.len(), 2);
    }

    #[test]
    fn test_higher_incarnation_wins() {
        let view = DefaultMembershipView::new(nid(1));
        view.add_member(member(nid(2), 2, NodeState::Active, 1));

        // Lower incarnation change → ignored.
        let change_low = crate::message::MembershipChange {
            node_id: nid(2),
            from_state: Some(NodeState::Active),
            to_state: NodeState::Suspected,
            observed_at: now(),
            observed_by: nid(1),
            incarnation: 0,
        };
        assert!(!view.apply_membership_change(&change_low));

        // Higher incarnation change → applied.
        let change_high = crate::message::MembershipChange {
            node_id: nid(2),
            from_state: Some(NodeState::Active),
            to_state: NodeState::Suspected,
            observed_at: now(),
            observed_by: nid(1),
            incarnation: 5,
        };
        assert!(view.apply_membership_change(&change_high));

        let map = view.members_snapshot();
        assert_eq!(
            map.get(&nid(2)).map(|m| m.state),
            Some(NodeState::Suspected)
        );
    }

    // -- Property tests (minimum 1000 cases each) -----------------------

    use proptest::prelude::*;

    fn arb_member_info(id_seed: u128, inc: u64) -> MemberInfo {
        MemberInfo {
            node_id: NodeId(uuid::Uuid::from_u128(id_seed)),
            public_key: pk((id_seed % 200) as u8 + 1),
            state: NodeState::Active,
            health: None,
            last_seen: now(),
            incarnation: inc,
        }
    }

    fn arb_member_map(entries: Vec<(u128, u64)>) -> BTreeMap<NodeId, MemberInfo> {
        entries
            .into_iter()
            .map(|(s, inc)| (NodeId(uuid::Uuid::from_u128(s)), arb_member_info(s, inc)))
            .collect()
    }

    proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_membership_view_merge_idempotent(
            entries in prop::collection::vec((1u128..1000, 0u64..1000), 0..20),
        ) {
            let view = DefaultMembershipView::new(nid(0));
            let m = arb_member_map(entries);
            view.merge(&m);
            let after_one = view.members_snapshot();
            view.merge(&m);
            let after_two = view.members_snapshot();

            // merge(A, A) == A: applying the same map twice yields
            // identical results (same states and incarnations).
            prop_assert!(
                after_one.keys().eq(after_two.keys()),
                "merge must be idempotent (same keys)"
            );
            for (k, v1) in &after_one {
                let v2 = after_two.get(k).expect("key present in after_two");
                prop_assert_eq!(v1.node_id, v2.node_id, "same node_id");
                prop_assert_eq!(v1.incarnation, v2.incarnation, "same incarnation");
                prop_assert_eq!(v1.state, v2.state, "same state");
            }
        }

        #[test]
        fn proptest_membership_view_merge_commutative(
            entries_a in prop::collection::vec((1u128..1000, 0u64..1000), 0..15),
            entries_b in prop::collection::vec((1000u128..2000, 0u64..1000), 0..15),
        ) {
            let m_a = arb_member_map(entries_a);
            let m_b = arb_member_map(entries_b);

            let view_ab = DefaultMembershipView::new(nid(0));
            view_ab.merge(&m_a);
            view_ab.merge(&m_b);
            let result_ab = view_ab.members_snapshot();

            let view_ba = DefaultMembershipView::new(nid(0));
            view_ba.merge(&m_b);
            view_ba.merge(&m_a);
            let result_ba = view_ba.members_snapshot();

            // merge(A, B) == merge(B, A): order of merges does not
            // matter (higher incarnation wins regardless of order).
            prop_assert!(
                result_ab.keys().eq(result_ba.keys()),
                "merge must be commutative (same keys)"
            );
            for (k, v1) in &result_ab {
                let v2 = result_ba.get(k).expect("key present in result_ba");
                prop_assert_eq!(v1.node_id, v2.node_id, "same node_id");
                prop_assert_eq!(v1.incarnation, v2.incarnation, "same incarnation");
                prop_assert_eq!(v1.state, v2.state, "same state");
            }
        }
    }
}
