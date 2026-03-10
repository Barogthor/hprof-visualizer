//! Navigation Engine trait, `Engine::from_file()` factory, LRU cache,
//! `MemorySize` tracking, object resolution, and pagination logic.
//!
//! Public API surface:
//! - [`NavigationEngine`] — trait defining the high-level TUI API.
//! - [`Engine`] — concrete implementation constructed via [`Engine::from_file`].
//! - [`EngineConfig`] — configuration for the engine (placeholder until Story 6.1).
//! - [`ThreadInfo`], [`ThreadState`], [`FrameInfo`], [`VariableInfo`], [`FieldInfo`],
//!   [`EntryInfo`] — view model types returned by the trait methods.
//!
//! Legacy entry points (used by `hprof-cli`):
//! - [`open_hprof_file`] — full indexing without a progress callback.
//! - [`open_hprof_file_with_progress`] — full indexing with a byte-offset
//!   callback, suitable for driving a progress bar.

use std::path::Path;

pub use hprof_api::{NullProgressObserver, ParseProgressObserver};
pub use hprof_parser::{HprofError, HprofHeader, HprofVersion};

/// Debug log macro: routes to `tracing::debug!` when
/// `dev-profiling` feature is active, otherwise no-op.
#[cfg(feature = "dev-profiling")]
macro_rules! dbg_log {
    ($($arg:tt)*) => { tracing::debug!($($arg)*) };
}

#[cfg(not(feature = "dev-profiling"))]
macro_rules! dbg_log {
    ($($arg:tt)*) => {()};
}

pub mod cache;
mod engine;
mod engine_impl;
mod pagination;
pub(crate) mod resolver;

pub use engine::{
    CollectionPage, EntryInfo, FieldInfo, FieldValue, FrameInfo, LineNumber, NavigationEngine,
    ThreadInfo, ThreadState, VariableInfo, VariableValue,
};
pub use engine_impl::Engine;

/// Configuration for the navigation engine.
///
/// Currently a placeholder; Story 6.1 will populate this from TOML config
/// and CLI overrides. Implements `Default` for zero-config construction.
#[derive(Debug, Default)]
pub struct EngineConfig;

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

/// Opens `path`, indexes all structural records, and
/// reports progress via the [`ParseProgressObserver`].
///
/// The observer receives:
/// - `on_bytes_scanned` — absolute file offset
///   (throttled, plus once at scan end)
/// - `on_segment_completed` — per heap segment
///
/// ## Errors
/// - [`HprofError::MmapFailed`] — file not found or
///   OS mapping failed.
/// - [`HprofError::UnsupportedVersion`] — unrecognised
///   hprof version string.
/// - [`HprofError::TruncatedRecord`] — file header is
///   truncated.
pub fn open_hprof_file_with_progress(
    path: &Path,
    observer: &mut dyn ParseProgressObserver,
) -> Result<IndexSummary, HprofError> {
    let hfile = hprof_parser::HprofFile::from_path_with_progress(path, observer)?;
    Ok(IndexSummary {
        records_attempted: hfile.records_attempted,
        records_indexed: hfile.records_indexed,
        warnings: hfile.index_warnings,
    })
}

/// Opens `path` and indexes all structural records
/// without progress.
///
/// Convenience wrapper around
/// [`open_hprof_file_with_progress`].
///
/// ## Errors
/// - [`HprofError::MmapFailed`] — file not found or
///   OS mapping failed.
/// - [`HprofError::UnsupportedVersion`] — unrecognised
///   hprof version string.
/// - [`HprofError::TruncatedRecord`] — file header is
///   truncated.
pub fn open_hprof_file(path: &Path) -> Result<IndexSummary, HprofError> {
    open_hprof_file_with_progress(path, &mut NullProgressObserver)
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
    fn open_hprof_file_with_progress_on_valid_file_calls_observer() {
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

        let mut bytes = minimal_hprof_bytes();
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
        let result = open_hprof_file_with_progress(tmp.path(), &mut obs);
        assert!(result.is_ok());
        assert!(obs.call_count >= 1, "observer must be called at least once");
        assert_eq!(
            obs.last_offset,
            Some(bytes.len() as u64),
            "should report the absolute file offset"
        );
    }

    #[test]
    fn open_hprof_file_with_progress_on_missing_path_returns_mmap_failed() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing = tmp.path().to_path_buf();
        drop(tmp);

        let result = open_hprof_file_with_progress(&missing, &mut NullProgressObserver);
        assert!(matches!(result, Err(HprofError::MmapFailed(_))));
    }

    #[test]
    fn test_observer_captures_event_sequence() {
        use hprof_api::{ProgressEvent, TestObserver};

        let mut bytes = minimal_hprof_bytes();
        // STRING record (tag 0x01)
        bytes.push(0x01);
        bytes.extend_from_slice(&0u32.to_be_bytes());
        let id_bytes = 1u64.to_be_bytes();
        let str_payload = b"hello";
        let len = id_bytes.len() + str_payload.len();
        bytes.extend_from_slice(&(len as u32).to_be_bytes());
        bytes.extend_from_slice(&id_bytes);
        bytes.extend_from_slice(str_payload);

        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(&bytes).unwrap();
        tmp.flush().unwrap();

        let mut obs = TestObserver::default();
        let result = open_hprof_file_with_progress(tmp.path(), &mut obs);
        assert!(result.is_ok());

        // Must have at least one BytesScanned event
        let scan_events: Vec<_> = obs
            .events
            .iter()
            .filter(|e| matches!(e, ProgressEvent::BytesScanned(_)))
            .collect();
        assert!(
            !scan_events.is_empty(),
            "expected at least one BytesScanned event"
        );

        // BytesScanned offsets must be monotonically
        // increasing
        let offsets: Vec<u64> = obs
            .events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::BytesScanned(pos) => Some(*pos),
                _ => None,
            })
            .collect();
        for w in offsets.windows(2) {
            assert!(
                w[1] > w[0],
                "BytesScanned not strictly increasing: {} <= {}",
                w[1],
                w[0]
            );
        }

        // Final BytesScanned should be total file size
        assert_eq!(
            *offsets.last().unwrap(),
            bytes.len() as u64,
            "final scan offset should equal file size"
        );

        // No segment events for a file without heap dumps
        let seg_events: Vec<_> = obs
            .events
            .iter()
            .filter(|e| matches!(e, ProgressEvent::SegmentCompleted { .. }))
            .collect();
        assert!(
            seg_events.is_empty(),
            "no segment events expected for a \
             heap-dump-free file"
        );
    }
}
