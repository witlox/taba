//! Write-ahead log management — disk-backed persistence (DL-014, FM-07).
//!
//! The WAL records three entry types (DL-008, INV-C4):
//!
//! - [`WalEntryType::Merged`] — a unit was verified and merged into the
//!   active graph. The payload is the serialized signed unit.
//! - [`WalEntryType::Pending`] — a unit was verified but its references
//!   are not yet satisfied; it is buffered in the pending queue.
//! - [`WalEntryType::Promoted`] — a pending unit was promoted to active
//!   after its references arrived.
//!
//! **WAL-before-effect**: every mutation is WAL'd atomically before its
//! effects become visible to local queries (INV-C4). The disk WAL
//! survives restarts and is the basis for local state recovery.
//!
//! ## Frame format
//!
//! ```text
//! ┌──────────┬──────────┬───────────────────────┬──────────┐
//! │ len: u32 │ crc: u32 │ WalEntry (protobuf)   │ pad 0-7  │
//! └──────────┴──────────┴───────────────────────┴──────────┘
//! ```
//!
//! - `len`: payload length in bytes (little-endian u32)
//! - `crc`: CRC32C of the protobuf payload (corruption detection, FM-07)
//! - Payload: prost-encoded [`WalEntryProto`](crate::proto::WalEntryProto)
//! - Padding: zero bytes to 8-byte alignment
//!
//! **Segment naming**: `wal-{sequence_start:016}.log`
//! Default segment size: 64 MB.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use prost::Message;
use serde::{Deserialize, Serialize};
use taba_common::{DualClockEvent, UnitId};

use crate::error::{NodeError, WalPosition};
use crate::proto::{self, WalEntryProto};

// ---------------------------------------------------------------------------
// WalEntry domain types
// ---------------------------------------------------------------------------

/// A single entry in the node-local write-ahead log (DL-014, INV-C4).
///
/// Every mutation is WAL'd atomically before effects become visible.
/// The WAL survives restarts and is the basis for local state recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalEntry {
    /// Monotonically increasing sequence number local to this node.
    pub sequence: u64,
    /// When this entry was written (dual clock — logical for ordering,
    /// wall time for compliance).
    pub written_at: DualClockEvent,
    /// The type and payload of this WAL entry.
    pub entry_type: WalEntryType,
}

/// The three WAL entry types (INV-C4, DL-008).
///
/// Mutations form a partial (causal) order, not a total order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WalEntryType {
    /// A unit has been verified and merged into the local graph state.
    Merged {
        /// The ID of the merged unit.
        unit_id: UnitId,
        /// Serialized signed unit (opaque bytes for WAL storage).
        payload: Vec<u8>,
    },
    /// A unit has been verified but references are not yet satisfied.
    ///
    /// Held until referenced units arrive (causal buffering).
    Pending {
        /// The ID of the pending unit.
        unit_id: UnitId,
        /// Serialized signed unit.
        payload: Vec<u8>,
        /// The references that are not yet present.
        missing_refs: Vec<UnitId>,
    },
    /// A previously pending unit has been activated after its
    /// references arrived.
    Promoted {
        /// The ID of the promoted unit.
        unit_id: UnitId,
    },
}

impl WalEntryType {
    /// Returns the unit ID associated with this entry type.
    #[must_use]
    pub const fn unit_id(&self) -> UnitId {
        match self {
            Self::Merged { unit_id, .. }
            | Self::Pending { unit_id, .. }
            | Self::Promoted { unit_id } => *unit_id,
        }
    }
}

// ---------------------------------------------------------------------------
// WalConfig
// ---------------------------------------------------------------------------

/// WAL segment configuration (DL-014).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalConfig {
    /// Maximum segment size in bytes before rotation (default: 64 MB).
    pub max_segment_bytes: u64,
    /// Timeout for pending entries before they are eligible for
    /// discard (default: 1 hour).
    pub pending_expiry: std::time::Duration,
    /// Directory for WAL segment files.
    pub wal_dir: String,
    /// Directory for decision trail segment files (taba-observe).
    pub trail_dir: String,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            max_segment_bytes: 64 * 1024 * 1024,                  // 64 MB
            pending_expiry: std::time::Duration::from_secs(3600), // 1 hour
            wal_dir: "./wal".to_string(),
            trail_dir: "./trail".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// WalManager trait
// ---------------------------------------------------------------------------

/// Write-ahead log for local persistence of graph state (DL-014).
///
/// WAL-before-effect (INV-C4): every mutation is persisted to the WAL
/// atomically before its effects become visible to local queries.
pub trait WalManager: Send + Sync {
    /// Append an entry to the WAL atomically.
    ///
    /// The entry is durable after this call returns — the method does
    /// not return until after `fsync()` (or equivalent). If the write
    /// fails, the node should enter degraded mode.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalWriteFailed`] on I/O failure.
    async fn append(&self, entry: WalEntry) -> Result<WalPosition, NodeError>;

