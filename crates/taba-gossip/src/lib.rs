//! SWIM-based membership protocol, failure detection, and signed
//! gossip transport for taba.
//!
//! All gossip messages are signed with the sending node's identity key
//! (INV-R3). Membership state changes require 2 independent witnesses
//! (DL-009). The protocol uses Lifeguard-style auto-tuning (DL-016).
//!
//! ## Modules
//!
//! - [`error`] — [`GossipError`] variants (security, transport,//!   membership, cross-domain, fleet)
//! - [`message`] — [`GossipMessage`], [`GossipPayload`], [`SwimProbe`],
//!   [`MembershipChange`], [`WitnessConfirmation`], [`NodeState`]
//! - [`transport`] — [`NodeAddr`], [`GossipTransport`] trait,
//!   [`InMemoryTransport`]
//! - [`membership`] — [`MembershipView`] trait, [`MembershipViewData`],
//!   [`MemberInfo`], [`NodeHealth`], [`GossipParams`], [`MembershipSnapshot`]
//! - [`swim`] — [`MembershipProtocol`] trait, [`SwimProtocol`]
//! - [`capability`] — [`CapabilityAdvertiser`] trait,
//!   [`DefaultCapabilityAdvertiser`]
//! - [`cross_domain`] — [`CrossDomainGossip`] trait,
//!   [`DefaultCrossDomainGossip`]
//! - [`fleet`] — [`FleetCommandService`] trait,
//!   [`DefaultFleetCommandService`]
//!
//! [`MembershipView`]: membership::MembershipView
//! [`MembershipViewData`]: membership::MembershipViewData
//! [`MembershipSnapshot`]: membership::MembershipSnapshot
//! [`GossipTransport`]: transport::GossipTransport
//! [`CapabilityAdvertiser`]: capability::CapabilityAdvertiser
//! [`CrossDomainGossip`]: cross_domain::CrossDomainGossip
//! [`FleetCommandService`]: fleet::FleetCommandService

// Allow async fn in traits: taba targets Rust 1.85 with native AFIT, but
// several trait methods here intentionally do not return `impl Future` and
// are consumed from runtime contexts where `Send` is not required by
// callers in tests.
#![allow(
    async_fn_in_trait,
    clippy::unused_async,
    clippy::unused_async_trait_impl
)]
#![allow(unknown_lints)]
#![allow(clippy::significant_drop_tightening)]
// Test helper names intentionally differ from domain terms.
#![allow(clippy::similar_names)]

pub mod capability;
pub mod cross_domain;
pub mod error;
pub mod fleet;
pub mod membership;
pub mod message;
pub mod swim;
pub mod transport;

// Re-export the primary public types and traits at the crate root for
// ergonomic access (`taba_gossip::GossipMessage` instead of
// `taba_gossip::message::GossipMessage`).

pub use capability::{CapabilityAdvertiser, DefaultCapabilityAdvertiser};
pub use cross_domain::{CrossDomainGossip, DefaultCrossDomainGossip, ForwardingResult};
pub use error::GossipError;
pub use fleet::{DefaultFleetCommandService, FleetCommandService};
pub use membership::{
    DefaultMembershipView, GossipParams, HealthAssessment, JoinResult, MemberInfo, MemberRecord,
    MembershipView, MembershipViewData, NodeHealth,
};
pub use message::{
    GossipMessage, GossipPayload, MembershipChange, NodeState, ProbeType, SwimProbe,
    WitnessConfirmation,
};
pub use swim::{MembershipProtocol, ProbeOutcome, SwimProtocol};
pub use transport::{GossipTransport, InMemoryTransport, NodeAddr, UdpTransport};

// Re-export the real MembershipSnapshot from taba-solver (A009). taba-gossip
// populates this snapshot from its live membership view; the solver reads it
// for placement decisions (INV-C3, INV-R5).
pub use taba_solver::MembershipSnapshot;
