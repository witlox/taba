//! Identity newtypes, clocks, and fixed-point arithmetic.
//!
//! This module defines the foundational newtypes, identifiers, and temporal
//! primitives used throughout the system. Newtypes enforce domain semantics
//! at the type level — a [`NodeId`] cannot be accidentally used where an
//! [`AuthorId`] is expected.
//!
//! All types derive `Serialize` and `Deserialize` for persistence and wire
//! transmission. Fixed-point arithmetic ([`Ppm`], [`SignedPpm`]) avoids
//! floating-point to guarantee identical results across platforms (INV-C3, A2).

use std::ops::{Add, Div, Mul, Sub};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Identity newtypes
// ---------------------------------------------------------------------------

/// Globally unique, immutable identifier for a unit.
/// Assigned at creation time and never changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub Uuid);

/// Identity of a node in the taba cluster.
///
/// Deterministically derived: `NodeId = SHA-256(Ed25519_public_key_bytes)`,
/// truncated to 128 bits, encoded as UUID v8. This derivation is platform-
/// independent and must produce identical results on all architectures.
/// Used as tiebreaker in partition resolution (lexicographic ordering, INV-C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub Uuid);

/// Identity of an authenticated author with scoped authority.
/// Bound to an Ed25519 key pair and scoped by (unit type, trust domain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AuthorId(pub Uuid);

/// Identity of a trust domain — an authorization boundary.
/// Trust domains are themselves governance units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrustDomainId(pub Uuid);

/// Identity of a taba cluster.
/// Bound into signature context to prevent cross-cluster replay (INV-S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClusterId(pub Uuid);

/// Identity of a Shamir ceremony instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CeremonyId(pub Uuid);

/// Identity of an erasure-coded shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ShardId(pub Uuid);

/// Identity of a delegation token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DelegationTokenId(pub Uuid);

/// Identity of a policy unit (used for promotion policy dedup).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PolicyId(pub Uuid);

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

/// Wall-clock timestamp for duration-based operations (retention, compliance).
/// NOT used for causal ordering — use [`LogicalClock`] for that (INV-T2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WallTime {
    /// Milliseconds since Unix epoch.
    pub millis: u64,
}

/// Lamport-style logical clock for causal ordering (INV-T1).
///
/// Monotonically increasing per node. On inter-node communication:
/// `local = max(local, remote) + 1`.
/// Authoritative for ordering, key revocation, signature validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogicalClock(pub u64);

impl LogicalClock {
    /// Advance the clock on local event.
    pub const fn tick(&mut self) {
        self.0 += 1;
    }

    /// Sync with a remote clock (max + 1).
    pub const fn sync(&mut self, remote: Self) {
        // `u64::max` is not yet const-stable, so use a direct comparison.
        self.0 = if self.0 > remote.0 { self.0 } else { remote.0 } + 1;
    }
}

/// Every event records this triple (INV-T2).
///
/// Logical clock is authoritative for ordering.
/// Wall time + timezone are authoritative for retention/compliance.
/// Wall time + timezone are informational for human display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DualClockEvent {
    pub logical_clock: LogicalClock,
    pub wall_time: WallTime,
    pub timezone: String,
}

// A002: No alias. Use DualClockEvent everywhere. The name "Timestamp"
// was ambiguous (wall time? logical? both?). DualClockEvent is explicit.

/// Clock quality reported by the node (INV-N1 capability).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ClockQuality {
    /// NTP-synced (seconds accuracy).
    Ntp,
    /// PTP-synced (microseconds accuracy).
    Ptp,
    /// GPS-disciplined (nanoseconds accuracy).
    Gps,
    /// No synchronization.
    Unsync,
}

// ---------------------------------------------------------------------------
// Fixed-point arithmetic
// ---------------------------------------------------------------------------

/// Fixed-point parts-per-million value for deterministic solver arithmetic.
///
/// All solver scoring and placement calculations use this type instead of
/// floating-point to guarantee identical results across platforms (INV-C3, A2).
/// Scale factor: `10^6`. A value of `1_000_000` represents 1.0.
/// Division rounds toward zero (Rust integer division default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ppm(pub u64);

impl Ppm {
    /// Scale factor: `1_000_000` ppm == 1.0.
    const MILLION: u128 = 1_000_000;

    /// Represents 1.0 in fixed-point (`1_000_000` ppm).
    pub const ONE: Self = Self(1_000_000);

