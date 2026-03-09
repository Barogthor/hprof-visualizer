//! Heap segment object extraction — sequential and parallel
//! paths.

use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt};
use hprof_api::ProgressNotifier;
use rayon::prelude::*;

use super::hprof_primitives::{
    PARALLEL_THRESHOLD, gc_root_skip_size, parse_class_dump, primitive_element_size, skip_n,
};
use super::{
    ClassDumpEntry, FilterEntry, FirstPassContext, ObjectOffset, RawFrameRoot, RawThreadObject,
};
use crate::tags::HeapSubTag;
use crate::read_id;

/// Per-worker output from heap segment extraction.
pub(super) struct HeapSegmentResult {
    pub(super) all_offsets: Vec<ObjectOffset>,
    pub(super) filter_ids: Vec<FilterEntry>,
    pub(super) raw_frame_roots: Vec<RawFrameRoot>,
    pub(super) raw_thread_objects: Vec<RawThreadObject>,
    pub(super) class_dumps: Vec<ClassDumpEntry>,
    pub(super) warnings: Vec<String>,
}

/// Extracts heap sub-record data from a single segment.
/// No mutable shared state — all output goes into the
/// returned [`HeapSegmentResult`].
pub(super) fn extract_heap_segment(
    payload: &[u8],
    data_offset: usize,
    id_size: u32,
) -> HeapSegmentResult {
    let mut cursor = Cursor::new(payload);
    let est_records = payload.len() / 40;
    let mut result = HeapSegmentResult {
        all_offsets: Vec::with_capacity(est_records),
        filter_ids: Vec::with_capacity(est_records),
        raw_frame_roots: Vec::new(),
        raw_thread_objects: Vec::new(),
        class_dumps: Vec::new(),
        warnings: Vec::new(),
    };

    while let Ok(raw) = cursor.read_u8() {
        let sub_tag = HeapSubTag::from(raw);
        let sub_record_start = data_offset + cursor.position() as usize;

        let ok = match sub_tag {
            HeapSubTag::GcRootJavaFrame => {
                let Ok(object_id) = read_id(&mut cursor, id_size) else {
                    result.warnings.push(format!(
                        "{sub_tag} at offset \
                         {sub_record_start}: truncated \
                         object_id"
                    ));
                    break;
                };
                let Ok(thread_serial) = cursor.read_u32::<BigEndian>() else {
                    result.warnings.push(format!(
                        "{sub_tag} at offset \
                         {sub_record_start}: truncated \
                         thread_serial"
                    ));
                    break;
                };
                let Ok(frame_number) = cursor.read_i32::<BigEndian>() else {
                    result.warnings.push(format!(
                        "{sub_tag} at offset \
                         {sub_record_start}: truncated \
                         frame_number"
                    ));
                    break;
                };
                result.raw_frame_roots.push(RawFrameRoot {
                    object_id,
                    thread_serial,
                    frame_number,
                });
                true
            }
            HeapSubTag::GcRootThreadObj => {
                let Ok(object_id) = read_id(&mut cursor, id_size) else {
                    result.warnings.push(format!(
                        "{sub_tag} at offset \
                         {sub_record_start}: truncated \
                         object_id"
                    ));
                    break;
                };
                let Ok(thread_serial) = cursor.read_u32::<BigEndian>() else {
                    result.warnings.push(format!(
                        "{sub_tag} at offset \
                         {sub_record_start}: truncated \
                         thread_serial"
                    ));
                    break;
                };
                let Ok(_stack_trace_serial) = cursor.read_u32::<BigEndian>() else {
                    result.warnings.push(format!(
                        "{sub_tag} at offset \
                         {sub_record_start}: truncated \
                         stack_trace_serial"
                    ));
                    break;
                };
                result.raw_thread_objects.push(RawThreadObject {
                    object_id,
                    thread_serial,
                });
                true
            }

            HeapSubTag::ClassDump => match parse_class_dump(&mut cursor, id_size) {
                Some(info) => {
                    result.class_dumps.push(ClassDumpEntry {
                        class_object_id: info.class_object_id,
                        info,
                    });
                    true
                }
                None => {
                    result.warnings.push(
                        "truncated CLASS_DUMP \
                             sub-record — skipping"
                            .to_string(),
                    );
                    false
                }
            },

            HeapSubTag::InstanceDump => {
                let Ok(obj_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                result.filter_ids.push(FilterEntry {
                    data_offset: sub_record_start,
                    object_id: obj_id,
                });
                result.all_offsets.push(ObjectOffset {
                    object_id: obj_id,
                    offset: (sub_record_start - 1) as u64,
                });
                let Ok(_) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(_) = read_id(&mut cursor, id_size) else {
                    break;
                };
                let Ok(num_bytes) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                skip_n(&mut cursor, num_bytes as usize)
            }

            HeapSubTag::ObjectArrayDump => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                result.filter_ids.push(FilterEntry {
                    data_offset: sub_record_start,
                    object_id: arr_id,
                });
                let Ok(_) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(num_elements) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(_) = read_id(&mut cursor, id_size) else {
                    break;
                };
                skip_n(&mut cursor, num_elements as usize * id_size as usize)
            }

            HeapSubTag::PrimArrayDump => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                result.filter_ids.push(FilterEntry {
                    data_offset: sub_record_start,
                    object_id: arr_id,
                });
                result.all_offsets.push(ObjectOffset {
                    object_id: arr_id,
                    offset: (sub_record_start - 1) as u64,
                });
                let Ok(_) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(num_elements) = cursor.read_u32::<BigEndian>() else {
                    break;
                };
                let Ok(elem_type) = cursor.read_u8() else {
                    break;
                };
                let elem_size = primitive_element_size(elem_type);
                if elem_size == 0 {
                    break;
                }
                skip_n(&mut cursor, num_elements as usize * elem_size)
            }

            t if gc_root_skip_size(t, id_size).is_some() => {
                skip_n(&mut cursor, gc_root_skip_size(t, id_size).unwrap())
            }
            _ => break,
        };

        if !ok {
            break;
        }
    }

    result
}

