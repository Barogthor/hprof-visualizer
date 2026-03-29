//! Parsing for `STRING_IN_UTF8` records (tag `0x01`).
//!
//! Provides [`HprofStringRef`] and [`parse_string_ref`] for building
//! lazy string references that store only offset+length instead of
//! allocating string content during the first pass.

use std::io::Cursor;

use hprof_api::MemorySize;

use crate::{HprofError, read_id};

/// A lazy reference to a `STRING_IN_UTF8` record's content.
///
/// Stores only the location (offset + length) of the string bytes
/// in the records section, deferring actual UTF-8 decoding to
/// [`crate::HprofFile::resolve_string`].
///
/// ## Fields
/// - `id`: `u64` — object ID of the string
/// - `offset`: `u64` — byte offset relative to records section start
/// - `len`: `u32` — byte length of the UTF-8 content
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
    /// Resolves this reference into an owned `String` by reading
    /// content bytes from `data`.
    ///
    /// `data` must be the records section slice (offsets are
    /// relative to records section start). Returns an empty
    /// string if the offset/length is out of bounds. Invalid
    /// UTF-8 bytes are replaced with `\u{FFFD}`.
    pub fn resolve(&self, data: &[u8]) -> String {
        let start = self.offset as usize;
        #[allow(clippy::manual_saturating_arithmetic)]
        let end = start
            .checked_add(self.len as usize)
            .unwrap_or(usize::MAX);
        match data.get(start..end) {
            Some(bytes) => {
                String::from_utf8_lossy(bytes).into_owned()
            }
            None => String::new(),
        }
    }
}

/// Builds a lazy [`HprofStringRef`] from a `STRING_IN_UTF8` payload.
///
/// Instead of reading and allocating the string content, this only
/// records the cursor position (as an offset relative to the records
/// section) and the content length. The cursor is advanced past the
/// payload bytes.
///
/// ## Parameters
/// - `cursor`: positioned at start of record payload
/// - `id_size`: byte width of object IDs (4 or 8)
/// - `payload_length`: total payload length in bytes
/// - `record_body_start`: byte position of this record's body
///   within the records section slice
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if the payload is shorter
///   than expected
pub fn parse_string_ref(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
    payload_length: u32,
    record_body_start: u64,
) -> Result<HprofStringRef, HprofError> {
    if payload_length < id_size {
        return Err(HprofError::TruncatedRecord);
    }

    let id = read_id(cursor, id_size)?;
    let content_len = payload_length - id_size;
    let offset = record_body_start + id_size as u64;

    // Advance cursor past the content bytes
    let new_pos = cursor.position() + content_len as u64;
    if new_pos > cursor.get_ref().len() as u64 {
        return Err(HprofError::TruncatedRecord);
    }
    cursor.set_position(new_pos);

    Ok(HprofStringRef {
        id,
        offset,
        len: content_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

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

    #[test]
    fn parse_string_ref_id_size_8() {
        let mut data = Vec::new();
        data.extend_from_slice(&5u64.to_be_bytes()); // id = 5
        data.extend_from_slice(b"main"); // content
        let mut cursor = Cursor::new(data.as_slice());
        let s = parse_string_ref(&mut cursor, 8, 8 + 4, 100).unwrap();
        assert_eq!(s.id, 5);
        assert_eq!(s.offset, 100 + 8); // record_body_start + id_size
        assert_eq!(s.len, 4);
    }

    #[test]
    fn parse_string_ref_id_size_4() {
        let mut data = Vec::new();
        data.extend_from_slice(&7u32.to_be_bytes()); // id = 7
        data.extend_from_slice(b"hello"); // content
        let mut cursor = Cursor::new(data.as_slice());
        let s = parse_string_ref(&mut cursor, 4, 4 + 5, 50).unwrap();
        assert_eq!(s.id, 7);
        assert_eq!(s.offset, 50 + 4);
        assert_eq!(s.len, 5);
    }

    #[test]
    fn parse_string_ref_truncated_payload() {
        let data = vec![0u8; 4];
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_string_ref(&mut cursor, 8, 8, 0).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn parse_string_ref_empty_content() {
        let mut data = Vec::new();
        data.extend_from_slice(&42u64.to_be_bytes()); // id = 42
        let mut cursor = Cursor::new(data.as_slice());
        let s = parse_string_ref(&mut cursor, 8, 8, 0).unwrap();
        assert_eq!(s.id, 42);
        assert_eq!(s.len, 0);
    }

    #[test]
    fn parse_string_ref_cursor_advances_past_content() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        data.extend_from_slice(b"abc");
        let mut cursor = Cursor::new(data.as_slice());
        let _ = parse_string_ref(&mut cursor, 8, 8 + 3, 0).unwrap();
        assert_eq!(cursor.position(), 11); // 8 (id) + 3 (content)
    }

    #[test]
    fn parse_string_ref_payload_shorter_than_id_size_returns_truncated() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_string_ref(&mut cursor, 8, 4, 0).unwrap_err();
        assert!(matches!(err, HprofError::TruncatedRecord));
    }

    #[test]
    fn parse_string_ref_content_beyond_buffer_returns_truncated() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u64.to_be_bytes());
        // payload says 100 bytes of content, but buffer has none
        let mut cursor = Cursor::new(data.as_slice());
        let err = parse_string_ref(&mut cursor, 8, 8 + 100, 0).unwrap_err();
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
    fn round_trip_string_ref() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(99, "thread-1")
            .build();
        let hdr_end = advance_past_header(&bytes);
        let payload = &bytes[hdr_end..];
        let mut cursor = Cursor::new(payload);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x01);
        let body_start = cursor.position();
        let s = parse_string_ref(&mut cursor, 8, rec.length, body_start).unwrap();
        assert_eq!(s.id, 99);
        assert_eq!(s.len, 8); // "thread-1".len()
        // Verify offset points to string content in the payload
        let resolved = String::from_utf8_lossy(
            &payload[s.offset as usize..(s.offset as usize + s.len as usize)],
        );
        assert_eq!(resolved, "thread-1");
    }

    #[test]
    fn round_trip_string_ref_id_size_4() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 4)
            .add_string(3, "foo")
            .build();
        let hdr_end = advance_past_header(&bytes);
        let payload = &bytes[hdr_end..];
        let mut cursor = Cursor::new(payload);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x01);
        let body_start = cursor.position();
        let s = parse_string_ref(&mut cursor, 4, rec.length, body_start).unwrap();
        assert_eq!(s.id, 3);
        assert_eq!(s.len, 3); // "foo".len()
        let resolved = String::from_utf8_lossy(
            &payload[s.offset as usize..(s.offset as usize + s.len as usize)],
        );
        assert_eq!(resolved, "foo");
    }
}