    /// Returns the underlying raw value (parts per million).
    #[must_use]
    pub const fn as_raw(self) -> u64 {
        self.0
    }
}

impl Add for Ppm {
    type Output = Self;

    /// Add two ppm values.
    ///
    /// Both operands are in the same ppm scale, so the result is a simple
    /// sum. Saturates at `u64::MAX` on overflow.
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sub for Ppm {
    type Output = Self;

    /// Subtract `rhs` from `self`.
    ///
    /// Both operands are in the same ppm scale. Since `Ppm` is unsigned,
    /// the result saturates at zero instead of going negative.
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl Mul for Ppm {
    type Output = Self;

    /// Multiply two ppm values with scale correction.
    ///
    /// Because both operands are in ppm (`10^-6` scale), the product is
    /// divided by `1_000_000` to maintain the same scale:
    /// `(a * b) / 1_000_000`. Intermediate computation uses `u128` to
    /// avoid overflow.
    fn mul(self, rhs: Self) -> Self::Output {
        // Result fits in u64 for all valid Ppm values (fractions near [0, 1]).
        // Extreme inputs near u64::MAX are not valid ppm values.
        #[allow(clippy::cast_possible_truncation)]
        Self(((u128::from(self.0) * u128::from(rhs.0)) / Self::MILLION) as u64)
    }
}

impl Div for Ppm {
    type Output = Self;

    /// Divide `self` by `rhs` with scale correction.
    ///
    /// Because both operands are in ppm, the dividend is multiplied by
    /// `1_000_000` before dividing: `(a * 1_000_000) / b`. This preserves
    /// the ppm scale. Division rounds toward zero (Rust integer division
    /// default). Intermediate computation uses `u128` to avoid overflow.
    ///
    /// # Panics
    ///
    /// Panics if `rhs` is zero, consistent with Rust integer division.
    fn div(self, rhs: Self) -> Self::Output {
        #[allow(clippy::cast_possible_truncation)]
        Self(((u128::from(self.0) * Self::MILLION) / u128::from(rhs.0)) as u64)
    }
}

/// Signed fixed-point ppm for calculations that may go negative
/// (e.g., score deltas, cost differences).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SignedPpm(pub i64);

impl SignedPpm {
    /// Scale factor: `1_000_000` ppm == 1.0.
    const MILLION: i128 = 1_000_000;

    /// Represents 1.0 in fixed-point (`1_000_000` ppm).
    pub const ONE: Self = Self(1_000_000);

    /// Returns the underlying raw value (signed parts per million).
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

impl Add for SignedPpm {
    type Output = Self;

    /// Add two signed ppm values.
    ///
    /// Both operands are in the same ppm scale. Saturates at `i64::MAX`
    /// or `i64::MIN` on overflow.
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Sub for SignedPpm {
    type Output = Self;

    /// Subtract `rhs` from `self`.
    ///
    /// Unlike [`Ppm`] subtraction, signed subtraction can produce
    /// negative values, which is the primary use case for `SignedPpm`
    /// (score deltas, cost differences).
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.saturating_sub(rhs.0))
    }
}

impl Mul for SignedPpm {
    type Output = Self;

    /// Multiply two signed ppm values with scale correction.
    ///
    /// `(a * b) / 1_000_000` with intermediate `i128` to avoid overflow.
    fn mul(self, rhs: Self) -> Self::Output {
        #[allow(clippy::cast_possible_truncation)]
        Self(((i128::from(self.0) * i128::from(rhs.0)) / Self::MILLION) as i64)
    }
}

impl Div for SignedPpm {
    type Output = Self;

    /// Divide `self` by `rhs` with scale correction.
    ///
    /// `(a * 1_000_000) / b` with intermediate `i128` to avoid overflow.
    /// Division rounds toward zero (Rust integer division default).
    ///
    /// # Panics
    ///
    /// Panics if `rhs` is zero, consistent with Rust integer division.
    fn div(self, rhs: Self) -> Self::Output {
        #[allow(clippy::cast_possible_truncation)]
        Self(((i128::from(self.0) * Self::MILLION) / i128::from(rhs.0)) as i64)
    }
}

// ---------------------------------------------------------------------------
// Version tracking
// ---------------------------------------------------------------------------

/// Monotonically increasing version number for policy supersession chains
/// and solver version gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Version(pub u64);

/// Validity window for bounded tasks and delegation tokens.
/// Optional on services (omitted = valid indefinitely per INV-W1).
/// Set on bounded tasks (auto-terminate on deadline per INV-W2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidityWindow {
    /// Logical clock range for causal validity (authoritative).
    pub lc_range: Option<(LogicalClock, LogicalClock)>,
    /// Wall-time deadline for human/compliance purposes.
    pub wall_time_deadline: Option<WallTime>,
}

/// SHA256 content hash for artifact integrity and dedup (INV-A1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest(pub String);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    // -- LogicalClock -------------------------------------------------------

