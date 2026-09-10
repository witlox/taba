//! Cryptographic signing, verification, capability enforcement, taint
//! computation, Shamir key ceremony, delegation token management, and
//! key revocation lifecycle (causal model per INV-S3).
//!
//! ## Modules
//!
//! - [`crypto`] — Ed25519 keys, signatures, key identifiers
//! - [`signing`] — unit signing with bound context (INV-S3)
//! - [`verification`] — signature verification, causal revocation check
//! - [`scope`] — author scope checking, type-dependent uniqueness (INV-S5, INV-S8)
//! - [`enforcement`] — runtime capability enforcement (INV-S1, INV-S2)
//! - [`taint`] — provenance graph traversal, classification union (INV-S4, INV-S7, INV-S9)
//! - [`delegation`] — delegation token creation, validation, revocation (INV-W4, INV-W4a)
//! - [`ceremony`] — Shamir key ceremony, solo bootstrap, trust governance types
//! - [`error`] — all security error variants

pub mod ceremony;
pub mod crypto;
pub mod delegation;
pub mod enforcement;
pub mod error;
pub mod scope;
pub mod signing;
pub mod taint;
pub mod verification;

pub use ceremony::*;
pub use crypto::*;
pub use delegation::*;
pub use enforcement::*;
pub use error::*;
pub use scope::*;
pub use signing::*;
pub use taint::*;
pub use verification::*;
