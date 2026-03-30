//! Heap segment object extraction — sequential and parallel
//! paths.

use std::ops::Range;

use super::hprof_primitives::PARALLEL_THRESHOLD;
use super::offset_lookup::{EntryPointTracker, SegmentEntryPoint};
use super::{ClassDumpEntry, FilterEntry, FirstPassContext, RawFrameRoot, RawThreadObject};
use crate::heap_reader::{HeapSubRecord, HeapSubRecordIter};
use crate::id::IdSize;
use crate::indexer::HeapRecordRange;
use hprof_api::ProgressNotifier;

/// Per-worker output from heap segment extraction.
pub(super) struct HeapSegmentResult {
    pub(super) filter_ids: Vec<FilterEntry>,
    pub(super) segment_entry_points: Vec<SegmentEntryPoint>,
    pub(super) raw_frame_roots: Vec<RawFrameRoot>,
    pub(super) raw_thread_objects: Vec<RawThreadObject>,
    pub(super) class_dumps: Vec<ClassDumpEntry>,
    pub(super) warnings: Vec<String>,
}

impl HeapSegmentResult {
    fn is_empty(&self) -> bool {
        self.filter_ids.is_empty()
            && self.class_dumps.is_empty()
            && self.raw_frame_roots.is_empty()
            && self.raw_thread_objects.is_empty()
            && self.warnings.is_empty()
    }

