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

#[cfg(feature = "test-utils")]
use hprof_api::MemorySize;
use hprof_api::{MemoryBudget, ProgressNotifier};
use rustc_hash::FxHashSet;

use crate::ClassDumpInfo;
use crate::format::IdSize;
#[cfg(feature = "test-utils")]
use crate::indexer::DiagnosticInfo;
use crate::indexer::IndexResult;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::{SegmentFilter, SegmentFilterBuilder};

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
pub(crate) mod offset_lookup;
mod record_scan;
mod thread_resolution;

use hprof_primitives::MAX_WARNINGS;

#[cfg(test)]
mod tests;

/// Mutable state threaded through all first-pass phases.
struct FirstPassContext<'a> {
    data: &'a [u8],
    id_size: IdSize,
    base_offset: u64,
    result: IndexResult,
    seg_builder: Option<SegmentFilterBuilder>,
    /// Pre-built filters + warnings from early
    /// `seg_builder.finish()` call.
    built_filters: Option<(Vec<SegmentFilter>, Vec<String>)>,
    segment_entry_points: Vec<offset_lookup::SegmentEntryPoint>,
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
    fn new(data: &'a [u8], id_size: IdSize, base_offset: u64, budget: MemoryBudget) -> Self {
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
            seg_builder: Some(SegmentFilterBuilder::new()),
            built_filters: None,
            segment_entry_points: Vec::new(),
            raw_frame_roots: Vec::new(),
            raw_thread_objects: Vec::new(),
            suppressed_warnings: 0,
            last_progress_bytes: 0,
            last_progress_at: Instant::now(),
            cursor_position: 0,
            budget,
        }
    }

    fn push_warning(&mut self, msg: impl FnOnce() -> String) {
        Self::push_warning_raw(
            &mut self.result.warnings,
            &mut self.suppressed_warnings,
            msg,
        );
    }

    /// Pushes a warning using separate mutable refs
    /// (for use where `&mut self` would cause borrow
    /// conflicts).
    fn push_warning_raw(
        warnings: &mut Vec<String>,
        suppressed: &mut u64,
        msg: impl FnOnce() -> String,
    ) {
        if warnings.len() < MAX_WARNINGS {
            warnings.push(msg());
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

    /// Finishes the segment filter builder early,
    /// returning `(filters, entry_points)` for immediate
    /// use in thread resolution.
    ///
    /// Any filter-build warnings are pushed directly into
    /// `self.result.warnings` so they are never lost.
    fn finish_filters_early(
        &mut self,
    ) -> (Vec<SegmentFilter>, Vec<offset_lookup::SegmentEntryPoint>) {
        let (filters, warnings) = self
            .seg_builder
            .take()
            .expect("seg_builder already consumed")
            .finish();
        for w in warnings {
            self.push_warning(|| w);
        }
        self.built_filters = Some((Vec::new(), Vec::new()));
        let mut entry_points = std::mem::take(&mut self.segment_entry_points);
        entry_points.sort_unstable_by_key(|ep| ep.segment_index);
        (filters, entry_points)
    }

    fn finish(mut self) -> IndexResult {
        self.push_suppressed_summary();

        let (filters, filter_warnings) = if let Some(builder) = self.seg_builder.take() {
            #[cfg(feature = "dev-profiling")]
            tracing::debug!(
                completed_segments = builder.completed_count(),
                pending_ids = builder.pending_id_count(),
                "segment_filter_build: pre-finish"
            );
            builder.finish()
        } else {
            self.built_filters.take().unwrap_or_default()
        };

        self.result.segment_filters = filters;
        self.result.warnings.extend(filter_warnings);
        self.result
    }
}

/// Resolves all unique field `name_string_id` values from every
/// `ClassDumpInfo` (instance and static fields) into
/// `ctx.result.index.field_names`.
///
/// Called once after heap extraction completes, when all
/// `class_dumps` entries are fully populated. Missing string
/// IDs are collected as warnings rather than hard errors.
fn preload_field_names(ctx: &mut FirstPassContext) {
    let mut field_name_ids = FxHashSet::default();
    for class_dump in ctx.result.index.class_dumps.values() {
        for field in &class_dump.instance_fields {
            field_name_ids.insert(field.name_string_id);
        }
        for field in &class_dump.static_fields {
            field_name_ids.insert(field.name_string_id);
        }
    }

    ctx.result.index.field_names.reserve(field_name_ids.len());

    let mut resolved_names = Vec::with_capacity(field_name_ids.len());
    let mut missing_name_ids = Vec::new();
    {
        let strings = &ctx.result.index.strings;
        for name_string_id in field_name_ids {
            if let Some(sref) = strings.get(&name_string_id) {
                resolved_names.push((name_string_id, sref.resolve(ctx.data)));
            } else {
                missing_name_ids.push(name_string_id);
            }
        }
    }

    for name_string_id in missing_name_ids {
        ctx.push_warning(|| {
            format!("field name string id {name_string_id} missing from STRING records")
        });
    }

    for (name_string_id, name) in resolved_names {
        ctx.result.index.field_names.insert(name_string_id, name);
    }

    #[cfg(feature = "dev-profiling")]
    {
        let field_names_heap_bytes = field_names_heap_bytes(&ctx.result.index);
        tracing::info!(
            field_names_entries = ctx.result.index.field_names.len(),
            field_names_heap_bytes,
            "first-pass preloaded field_names cache"
        );
    }
}

#[cfg(feature = "dev-profiling")]
fn field_names_heap_bytes(index: &PreciseIndex) -> usize {
    hprof_api::fxhashmap_memory_size::<u64, String>(index.field_names.capacity())
        + index
            .field_names
            .values()
            .map(|s| s.capacity())
            .sum::<usize>()
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
/// - `phase_changed` at filter build and each thread
///   resolution round
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
    id_size: IdSize,
    base_offset: u64,
    notifier: &mut ProgressNotifier,
    budget: MemoryBudget,
) -> IndexResult {
    #[cfg(feature = "dev-profiling")]
    let _first_pass_span = tracing::info_span!("first_pass").entered();

    let mut ctx = FirstPassContext::new(data, id_size, base_offset, budget);
    record_scan::scan_records(&mut ctx, notifier);
    heap_extraction::extract_all(&mut ctx, notifier);
    preload_field_names(&mut ctx);

    // Build segment filters BEFORE thread resolution.
    // Filters are needed for batched lookups.
    notifier.phase_changed("Building segment filters\u{2026}");
    let (filters, entry_points) = ctx.finish_filters_early();

    #[cfg(feature = "test-utils")]
    {
        ctx.result.diagnostics = DiagnosticInfo {
            entry_point_count: entry_points.len(),
            filter_lookup_ms: 0,
            precise_index_heap_bytes: ctx.result.index.memory_size(),
        };
    }

    // Resolve thread objects via batched filter
    // lookups.
    thread_resolution::resolve_all(&mut ctx, &filters, &entry_points, notifier);

    // Post-indexation coherence validation.
    crate::indexer::validation::validate_index(&mut ctx.result);

    // Store filters back for finish() to consume.
    ctx.built_filters = Some((filters, Vec::new()));

    {
        #[cfg(feature = "dev-profiling")]
        let _seg_filter_span = tracing::info_span!("segment_filter_build").entered();
        ctx.finish()
    }
}
