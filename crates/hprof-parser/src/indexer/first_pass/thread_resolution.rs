//! Thread synthesis, GC root correlation, and
//! filter-based transitive offset resolution.

use std::collections::HashSet;
#[cfg(feature = "test-utils")]
use std::time::Instant;

use rustc_hash::FxHashMap;

use hprof_api::ProgressNotifier;

use super::FirstPassContext;
use super::offset_lookup::{SegmentEntryPoint, batch_lookup_by_filter};
use crate::id::IdSize;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::SegmentFilter;
use crate::java_types::PRIM_TYPE_OBJECT_REF;
use crate::java_types::value_byte_size;
use crate::reader::{InstanceDumpBody, RecordReader};
use crate::tags::HeapSubTag;
use crate::HprofThread;

/// An object-reference field extracted by name from
/// an instance's field data.
struct ObjRef {
    field_name: String,
    ref_id: u64,
}

/// Synthesises thread entries from STACK_TRACE records
/// when the file has no START_THREAD (0x06).
fn synthesise_threads(ctx: &mut FirstPassContext) {
    let traces: Vec<_> = ctx
        .result
        .index
        .stack_traces
        .values()
        .filter(|t| t.thread_serial > 0)
        .map(|t| (t.thread_serial, t.stack_trace_serial))
        .collect();
    for (thread_serial, stack_trace_serial) in traces {
        ctx.result
            .index
            .threads
            .entry(thread_serial)
            .or_insert(HprofThread {
                thread_serial,
                object_id: 0,
                stack_trace_serial,
                name_string_id: 0,
                group_name_string_id: 0,
                group_parent_name_string_id: 0,
            });
    }
}

/// Populates `thread_object_ids` from ROOT_THREAD_OBJ
/// sub-records and patches synthetic thread `object_id`.
fn populate_thread_object_ids(ctx: &mut FirstPassContext) {
    for rto in std::mem::take(&mut ctx.raw_thread_objects) {
        ctx.result
            .index
            .thread_object_ids
            .insert(rto.thread_serial, rto.object_id);
        if let Some(thread) = ctx.result.index.threads.get_mut(&rto.thread_serial)
            && thread.object_id == 0
        {
            thread.object_id = rto.object_id;
        }
    }
}

/// Correlates GC_ROOT_JAVA_FRAME roots with stack frames.
fn correlate_frame_roots(ctx: &mut FirstPassContext) {
    for root in std::mem::take(&mut ctx.raw_frame_roots) {
        if root.frame_number < 0 {
            continue;
        }
        let Some(thread) = ctx.result.index.threads.get(&root.thread_serial) else {
            continue;
        };
        let Some(trace) = ctx
            .result
            .index
            .stack_traces
            .get(&thread.stack_trace_serial)
        else {
            continue;
        };
        let Some(&frame_id) = trace.frame_ids.get(root.frame_number as usize) else {
            continue;
        };
        ctx.result
            .index
            .java_frame_roots
            .entry(frame_id)
            .or_default()
            .push(root.object_id);
    }
}

/// Runs one batch filter lookup round: emits the phase
/// label, resolves `ids`, merges warnings, inserts found
/// offsets into `instance_offsets`, and returns the raw
/// lookup map for callers that need it.
fn run_batch_round(
    ctx: &mut FirstPassContext,
    filters: &[SegmentFilter],
    entry_points: &[SegmentEntryPoint],
    ids: &HashSet<u64>,
    notifier: &mut ProgressNotifier,
    phase: &str,
) -> FxHashMap<u64, u64> {
    notifier.phase_changed(phase);
    let (found, warns) = batch_lookup_by_filter(
        filters,
        entry_points,
        ctx.data,
        ctx.id_size,
        ids,
        &ctx.result.heap_record_ranges,
    );
    for w in warns {
        ctx.push_warning(|| w);
    }
    for (&id, &offset) in &found {
        ctx.result.index.instance_offsets.insert(id, offset);
    }
    found
}

