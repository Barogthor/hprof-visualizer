//! Hprof binary format primitives.
//!
//! Groups the low-level types and functions that
//! describe the hprof binary wire format:
//!
//! - [`IdSize`] + [`read_id`] — object ID width
//!   (4 or 8 bytes)
//! - [`HprofVersion`] + [`HprofHeader`] +
//!   [`parse_header`] — file header parsing
//! - [`HprofStringRef`] — lazy string references
//! - [`RecordHeader`] — record tag + payload length

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::HprofError;
use hprof_api::MemorySize;

// ── ID size ─────────────────────────────────────

/// Byte width of hprof object identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdSize {
    /// 4-byte (32-bit) object IDs.
    Four,
    /// 8-byte (64-bit) object IDs.
    Eight,
}

impl IdSize {
    /// Parses a raw `u32` from the file header into an
    /// `IdSize`.
    ///
    /// Returns `Err(HprofError::CorruptedData)` if `value`
    /// is not 4 or 8.
    pub fn from_raw(value: u32) -> Result<Self, HprofError> {
        match value {
            4 => Ok(Self::Four),
            8 => Ok(Self::Eight),
            _ => Err(HprofError::CorruptedData(format!(
                "invalid id_size: {value}, \
                 expected 4 or 8"
            ))),
        }
    }

    /// Returns the byte width as `u32`.
    pub fn as_u32(self) -> u32 {
        match self {
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    /// Returns the byte width as `usize`.
    pub fn as_usize(self) -> usize {
        self.as_u32() as usize
    }
}

/// Reads an object ID of `id_size` bytes from `cursor`
/// as a big-endian `u64`.
///
/// # Parameters
/// - `cursor`: `&mut Cursor<&[u8]>` — positioned at the
///   first byte of the ID.
/// - `id_size`: [`IdSize`] — byte width (4 or 8).
///
/// # Errors
/// - [`HprofError::TruncatedRecord`] — fewer bytes remain
///   than `id_size`.
pub fn read_id(cursor: &mut Cursor<&[u8]>, id_size: IdSize) -> Result<u64, HprofError> {
    match id_size {
        IdSize::Four => cursor
            .read_u32::<BigEndian>()
            .map(|v| v as u64)
            .map_err(|_| HprofError::TruncatedRecord),
        IdSize::Eight => cursor
            .read_u64::<BigEndian>()
            .map_err(|_| HprofError::TruncatedRecord),
    }
}

// ── Header ──────────────────────────────────────

/// Known hprof format versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HprofVersion {
    /// "JAVA PROFILE 1.0.1"
    V1_0_1,
    /// "JAVA PROFILE 1.0.2"
    V1_0_2,
}

/// Parsed contents of an hprof file header.
#[derive(Debug, Clone)]
pub struct HprofHeader {
    /// Format version of the dump.
    pub version: HprofVersion,
    /// Byte width of object IDs in this dump.
    pub id_size: IdSize,
    /// Byte offset where records begin (after version
    /// string, id_size, and timestamp).
    pub records_start: usize,
}

/// Computes the byte offset where records begin.
///
/// Returns `Err(HprofError::TruncatedRecord)` if the
/// arithmetic overflows (corrupt / impossibly large
/// version string).
fn records_start_offset(null_pos: usize) -> Result<usize, HprofError> {
    null_pos
        .checked_add(1 + 4 + 8)
        .ok_or(HprofError::TruncatedRecord)
}

/// Parses an hprof header from a raw byte slice.
///
/// # Parameters
/// - `data: &[u8]` — raw bytes starting at offset 0
///   of the hprof file.
///
/// # Returns
/// - `Ok(HprofHeader)` on success.
/// - `Err(HprofError::TruncatedRecord)` if the data
///   ends before a complete header.
/// - `Err(HprofError::UnsupportedVersion(String))` if
///   the version string is not
///   `"JAVA PROFILE 1.0.1"` or `"JAVA PROFILE 1.0.2"`.
/// - `Err(HprofError::CorruptedData(String))` if the
///   version bytes are not valid UTF-8.
pub fn parse_header(data: &[u8]) -> Result<HprofHeader, HprofError> {
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or(HprofError::TruncatedRecord)?;

    let version_str = std::str::from_utf8(&data[..null_pos])
        .map_err(|e| HprofError::CorruptedData(e.to_string()))?;

    let version = match version_str {
        "JAVA PROFILE 1.0.1" => HprofVersion::V1_0_1,
        "JAVA PROFILE 1.0.2" => HprofVersion::V1_0_2,
        other => return Err(HprofError::UnsupportedVersion(other.to_owned())),
    };

    let mut cursor = Cursor::new(&data[null_pos + 1..]);
    let raw_id_size = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let id_size = IdSize::from_raw(raw_id_size)?;
    let _timestamp = cursor
        .read_u64::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;

    let records_start = records_start_offset(null_pos)?;
    Ok(HprofHeader {
        version,
        id_size,
        records_start,
    })
}

// ── Strings ─────────────────────────────────────

/// A lazy reference to a `STRING_IN_UTF8` record's
/// content.
///
/// Stores only the location (offset + length) of the
/// string bytes in the records section, deferring
/// actual UTF-8 decoding to
/// [`crate::HprofFile::resolve_string`].
///
/// ## Fields
/// - `id`: `u64` -- object ID of the string
/// - `offset`: `u64` -- byte offset relative to records
///   section start
/// - `len`: `u32` -- byte length of the UTF-8 content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HprofStringRef {
    pub id: u64,
    pub offset: u64,
    pub len: u32,
}

