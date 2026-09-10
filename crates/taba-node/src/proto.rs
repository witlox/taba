//! Protobuf message definitions for WAL serialization (DL-014).
//!
//! These types are defined manually with `prost::Message` derive to
//! avoid the build complexity of `prost-build` / `protoc` for M3.
//! The definitions match `proto/taba/v1/wal.proto` for future
//! migration to generated code.
//!
//! ## Frame format
//!
//! ```text
//! ┌──────────┬──────────┬───────────────────────┬──────────┐
//! │ len: u32 │ crc: u32 │ WalEntry (protobuf)   │ pad 0-7  │
//! └──────────┴──────────┴───────────────────────┴──────────┘
//! ```

use prost::Message;
use taba_common::{DualClockEvent, LogicalClock, UnitId, WallTime};

use crate::error::{NodeError, WalPosition};
use crate::wal::{WalEntry, WalEntryType};

// ---------------------------------------------------------------------------
// Proto message types (matching proto/taba/v1/wal.proto)
// ---------------------------------------------------------------------------

/// A single entry in the node-local write-ahead log.
#[derive(Clone, PartialEq, Eq, Message)]
pub struct WalEntryProto {
    /// Monotonically increasing sequence number local to this node.
    #[prost(uint64, tag = "1")]
    pub sequence: u64,
    /// When this entry was written (dual clock).
    #[prost(message, tag = "2")]
    pub written_at: Option<DualClockEventProto>,
    /// The type and payload of this WAL entry.
    #[prost(message, tag = "3")]
    pub entry_type: Option<WalEntryTypeProto>,
}

/// The three WAL entry types (INV-C4, DL-008).
#[derive(Clone, PartialEq, Eq, Message)]
pub struct WalEntryTypeProto {
    /// The kind of entry (Merged, Pending, or Promoted).
    #[prost(oneof = "Kind", tags = "1, 2, 3")]
    pub kind: Option<Kind>,
}

/// Oneof variants for [`WalEntryTypeProto`].
#[derive(Clone, PartialEq, Eq, prost::Oneof)]
pub enum Kind {
    /// A unit has been verified and merged into the local graph state.
    #[prost(message, tag = "1")]
    Merged(MergedEntryProto),
    /// A unit has been verified but references are not yet satisfied.
    #[prost(message, tag = "2")]
    Pending(PendingEntryProto),
    /// A previously pending unit has been activated.
    #[prost(message, tag = "3")]
    Promoted(PromotedEntryProto),
}

/// Merged entry: a unit was verified and merged into the graph.
#[derive(Clone, PartialEq, Eq, Message)]
pub struct MergedEntryProto {
    /// The ID of the merged unit (UUID string).
    #[prost(string, tag = "1")]
    pub unit_id: String,
    /// Serialized signed unit (opaque bytes for WAL storage).
    #[prost(bytes, tag = "2")]
    pub payload: Vec<u8>,
}

/// Pending entry: a unit was verified but references are unsatisfied.
#[derive(Clone, PartialEq, Eq, Message)]
pub struct PendingEntryProto {
    /// The ID of the pending unit (UUID string).
    #[prost(string, tag = "1")]
    pub unit_id: String,
    /// Serialized signed unit.
    #[prost(bytes, tag = "2")]
    pub payload: Vec<u8>,
    /// The references that are not yet present (UUID strings).
    #[prost(string, repeated, tag = "3")]
    pub missing_refs: Vec<String>,
}

/// Promoted entry: a pending unit was promoted after refs arrived.
#[derive(Clone, PartialEq, Eq, Message)]
pub struct PromotedEntryProto {
    /// The ID of the promoted unit (UUID string).
    #[prost(string, tag = "1")]
    pub unit_id: String,
}

/// Dual clock event (INV-T2). Logical clock for ordering,
/// wall time for retention/compliance, timezone for display.
#[derive(Clone, PartialEq, Eq, Message)]
pub struct DualClockEventProto {
    /// Lamport-style logical clock for causal ordering.
    #[prost(uint64, tag = "1")]
    pub logical_clock: u64,
    /// Milliseconds since Unix epoch.
    #[prost(uint64, tag = "2")]
    pub wall_time_millis: u64,
    /// IANA timezone string (e.g., `"UTC"`).
    #[prost(string, tag = "3")]
    pub timezone: String,
}

// ---------------------------------------------------------------------------
// Conversion: domain → proto
// ---------------------------------------------------------------------------

/// Converts a [`DualClockEvent`] to its proto representation.
#[must_use]
pub fn dual_clock_to_proto(event: &DualClockEvent) -> DualClockEventProto {
    DualClockEventProto {
        logical_clock: event.logical_clock.0,
        wall_time_millis: event.wall_time.millis,
        timezone: event.timezone.clone(),
    }
}