/// Synthesises threads, correlates GC roots, and
/// resolves transitive offsets via batched filter
/// lookups.
pub(super) fn resolve_all(
    ctx: &mut FirstPassContext,
    filters: &[SegmentFilter],
    entry_points: &[SegmentEntryPoint],
    notifier: &mut ProgressNotifier,
) {
    #[cfg(feature = "dev-profiling")]
    let _thread_cache_span = tracing::info_span!("thread_cache_build").entered();

    synthesise_threads(ctx);
    populate_thread_object_ids(ctx);
    correlate_frame_roots(ctx);

    let data = ctx.data;
    let id_size = ctx.id_size;

    #[cfg(feature = "test-utils")]
    let t0 = Instant::now();

    let thread_ids: HashSet<u64> =
        ctx.result.index.thread_object_ids.values().copied().collect();

    if !thread_ids.is_empty() {
        // Round 0: locate thread object instances.
        run_batch_round(
            ctx,
            filters,
            entry_points,
            &thread_ids,
            notifier,
            "Resolving threads (round 1/3)\u{2026}",
        );

        // Round 1: collect name/holder refs from thread instances.
        let mut round1_ids: HashSet<u64> = HashSet::new();
        let mut name_ref_ids: HashSet<u64> = HashSet::new();
        for offset in ctx.result.index.instance_offsets.values() {
            let Some(inst) = read_raw_instance_at(data, offset, id_size) else {
                continue;
            };
            for r in extract_obj_refs(
                inst.field_data,
                inst.class_object_id,
                &["name", "holder"],
                &ctx.result.index,
                id_size,
                data,
            ) {
                if !ctx.result.index.instance_offsets.contains(&r.ref_id) {
                    round1_ids.insert(r.ref_id);
                    if r.field_name == "name" {
                        name_ref_ids.insert(r.ref_id);
                    }
                }
            }
        }

        if !round1_ids.is_empty() {
            let found1 = run_batch_round(
                ctx,
                filters,
                entry_points,
                &round1_ids,
                notifier,
                "Resolving threads (round 2/3)\u{2026}",
            );

            // Round 2: collect value refs from String instances.
            let mut round2_ids: HashSet<u64> = HashSet::new();
            for id in &name_ref_ids {
                let Some(&str_offset) = found1.get(id) else {
                    continue;
                };
                let Some(str_inst) = read_raw_instance_at(data, str_offset, id_size) else {
                    continue;
                };
                for r in extract_obj_refs(
                    str_inst.field_data,
                    str_inst.class_object_id,
                    &["value"],
                    &ctx.result.index,
                    id_size,
                    data,
                ) {
                    if !ctx.result.index.instance_offsets.contains(&r.ref_id) {
                        round2_ids.insert(r.ref_id);
                    }
                }
            }

            if !round2_ids.is_empty() {
                run_batch_round(
                    ctx,
                    filters,
                    entry_points,
                    &round2_ids,
                    notifier,
                    "Resolving threads (round 3/3)\u{2026}",
                );
            }
        }
    }

    #[cfg(feature = "test-utils")]
    {
        ctx.result.diagnostics.filter_lookup_ms = t0.elapsed().as_millis() as u64;
    }
}

/// Reads an `INSTANCE_DUMP` sub-record at `offset` in
/// `data`.
fn read_raw_instance_at<'a>(
    data: &'a [u8],
    offset: u64,
    id_size: IdSize,
) -> Option<InstanceDumpBody<'a>> {
    let start = offset as usize;
    if start >= data.len() {
        return None;
    }
    let slice = &data[start..];
    let mut reader = RecordReader::new(slice, id_size);
    if HeapSubTag::from(reader.read_u8()?) != HeapSubTag::InstanceDump {
        return None;
    }
    reader.parse_instance_dump_body()
}

/// Extracts `ObjectRef` (type 2) field values by name
/// from raw instance field data.
fn extract_obj_refs(
    field_data: &[u8],
    class_object_id: u64,
    target_names: &[&str],
    index: &PreciseIndex,
    id_size: IdSize,
    records_data: &[u8],
) -> Vec<ObjRef> {
    let mut chain: Vec<u64> = Vec::new();
    let mut visited = HashSet::new();
    let mut current = class_object_id;
    loop {
        if !visited.insert(current) {
            break;
        }
        let Some(info) = index.class_dumps.get(&current) else {
            break;
        };
        chain.push(current);
        if info.super_class_id == 0 {
            break;
        }
        current = info.super_class_id;
    }

    let mut reader = RecordReader::new(field_data, id_size);
    let mut results = Vec::new();

    for &cid in &chain {
        let Some(info) = index.class_dumps.get(&cid) else {
            continue;
        };
        for field in &info.instance_fields {
            let field_size = value_byte_size(field.field_type, id_size);
            if field_size == 0 {
                return results;
            }
            if field.field_type == PRIM_TYPE_OBJECT_REF {
                let Some(ref_id) = reader.read_id() else {
                    return results;
                };
                if ref_id != 0 {
                    let name: String = index
                        .strings
                        .get(&field.name_string_id)
                        .map(|sref| sref.resolve(records_data))
                        .unwrap_or_default();
                    if target_names.contains(&name.as_str()) {
                        results.push(ObjRef { field_name: name, ref_id });
                    }
                }
            } else if !reader.skip(field_size) {
                return results;
            }
        }
    }
    results
}
