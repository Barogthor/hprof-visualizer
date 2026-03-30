//! Record-level types for hprof binary files.
//!
//! Provides [`RecordHeader`] -- tag + payload length
//! extracted from the 9-byte record header.

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