/// Converts a [`WalEntry`] to its proto representation.
#[must_use]
pub fn wal_entry_to_proto(entry: &WalEntry) -> WalEntryProto {
    WalEntryProto {
        sequence: entry.sequence,
        written_at: Some(dual_clock_to_proto(&entry.written_at)),
        entry_type: Some(wal_entry_type_to_proto(&entry.entry_type)),
    }
}

/// Converts a [`WalEntryType`] to its proto representation.
#[must_use]
pub fn wal_entry_type_to_proto(entry_type: &WalEntryType) -> WalEntryTypeProto {
    let kind = match entry_type {
        WalEntryType::Merged { unit_id, payload } => Kind::Merged(MergedEntryProto {
            unit_id: unit_id.0.to_string(),
            payload: payload.clone(),
        }),
        WalEntryType::Pending {
            unit_id,
            payload,
            missing_refs,
        } => Kind::Pending(PendingEntryProto {
            unit_id: unit_id.0.to_string(),
            payload: payload.clone(),
            missing_refs: missing_refs.iter().map(|id| id.0.to_string()).collect(),
        }),
        WalEntryType::Promoted { unit_id } => Kind::Promoted(PromotedEntryProto {
            unit_id: unit_id.0.to_string(),
        }),
    };
    WalEntryTypeProto { kind: Some(kind) }
}

// ---------------------------------------------------------------------------
// Conversion: proto → domain
// ---------------------------------------------------------------------------

/// Converts a [`DualClockEventProto`] to its domain representation.
///
/// # Errors
///
/// Returns [`NodeError::WalCorrupted`] if the proto is missing.
pub fn dual_clock_from_proto(
    proto: &Option<DualClockEventProto>,
) -> Result<DualClockEvent, NodeError> {
    let p = proto.as_ref().ok_or_else(|| NodeError::WalCorrupted {
        position: WalPosition::ZERO,
        reason: "DualClockEvent is missing in WAL entry".to_string(),
    })?;
    Ok(DualClockEvent {
        logical_clock: LogicalClock(p.logical_clock),
        wall_time: WallTime {
            millis: p.wall_time_millis,
        },
        timezone: p.timezone.clone(),
    })
}

/// Converts a [`WalEntryProto`] to its domain representation.
///
/// # Errors
///
/// - [`NodeError::WalCorrupted`] if the entry is malformed (missing
///   fields, invalid UUIDs).
pub fn wal_entry_from_proto(proto: &WalEntryProto) -> Result<WalEntry, NodeError> {
    let written_at = dual_clock_from_proto(&proto.written_at)?;
    let entry_type = wal_entry_type_from_proto(&proto.entry_type)?;

    Ok(WalEntry {
        sequence: proto.sequence,
        written_at,
        entry_type,
    })
}

/// Converts a [`WalEntryTypeProto`] to its domain representation.
///
/// # Errors
///
/// - [`NodeError::WalCorrupted`] if the entry type is missing or
///   contains invalid UUIDs.
pub fn wal_entry_type_from_proto(
    proto: &Option<WalEntryTypeProto>,
) -> Result<WalEntryType, NodeError> {
    let p = proto.as_ref().ok_or_else(|| NodeError::WalCorrupted {
        position: WalPosition::ZERO,
        reason: "WalEntryType is missing in WAL entry".to_string(),
    })?;

    let kind = p.kind.as_ref().ok_or_else(|| NodeError::WalCorrupted {
        position: WalPosition::ZERO,
        reason: "WalEntryType kind is missing".to_string(),
    })?;

    match kind {
        Kind::Merged(m) => {
            let unit_id = parse_unit_id(&m.unit_id)?;
            Ok(WalEntryType::Merged {
                unit_id,
                payload: m.payload.clone(),
            })
        }
        Kind::Pending(p) => {
            let unit_id = parse_unit_id(&p.unit_id)?;
            let mut missing_refs = Vec::with_capacity(p.missing_refs.len());
            for ref_str in &p.missing_refs {
                missing_refs.push(parse_unit_id(ref_str)?);
            }
            Ok(WalEntryType::Pending {
                unit_id,
                payload: p.payload.clone(),
                missing_refs,
            })
        }
        Kind::Promoted(m) => {
            let unit_id = parse_unit_id(&m.unit_id)?;
            Ok(WalEntryType::Promoted { unit_id })
        }
    }
}