/// Merges a per-segment result into the shared context.
pub(super) fn merge_segment_result(ctx: &mut FirstPassContext, seg_result: HeapSegmentResult) {
    ctx.all_offsets.extend(seg_result.all_offsets);
    for entry in seg_result.filter_ids {
        ctx.seg_builder.add(entry.data_offset, entry.object_id);
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
        ctx.push_warning(w);
    }
}

/// Extracts all heap segments — parallel or sequential
/// depending on total heap size. Reports segment-level
/// progress via [`ProgressNotifier::segment_completed`].
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
    let total_segments = ranges.len();
    let mut segments_done: usize = 0;

    if total_heap_bytes >= PARALLEL_THRESHOLD {
        #[cfg(feature = "dev-profiling")]
        let _par_span = tracing::info_span!("parallel_heap_extraction").entered();

        let batch_size = rayon::current_num_threads().max(1);
        for batch in ranges.chunks(batch_size) {
            let batch_results: Vec<HeapSegmentResult> = batch
                .par_iter()
                .map(|r| {
                    let start = r.payload_start as usize;
                    let end = start + r.payload_length as usize;
                    extract_heap_segment(&data[start..end], start, id_size)
                })
                .collect();

            for seg_result in batch_results {
                merge_segment_result(ctx, seg_result);
                segments_done += 1;
                notifier.segment_completed(segments_done, total_segments);
            }
        }
    } else {
        #[cfg(feature = "dev-profiling")]
        let _seq_span = tracing::info_span!("sequential_heap_extraction").entered();

        for r in &ranges {
            let start = r.payload_start as usize;
            let end = start + r.payload_length as usize;
            let seg_result = extract_heap_segment(&data[start..end], start, id_size);
            merge_segment_result(ctx, seg_result);
            segments_done += 1;
            notifier.segment_completed(segments_done, total_segments);
        }
    }
}
