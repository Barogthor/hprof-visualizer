//! Top-level entry point for opening and indexing an hprof file.
//!
//! [`HprofFile`] memory-maps the file, parses its header, and runs the
//! first-pass indexer in one call, making all structural metadata available
//! via [`HprofFile::index`] after construction. Truncated or corrupted
//! records are non-fatal and collected in [`HprofFile::index_warnings`].
//!
//! Use [`HprofFile::from_path_with_progress`] to receive byte-offset callbacks
//! during indexing, or [`HprofFile::from_path`] for a no-op convenience
//! wrapper.

use std::path::Path;

use memmap2::Mmap;

use crate::indexer::{first_pass::run_first_pass, precise::PreciseIndex, segment::SegmentFilter};
use crate::{HprofError, HprofHeader, open_readonly, parse_header};

/// An open hprof file with a parsed header and populated structural index.
///
/// ## Fields
/// - `header`: [`HprofHeader`] — parsed file header (version, id_size,
///   timestamp).
/// - `index`: [`PreciseIndex`] — O(1) lookup maps for all structural records.
/// - `index_warnings`: non-fatal parse errors collected during indexing.
/// - `records_attempted`: known-type records whose payload window was within
///   bounds. Unknown-tag records are silently skipped and not counted here.
/// - `records_indexed`: records successfully parsed and inserted into the index.
/// - `segment_filters`: probabilistic per-segment filters for object ID
///   resolution. Each [`SegmentFilter`] covers a 64 MiB slice of the records
///   section and allows fast candidate-segment lookup before a targeted scan.
///
/// The internal `_mmap` field keeps the memory mapping alive for the duration
/// of this struct's lifetime. It must not be dropped early.
#[derive(Debug)]
pub struct HprofFile {
    /// Keeps the memory mapping alive — must not be removed.
    _mmap: Mmap,
    /// Parsed hprof file header.
    pub header: HprofHeader,
    /// O(1) lookup index built from the first sequential pass.
    pub index: PreciseIndex,
    /// Warnings collected during indexing (non-fatal parse errors).
    pub index_warnings: Vec<String>,
    /// Records whose header and payload window were valid.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
    /// Probabilistic per-segment filters for object ID resolution.
    // `SegmentFilter` is `pub(crate)` until the engine crate is built (Story 3.1).
    #[allow(private_interfaces)]
    pub segment_filters: Vec<SegmentFilter>,
}

impl HprofFile {
    /// Opens `path` as a read-only mmap, parses the header, and indexes all
    /// structural records, calling `progress_fn` with the current byte offset
    /// every [`PROGRESS_REPORT_INTERVAL`] bytes and once after the final record.
    ///
    /// Truncated or corrupted records are non-fatal: they are collected in
    /// [`HprofFile::index_warnings`] and indexing continues where possible.
    ///
    /// The byte offset passed to `progress_fn` is an absolute file offset from
    /// the beginning of the file (including the header).
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is truncated.
    pub fn from_path_with_progress(
        path: &Path,
        mut progress_fn: impl FnMut(u64),
    ) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header_end(&mmap)?;
        let base_offset = records_start as u64;
        let result = run_first_pass(&mmap[records_start..], header.id_size, |bytes| {
            progress_fn(base_offset.saturating_add(bytes));
        });
        Ok(Self {
            _mmap: mmap,
            header,
            index: result.index,
            index_warnings: result.warnings,
            records_attempted: result.records_attempted,
            records_indexed: result.records_indexed,
            segment_filters: result.segment_filters,
        })
    }

    /// Opens `path` and indexes it without a progress callback.
    ///
    /// Convenience wrapper around [`HprofFile::from_path_with_progress`].
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is truncated.
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        Self::from_path_with_progress(path, |_| {})
    }
}

/// Returns the byte offset of the first record in `data`.
///
/// Scans for the null terminator of the version string, then skips
/// `id_size` (u32, 4 bytes) and timestamp (u64, 8 bytes).
///
/// ## Errors
/// - [`HprofError::TruncatedRecord`] if no null byte is found.
fn header_end(data: &[u8]) -> Result<usize, HprofError> {
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or(HprofError::TruncatedRecord)?;
    Ok(null_pos + 1 + 4 + 8) // null-term + id_size(u32) + timestamp(u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn from_path_non_existent_returns_mmap_failed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing_path = tmp.path().to_path_buf();
        drop(tmp);
        let result = HprofFile::from_path(&missing_path);
        assert!(matches!(result, Err(HprofError::MmapFailed(_))));
    }

    #[test]
    fn from_path_truncated_record_returns_partial_with_warning() {
        // Valid header + incomplete record (tag only, missing time_offset+length)
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(0x01); // tag byte only — truncated mid-header

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap(); // Ok, not Err
        assert!(!hfile.index_warnings.is_empty());
        assert!(hfile.index.strings.is_empty());
    }

    #[test]
    fn from_path_with_progress_on_valid_file_calls_callback_at_least_once() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        // Add one string record so the records section is non-empty.
        bytes.push(0x01); // tag
        bytes.extend_from_slice(&0u32.to_be_bytes()); // time_offset
        let id_bytes = 1u64.to_be_bytes();
        bytes.extend_from_slice(&(id_bytes.len() as u32).to_be_bytes()); // length
        bytes.extend_from_slice(&id_bytes); // payload

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mut call_count = 0usize;
        let mut last = None;
        HprofFile::from_path_with_progress(tmp.path(), |bytes| {
            call_count += 1;
            last = Some(bytes);
        })
        .unwrap();
        assert!(
            call_count >= 1,
            "progress callback must be called at least once"
        );
        assert_eq!(
            last,
            Some(bytes.len() as u64),
            "final callback should report absolute file offset"
        );
    }

    #[test]
    fn from_path_on_valid_file_compiles_and_succeeds() {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let result = HprofFile::from_path(tmp.path());
        assert!(result.is_ok(), "from_path must succeed with no-op callback");
    }

    #[test]
    fn from_path_valid_file_parses_header() {
        use crate::HprofVersion;

        // Build a minimal valid hprof file (header only, no records)
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes()); // id_size
        bytes.extend_from_slice(&0u64.to_be_bytes()); // timestamp

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.header.version, HprofVersion::V1_0_2);
        assert_eq!(hfile.header.id_size, 8);
        assert!(hfile.index.strings.is_empty());
        assert!(hfile.index_warnings.is_empty());
        assert_eq!(hfile.records_attempted, 0);
        assert_eq!(hfile.records_indexed, 0);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;
    use std::io::Write;

    #[test]
    fn from_path_with_instance_produces_one_segment_filter() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[])
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.segment_filters.len(), 1);
    }

    #[test]
    fn from_path_with_string_record_indexed() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(99, "thread-main")
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.index.strings.len(), 1);
        assert_eq!(hfile.index.strings[&99].value, "thread-main");
        assert!(hfile.index_warnings.is_empty());
        assert_eq!(hfile.records_attempted, 1);
        assert_eq!(hfile.records_indexed, 1);
    }
}