    #[test]
    fn test_logical_clock_tick() {
        let mut clock = LogicalClock(0);
        assert_eq!(clock, LogicalClock(0));

        clock.tick();
        assert_eq!(clock, LogicalClock(1));

        clock.tick();
        assert_eq!(clock, LogicalClock(2));
    }

    #[test]
    fn test_logical_clock_sync() {
        // local=5, remote=10 → max(5,10)+1 = 11
        let mut local = LogicalClock(5);
        let remote = LogicalClock(10);
        local.sync(remote);
        assert_eq!(local, LogicalClock(11));
    }

    #[test]
    fn test_logical_clock_sync_with_older_remote() {
        // local=20, remote=5 → max(20,5)+1 = 21
        let mut local = LogicalClock(20);
        let remote = LogicalClock(5);
        local.sync(remote);
        assert_eq!(local, LogicalClock(21));
    }

    #[test]
    fn test_logical_clock_sync_equal() {
        // local=10, remote=10 → max(10,10)+1 = 11
        let mut local = LogicalClock(10);
        let remote = LogicalClock(10);
        local.sync(remote);
        assert_eq!(local, LogicalClock(11));
    }

    // -- Ppm ----------------------------------------------------------------

    #[test]
    fn test_ppm_add() {
        let a = Ppm(500_000);
        let b = Ppm(300_000);
        assert_eq!(a + b, Ppm(800_000));
        assert_eq!(a.add(b), Ppm(800_000));
    }

    #[test]
    fn test_ppm_represents_one() {
        // 1_000_000 ppm == 1.0
        assert_eq!(Ppm::ONE, Ppm(1_000_000));
        assert_eq!(Ppm::ONE.as_raw(), 1_000_000);
    }

    #[test]
    fn test_ppm_mul_scale_correction() {
        // 0.5 * 2.0 = 1.0 → 500_000 * 2_000_000 / 1_000_000 = 1_000_000
        let half = Ppm(500_000);
        let two = Ppm(2_000_000);
        assert_eq!(half * two, Ppm(1_000_000));
    }

    #[test]
    fn test_ppm_div_rounds_toward_zero() {
        // 1.0 / 3.0 = 0.333... → 1_000_000 * 1_000_000 / 3_000_000 = 333_333
        // (integer division truncates toward zero)
        let one = Ppm(1_000_000);
        let three = Ppm(3_000_000);
        assert_eq!(one / three, Ppm(333_333));
    }

    #[test]
    fn test_ppm_sub_saturates_at_zero() {
        let a = Ppm(100_000);
        let b = Ppm(300_000);
        // unsigned subtraction saturates at zero
        assert_eq!(a - b, Ppm(0));
    }

    // -- SignedPpm ----------------------------------------------------------

    #[test]
    fn test_signed_ppm_subtraction_can_go_negative() {
        let a = SignedPpm(100_000);
        let b = SignedPpm(300_000);
        assert_eq!(a - b, SignedPpm(-200_000));
    }

    #[test]
    fn test_signed_ppm_add() {
        let a = SignedPpm(500_000);
        let b = SignedPpm(-300_000);
        assert_eq!(a + b, SignedPpm(200_000));
    }

    // -- Identity newtype equality & ordering -------------------------------

    #[test]
    fn test_identity_newtype_equality() {
        let uuid = Uuid::new_v4();
        let a = UnitId(uuid);
        let b = UnitId(uuid);
        assert_eq!(a, b);

        // Different UUIDs are not equal
        let c = UnitId(Uuid::new_v4());
        assert_ne!(a, c);
    }

    #[test]
    fn test_identity_newtype_ordering() {
        let uuid_low = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);
        let uuid_high = Uuid::from_u128(0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF);

        let low = NodeId(uuid_low);
        let high = NodeId(uuid_high);

