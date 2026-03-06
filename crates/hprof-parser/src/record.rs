//! Record-level parsing for hprof binary files.
//!
//! Provides:
//! - [`RecordHeader`] — tag + payload length extracted from the 9-byte record header.
//! - [`parse_record_header`] — reads tag, discards `time_offset`, reads `length`.
//! - [`skip_record`] — advances the cursor past a record's payload without
//!   interpreting it.  Used for unknown record types (FR7) and any record the
//!   caller chooses not to process.

use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use crate::HprofError;

/// Parsed header of a single hprof record.
///
/// Extracted from the 9-byte prefix that precedes every record payload:
/// `tag(u8)` + `time_offset(u32 BE, discarded)` + `length(u32 BE)`.
#[derive(Debug, Clone, Copy)]
pub struct RecordHeader {
    /// Tag byte identifying the record type (e.g. `0x01` = STRING_IN_UTF8).
    pub tag: u8,
    /// Payload byte length — number of bytes immediately following this header.
    pub length: u32,
}

/// Reads the 9-byte record header from `cursor`.
///
/// On success the cursor is advanced by exactly 9 bytes and a [`RecordHeader`]
/// is returned. The `time_offset` field is consumed but not stored.
///
/// # Errors
/// - [`HprofError::TruncatedRecord`] — fewer than 9 bytes remain.
pub fn parse_record_header(cursor: &mut Cursor<&[u8]>) -> Result<RecordHeader, HprofError> {
    let tag = cursor.read_u8().map_err(|_| HprofError::TruncatedRecord)?;
    let _time_offset = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let length = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    Ok(RecordHeader { tag, length })
}

/// Advances `cursor` past the payload described by `header`.
///
/// Used to skip unknown record types (FR7) or any record the caller does not
/// need to parse.  The cursor must be positioned immediately after the record
/// header (i.e. at the first byte of the payload).
///
/// # Errors
/// - [`HprofError::TruncatedRecord`] — `header.length` exceeds remaining bytes.
pub fn skip_record(cursor: &mut Cursor<&[u8]>, header: &RecordHeader) -> Result<(), HprofError> {
    let pos = cursor.position() as usize;
    let remaining = cursor.get_ref().len().saturating_sub(pos);
    if remaining < header.length as usize {
        return Err(HprofError::TruncatedRecord);
    }
    cursor.set_position(pos as u64 + header.length as u64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_header_bytes(tag: u8, time_offset: u32, length: u32) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend_from_slice(&time_offset.to_be_bytes());
        v.extend_from_slice(&length.to_be_bytes());
        v
    }

    // --- parse_record_header ---

    #[test]
    fn parse_valid_record_header() {
        // tag=0x01, time_offset=0, length=12
        let bytes = make_header_bytes(0x01, 0, 12);
        let mut cursor = Cursor::new(bytes.as_slice());
        let header = parse_record_header(&mut cursor).unwrap();
        assert_eq!(header.tag, 0x01);
        assert_eq!(header.length, 12);
        assert_eq!(cursor.position(), 9);
    }

    #[test]
    fn parse_record_header_preserves_tag_and_length() {
        // tag=0xFF (unknown), time_offset=999, length=42
        let bytes = make_header_bytes(0xFF, 999, 42);
        let mut cursor = Cursor::new(bytes.as_slice());
        let header = parse_record_header(&mut cursor).unwrap();
        assert_eq!(header.tag, 0xFF);
        assert_eq!(header.length, 42);
        assert_eq!(cursor.position(), 9);
    }

    #[test]
    fn parse_record_header_truncated_on_length_returns_error() {
        // 5 bytes: tag(1) + time_offset(4) — truncated before length field
        let bytes = [0x01u8, 0x00, 0x00, 0x00, 0x00];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = parse_record_header(&mut cursor).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn parse_record_header_truncated_on_time_offset_returns_error() {
        // 1 byte: tag only — truncated before time_offset field
        let bytes = [0x01u8];
        let mut cursor = Cursor::new(bytes.as_slice());
        let err = parse_record_header(&mut cursor).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn parse_record_header_empty_returns_error() {
        let bytes: &[u8] = &[];
        let mut cursor = Cursor::new(bytes);
        let err = parse_record_header(&mut cursor).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    // --- skip_record ---

    #[test]
    fn skip_record_from_non_zero_cursor_position() {
        // Simulates real usage: cursor is at position 9 after parse_record_header.
        // Build a 9-byte header + 6-byte payload concatenated.
        let mut data = make_header_bytes(0x01, 0, 6);
        data.extend_from_slice(&[0xBB_u8; 6]);
        let mut cursor = Cursor::new(data.as_slice());
        // Advance past the header as parse_record_header would
        let header = parse_record_header(&mut cursor).unwrap();
        assert_eq!(cursor.position(), 9);
        skip_record(&mut cursor, &header).unwrap();
        assert_eq!(cursor.position(), 15); // 9 header + 6 payload
    }

    #[test]
    fn skip_record_advances_cursor_correctly() {
        // payload = 6 bytes
        let payload = [0xAA_u8; 6];
        let mut cursor = Cursor::new(payload.as_slice());
        let header = RecordHeader {
            tag: 0x01,
            length: 6,
        };
        skip_record(&mut cursor, &header).unwrap();
        assert_eq!(cursor.position(), 6);
    }

    #[test]
    fn skip_record_zero_length_is_noop() {
        let payload = [0x00_u8; 4];
        let mut cursor = Cursor::new(payload.as_slice());
        let header = RecordHeader {
            tag: 0x01,
            length: 0,
        };
        skip_record(&mut cursor, &header).unwrap();
        assert_eq!(cursor.position(), 0);
    }

    #[test]
    fn skip_record_length_exceeds_remaining_returns_truncated() {
        let payload = [0x01_u8; 3];
        let mut cursor = Cursor::new(payload.as_slice());
        let header = RecordHeader {
            tag: 0x02,
            length: 10,
        };
        let err = skip_record(&mut cursor, &header).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn skip_record_unknown_tag_with_valid_length_succeeds() {
        // AC #3: unknown tag 0xFF + valid length → skip succeeds
        let payload = [0xBE_u8; 5];
        let mut cursor = Cursor::new(payload.as_slice());
        let header = RecordHeader {
            tag: 0xFF,
            length: 5,
        };
        skip_record(&mut cursor, &header).unwrap();
        assert_eq!(cursor.position(), 5);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::header::parse_header;
    use crate::test_utils::HprofTestBuilder;
    use std::io::Cursor;

    fn hprof_header_end(bytes: &[u8]) -> usize {
        // version string ends at first null byte
        let null_pos = bytes.iter().position(|&b| b == 0).unwrap();
        null_pos + 1 + 4 + 8 // null + id_size(u32) + timestamp(u64)
    }

    #[test]
    fn round_trip_string_record_header_and_skip() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();
        let _header = parse_header(&bytes).unwrap();
        let hdr_end = hprof_header_end(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x01);
        // payload = id(8) + "main"(4) = 12 bytes
        assert_eq!(rec.length, 8 + 4);
        skip_record(&mut cursor, &rec).unwrap();
        // 9-byte record header + 12-byte payload = cursor at end of slice
        assert_eq!(cursor.position() as usize, cursor.get_ref().len());
    }

    #[test]
    fn skip_unknown_record_does_not_error() {
        // Build a file with a record then corrupt its tag to 0xFF
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "hello")
            .corrupt_record_at(0)
            .build();
        let hdr_end = hprof_header_end(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0xFF);
        // skip_record must succeed regardless of unknown tag
        skip_record(&mut cursor, &rec).unwrap();
    }
}