/// Parses a UUID string into a [`UnitId`].
fn parse_unit_id(s: &str) -> Result<UnitId, NodeError> {
    uuid::Uuid::parse_str(s)
        .map(UnitId)
        .map_err(|e| NodeError::WalCorrupted {
            position: WalPosition::ZERO,
            reason: format!("invalid unit ID '{s}': {e}"),
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_clock(logical: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(logical),
            wall_time: WallTime {
                millis: 1000 + logical,
            },
            timezone: "UTC".to_string(),
        }
    }

    #[test]
    fn test_dual_clock_roundtrip() {
        let event = test_clock(42);
        let proto = dual_clock_to_proto(&event);
        let restored = dual_clock_from_proto(&Some(proto)).expect("from proto");
        assert_eq!(restored, event);
    }

    #[test]
    fn test_wal_entry_merged_roundtrip() {
        let entry = WalEntry {
            sequence: 1,
            written_at: test_clock(10),
            entry_type: WalEntryType::Merged {
                unit_id: UnitId(uuid::Uuid::new_v4()),
                payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
            },
        };

        let proto = wal_entry_to_proto(&entry);
        let encoded = proto.encode_to_vec();
        let decoded = WalEntryProto::decode(encoded.as_slice()).expect("decode");
        let restored = wal_entry_from_proto(&decoded).expect("from proto");

        assert_eq!(restored.sequence, entry.sequence);
        assert_eq!(restored.written_at, entry.written_at);
        match (&restored.entry_type, &entry.entry_type) {
            (
                WalEntryType::Merged {
                    unit_id: r_id,
                    payload: r_payload,
                },
                WalEntryType::Merged {
                    unit_id: e_id,
                    payload: e_payload,
                },
            ) => {
                assert_eq!(r_id, e_id);
                assert_eq!(r_payload, e_payload);
            }
            _ => panic!("entry type mismatch"),
        }
    }

    #[test]
    fn test_wal_entry_pending_roundtrip() {
        let entry = WalEntry {
            sequence: 2,
            written_at: test_clock(20),
            entry_type: WalEntryType::Pending {
                unit_id: UnitId(uuid::Uuid::new_v4()),
                payload: vec![0xCA, 0xFE],
                missing_refs: vec![UnitId(uuid::Uuid::new_v4()), UnitId(uuid::Uuid::new_v4())],
            },
        };

        let proto = wal_entry_to_proto(&entry);
        let encoded = proto.encode_to_vec();
        let decoded = WalEntryProto::decode(encoded.as_slice()).expect("decode");
        let restored = wal_entry_from_proto(&decoded).expect("from proto");

        assert_eq!(restored.sequence, entry.sequence);
        match (&restored.entry_type, &entry.entry_type) {
            (
                WalEntryType::Pending {
                    unit_id: r_id,
                    payload: r_payload,
                    missing_refs: r_refs,
                },
                WalEntryType::Pending {
                    unit_id: e_id,
                    payload: e_payload,
                    missing_refs: e_refs,
                },
            ) => {
                assert_eq!(r_id, e_id);
                assert_eq!(r_payload, e_payload);
                assert_eq!(r_refs, e_refs);
            }
            _ => panic!("entry type mismatch"),
        }
    }

    #[test]
    fn test_wal_entry_promoted_roundtrip() {
        let entry = WalEntry {
            sequence: 3,
            written_at: test_clock(30),
            entry_type: WalEntryType::Promoted {
                unit_id: UnitId(uuid::Uuid::new_v4()),
            },
        };

        let proto = wal_entry_to_proto(&entry);
        let encoded = proto.encode_to_vec();
        let decoded = WalEntryProto::decode(encoded.as_slice()).expect("decode");
        let restored = wal_entry_from_proto(&decoded).expect("from proto");

        assert_eq!(restored.sequence, entry.sequence);
        match (&restored.entry_type, &entry.entry_type) {
            (
                WalEntryType::Promoted { unit_id: r_id },
                WalEntryType::Promoted { unit_id: e_id },
            ) => {
                assert_eq!(r_id, e_id);
            }
            _ => panic!("entry type mismatch"),
        }
    }

    #[test]
    fn test_invalid_unit_id_returns_error() {
        let proto = WalEntryTypeProto {
            kind: Some(Kind::Promoted(PromotedEntryProto {
                unit_id: "not-a-uuid".to_string(),
            })),
        };

        let result = wal_entry_type_from_proto(&Some(proto));
        assert!(
            matches!(result, Err(NodeError::WalCorrupted { .. })),
            "invalid UUID should return WalCorrupted, got: {result:?}"
        );
    }

    #[test]
    fn test_missing_entry_type_returns_error() {
        let result = wal_entry_type_from_proto(&None);
        assert!(
            matches!(result, Err(NodeError::WalCorrupted { .. })),
            "missing entry type should return WalCorrupted"
        );
    }
}