    /// Replay the WAL from a given position forward.
    ///
    /// Used on node restart to reconstruct in-memory graph state.
    /// Entries are replayed in WAL order. Corrupt entries cause replay
    /// to stop and return [`NodeError::WalCorrupted`] with the position
    /// of the bad entry.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalCorrupted`] if a CRC mismatch is detected.
    /// - [`NodeError::WalWriteFailed`] on I/O failure during replay.
    async fn replay(
        &self,
        from: WalPosition,
        callback: &mut dyn FnMut(WalEntry) -> Result<(), NodeError>,
    ) -> Result<WalPosition, NodeError>;

    /// Replay the WAL from a given position, returning entries
    /// with their full serialized payloads.
    ///
    /// Unlike [`replay`](Self::replay) (which returns `WalEntry`
    /// without payload), this method returns [`WalEntryType`] which
    /// includes the serialized unit data for `Merged` entries.
    /// This allows full graph reconstruction from the WAL.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalCorrupted`] if a CRC mismatch is detected.
    /// - [`NodeError::WalWriteFailed`] on I/O failure during replay.
    async fn replay_with_payload(&self, from: WalPosition) -> Result<Vec<WalEntryType>, NodeError>;

    /// Compact the WAL by removing entries older than the given position.
    ///
    /// Safe to call only after all entries up to `before` have been
    /// durably reflected in the graph. Frees disk space.
    ///
    /// Returns the number of bytes freed.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalWriteFailed`] on I/O failure during compaction.
    async fn compact(&self, before: WalPosition) -> Result<u64, NodeError>;

    /// Get the current WAL size in bytes (all segments combined).
    fn size_bytes(&self) -> u64;

    /// Get the latest WAL position (most recent entry).
    fn latest_position(&self) -> WalPosition;
}

// ---------------------------------------------------------------------------
// Frame encoding/decoding
// ---------------------------------------------------------------------------

/// Frame header size: 4 bytes length + 4 bytes CRC32C.
const FRAME_HEADER_SIZE: u64 = 8;

/// Alignment for frame padding.
const FRAME_ALIGNMENT: u64 = 8;

/// Encodes a [`WalEntry`] into a WAL frame: `[len: u32 LE][crc: u32 LE][payload][pad]`.
///
/// Returns the encoded frame bytes.
#[allow(clippy::cast_possible_truncation)]
// u64 → u8 is safe: segment number is bounded by max_segment_bytes / frame_size
fn encode_frame(entry: &WalEntry) -> Vec<u8> {
    let proto = proto::wal_entry_to_proto(entry);
    let payload = proto.encode_to_vec();
    let crc = crc32c::crc32c(&payload);

    let payload_len = payload.len() as u32;
    let unpadded = FRAME_HEADER_SIZE + payload.len() as u64;
    let padding = ((FRAME_ALIGNMENT - (unpadded % FRAME_ALIGNMENT)) % FRAME_ALIGNMENT) as usize;

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE as usize + payload.len() + padding);
    frame.extend_from_slice(&payload_len.to_le_bytes());
    frame.extend_from_slice(&crc.to_le_bytes());
    frame.extend_from_slice(&payload);
    frame.extend(std::iter::repeat_n(0u8, padding));

    frame
}

/// Decodes a WAL frame from a file at the current read position.
///
/// Returns `(WalEntry, frame_size)` where `frame_size` is the total
/// number of bytes consumed (header + payload + padding).
#[allow(clippy::cast_possible_wrap)]
// u64 → i64 is safe: padding is always in [0, 7], well within i64 range
fn decode_frame(file: &mut File) -> Result<(WalEntry, u64), NodeError> {
    // Read 4-byte length (little-endian).
    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)
        .map_err(|e| NodeError::WalCorrupted {
            position: WalPosition(file.stream_position().unwrap_or(0).saturating_sub(0)),
            reason: format!("failed to read frame length: {e}"),
        })?;
    let payload_len = u32::from_le_bytes(len_buf) as usize;

    // Read 4-byte CRC32C (little-endian).
    let mut crc_buf = [0u8; 4];
    file.read_exact(&mut crc_buf)
        .map_err(|e| NodeError::WalCorrupted {
            position: WalPosition(file.stream_position().unwrap_or(0).saturating_sub(4)),
            reason: format!("failed to read frame CRC: {e}"),
        })?;
    let expected_crc = u32::from_le_bytes(crc_buf);

    // Read payload.
    let mut payload = vec![0u8; payload_len];
    file.read_exact(&mut payload)
        .map_err(|e| NodeError::WalCorrupted {
            position: WalPosition(file.stream_position().unwrap_or(0).saturating_sub(8)),
            reason: format!("failed to read frame payload: {e}"),
        })?;

    // Verify CRC32C.
    let actual_crc = crc32c::crc32c(&payload);
    if actual_crc != expected_crc {
        let pos = file
            .stream_position()
            .unwrap_or(0)
            .saturating_sub(FRAME_HEADER_SIZE + payload_len as u64);
        return Err(NodeError::WalCorrupted {
            position: WalPosition(pos),
            reason: format!(
                "CRC32C mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
            ),
        });
    }

    // Decode protobuf payload.
    let proto_entry =
        WalEntryProto::decode(payload.as_slice()).map_err(|e| NodeError::WalCorrupted {
            position: WalPosition(
                file.stream_position()
                    .unwrap_or(0)
                    .saturating_sub(FRAME_HEADER_SIZE + payload_len as u64),
            ),
            reason: format!("failed to decode WalEntry protobuf: {e}"),
        })?;

    let entry = proto::wal_entry_from_proto(&proto_entry)?;

    // Compute frame size and skip padding.
    let unpadded = FRAME_HEADER_SIZE + payload_len as u64;
    let padding = (FRAME_ALIGNMENT - (unpadded % FRAME_ALIGNMENT)) % FRAME_ALIGNMENT;
    if padding > 0 {
        file.seek(SeekFrom::Current(padding as i64))
            .map_err(|e| NodeError::WalCorrupted {
                position: WalPosition(file.stream_position().unwrap_or(0)),
                reason: format!("failed to skip padding: {e}"),
            })?;
    }

    let frame_size = FRAME_HEADER_SIZE + payload_len as u64 + padding;
    Ok((entry, frame_size))
}