impl MemorySize for HprofStringRef {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl HprofStringRef {
    /// Resolves this reference into an owned `String` by
    /// reading content bytes from `data`.
    ///
    /// `data` must be the records section slice (offsets
    /// are relative to records section start). Returns an
    /// empty string if the offset/length is out of bounds.
    /// Invalid UTF-8 bytes are replaced with `\u{FFFD}`.
    pub fn resolve(&self, data: &[u8]) -> String {
        let start = self.offset as usize;
        // Intentional: we want usize::MAX (not
        // saturating_add) so the subsequent
        // `data.get(start..end)` returns None on overflow.
        #[allow(clippy::manual_saturating_arithmetic)]
        let end = start.checked_add(self.len as usize).unwrap_or(usize::MAX);
        match data.get(start..end) {
            Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            None => String::new(),
        }
    }
}

// ── Record header ───────────────────────────────

/// Parsed header of a single hprof record.
///
/// Extracted from the 9-byte prefix that precedes every
/// record payload:
/// `tag(u8)` + `time_offset(u32 BE, discarded)` +
/// `length(u32 BE)`.
#[derive(Debug, Clone, Copy)]
pub struct RecordHeader {
    /// Tag byte identifying the record type
    /// (e.g. `0x01` = STRING_IN_UTF8).
    pub tag: u8,
    /// Payload byte length -- number of bytes
    /// immediately following this header.
    pub length: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── ID tests ────────────────────────────────

    #[test]
    fn read_id_4_byte() {
        let bytes = 0x0102_0304u32.to_be_bytes();
        let mut cursor = Cursor::new(bytes.as_slice());
        let id = read_id(&mut cursor, IdSize::Four).unwrap();
        assert_eq!(id, 0x0102_0304u64);
        assert_eq!(cursor.position(), 4);
    }

    #[test]
    fn read_id_8_byte() {
        let bytes = 0x0102_0304_0506_0708u64.to_be_bytes();
        let mut cursor = Cursor::new(bytes.as_slice());
        let id = read_id(&mut cursor, IdSize::Eight).unwrap();
        assert_eq!(id, 0x0102_0304_0506_0708u64);
        assert_eq!(cursor.position(), 8);
    }

