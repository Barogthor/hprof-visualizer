//! Thread synthesis, GC root correlation, and
//! filter-based transitive offset resolution.

use std::collections::HashSet;
use std::io::Cursor;
#[cfg(feature = "test-utils")]
use std::time::Instant;

use byteorder::{BigEndian, ReadBytesExt};

use hprof_api::ProgressNotifier;

use super::FirstPassContext;
use super::hprof_primitives::value_byte_size;
use super::offset_lookup::batch_lookup_by_filter;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::SegmentFilter;
use crate::java_types::PRIM_TYPE_OBJECT_REF;
use crate::tags::HeapSubTag;
use crate::{HprofThread, read_id};

/// Parsed INSTANCE_DUMP header: class identity and
/// raw field bytes.
struct RawInstance<'a> {
    class_object_id: u64,
    field_data: &'a [u8],
}

/// An object-reference field extracted by name from
/// an instance's field data.
struct ObjRef {
    field_name: String,
    ref_id: u64,
}

/// Synthesises threads, correlates GC roots, and
/// resolves transitive offsets via batched filter
/// lookups.
pub(super) fn resolve_all(
    ctx: &mut FirstPassContext,
    filters: &[SegmentFilter],
    entry_points: &[super::offset_lookup::SegmentEntryPoint],
    notifier: &mut ProgressNotifier,
) {
    #[cfg(feature = "dev-profiling")]
    let _thread_cache_span = tracing::info_span!("thread_cache_build").entered();

    // Synthesise thread entries from STACK_TRACE
    // records when the file has no START_THREAD (0x06).
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

    // Populate thread_object_ids from ROOT_THREAD_OBJ
    // sub-records and update synthetic threads.
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

    // Correlate GC_ROOT_JAVA_FRAME roots with stack
    // traces.
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
        let idx = root.frame_number as usize;
        let Some(&frame_id) = trace.frame_ids.get(idx) else {
            continue;
        };
        ctx.result
            .index
            .java_frame_roots
            .entry(frame_id)
            .or_default()
            .push(root.object_id);
    }

    // ── Batched filter lookups (3 rounds) ──

    let data = ctx.data;
    let id_size = ctx.id_size;

    #[cfg(feature = "test-utils")]
    let t0 = Instant::now();

    // Round 0: thread object offsets
    let thread_ids: HashSet<u64> = ctx
        .result
        .index
        .thread_object_ids
        .values()
        .copied()
        .collect();

    if !thread_ids.is_empty() {
        notifier.phase_changed("Resolving threads (round 1/3)\u{2026}");
        let (found, warns) = batch_lookup_by_filter(
            filters,
            entry_points,
            data,
            id_size,
            &thread_ids,
            &ctx.result.heap_record_ranges,
        );
        for w in warns {
            ctx.push_warning(w);
        }
        for (&id, &offset) in &found {
            ctx.result.index.instance_offsets.insert(id, offset);
        }

        // Round 1: transitive refs (name, holder)
        let mut round1_ids: HashSet<u64> = HashSet::new();
        let thread_offsets: Vec<u64> = ctx.result.index.instance_offsets.values();

        let mut string_offsets: Vec<(u64, u64)> = Vec::new();

        for offset in thread_offsets {
            let Some(inst) = read_raw_instance_at(data, offset, id_size) else {
                continue;
            };
            let refs = extract_obj_refs(
                inst.field_data,
                inst.class_object_id,
                &["name", "holder"],
                &ctx.result.index,
                id_size,
                data,
            );
            for r in &refs {
                if !ctx.result.index.instance_offsets.contains(&r.ref_id) {
                    round1_ids.insert(r.ref_id);
                    if r.field_name == "name" {
                        string_offsets.push((r.ref_id, 0));
                    }
                }
            }
        }

        if !round1_ids.is_empty() {
            notifier.phase_changed("Resolving threads (round 2/3)\u{2026}");
            let (found1, warns1) = batch_lookup_by_filter(
                filters,
                entry_points,
                data,
                id_size,
                &round1_ids,
                &ctx.result.heap_record_ranges,
            );
            for w in warns1 {
                ctx.push_warning(w);
            }
            for (id, offset) in &found1 {
                ctx.result.index.instance_offsets.insert(*id, *offset);
            }

            // Update string_offsets with resolved
            // offsets for round 2
            for (id, off) in &mut string_offsets {
                if let Some(&resolved) = found1.get(id) {
                    *off = resolved;
                }
            }

            // Round 2: value refs from String instances
            let mut round2_ids: HashSet<u64> = HashSet::new();
            for (_, str_offset) in &string_offsets {
                if *str_offset == 0 {
                    continue;
                }
                let Some(str_inst) = read_raw_instance_at(data, *str_offset, id_size) else {
                    continue;
                };
                let refs = extract_obj_refs(
                    str_inst.field_data,
                    str_inst.class_object_id,
                    &["value"],
                    &ctx.result.index,
                    id_size,
                    data,
                );
                for r in &refs {
                    if !ctx.result.index.instance_offsets.contains(&r.ref_id) {
                        round2_ids.insert(r.ref_id);
                    }
                }
            }

            if !round2_ids.is_empty() {
                notifier.phase_changed(
                    "Resolving threads (round 3/3)\
                     \u{2026}",
                );
                let (found2, warns2) = batch_lookup_by_filter(
                    filters,
                    entry_points,
                    data,
                    id_size,
                    &round2_ids,
                    &ctx.result.heap_record_ranges,
                );
                for w in warns2 {
                    ctx.push_warning(w);
                }
                for (&id, &offset) in &found2 {
                    ctx.result.index.instance_offsets.insert(id, offset);
                }
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
fn read_raw_instance_at<'a>(data: &'a [u8], offset: u64, id_size: u32) -> Option<RawInstance<'a>> {
    let start = offset as usize;
    if start >= data.len() {
        return None;
    }
    let slice = &data[start..];
    let mut cursor = Cursor::new(slice);
    if HeapSubTag::from(cursor.read_u8().ok()?) != HeapSubTag::InstanceDump {
        return None;
    }
    let _obj_id = read_id(&mut cursor, id_size).ok()?;
    let _stack_serial = cursor.read_u32::<BigEndian>().ok()?;
    let class_object_id = read_id(&mut cursor, id_size).ok()?;
    let num_bytes = cursor.read_u32::<BigEndian>().ok()? as usize;
    let pos = cursor.position() as usize;
    if pos + num_bytes > slice.len() {
        return None;
    }
    Some(RawInstance {
        class_object_id,
        field_data: &slice[pos..pos + num_bytes],
    })
}

/// Extracts `ObjectRef` (type 2) field values by name
/// from raw instance field data.
fn extract_obj_refs(
    field_data: &[u8],
    class_object_id: u64,
    target_names: &[&str],
    index: &PreciseIndex,
    id_size: u32,
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

    let mut cursor = Cursor::new(field_data);
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
                let Ok(ref_id) = read_id(&mut cursor, id_size) else {
                    return results;
                };
                if ref_id != 0 {
                    let name: String = index
                        .strings
                        .get(&field.name_string_id)
                        .map(|sref| sref.resolve(records_data))
                        .unwrap_or_default();
                    if target_names.contains(&name.as_str()) {
                        results.push(ObjRef {
                            field_name: name,
                            ref_id,
                        });
                    }
                }
            } else {
                let pos = cursor.position() as usize + field_size;
                if pos > field_data.len() {
                    return results;
                }
                cursor.set_position(pos as u64);
            }
        }
    }
    results
}