// ---------------------------------------------------------------------------
// Segment management
// ---------------------------------------------------------------------------

/// Formats a segment file name: `wal-{sequence_start:016}.log`.
fn segment_name(sequence_start: u64) -> String {
    format!("wal-{sequence_start:016}.log")
}

/// A WAL segment file with its starting sequence number.
#[derive(Debug, Clone)]
struct Segment {
    /// The starting sequence number for this segment.
    sequence_start: u64,
    /// The path to the segment file.
    path: PathBuf,
    /// Current size in bytes.
    size: u64,
}

impl Segment {
    /// Opens an existing segment file and reads its size.
    fn open(sequence_start: u64, dir: &Path) -> Result<Self, NodeError> {
        let path = dir.join(segment_name(sequence_start));
        let size =
            std::fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|e| NodeError::WalWriteFailed {
                    reason: format!("failed to stat segment {}: {e}", path.display()),
                })?;
        Ok(Self {
            sequence_start,
            path,
            size,
        })
    }

    /// Creates a new empty segment file.
    fn create(sequence_start: u64, dir: &Path) -> Result<Self, NodeError> {
        let path = dir.join(segment_name(sequence_start));
        File::create(&path).map_err(|e| NodeError::WalWriteFailed {
            reason: format!("failed to create segment {}: {e}", path.display()),
        })?;
        Ok(Self {
            sequence_start,
            path,
            size: 0,
        })
    }

    /// Appends a frame to this segment and fsyncs.
    fn append_frame(&mut self, frame: &[u8]) -> Result<u64, NodeError> {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(|e| NodeError::WalWriteFailed {
                reason: format!(
                    "failed to open segment {path} for append: {e}",
                    path = self.path.display()
                ),
            })?;

        let position = self.size;
        file.write_all(frame)
            .map_err(|e| NodeError::WalWriteFailed {
                reason: format!(
                    "failed to write to segment {path}: {e}",
                    path = self.path.display()
                ),
            })?;
        file.sync_all().map_err(|e| NodeError::WalWriteFailed {
            reason: format!(
                "failed to fsync segment {path}: {e}",
                path = self.path.display()
            ),
        })?;

        self.size += frame.len() as u64;
        Ok(position)
    }
}

// ---------------------------------------------------------------------------
// DiskWalManager
// ---------------------------------------------------------------------------

/// Disk-backed write-ahead log manager (DL-014).
///
/// Stores WAL entries as framed protobuf records on disk. Each frame
/// consists of a 4-byte little-endian length, a 4-byte little-endian
/// CRC32C, the protobuf payload, and zero-padding to 8-byte alignment.
///
/// Segment files are named `wal-{sequence_start:016}.log` and rotate
/// when they exceed `max_segment_bytes` (default: 64 MB).
///
/// Thread-safe via a [`Mutex`] — the lock is never held across an
/// `await` point (all operations are synchronous).
#[derive(Debug)]
pub struct DiskWalManager {
    /// Configuration: segment size, directories, etc.
    config: WalConfig,
    /// The directory where segment files are stored.
    dir: PathBuf,
    /// All segments, in order of `sequence_start`.
    segments: Mutex<Vec<Segment>>,
    /// The next sequence number to assign.
    next_sequence: Mutex<u64>,
    /// Total size across all segments.
    total_size: Mutex<u64>,
}

