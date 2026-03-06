//! Parsing for `STRING_IN_UTF8` records (tag `0x01`).
//!
//! Provides [`HprofString`] and [`parse_string_record`] for deserializing
//! string records from an hprof file payload.

use std::io::{Cursor, Read};

use crate::{HprofError, read_id};

/// A parsed `STRING_IN_UTF8` record.
///
/// ## Fields
/// - `id`: `u64` — object ID of the string (width determined by `id_size`)
/// - `value`: `String` — UTF-8 content of the string
#[derive(Debug, Clone)]
pub struct HprofString {
    pub id: u64,
    pub value: String,
}

/// Parses a `STRING_IN_UTF8` record payload from `cursor`.
///
/// The cursor must be positioned immediately after the 9-byte record header.
/// `payload_length` is the value from the record header and is used to
/// compute how many bytes belong to the UTF-8 content.
///
/// ## Parameters
/// - `cursor`: positioned at start of record payload
/// - `id_size`: byte width of object IDs (4 or 8)
/// - `payload_length`: total payload length in bytes
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if the payload is shorter than expected
/// - [`HprofError::CorruptedData`] if the content is not valid UTF-8
pub fn parse_string_record(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
    payload_length: u32,
) -> Result<HprofString, HprofError> {
    if payload_length < id_size {
        return Err(HprofError::TruncatedRecord);
    }

    let id = read_id(cursor, id_size)?;
    let content_len = (payload_length - id_size) as usize;
    let mut content_bytes = vec![0u8; content_len];
    cursor
        .read_exact(&mut content_bytes)
        .map_err(|_| HprofError::TruncatedRecord)?;
    let value = String::from_utf8(content_bytes)
        .map_err(|e| HprofError::CorruptedData(format!("invalid UTF-8 in string: {e}")))?;
    Ok(HprofString { id, value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_string_id_size_8() {
        let mut data = Vec::new();
        data.extend_from_slice(&5u64.to_be_bytes()); // id = 5
        data.extend_from_slice(b"main"); // content
        let mut cursor = Cursor::new(data.as_slice());
        let s = parse_string_record(&mut cursor, 8, 8 + 4).unwrap();
        assert_eq!(s.id, 5);
        assert_eq!(s.value, "main");
    }

    #[test]
    fn parse_string_id_size_4() {
        let mut data = Vec::new();
        data.extend_from_slice(&7u32.to_be_bytes()); // id = 7
        data.extend_from_slice(b"hello"); // content
        let mut cursor = Cursor::new(data.as_slice());
        let s = parse_string_record(&mut cursor, 4, 4 + 5).unwrap();
        assert_eq!(s.id, 7);
        assert_eq!(s.value, "hello");
    }

    #[test]
    fn parse_string_truncated_payload() {
        // payload_length says id_size=8 but buffer only has 4 bytes
        let data = vec![0u8; 4];
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_string_record(&mut cursor, 8, 8).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn parse_string_empty_content() {
        let mut data = Vec::new();
        data.extend_from_slice(&42u64.to_be_bytes()); // id = 42
        // no content bytes
        let mut cursor = Cursor::new(data.as_slice());
        let s = parse_string_record(&mut cursor, 8, 8).unwrap();
        assert_eq!(s.id, 42);
        assert_eq!(s.value, "");
    }

    #[test]
    fn parse_string_invalid_utf8_returns_corrupted_data() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes()); // id = 1
        data.extend_from_slice(&[0x80, 0xFF]); // invalid UTF-8 bytes
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_string_record(&mut cursor, 8, 8 + 2).unwrap_err();
        assert!(matches!(err, HprofError::CorruptedData(_)));
    }

    #[test]
    fn parse_string_payload_shorter_than_id_size_returns_truncated() {
        // payload_length says only 4 bytes exist, but id_size requires 8.
        // Even if the cursor has enough bytes, this record contract is invalid.
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_string_record(&mut cursor, 8, 4).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::record::parse_record_header;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};
    use std::io::Cursor;

    #[test]
    fn round_trip_string() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(99, "thread-1")
            .build();
        let hdr_end = advance_past_header(&bytes);
        let payload = &bytes[hdr_end..];
        let mut cursor = Cursor::new(payload);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x01);
        let s = parse_string_record(&mut cursor, 8, rec.length).unwrap();
        assert_eq!(s.id, 99);
        assert_eq!(s.value, "thread-1");
    }

    #[test]
    fn round_trip_string_id_size_4() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 4)
            .add_string(3, "foo")
            .build();
        let hdr_end = advance_past_header(&bytes);
        let payload = &bytes[hdr_end..];
        let mut cursor = Cursor::new(payload);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x01);
        let s = parse_string_record(&mut cursor, 4, rec.length).unwrap();
        assert_eq!(s.id, 3);
        assert_eq!(s.value, "foo");
    }
}