        assert!(low < high, "lower UUID should sort before higher UUID");
        assert!(high > low);
    }

    // -- ValidityWindow -----------------------------------------------------

    #[test]
    fn test_validity_window_optional_fields() {
        // Both None is valid (e.g., a service with no deadline)
        let window = ValidityWindow {
            lc_range: None,
            wall_time_deadline: None,
        };
        assert!(window.lc_range.is_none());
        assert!(window.wall_time_deadline.is_none());

        // Only lc_range set
        let window = ValidityWindow {
            lc_range: Some((LogicalClock(10), LogicalClock(100))),
            wall_time_deadline: None,
        };
        assert!(window.lc_range.is_some());
        assert!(window.wall_time_deadline.is_none());

        // Only wall_time_deadline set
        let window = ValidityWindow {
            lc_range: None,
            wall_time_deadline: Some(WallTime { millis: 1_000 }),
        };
        assert!(window.lc_range.is_none());
        assert!(window.wall_time_deadline.is_some());

        // Both set
        let window = ValidityWindow {
            lc_range: Some((LogicalClock(10), LogicalClock(100))),
            wall_time_deadline: Some(WallTime { millis: 1_000 }),
        };
        assert!(window.lc_range.is_some());
        assert!(window.wall_time_deadline.is_some());
    }

    // -- WallTime ordering --------------------------------------------------

    #[test]
    fn test_wall_time_ordering() {
        let earlier = WallTime { millis: 1_000 };
        let later = WallTime { millis: 2_000 };

        assert!(earlier < later, "earlier millis should sort before later");
        assert!(later > earlier);
        assert_eq!(earlier, WallTime { millis: 1_000 });
    }

    // -- Serialization roundtrip --------------------------------------------

    #[test]
    fn test_serialization_roundtrip_logical_clock() {
        let clock = LogicalClock(42);
        let json = serde_json::to_string(&clock).expect("serialize LogicalClock");
        let decoded: LogicalClock = serde_json::from_str(&json).expect("deserialize LogicalClock");
        assert_eq!(clock, decoded);
    }

    #[test]
    fn test_serialization_roundtrip_ppm() {
        let ppm = Ppm(750_000);
        let json = serde_json::to_string(&ppm).expect("serialize Ppm");
        let decoded: Ppm = serde_json::from_str(&json).expect("deserialize Ppm");
        assert_eq!(ppm, decoded);
    }

    #[test]
    fn test_serialization_roundtrip_clock_quality() {
        for variant in [
            ClockQuality::Ntp,
            ClockQuality::Ptp,
            ClockQuality::Gps,
            ClockQuality::Unsync,
        ] {
            let json = serde_json::to_string(&variant).expect("serialize ClockQuality");
            let decoded: ClockQuality =
                serde_json::from_str(&json).expect("deserialize ClockQuality");
            assert_eq!(variant, decoded);
        }
    }

    #[test]
    fn test_serialization_roundtrip_cluster_config() {
        // ClusterConfig does not derive PartialEq, so we compare the JSON
        // strings before and after round-tripping.
        let config = crate::config::ClusterConfig::default();
        let json1 = serde_json::to_string(&config).expect("serialize ClusterConfig");
        let decoded: crate::config::ClusterConfig =
            serde_json::from_str(&json1).expect("deserialize ClusterConfig");
        let json2 = serde_json::to_string(&decoded).expect("re-serialize ClusterConfig");
        assert_eq!(json1, json2, "JSON must be identical after round-trip");
    }

    // -- Property tests -----------------------------------------------------

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config {
            cases: 1000,
            ..proptest::test_runner::Config::default()
        })]

        #[test]
        fn proptest_logical_clock_sync_monotonic(
            a in 0u64..1_000_000_000,
            b in 0u64..1_000_000_000,
        ) {
            let mut local = LogicalClock(a);
            let remote = LogicalClock(b);
            local.sync(remote);
            // sync computes max(a, b) + 1, which is strictly greater than both
            prop_assert!(local.0 > a, "after sync, clock must exceed original local");
            prop_assert!(local.0 > b, "after sync, clock must exceed remote");
        }

        #[test]
        fn proptest_ppm_addition_commutative(
            a in 0u64..1_000_000_000,
            b in 0u64..1_000_000_000,
        ) {
            let pa = Ppm(a);
            let pb = Ppm(b);
            prop_assert_eq!(pa + pb, pb + pa);
        }

        #[test]
        fn proptest_serialization_roundtrip(
            value in 0u64..u64::MAX,
        ) {
            let clock = LogicalClock(value);
            let json = serde_json::to_string(&clock).expect("serialize");
            let decoded: LogicalClock = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(clock, decoded);
        }
    }
}