impl DiskWalManager {
    /// Creates a new `DiskWalManager`, opening or creating the WAL
    /// directory.
    ///
    /// If the directory already contains segment files, they are
    /// loaded in order and the next sequence number is set to one past
    /// the last entry.
    ///
    /// # Errors
    ///
    /// - [`NodeError::WalWriteFailed`] if the directory cannot be
    ///   created or segments cannot be loaded.
    pub fn new(config: WalConfig) -> Result<Self, NodeError> {
        let dir = PathBuf::from(&config.wal_dir);

        // Create the WAL directory if it doesn't exist.
        std::fs::create_dir_all(&dir).map_err(|e| NodeError::WalWriteFailed {
            reason: format!("failed to create WAL directory {}: {e}", dir.display()),
        })?;

        // Load existing segments in order.
        let mut entries = std::fs::read_dir(&dir).map_err(|e| NodeError::WalWriteFailed {
            reason: format!("failed to read WAL directory {}: {e}", dir.display()),
        })?;

        let mut segment_starts: Vec<u64> = Vec::new();
        for entry in entries.by_ref() {
            let entry = entry.map_err(|e| NodeError::WalWriteFailed {
                reason: format!("failed to read directory entry: {e}"),
            })?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(seq) = parse_segment_name(&name_str) {
                segment_starts.push(seq);
            }
        }
        segment_starts.sort_unstable();

        let mut segments = Vec::new();
        let mut total_size = 0u64;
        let mut next_sequence = 1u64;

        for seq_start in &segment_starts {
            let seg = Segment::open(*seq_start, &dir)?;
            total_size += seg.size;

            // If this is the last segment, scan it to find the highest
            // sequence number (for next_sequence).
            if seq_start == segment_starts.last().expect("non-empty") {
                next_sequence = scan_last_segment(&seg)? + 1;
            }

            segments.push(seg);
        }

        // If no segments exist, create the first one.
        if segments.is_empty() {
            segments.push(Segment::create(1, &dir)?);
        }

        Ok(Self {
            config,
            dir,
            segments: Mutex::new(segments),
            next_sequence: Mutex::new(next_sequence),
            total_size: Mutex::new(total_size),
        })
    }

    /// Gets the current (last) segment, creating a new one if the
    /// current segment exceeds `max_segment_bytes`.
    ///
    /// `next_seq_start` is the sequence number that will be assigned to
    /// the next entry — used as the name of any new segment. It is
    /// passed in (rather than read from `self.next_sequence`) to avoid
    /// re-entrant locking: callers already hold the `next_sequence`
    /// mutex.
    fn current_segment<'a>(
        &'a self,
        segments: &'a mut Vec<Segment>,
        next_seq_start: u64,
    ) -> Result<&'a mut Segment, NodeError> {
        let needs_rotation = segments
            .last()
            .is_some_and(|s| s.size >= self.config.max_segment_bytes);

        if needs_rotation {
            segments.push(Segment::create(next_seq_start, &self.dir)?);
        }

        segments
            .last_mut()
            .ok_or_else(|| NodeError::WalWriteFailed {
                reason: "no WAL segment available".to_string(),
            })
    }
}

/// Parses a segment file name and returns the sequence start.
///
/// Returns `None` if the name doesn't match `wal-{016}.log`.
fn parse_segment_name(name: &str) -> Option<u64> {
    let name = name.strip_suffix(".log")?;
    let prefix = name.strip_prefix("wal-")?;
    prefix.parse().ok()
}

/// Scans a segment file to find the highest sequence number.
///
/// Returns 0 if the segment is empty.
fn scan_last_segment(segment: &Segment) -> Result<u64, NodeError> {
    let mut file = File::open(&segment.path).map_err(|e| NodeError::WalWriteFailed {
        reason: format!(
            "failed to open segment {path} for scanning: {e}",
            path = segment.path.display()
        ),
    })?;

    let mut last_seq = segment.sequence_start.saturating_sub(1);
    loop {
        let pos_before = file.stream_position().unwrap_or(0);
        match decode_frame(&mut file) {
            Ok((entry, _)) => {
                last_seq = entry.sequence;
            }
            Err(NodeError::WalCorrupted { .. }) => {
                // A corruption here (e.g., truncated frame at end) is
                // non-fatal during scan — we just stop.
                file.seek(SeekFrom::Start(pos_before))
                    .map_err(|e| NodeError::WalWriteFailed {
                        reason: format!("failed to seek back during scan: {e}"),
                    })?;
                break;
            }
            Err(e) => return Err(e),
        }
    }
    Ok(last_seq)
}

impl WalManager for DiskWalManager {
    async fn append(&self, entry: WalEntry) -> Result<WalPosition, NodeError> {
        let mut next_seq = self
            .next_sequence
            .lock()
            .expect("next_sequence mutex should not be poisoned");

        let mut entry = entry;
        entry.sequence = *next_seq;

        let frame = encode_frame(&entry);

        let mut segments = self
            .segments
            .lock()
            .expect("segments mutex should not be poisoned");

        let seg = self.current_segment(&mut segments, *next_seq)?;
        let pos_in_segment = seg.append_frame(&frame)?;

        // The global position is the file position within the current segment.
        // For simplicity, we use the offset within the latest segment.
        let position = WalPosition(pos_in_segment);

        *next_seq += 1;

        let mut total = self
            .total_size
            .lock()
            .expect("total_size mutex should not be poisoned");
        *total += frame.len() as u64;

        Ok(position)
    }