    #[test]
    fn read_id_insufficient_bytes_4_returns_truncated() {
        let bytes = [0x01u8, 0x02];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, IdSize::Four).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn read_id_insufficient_bytes_8_returns_truncated() {
        let bytes = [0x01u8, 0x02, 0x03, 0x04];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = read_id(&mut cursor, IdSize::Eight).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn id_size_from_raw_valid() {
        assert_eq!(IdSize::from_raw(4).unwrap(), IdSize::Four);
        assert_eq!(IdSize::from_raw(8).unwrap(), IdSize::Eight);
    }

    #[test]
    fn id_size_from_raw_invalid() {
        assert!(IdSize::from_raw(0).is_err());
        assert!(IdSize::from_raw(3).is_err());
        assert!(IdSize::from_raw(16).is_err());
    }

    #[test]
    fn id_size_as_u32() {
        assert_eq!(IdSize::Four.as_u32(), 4);
        assert_eq!(IdSize::Eight.as_u32(), 8);
    }

    #[test]
    fn id_size_as_usize() {
        assert_eq!(IdSize::Four.as_usize(), 4);
        assert_eq!(IdSize::Eight.as_usize(), 8);
    }

    // ── Header tests ────────────────────────────

    #[test]
    fn records_start_offset_overflow_returns_truncated() {
        let err = records_start_offset(usize::MAX - 5).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn records_start_offset_normal_returns_correct_value() {
        let offset = records_start_offset(19).unwrap();
        assert_eq!(offset, 19 + 1 + 4 + 8);
    }

    #[test]
    fn truncated_no_null_returns_truncated_record() {
        let data = b"JAVA PROFILE";
        assert!(matches!(
            parse_header(data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn truncated_after_version_string_returns_truncated_record() {
        let data = b"JAVA PROFILE 1.0.2\0";
        assert!(matches!(
            parse_header(data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn truncated_missing_timestamp_returns_truncated_record() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());
        assert!(matches!(
            parse_header(&data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn invalid_version_returns_unsupported_version() {
        let mut data = b"NOT HPROF\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        let err = parse_header(&data).unwrap_err();
        assert!(matches!(err, HprofError::UnsupportedVersion(_)));
        if let HprofError::UnsupportedVersion(s) = err {
            assert_eq!(s, "NOT HPROF");
        }
    }

    #[test]
    fn empty_input_returns_truncated_record() {
        assert!(matches!(
            parse_header(&[]),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn invalid_id_size_3_returns_corrupted_data() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&3u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        assert!(matches!(
            parse_header(&data),
            Err(HprofError::CorruptedData(_))
        ));
    }

    #[test]
    fn invalid_id_size_0_returns_corrupted_data() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        assert!(matches!(
            parse_header(&data),
            Err(HprofError::CorruptedData(_))
        ));
    }

    #[test]
    fn valid_101_4byte_ids() {
        let mut data = b"JAVA PROFILE 1.0.1\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        let header = parse_header(&data).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_1);
        assert_eq!(header.id_size, IdSize::Four);
    }

    #[test]
    fn valid_102_8byte_ids() {
        let mut data = b"JAVA PROFILE 1.0.2\0".to_vec();
        data.extend_from_slice(&8u32.to_be_bytes());
        data.extend_from_slice(&0u64.to_be_bytes());
        let header = parse_header(&data).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_2);
        assert_eq!(header.id_size, IdSize::Eight);
    }

    // ── String tests ────────────────────────────

    #[test]
    fn resolve_overflow_offset_plus_len_returns_empty() {
        let s = HprofStringRef {
            id: 1,
            offset: u64::MAX - 1,
            len: 10,
        };
        let data = [0u8; 32];
        assert_eq!(s.resolve(&data), "");
    }

    #[test]
    fn hprof_string_ref_returns_static_size() {
        let s = HprofStringRef {
            id: 5,
            offset: 100,
            len: 5,
        };
        assert_eq!(s.memory_size(), std::mem::size_of::<HprofStringRef>());
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;

    #[test]
    fn parse_valid_102_8byte_ids_from_builder() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();
        let header = parse_header(&bytes).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_2);
        assert_eq!(header.id_size, IdSize::Eight);
    }

    #[test]
    fn parse_valid_101_4byte_ids_from_builder() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.1", 4).build();
        let header = parse_header(&bytes).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_1);
        assert_eq!(header.id_size, IdSize::Four);
    }
}
