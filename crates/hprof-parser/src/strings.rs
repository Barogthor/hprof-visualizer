//! Lazy string references for `STRING_IN_UTF8` records
//! (tag `0x01`).
//!
//! [`HprofStringRef`] stores only offset + length,
//! deferring actual UTF-8 decoding to
//! [`resolve()`](HprofStringRef::resolve).
//!
//! Parsing is handled by
//! [`RecordReader::parse_string_ref`](crate::reader::RecordReader::parse_string_ref).

use hprof_api::MemorySize;

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
        #[allow(clippy::manual_saturating_arithmetic)]
        let end = start.checked_add(self.len as usize).unwrap_or(usize::MAX);
        match data.get(start..end) {
            Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