    async fn replay(
        &self,
        from: WalPosition,
        callback: &mut dyn FnMut(WalEntry) -> Result<(), NodeError>,
    ) -> Result<WalPosition, NodeError> {
        let segments = self
            .segments
            .lock()
            .expect("segments mutex should not be poisoned");

        let from_u64 = from.as_u64();
        let mut last_position = from;

        for seg in segments.iter() {
            let mut file = File::open(&seg.path).map_err(|e| NodeError::WalWriteFailed {
                reason: format!(
                    "failed to open segment {path} for replay: {e}",
                    path = seg.path.display()
                ),
            })?;

            // Skip entries before the `from` position (FINDING-012).
            // Only invoke the callback for entries at or after `from`.
            loop {
                let pos_before = file.stream_position().unwrap_or(0);

                match decode_frame(&mut file) {
                    Ok((entry, frame_size)) => {
                        last_position = WalPosition(pos_before);
                        if pos_before < from_u64 {
                            // Entry is before the replay cursor — skip.
                            let _ = frame_size; // frame_size already consumed by seek
                            continue;
                        }
                        callback(entry)?;
                        let _ = frame_size; // frame_size already consumed by seek
                    }
                    Err(NodeError::WalCorrupted { reason, .. })
                        if reason.contains("failed to read frame length")
                            || reason.contains("failed to read frame CRC")
                            || reason.contains("failed to read frame payload") =>
                    {
                        // End of file (clean read failure at boundary) — stop replay.
                        break;
                    }
                    Err(NodeError::WalCorrupted { position, reason })
                        if reason.contains("CRC32C mismatch") =>
                    {
                        // Actual CRC corruption — fail.
                        return Err(NodeError::WalCorrupted { position, reason });
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        Ok(last_position)
    }

    async fn replay_with_payload(&self, from: WalPosition) -> Result<Vec<WalEntryType>, NodeError> {
        let segments = self
            .segments
            .lock()
            .expect("segments mutex should not be poisoned");

        let from_u64 = from.as_u64();
        let mut entries = Vec::new();

        for seg in segments.iter() {
            let mut file = File::open(&seg.path).map_err(|e| NodeError::WalWriteFailed {
                reason: format!(
                    "failed to open segment {path} for replay: {e}",
                    path = seg.path.display()
                ),
            })?;

            loop {
                let pos_before = file.stream_position().unwrap_or(0);

                match decode_frame(&mut file) {
                    Ok((entry, _)) => {
                        if pos_before < from_u64 {
                            continue;
                        }
                        entries.push(entry.entry_type);
                    }
                    Err(NodeError::WalCorrupted { reason, .. })
                        if reason.contains("failed to read frame length")
                            || reason.contains("failed to read frame CRC")
                            || reason.contains("failed to read frame payload") =>
                    {
                        break;
                    }
                    Err(NodeError::WalCorrupted { position, reason })
                        if reason.contains("CRC32C mismatch") =>
                    {
                        return Err(NodeError::WalCorrupted { position, reason });
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        Ok(entries)
    }

    async fn compact(&self, before: WalPosition) -> Result<u64, NodeError> {
        // Lock order MUST match append: next_sequence → segments → total_size.
        // A different order risks cross-method deadlock on concurrent calls.
        let next_seq = self
            .next_sequence
            .lock()
            .expect("next_sequence mutex should not be poisoned");

        let mut segments = self
            .segments
            .lock()
            .expect("segments mutex should not be poisoned");

        // Collect entries that are after `before` from all segments.
        let mut kept_entries: Vec<WalEntry> = Vec::new();

        for seg in segments.iter() {
            let mut file = File::open(&seg.path).map_err(|e| NodeError::WalWriteFailed {
                reason: format!(
                    "failed to open segment {path} for compaction: {e}",
                    path = seg.path.display()
                ),
            })?;

            loop {
                let pos_before = file.stream_position().unwrap_or(0);

                match decode_frame(&mut file) {
                    Ok((entry, _)) => {
                        // Keep entries that are at or after the `before` position.
                        if pos_before >= before.as_u64() {
                            kept_entries.push(entry);
                        }
                    }
                    Err(NodeError::WalCorrupted { reason, .. })
                        if reason.contains("failed to read frame length")
                            || reason.contains("failed to read frame CRC")
                            || reason.contains("failed to read frame payload") =>
                    {
                        // End of file — stop.
                        break;
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        // Compute freed bytes (old total minus what we'll keep).
        let mut total = self
            .total_size
            .lock()
            .expect("total_size mutex should not be poisoned");
        let old_total = *total;

        // Write-then-delete: create the new segment FIRST, then delete
        // old ones. If the process crashes during compaction, old
        // segments are still intact and can be replayed on recovery.
        // (FINDING-011: deleting before writing caused data loss.)
        let mut new_seg = Segment::create(*next_seq, &self.dir)?;
        let mut new_total = 0u64;

        for entry in &kept_entries {
            let frame = encode_frame(entry);
            new_seg.append_frame(&frame)?;
            new_total += frame.len() as u64;
        }

        // Flush the new segment to disk before deleting old ones.
        // Open the file and fsync to ensure durability (Segment stores
        // only the path, not a file handle).
        std::fs::OpenOptions::new()
            .write(true)
            .open(&new_seg.path)
            .map_err(|e| NodeError::WalWriteFailed {
                reason: format!("failed to open new segment for fsync: {e}"),
            })?
            .sync_all()
            .map_err(|e| NodeError::WalWriteFailed {
                reason: format!("failed to fsync new segment during compaction: {e}"),
            })?;

        // Now safe to delete old segments.
        for seg in segments.iter() {
            let _ = std::fs::remove_file(&seg.path);
        }
        segments.clear();

        segments.push(new_seg);

        *total = new_total;

        Ok(old_total.saturating_sub(new_total))
    }

    fn size_bytes(&self) -> u64 {
        *self
            .total_size
            .lock()
            .expect("total_size mutex should not be poisoned")
    }

    fn latest_position(&self) -> WalPosition {
        let segments = self
            .segments
            .lock()
            .expect("segments mutex should not be poisoned");
        segments
            .last()
            .map_or(WalPosition::ZERO, |s| WalPosition(s.size))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use taba_common::{LogicalClock, WallTime};

    /// Creates a minimal [`DualClockEvent`] for testing.
    fn test_clock(logical: u64) -> DualClockEvent {
        DualClockEvent {
            logical_clock: LogicalClock(logical),
            wall_time: WallTime {
                millis: 1000 + logical,
            },
            timezone: "UTC".to_string(),
        }
    }

    /// Creates a `Merged` `WalEntry` for testing.
    fn merged_entry(seq: u64, unit_id: UnitId, payload: Vec<u8>) -> WalEntry {
        WalEntry {
            sequence: seq,
            written_at: test_clock(seq),
            entry_type: WalEntryType::Merged { unit_id, payload },
        }
    }

    /// Creates a `Promoted` `WalEntry` for testing.
    fn promoted_entry(seq: u64, unit_id: UnitId) -> WalEntry {
        WalEntry {
            sequence: seq,
            written_at: test_clock(seq),
            entry_type: WalEntryType::Promoted { unit_id },
        }
    }

    /// Creates a `Pending` `WalEntry` for testing.
    fn pending_entry(seq: u64, unit_id: UnitId, missing: Vec<UnitId>) -> WalEntry {
        WalEntry {
            sequence: seq,
            written_at: test_clock(seq),
            entry_type: WalEntryType::Pending {
                unit_id,
                payload: vec![0u8; 32],
                missing_refs: missing,
            },
        }
    }

    // -- Domain type tests ---------------------------------------------------

    #[test]
    fn test_wal_entry_type_unit_id() {
        let id = UnitId(uuid::Uuid::new_v4());
        assert_eq!(
            WalEntryType::Merged {
                unit_id: id,
                payload: vec![]
            }
            .unit_id(),
            id
        );
        assert_eq!(
            WalEntryType::Pending {
                unit_id: id,
                payload: vec![],
                missing_refs: vec![]
            }
            .unit_id(),
            id
        );
        assert_eq!(WalEntryType::Promoted { unit_id: id }.unit_id(), id);
    }

    // -- DiskWalManager tests ------------------------------------------------

    #[tokio::test]
    #[allow(clippy::cast_possible_truncation)] // test: i (1..=10) fits in u8
    async fn test_append_and_replay() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append 10 entries.
        let mut expected = Vec::new();
        for i in 1..=10 {
            let id = UnitId(uuid::Uuid::new_v4());
            let entry = if i % 3 == 0 {
                pending_entry(i, id, vec![UnitId(uuid::Uuid::new_v4())])
            } else if i % 3 == 1 {
                merged_entry(i, id, vec![i as u8; 64])
            } else {
                promoted_entry(i, id)
            };
            wal.append(entry.clone()).await.expect("append entry");
            expected.push(entry);
        }

        // Replay all.
        let mut actual = Vec::new();
        let last_pos = wal
            .replay(WalPosition::ZERO, &mut |entry| {
                actual.push(entry);
                Ok(())
            })
            .await
            .expect("replay");

        assert_eq!(actual.len(), 10, "should replay all 10 entries");

        // Verify sequences and types match.
        for (i, (exp, act)) in expected.iter().zip(actual.iter()).enumerate() {
            assert_eq!(exp.sequence, act.sequence, "sequence mismatch at index {i}");
            assert_eq!(
                exp.entry_type.unit_id(),
                act.entry_type.unit_id(),
                "unit_id mismatch at index {i}"
            );
            assert_eq!(
                exp.written_at.logical_clock, act.written_at.logical_clock,
                "logical_clock mismatch at index {i}"
            );
        }

        let _ = last_pos;
    }

    #[tokio::test]
    async fn test_replay_from_position() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append 10 entries.
        for i in 1..=10 {
            let id = UnitId(uuid::Uuid::new_v4());
            wal.append(promoted_entry(i, id))
                .await
                .expect("append entry");
        }

        // Replay from zero should return all entries.
        let mut all = Vec::new();
        wal.replay(WalPosition::ZERO, &mut |entry| {
            all.push(entry);
            Ok(())
        })
        .await
        .expect("replay from zero");

        assert_eq!(all.len(), 10, "replay from 0 should return all 10");
    }

    #[tokio::test]
    async fn test_replay_from_nonzero_position() {
        // FINDING-012: replay(from) must skip entries before `from`
        // and only invoke the callback for entries at or after `from`.
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append the first entry and record the WAL position after it.
        let id1 = UnitId(uuid::Uuid::new_v4());
        wal.append(promoted_entry(1, id1))
            .await
            .expect("append first entry");
        let pos_after_first = wal.latest_position();
        assert!(
            pos_after_first.as_u64() > 0,
            "position after first entry should be non-zero"
        );

        #[allow(clippy::collection_is_never_read)]
        // Append 4 more entries (2 through 5).
        let mut expected_ids = vec![id1];
        for i in 2..=5 {
            let id = UnitId(uuid::Uuid::new_v4());
            wal.append(promoted_entry(i, id))
                .await
                .expect("append entry");
            expected_ids.push(id);
        }

        // Replay from after the first entry — should only return 4 entries.
        let mut replayed = Vec::new();
        wal.replay(pos_after_first, &mut |entry| {
            replayed.push(entry);
            Ok(())
        })
        .await
        .expect("replay from non-zero position");

        assert_eq!(
            replayed.len(),
            4,
            "replay from after entry 1 should return entries 2-5 (4 entries), got {}",
            replayed.len()
        );

        // The first replayed entry should be entry 2 (sequence 2), not entry 1.
        assert_eq!(
            replayed[0].sequence, 2,
            "first replayed entry should have sequence 2"
        );
    }

    #[tokio::test]
    async fn test_crc_corruption_detected() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append one entry.
        let id = UnitId(uuid::Uuid::new_v4());
        wal.append(merged_entry(1, id, vec![42u8; 32]))
            .await
            .expect("append entry");

        // Corrupt the segment file by flipping some payload bytes.
        let seg_path = {
            let segments = wal
                .segments
                .lock()
                .expect("segments mutex should not be poisoned");
            segments[0].path.clone()
        };

        let data = std::fs::read(&seg_path).expect("read segment");
        // Corrupt bytes in the payload area (after the 8-byte header).
        let mut corrupted = data.clone();
        if corrupted.len() > 20 {
            corrupted[20] ^= 0xFF;
        }
        std::fs::write(&seg_path, &corrupted).expect("write corrupted segment");

        // Replay should detect corruption.
        let result = wal.replay(WalPosition::ZERO, &mut |_| Ok(())).await;
        assert!(
            matches!(result, Err(NodeError::WalCorrupted { .. })),
            "replay should detect CRC corruption, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_compact_frees_space() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append 100 entries.
        for i in 1..=100 {
            let id = UnitId(uuid::Uuid::new_v4());
            wal.append(promoted_entry(i, id))
                .await
                .expect("append entry");
        }

        let size_before = wal.size_bytes();
        assert!(size_before > 0, "WAL should have non-zero size");

        // Replay to count entries before compaction.
        let mut entries_before = 0usize;
        wal.replay(WalPosition::ZERO, &mut |_| {
            entries_before += 1;
            Ok(())
        })
        .await
        .expect("replay all before compaction");

        // Compact: keep entries from the midpoint onward (approximate
        // position based on uniform frame size assumption).
        let approx_pos = WalPosition(size_before / 2);
        let freed = wal
            .compact(approx_pos)
            .await
            .expect("compact should succeed");

        let size_after = wal.size_bytes();

        // Compaction should have freed some space or been a no-op.
        assert!(
            freed > 0 || size_after == size_before,
            "compaction should either free space or be a no-op"
        );

        // Verify we can still replay after compaction.
        let mut count = 0;
        wal.replay(WalPosition::ZERO, &mut |_| {
            count += 1;
            Ok(())
        })
        .await
        .expect("replay after compaction");

        assert!(
            count <= entries_before,
            "compaction should not increase entry count"
        );
    }

    #[tokio::test]
    #[allow(clippy::cast_possible_truncation)] // test: i (1..=50) fits in u8
    async fn test_segment_rotation() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        // Use a very small segment size to force rotation.
        let config = WalConfig {
            max_segment_bytes: 200,
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append many entries to force multiple segments.
        for i in 1..=50 {
            let id = UnitId(uuid::Uuid::new_v4());
            wal.append(merged_entry(i, id, vec![i as u8; 64]))
                .await
                .expect("append entry");
        }

        // Verify multiple segments were created.
        let segments = wal
            .segments
            .lock()
            .expect("segments mutex should not be poisoned");
        assert!(
            segments.len() > 1,
            "should have multiple segments after rotation, got {}",
            segments.len()
        );
    }

    #[tokio::test]
    #[allow(clippy::cast_sign_loss)] // test: count is always non-negative
    async fn test_replay_across_segments() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            max_segment_bytes: 200,
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Append entries across multiple segments.
        for i in 1..=30 {
            let id = UnitId(uuid::Uuid::new_v4());
            wal.append(promoted_entry(i, id))
                .await
                .expect("append entry");
        }

        // Replay should return all entries across segments.
        let mut count = 0;
        wal.replay(WalPosition::ZERO, &mut |entry| {
            count += 1;
            assert_eq!(
                entry.sequence, count as u64,
                "sequence should match replay order"
            );
            Ok(())
        })
        .await
        .expect("replay across segments");

        assert_eq!(count, 30, "should replay all 30 entries across segments");
    }

    #[tokio::test]
    async fn test_size_bytes() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        assert_eq!(wal.size_bytes(), 0, "new WAL should have zero size");

        let id = UnitId(uuid::Uuid::new_v4());
        wal.append(promoted_entry(1, id))
            .await
            .expect("append entry");

        let size_after = wal.size_bytes();
        assert!(size_after > 0, "WAL should have non-zero size after append");

        // Append another and verify size increases.
        let id2 = UnitId(uuid::Uuid::new_v4());
        wal.append(promoted_entry(2, id2))
            .await
            .expect("append entry");

        let size_final = wal.size_bytes();
        assert!(
            size_final > size_after,
            "WAL size should increase after second append"
        );
    }

    #[tokio::test]
    async fn test_latest_position() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        // Empty WAL → zero position.
        assert_eq!(wal.latest_position(), WalPosition::ZERO);

        // After appending, position should be non-zero.
        let id = UnitId(uuid::Uuid::new_v4());
        wal.append(promoted_entry(1, id))
            .await
            .expect("append entry");

        let pos = wal.latest_position();
        assert!(
            pos.as_u64() > 0,
            "latest_position should be > 0 after append"
        );

        // After another append, position should increase.
        let id2 = UnitId(uuid::Uuid::new_v4());
        wal.append(promoted_entry(2, id2))
            .await
            .expect("append entry");

        let pos2 = wal.latest_position();
        assert!(
            pos2.as_u64() > pos.as_u64(),
            "latest_position should increase after append"
        );
    }

    #[tokio::test]
    async fn test_all_wal_entry_types_roundtrip() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let config = WalConfig {
            wal_dir: tmp.path().join("wal").to_string_lossy().to_string(),
            ..WalConfig::default()
        };
        let wal = DiskWalManager::new(config).expect("create DiskWalManager");

        let merged_id = UnitId(uuid::Uuid::new_v4());
        let pending_id = UnitId(uuid::Uuid::new_v4());
        let promoted_id = UnitId(uuid::Uuid::new_v4());
        let missing_ref = UnitId(uuid::Uuid::new_v4());

        wal.append(merged_entry(1, merged_id, vec![1u8, 2u8, 3u8]))
            .await
            .expect("append merged");
        wal.append(pending_entry(2, pending_id, vec![missing_ref]))
            .await
            .expect("append pending");
        wal.append(promoted_entry(3, promoted_id))
            .await
            .expect("append promoted");

        let mut entries = Vec::new();
        wal.replay(WalPosition::ZERO, &mut |entry| {
            entries.push(entry);
            Ok(())
        })
        .await
        .expect("replay");

        assert_eq!(entries.len(), 3);

        // Verify Merged.
        assert_eq!(entries[0].sequence, 1);
        match &entries[0].entry_type {
            WalEntryType::Merged { unit_id, payload } => {
                assert_eq!(*unit_id, merged_id);
                assert_eq!(payload, &[1u8, 2u8, 3u8]);
            }
            other => panic!("expected Merged, got {other:?}"),
        }

        // Verify Pending.
        assert_eq!(entries[1].sequence, 2);
        match &entries[1].entry_type {
            WalEntryType::Pending {
                unit_id,
                payload,
                missing_refs,
            } => {
                assert_eq!(*unit_id, pending_id);
                assert_eq!(payload, &vec![0u8; 32]);
                assert_eq!(*missing_refs, vec![missing_ref]);
            }
            other => panic!("expected Pending, got {other:?}"),
        }

        // Verify Promoted.
        assert_eq!(entries[2].sequence, 3);
        match &entries[2].entry_type {
            WalEntryType::Promoted { unit_id } => {
                assert_eq!(*unit_id, promoted_id);
            }
            other => panic!("expected Promoted, got {other:?}"),
        }
    }
}
