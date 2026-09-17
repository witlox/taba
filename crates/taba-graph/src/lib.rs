//! δ-state CRDT composition graph — the single source of desired state.
//!
//! The composition graph is the single source of desired state (INV-C1).
//! It is a δ-state CRDT (DL-012): merge is commutative, associative,
//! and idempotent (INV-C2). `GraphDelta` is a partial state, not an
//! operation log — merging deltas is idempotent.
//!
//! ## Architecture
//!
//! - [`crdt`] — CRDT data structure, merge algorithm, idempotency
//! - [`entry`] — graph entries, operations, merge results, policy chains
//! - [`error`] — all graph error variants
//! - [`graph`] — [`Graph`] trait and [`DefaultGraph`] implementation
//! - [`merge`] — [`MergePolicy`] trait and [`DefaultMergePolicy`]
//! - [`memory`] — [`MemoryMonitor`] trait and [`DefaultMemoryMonitor`]
//! - [`compaction`] — [`Compactor`] trait and [`DefaultCompactor`]
//! - [`query`] — [`GraphQuery`] trait and [`ProvenanceLink`]
//! - [`snapshot`] — immutable [`GraphSnapshot`] for solver consumption
//! - [`wal`] — in-memory write-ahead log (M2: no disk)
//!
//! ## Key invariants
//!
//! - **INV-C1**: The graph is the single source of desired state.
//! - **INV-C2**: Merge is commutative, associative, idempotent.
//! - **INV-C4**: WAL-before-effect, causal buffering (pending queue).
//! - **INV-C7**: Only one non-revoked policy per conflict tuple.
//! - **INV-D1**: Provenance chain is unbroken.
//! - **INV-R6**: Active graph per node ≤ configurable memory limit.
//! - **INV-S3**: Signature verification is a synchronous gate before
//!   merge. When a [`Verifier`] is configured via
//!   [`DefaultGraph::with_verifier`], signatures are
//!   cryptographically verified. When no verifier is configured
//!   (M2 default), structural validation only is performed.
//!
//! [`Verifier`]: taba_security::Verifier

#![warn(missing_docs)]
// Methods are async for future WAL I/O (M3). Currently synchronous.
#![allow(unknown_lints)]
// MutexGuard drop timing is not a correctness issue for in-memory M2.
#![allow(clippy::significant_drop_tightening)]

pub mod compaction;
pub mod crdt;
pub mod entry;
pub mod error;
pub mod graph;
pub mod memory;
pub mod merge;
pub mod query;
pub mod snapshot;
pub mod wal;

// Re-export key public types at the crate root for convenience.

// Error
pub use error::GraphError;

// Entry types
pub use entry::{
    GraphEntry, GraphOp, GraphStats, MergeResult, PendingEntry, PolicyChain, PolicyVersion,
    RejectedEntry, RejectionReason,
};

// CRDT
pub use crdt::{CompositionGraphData, GraphDelta};

// Snapshot
pub use snapshot::GraphSnapshot;

// Query
pub use query::{GraphQuery, ProvenanceLink};

// Merge
pub use merge::{DefaultMergePolicy, MergePolicy};

// Memory
pub use memory::{DefaultMemoryMonitor, MemoryMonitor};

// Compaction
pub use compaction::{CompactionAction, Compactor, DefaultCompactor};

// Graph
pub use graph::{DefaultGraph, Graph};

// WAL
pub use wal::{InMemoryWal, WalEntry};
