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

#[cfg(feature = "test-utils")]
use hprof_api::MemorySize;
use hprof_api::{MemoryBudget, ProgressNotifier};
use rustc_hash::FxHashSet;

use crate::ClassDumpInfo;
use crate::format::IdSize;
#[cfg(feature = "test-utils")]
use crate::indexer::DiagnosticInfo;
use crate::indexer::HeapRecordRange;
use crate::indexer::IndexResult;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::SegmentFilterBuilder;

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

/// Output of the record scanning phase.
pub(super) struct RecordScanOutput {
    pub(super) index: PreciseIndex,
    pub(super) heap_record_ranges: Vec<HeapRecordRange>,
    pub(super) warnings: Vec<String>,
    pub(super) suppressed_warnings: u64,
    pub(super) records_attempted: u64,
    pub(super) records_indexed: u64,
}

/// Output of the heap extraction phase.
pub(super) struct HeapExtractionOutput {
    pub(super) seg_builder: SegmentFilterBuilder,
    pub(super) segment_entry_points: Vec<offset_lookup::SegmentEntryPoint>,
    pub(super) raw_frame_roots: Vec<RawFrameRoot>,
    pub(super) raw_thread_objects: Vec<RawThreadObject>,
    pub(super) class_dumps: Vec<ClassDumpEntry>,
    pub(super) warnings: Vec<String>,
    pub(super) suppressed_warnings: u64,
}

/// Output of the thread resolution phase.
pub(super) struct ResolveOutput {
    pub(super) warnings: Vec<String>,
    pub(super) suppressed_warnings: u64,
    #[cfg(feature = "test-utils")]
    pub(super) filter_lookup_ms: u64,
}

/// Pushes a warning with [`MAX_WARNINGS`] cap.
pub(super) fn push_warning_capped(
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

/// Extends warnings with global cap semantics.
pub(super) fn extend_warnings_capped(
    warnings: &mut Vec<String>,
    suppressed: &mut u64,
    incoming: impl IntoIterator<Item = String>,
) {
    for w in incoming {
        push_warning_capped(warnings, suppressed, || w);
    }
}

/// Resolves all unique field `name_string_id` values from
/// every `ClassDumpInfo` (instance and static fields) into
/// `index.field_names`.
///
/// Called once after heap extraction completes, when all
/// `class_dumps` entries are fully populated. Missing
/// string IDs are collected as warnings.
fn preload_field_names(
    index: &mut PreciseIndex,
    data: &[u8],
    warnings: &mut Vec<String>,
    suppressed: &mut u64,
) {
    let mut field_name_ids = FxHashSet::default();
    for class_dump in index.class_dumps.values() {
        for field in &class_dump.instance_fields {
            field_name_ids.insert(field.name_string_id);
        }
        for field in &class_dump.static_fields {
            field_name_ids.insert(field.name_string_id);
        }
    }

    index.field_names.reserve(field_name_ids.len());

    let mut resolved_names = Vec::with_capacity(field_name_ids.len());
    let mut missing_name_ids = Vec::new();
    {
        let strings = &index.strings;
        for name_string_id in field_name_ids {
            if let Some(sref) = strings.get(&name_string_id) {
                resolved_names.push((name_string_id, sref.resolve(data)));
            } else {
                missing_name_ids.push(name_string_id);
            }
        }
    }

    for name_string_id in missing_name_ids {
        push_warning_capped(warnings, suppressed, || {
            format!(
                "field name string id \
                 {name_string_id} missing from \
                 STRING records"
            )
        });
    }

    for (name_string_id, name) in resolved_names {
        index.field_names.insert(name_string_id, name);
    }

    #[cfg(feature = "dev-profiling")]
    {
        let field_names_heap_bytes = field_names_heap_bytes(index);
        tracing::info!(
            field_names_entries = index.field_names.len(),
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

    // Phase 1: Record scanning
    let scan = record_scan::scan_records(data, id_size, base_offset, notifier);

    // Phase 2: Heap extraction
    let extraction =
        heap_extraction::extract_all(data, id_size, &scan.heap_record_ranges, budget, notifier);

    // Assemble index
    let mut index = scan.index;
    for entry in extraction.class_dumps {
        index.class_dumps.insert(entry.class_object_id, entry.info);
    }

    // Merge warnings with global cap
    let mut warnings = scan.warnings;
    let mut suppressed = scan.suppressed_warnings + extraction.suppressed_warnings;
    extend_warnings_capped(&mut warnings, &mut suppressed, extraction.warnings);

    // Phase 3: Preload field names
    preload_field_names(&mut index, data, &mut warnings, &mut suppressed);

    // Build segment filters
    notifier.phase_changed("Building segment filters\u{2026}");
    let (filters, filter_warnings) = {
        #[cfg(feature = "dev-profiling")]
        let _seg_filter_span = tracing::info_span!("segment_filter_build").entered();
        extraction.seg_builder.finish()
    };
    extend_warnings_capped(&mut warnings, &mut suppressed, filter_warnings);
    let mut entry_points = extraction.segment_entry_points;
    entry_points.sort_unstable_by_key(|ep| ep.segment_index);

    // Phase 4: Thread resolution
    let resolve_ctx = thread_resolution::ResolveCtx {
        data,
        id_size,
        heap_record_ranges: &scan.heap_record_ranges,
        filters: &filters,
        entry_points: &entry_points,
    };
    let resolve = thread_resolution::resolve_all(
        &resolve_ctx,
        &mut index,
        extraction.raw_frame_roots,
        extraction.raw_thread_objects,
        notifier,
    );
    suppressed += resolve.suppressed_warnings;
    extend_warnings_capped(&mut warnings, &mut suppressed, resolve.warnings);

    // Build result
    let mut result = IndexResult {
        index,
        warnings,
        records_attempted: scan.records_attempted,
        records_indexed: scan.records_indexed,
        segment_filters: filters,
        heap_record_ranges: scan.heap_record_ranges,
        #[cfg(feature = "test-utils")]
        diagnostics: DiagnosticInfo {
            entry_point_count: entry_points.len(),
            filter_lookup_ms: resolve.filter_lookup_ms,
            precise_index_heap_bytes: 0,
        },
    };

    #[cfg(feature = "test-utils")]
    {
        result.diagnostics.precise_index_heap_bytes = result.index.memory_size();
    }

    crate::indexer::validation::validate_index(&mut result);

    if suppressed > 0 {
        result.warnings.push(format!(
            "... {suppressed} additional warning(s) \
             suppressed (only first \
             {MAX_WARNINGS} shown)",
        ));
    }

    result
}
