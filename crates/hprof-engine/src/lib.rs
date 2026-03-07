//! Navigation Engine trait, `Engine::from_file()` factory, LRU cache,
//! `MemorySize` tracking, object resolution, and pagination logic.
//!
//! Public entry points for opening hprof files:
//! - [`open_hprof_file`] — full indexing without a progress callback.
//! - [`open_hprof_file_with_progress`] — full indexing with a byte-offset
//!   callback, suitable for driving a progress bar.

use std::path::Path;

pub use hprof_parser::{HprofError, HprofHeader, HprofVersion};

/// Summary of a completed first-pass indexing run.
///
/// Returned by [`open_hprof_file`] and [`open_hprof_file_with_progress`].
pub struct IndexSummary {
    /// Total known-type records whose payload window was within bounds.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
    /// Non-fatal warnings collected during indexing (truncated payloads, etc.).
    pub warnings: Vec<String>,
}

/// Opens `path`, indexes all structural records, and reports byte progress via
/// `progress_fn`.
///
/// `progress_fn` receives the current absolute file byte offset every 4 MiB,
/// at least once per second during long scans, and once after indexing
/// completes.
///
/// ## Errors
/// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
/// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
/// - [`HprofError::TruncatedRecord`] — file header is truncated.
pub fn open_hprof_file_with_progress(
    path: &Path,
    progress_fn: impl FnMut(u64),
) -> Result<IndexSummary, HprofError> {
    let hfile = hprof_parser::HprofFile::from_path_with_progress(path, progress_fn)?;
    Ok(IndexSummary {
        records_attempted: hfile.records_attempted,
        records_indexed: hfile.records_indexed,
        warnings: hfile.index_warnings,
    })
}

/// Opens `path` and indexes all structural records without a progress callback.
///
/// Convenience wrapper around [`open_hprof_file_with_progress`].
///
/// ## Errors
/// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
/// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
/// - [`HprofError::TruncatedRecord`] — file header is truncated.
pub fn open_hprof_file(path: &Path) -> Result<IndexSummary, HprofError> {
    open_hprof_file_with_progress(path, |_| {})
}

/// Opens an hprof file in read-only mmap mode and parses its header.
pub fn open_hprof_header(path: &Path) -> Result<HprofHeader, HprofError> {
    let mmap = hprof_parser::open_readonly(path)?;
    hprof_parser::parse_header(&mmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn minimal_hprof_bytes() -> Vec<u8> {
        let mut bytes = b"JAVA PROFILE 1.0.2\0".to_vec();
        bytes.extend_from_slice(&8u32.to_be_bytes()); // id_size
        bytes.extend_from_slice(&0u64.to_be_bytes()); // timestamp
        bytes
    }

    #[test]
    fn index_summary_struct_has_expected_fields() {
        let s = IndexSummary {
            records_attempted: 10,
            records_indexed: 8,
            warnings: vec!["warn".to_string()],
        };
        assert_eq!(s.records_attempted, 10);
        assert_eq!(s.records_indexed, 8);
        assert_eq!(s.warnings.len(), 1);
    }

    #[test]
    fn open_hprof_file_with_progress_on_valid_file_calls_callback_at_least_once() {
        let mut bytes = minimal_hprof_bytes();
        // Add a string record so the records section is non-empty.
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
        let result = open_hprof_file_with_progress(tmp.path(), |bytes| {
            call_count += 1;
            last = Some(bytes);
        });
        assert!(result.is_ok());
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
    fn open_hprof_file_with_progress_on_missing_path_returns_mmap_failed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing = tmp.path().to_path_buf();
        drop(tmp);

        let result = open_hprof_file_with_progress(&missing, |_| {});
        assert!(matches!(result, Err(HprofError::MmapFailed(_))));
    }
}
