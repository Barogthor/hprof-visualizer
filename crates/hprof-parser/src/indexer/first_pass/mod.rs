//! Single-pass indexing over hprof record bytes,
//! building an [`IndexResult`].
//!
//! [`run_first_pass`] iterates over every record in
//! `data` (a slice starting immediately after the file
//! header), parsing known record types into the index
//! and collecting any non-fatal errors as warnings.
//! The function is infallible — it always returns an
//! [`IndexResult`].
//!
//! For files with >= 32 MB of heap segment data, heap
//! extraction is parallelised via rayon.
//!
//! Progress is reported via [`ProgressNotifier`]:
//! absolute byte offsets during the record scan
//! (throttled), segment completion counts during heap
//! extraction, and done/total for name resolution.

use std::time::Instant;

use hprof_api::{MemoryBudget, ProgressNotifier};
#[cfg(feature = "test-utils")]
use hprof_api::MemorySize;

use crate::ClassDumpInfo;
#[cfg(feature = "test-utils")]
use crate::indexer::DiagnosticInfo;
use crate::indexer::IndexResult;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::SegmentFilterBuilder;

/// Object ID mapped to its byte offset in the records
/// section. Used for binary-search lookups after sorting.
#[derive(Debug, Clone, Copy)]
pub(super) struct ObjectOffset {
    pub(super) object_id: u64,
    pub(super) offset: u64,
}

/// `GC_ROOT_JAVA_FRAME` sub-record: links a heap object
/// to a specific frame in a thread's stack trace.
#[derive(Debug)]
pub(super) struct RawFrameRoot {
    pub(super) object_id: u64,
    pub(super) thread_serial: u32,
    pub(super) frame_number: i32,
}

/// `ROOT_THREAD_OBJ` sub-record: links a heap object to
/// a thread serial number.
#[derive(Debug)]
pub(super) struct RawThreadObject {
    pub(super) object_id: u64,
    pub(super) thread_serial: u32,
}

/// Object ID at a data offset, used for segment filter
/// construction.
#[derive(Debug)]
pub(super) struct FilterEntry {
    pub(super) data_offset: usize,
    pub(super) object_id: u64,
}

/// A `CLASS_DUMP` sub-record paired with its class
/// object ID.
#[derive(Debug)]
pub(super) struct ClassDumpEntry {
    pub(super) class_object_id: u64,
    pub(super) info: ClassDumpInfo,
}

mod heap_extraction;
mod hprof_primitives;
mod record_scan;
mod thread_resolution;

use hprof_primitives::MAX_WARNINGS;
pub(crate) use hprof_primitives::{parse_class_dump, value_byte_size};

#[cfg(test)]
mod tests;

/// Mutable state threaded through all first-pass phases.
struct FirstPassContext<'a> {
    data: &'a [u8],
    id_size: u32,
    base_offset: u64,
    result: IndexResult,
    seg_builder: SegmentFilterBuilder,
    all_offsets: Vec<ObjectOffset>,
    raw_frame_roots: Vec<RawFrameRoot>,
    raw_thread_objects: Vec<RawThreadObject>,
    suppressed_warnings: u64,
    last_progress_bytes: usize,
    last_progress_at: Instant,
    cursor_position: u64,
    /// Memory budget for chunked heap extraction.
    budget: MemoryBudget,
}

impl<'a> FirstPassContext<'a> {
    fn new(data: &'a [u8], id_size: u32, base_offset: u64, budget: MemoryBudget) -> Self {
        Self {
            data,
            id_size,
            base_offset,
            result: IndexResult {
                index: PreciseIndex::with_capacity(data.len()),
                warnings: Vec::new(),
                records_attempted: 0,
                records_indexed: 0,
                segment_filters: Vec::new(),
                heap_record_ranges: Vec::new(),
                #[cfg(feature = "test-utils")]
                diagnostics: DiagnosticInfo::default(),
            },
            seg_builder: SegmentFilterBuilder::new(),
            all_offsets: Vec::with_capacity((data.len() / 80).min(8_000_000)),
            raw_frame_roots: Vec::new(),
            raw_thread_objects: Vec::new(),
            suppressed_warnings: 0,
            last_progress_bytes: 0,
            last_progress_at: Instant::now(),
            cursor_position: 0,
            budget,
        }
    }

