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

use hprof_api::{MemoryBudget, NullProgressObserver, ParseProgressObserver, ProgressNotifier};
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
/// The internal `mmap` field keeps the memory mapping alive and is used
/// by [`HprofFile::resolve_string`] to lazily decode string content.
#[derive(Debug)]
pub struct HprofFile {
    /// Memory mapping — used by `resolve_string` and kept alive for
    /// the duration of this struct's lifetime.
    mmap: Mmap,
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
    // `SegmentFilter` is `pub(crate)` — object resolution via segment filters is Story 3.4.
    #[allow(private_interfaces)]
    pub segment_filters: Vec<SegmentFilter>,
    /// Byte offset of the first record (immediately after the file header).
    pub records_start: usize,
    /// Location of every HEAP_DUMP / HEAP_DUMP_SEGMENT
    /// record. See [`HeapRecordRange`].
    pub heap_record_ranges: Vec<crate::indexer::HeapRecordRange>,
}

impl HprofFile {
    /// Opens `path` as a read-only mmap, parses the
    /// header, and indexes all structural records,
    /// reporting progress through the observer.
    ///
    /// Truncated or corrupted records are non-fatal:
    /// they are collected in
    /// [`HprofFile::index_warnings`] and indexing
    /// continues where possible.
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or
    ///   OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised
    ///   hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is
    ///   truncated.
    pub fn from_path_with_progress(
        path: &Path,
        observer: &mut dyn ParseProgressObserver,
        budget: MemoryBudget,
    ) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header.records_start;
        let mut notifier = ProgressNotifier::new(observer);
        let result = run_first_pass(
            &mmap[records_start..],
            header.id_size,
            records_start as u64,
            &mut notifier,
            budget,
        );
        Ok(Self {
            mmap,
            header,
            index: result.index,
            index_warnings: result.warnings,
            records_attempted: result.records_attempted,
            records_indexed: result.records_indexed,
            segment_filters: result.segment_filters,
            records_start,
            heap_record_ranges: result.heap_record_ranges,
        })
    }

    /// Opens `path` and indexes it without progress.
    ///
    /// Convenience wrapper around
    /// [`HprofFile::from_path_with_progress`].
    ///
    /// ## Errors
    /// - [`HprofError::MmapFailed`] — file not found or
    ///   OS mapping failed.
    /// - [`HprofError::UnsupportedVersion`] — unrecognised
    ///   hprof version string.
    /// - [`HprofError::TruncatedRecord`] — file header is
    ///   truncated.
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        Self::from_path_with_progress(path, &mut NullProgressObserver, MemoryBudget::Unlimited)
    }

    /// Returns the raw bytes of the records section (immediately after the
    /// file header).
    pub fn records_bytes(&self) -> &[u8] {
        &self.mmap[self.records_start..]
    }

    /// Resolves a [`HprofStringRef`] into an owned `String` by reading
    /// the content bytes directly from the mmap.
    ///
    /// The offset in `sref` is relative to the records section start.
    /// Invalid UTF-8 bytes are replaced with `\u{FFFD}`.
    pub fn resolve_string(&self, sref: &crate::HprofStringRef) -> String {
        sref.resolve(&self.mmap[self.records_start..])
    }
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
    fn from_path_with_progress_on_valid_file_calls_observer() {
        use hprof_api::ParseProgressObserver;

        struct CountingObserver {
            call_count: usize,
            last_offset: Option<u64>,
        }
        impl ParseProgressObserver for CountingObserver {
            fn on_bytes_scanned(&mut self, position: u64) {
                self.call_count += 1;
                self.last_offset = Some(position);
            }
            fn on_segment_completed(&mut self, _done: usize, _total: usize) {}
            fn on_names_resolved(&mut self, _done: usize, _total: usize) {}
        }

        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
        bytes.extend_from_slice(&8u32.to_be_bytes());
        bytes.extend_from_slice(&0u64.to_be_bytes());
        bytes.push(0x01); // tag
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let id_bytes = 1u64.to_be_bytes();
        bytes.extend_from_slice(&(id_bytes.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&id_bytes);

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mut obs = CountingObserver {
            call_count: 0,
            last_offset: None,
        };
        HprofFile::from_path_with_progress(tmp.path(), &mut obs, MemoryBudget::Unlimited).unwrap();
        assert!(obs.call_count >= 1, "observer must be called at least once");
        assert_eq!(
            obs.last_offset,
            Some(bytes.len() as u64),
            "should report the absolute file offset"
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
        use crate::id::IdSize;

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
        assert_eq!(hfile.header.id_size, IdSize::Eight);
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
    fn hprof_file_has_records_start_field_and_records_bytes() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "x")
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        // records_start must be > 0 (past the header)
        assert!(hfile.records_start > 0);
        // records_bytes() slice must be shorter than the full mmap
        // (it excludes the header)
        assert!(hfile.records_bytes().len() < bytes.len());
    }

    #[test]
    fn heap_record_ranges_populated_for_instance_dump() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[1, 2, 3, 4])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();
        let hfile = HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.heap_record_ranges.len(), 1);
    }

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
        let sref = &hfile.index.strings[&99];
        assert_eq!(hfile.resolve_string(sref), "thread-main");
        assert!(hfile.index_warnings.is_empty());
        assert_eq!(hfile.records_attempted, 1);
        assert_eq!(hfile.records_indexed, 1);
    }

    #[test]
    fn from_path_preloads_field_name_cache_with_specific_string_id_mapping() {
        let name_string_id = 101u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(name_string_id, "count")
            .add_string(102, "com/example/Foo")
            .add_class(1, 0x100, 0, 102)
            .add_class_dump(0x100, 0, 4, &[(name_string_id, 10u8)])
            .add_instance(0x200, 0, 0x100, &42i32.to_be_bytes())
            .build();

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let hfile = HprofFile::from_path(tmp.path()).unwrap();

        assert!(
            hfile.index.field_names.contains_key(&name_string_id),
            "field_names must contain specific field name string id"
        );
        assert_eq!(
            hfile
                .index
                .field_names
                .get(&name_string_id)
                .map(String::as_str),
            Some("count")
        );
    }
}
