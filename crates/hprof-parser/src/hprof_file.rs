//! Top-level entry point for opening and indexing an hprof file.
//!
//! [`HprofFile`] memory-maps the file, parses its header, and runs the
//! first-pass indexer in one call, making all structural metadata available
//! via [`HprofFile::index`] after construction.

use std::path::Path;

use memmap2::Mmap;

use crate::indexer::{first_pass::run_first_pass, precise::PreciseIndex};
use crate::{HprofError, HprofHeader, open_readonly, parse_header};

/// An open hprof file with a parsed header and populated structural index.
///
/// ## Fields
/// - `header`: [`HprofHeader`] — parsed file header (version, id_size,
///   timestamp).
/// - `index`: [`PreciseIndex`] — O(1) lookup maps for all structural records.
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
}

impl HprofFile {
    /// Opens `path` as a read-only mmap, parses the header, and indexes all
    /// structural records in a single sequential pass.
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file truncated during header or
    ///   record parsing.
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header_end(&mmap)?;
        let index = run_first_pass(&mmap[records_start..], header.id_size)?;
        Ok(Self {
            _mmap: mmap,
            header,
            index,
        })
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
    fn from_path_truncated_record_returns_error() {
        // Valid header + incomplete record (tag only, missing time_offset+length)
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(0x01); // tag byte only — truncated

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let result = HprofFile::from_path(tmp.path());
        assert!(matches!(result, Err(HprofError::TruncatedRecord)));
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
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;
    use std::io::Write;

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
    }
}