    fn push_warning(&mut self, msg: String) {
        Self::push_warning_raw(
            &mut self.result.warnings,
            &mut self.suppressed_warnings,
            msg,
        );
    }

    /// Pushes a warning using separate mutable refs
    /// (for use where `&mut self` would cause borrow
    /// conflicts).
    fn push_warning_raw(warnings: &mut Vec<String>, suppressed: &mut u64, msg: String) {
        if warnings.len() < MAX_WARNINGS {
            warnings.push(msg);
        } else {
            *suppressed += 1;
        }
    }

    fn push_suppressed_summary(&mut self) {
        if self.suppressed_warnings > 0 {
            self.result.warnings.push(format!(
                "... {} additional warning(s) \
                 suppressed (only first \
                 {MAX_WARNINGS} shown)",
                self.suppressed_warnings
            ));
        }
    }

    fn sort_offsets(&mut self) {
        #[cfg(feature = "dev-profiling")]
        tracing::debug!(
            offsets_len = self.all_offsets.len(),
            offsets_capacity = self.all_offsets.capacity(),
            "sort_offsets: pre-sort state"
        );
        self.all_offsets.sort_unstable_by_key(|o| o.object_id);
    }

    fn finish(mut self) -> IndexResult {
        #[cfg(feature = "dev-profiling")]
        tracing::debug!(
            completed_segments = self.seg_builder.completed_count(),
            pending_ids = self.seg_builder.pending_id_count(),
            "segment_filter_build: pre-finish state"
        );
        self.push_suppressed_summary();
        let (filters, filter_warnings) = self.seg_builder.finish();
        self.result.segment_filters = filters;
        self.result.warnings.extend(filter_warnings);
        self.result
    }
}

/// Scans all records in `data` and returns a populated
/// [`IndexResult`].
///
/// Non-fatal errors (corrupted payloads, size
/// mismatches) are collected in
/// [`IndexResult::warnings`]. Fatal truncations
/// (mid-header EOF, payload window exceeds file) stop
/// iteration and are also recorded as warnings.
///
/// Progress is reported via the [`ProgressNotifier`]:
/// - `bytes_scanned` during sequential record scan
/// - `segment_completed` during heap extraction
///
/// ## Parameters
/// - `data`: raw bytes starting at the first record
///   (immediately after the hprof file header).
/// - `id_size`: byte width of object IDs, taken from
///   the hprof file header (4 or 8).
/// - `base_offset`: absolute file offset of `data[0]`
///   (i.e. `records_start`), added to relative scan
///   positions before reporting.
/// - `notifier`: progress observer wrapper.
/// - `budget`: memory budget for chunked heap
///   extraction.
pub fn run_first_pass(
    data: &[u8],
    id_size: u32,
    base_offset: u64,
    notifier: &mut ProgressNotifier,
    budget: MemoryBudget,
) -> IndexResult {
    #[cfg(feature = "dev-profiling")]
    let _first_pass_span = tracing::info_span!("first_pass").entered();

    let mut ctx = FirstPassContext::new(data, id_size, base_offset, budget);
    record_scan::scan_records(&mut ctx, notifier);
    heap_extraction::extract_all(&mut ctx, notifier);
    {
        #[cfg(feature = "dev-profiling")]
        let _sort_span = tracing::info_span!("sort_offsets").entered();
        ctx.sort_offsets();
    }
    // Capture coexistence watermark before thread_resolution clears
    // all_offsets (Vec::new() at thread_resolution.rs:104).
    #[cfg(feature = "test-utils")]
    {
        ctx.result.diagnostics = DiagnosticInfo {
            offsets_len: ctx.all_offsets.len(),
            offsets_capacity: ctx.all_offsets.capacity(),
            precise_index_heap_bytes: ctx.result.index.memory_size(),
        };
    }
    thread_resolution::resolve_all(&mut ctx);

    {
        #[cfg(feature = "dev-profiling")]
        let _seg_filter_span =
            tracing::info_span!("segment_filter_build").entered();
        ctx.finish()
    }
}