    /// Creates a result with pre-allocated capacity for
    /// `filter_ids` based on an estimated record count.
    pub(super) fn new_with_capacity(est: usize) -> Self {
        Self {
            filter_ids: Vec::with_capacity(est),
            segment_entry_points: Vec::new(),
            raw_frame_roots: Vec::new(),
            raw_thread_objects: Vec::new(),
            class_dumps: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Wrapper around one or more [`HeapSegmentResult`]
/// chunks extracted from a single heap segment.
///
/// Abstracts whether a segment was extracted as a single
/// chunk or split into N chunks. The caller merges via
/// [`HeapSegmentParsingResult::merge_into`] without
/// knowing the chunk count.
pub(super) struct HeapSegmentParsingResult {
    pub(super) chunks: Vec<HeapSegmentResult>,
}

impl HeapSegmentParsingResult {
    pub(super) fn new(chunks: Vec<HeapSegmentResult>) -> Self {
        Self { chunks }
    }

    /// Merges all chunks into the shared context.
    pub(super) fn merge_into(self, ctx: &mut FirstPassContext) {
        for chunk in self.chunks {
            merge_segment_result(ctx, chunk);
        }
    }
}

/// Extracts heap sub-record data from a single segment,
/// producing one or more chunks bounded by
/// `max_chunk_bytes`.
///
/// When `max_chunk_bytes >= payload.len()`, produces a
/// single chunk (no-op, identical to pre-chunking
/// behavior).
pub(super) fn extract_heap_segment(
    payload: &[u8],
    data_offset: usize,
    id_size: IdSize,
    max_chunk_bytes: usize,
) -> HeapSegmentParsingResult {
    let chunk_est = max_chunk_bytes.min(payload.len()) / 40;
    let mut chunks: Vec<HeapSegmentResult> = Vec::new();
    let mut result = HeapSegmentResult::new_with_capacity(chunk_est);
    let mut next_checkpoint = max_chunk_bytes;
    let mut ep_tracker = EntryPointTracker::new();

    let mut iter = HeapSubRecordIter::new(payload, id_size);

    while let Some(record) = iter.next() {
        let tag_pos = data_offset + iter.tag_position() as usize;
        ep_tracker.track(tag_pos);

        match record {
            HeapSubRecord::GcRootJavaFrame {
                object_id,
                thread_serial,
                frame_number,
            } => {
                result.raw_frame_roots.push(RawFrameRoot {
                    object_id,
                    thread_serial,
                    frame_number,
                });
            }
            HeapSubRecord::GcRootThreadObj {
                object_id,
                thread_serial,
                ..
            } => {
                result.raw_thread_objects.push(RawThreadObject {
                    object_id,
                    thread_serial,
                });
            }
            HeapSubRecord::ClassDump(info) => {
                result.class_dumps.push(ClassDumpEntry {
                    class_object_id: info.class_object_id,
                    info,
                });
            }
            HeapSubRecord::Instance { id, .. } => {
                result.filter_ids.push(FilterEntry {
                    data_offset: tag_pos,
                    object_id: id,
                });
            }
            HeapSubRecord::ObjectArray { id, .. } => {
                result.filter_ids.push(FilterEntry {
                    data_offset: tag_pos,
                    object_id: id,
                });
            }
            HeapSubRecord::PrimArray { id, .. } => {
                result.filter_ids.push(FilterEntry {
                    data_offset: tag_pos,
                    object_id: id,
                });
            }
            HeapSubRecord::GcRootOther { .. } => {}
        }

        let pos = iter.position() as usize;
        if pos >= next_checkpoint {
            chunks.push(result);
            result = HeapSegmentResult::new_with_capacity(chunk_est);
            next_checkpoint += max_chunk_bytes;
        }
    }

    let pos = iter.position() as usize;
    if pos < payload.len() {
        result.warnings.push(format!(
            "heap segment truncated or unknown sub-tag \
             at absolute offset {}: {} bytes unread",
            data_offset + pos,
            payload.len() - pos,
        ));
    }

    if !result.is_empty() {
        chunks.push(result);
    }
    let entry_points = ep_tracker.finish();
    if let Some(first) = chunks.first_mut() {
        first.segment_entry_points = entry_points;
    }
    HeapSegmentParsingResult::new(chunks)
}

/// Merges a per-segment result into the shared context.
pub(super) fn merge_segment_result(ctx: &mut FirstPassContext, seg_result: HeapSegmentResult) {
    for entry in seg_result.filter_ids {
        ctx.seg_builder
            .as_mut()
            .expect("seg_builder consumed early")
            .add(entry.data_offset, entry.object_id);
    }
    for ep in seg_result.segment_entry_points {
        // Results are sorted by payload_start, so the
        // first entry point per segment_index has the
        // lowest scan_offset — skip duplicates.
        let already = ctx
            .segment_entry_points
            .iter()
            .any(|e| e.segment_index == ep.segment_index);
        if !already {
            ctx.segment_entry_points.push(ep);
        }
    }
    ctx.raw_frame_roots.extend(seg_result.raw_frame_roots);
    ctx.raw_thread_objects.extend(seg_result.raw_thread_objects);
    for entry in seg_result.class_dumps {
        ctx.result
            .index
            .class_dumps
            .insert(entry.class_object_id, entry.info);
    }
    for w in seg_result.warnings {
        ctx.push_warning(|| w);
    }
}

/// Minimum chunk size to avoid micro-chunking overhead.
const CHUNK_FLOOR: usize = 64 * 1024 * 1024;

/// Minimum inter-segment batch payload to prevent
/// degenerate micro-batching (e.g. `budget_bytes = 0`).
const BATCH_FLOOR: u64 = 64 * 1024 * 1024;

/// Groups contiguous segments into batches whose
/// cumulative payload does not exceed
/// `max_batch_payload`.
///
/// A single segment exceeding `max_batch_payload` is
/// placed in its own solo batch (never skipped).
/// Returns index ranges into `ranges`.
pub(super) fn compute_batch_ranges(
    ranges: &[HeapRecordRange],
    max_batch_payload: u64,
) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let mut batches = Vec::new();
    let mut batch_start = 0;
    let mut batch_payload = 0u64;

    for (i, r) in ranges.iter().enumerate() {
        if batch_payload + r.payload_length > max_batch_payload && batch_start < i {
            batches.push(batch_start..i);
            batch_start = i;
            batch_payload = 0;
        }
        batch_payload += r.payload_length;
    }

    if batch_start < ranges.len() {
        batches.push(batch_start..ranges.len());
    }

    batches
}

/// Extracts all heap segments — parallel or sequential
/// depending on total heap size and thread count.
/// Reports byte-level progress via
/// [`ProgressNotifier::heap_bytes_extracted`].
pub(super) fn extract_all(ctx: &mut FirstPassContext, notifier: &mut ProgressNotifier) {
    let total_heap_bytes: u64 = ctx
        .result
        .heap_record_ranges
        .iter()
        .map(|r| r.payload_length)
        .sum();

    let ranges: Vec<_> = ctx.result.heap_record_ranges.clone();
    let data = ctx.data;
    let id_size = ctx.id_size;

    // Intra-segment chunk size (story 10.2).
    let max_chunk_bytes = match ctx.budget.bytes() {
        Some(b) => {
            let b = usize::try_from(b).unwrap_or(usize::MAX);
            let per_thread = b / rayon::current_num_threads().max(1);
            per_thread.max(CHUNK_FLOOR)
        }
        None => usize::MAX,
    };

    // Inter-segment batch payload limit (story 10.3).
    let max_batch_payload: u64 = ctx
        .budget
        .bytes()
        .map(|b| b.max(BATCH_FLOOR))
        .unwrap_or(u64::MAX);

    // Parallel path: total heap >= threshold AND more
    // than 1 rayon thread available. When
    // current_num_threads() == 1, rayon::scope workers
    // have no thread to run on while the main thread
    // blocks on rx.iter() — deadlock.
    if total_heap_bytes >= PARALLEL_THRESHOLD && rayon::current_num_threads() > 1 {
        #[cfg(feature = "dev-profiling")]
        let _par_span = tracing::info_span!("parallel_heap_extraction").entered();

        let batches = compute_batch_ranges(&ranges, max_batch_payload);

        // bytes_done is cumulative across ALL batches so
        // the progress bar never regresses at a batch
        // boundary.
        let mut bytes_done: u64 = 0;

        for (batch_idx, batch_range) in batches.iter().enumerate() {
            let batch = &ranges[batch_range.clone()];

            #[cfg(feature = "dev-profiling")]
            {
                let batch_payload: u64 = batch.iter().map(|r| r.payload_length).sum();
                tracing::info!(
                    "heap extraction batch {}/{}: \
                     {} segment(s), {} bytes payload, \
                     limit {} bytes",
                    batch_idx + 1,
                    batches.len(),
                    batch.len(),
                    batch_payload,
                    max_batch_payload,
                );
            }
            #[cfg(not(feature = "dev-profiling"))]
            let _ = batch_idx;

            let (tx, rx) = std::sync::mpsc::channel();

            rayon::in_place_scope(|s| {
                for r in batch {
                    let tx = tx.clone();
                    s.spawn(move |_| {
                        let start = r.payload_start as usize;
                        let end = start + r.payload_length as usize;
                        let result = extract_heap_segment(
                            &data[start..end],
                            start,
                            id_size,
                            max_chunk_bytes,
                        );
                        let _ = tx.send((r.payload_start, r.payload_length, result));
                    });
                }
                // CRITICAL: drop original tx so
                // rx.iter() terminates when all worker
                // clones are dropped.
                drop(tx);
            });

            // Drain after scope — calling thread
            // participates in rayon work-stealing
            // while the scope waits, maximising
            // parallelism. Sort by payload_start
            // before merging (SegmentFilterBuilder
            // requires non-decreasing offset order).
            let mut batch_results: Vec<_> = rx.into_iter().collect();
            batch_results.sort_unstable_by_key(|(start, _, _)| *start);
            for (_, payload_len, result) in batch_results.drain(..) {
                result.merge_into(ctx);
                bytes_done += payload_len;
                notifier.heap_bytes_extracted(bytes_done, total_heap_bytes);
            }
        }
    } else {
        #[cfg(feature = "dev-profiling")]
        let _seq_span = tracing::info_span!("sequential_heap_extraction").entered();

        let mut bytes_done: u64 = 0;
        for r in &ranges {
            let start = r.payload_start as usize;
            let end = start + r.payload_length as usize;
            let parsing_result =
                extract_heap_segment(&data[start..end], start, id_size, max_chunk_bytes);
            parsing_result.merge_into(ctx);
            bytes_done += r.payload_length;
            notifier.heap_bytes_extracted(bytes_done, total_heap_bytes);
        }
    }
}
