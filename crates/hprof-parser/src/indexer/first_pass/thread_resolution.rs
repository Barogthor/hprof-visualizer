//! Thread synthesis, GC root correlation, and transitive
//! offset resolution.

use std::collections::HashSet;
use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt};

use super::FirstPassContext;
use super::hprof_primitives::value_byte_size;
use crate::indexer::precise::PreciseIndex;
use crate::java_types::PRIM_TYPE_OBJECT_REF;
use crate::tags::HeapSubTag;
use crate::{HprofThread, read_id};

/// Synthesises threads, correlates GC roots, and resolves
/// transitive offsets for thread objects.
pub(super) fn resolve_all(ctx: &mut FirstPassContext) {
    #[cfg(feature = "dev-profiling")]
    let _thread_cache_span = tracing::info_span!("thread_cache_build").entered();

    // Synthesise thread entries from STACK_TRACE records
    // when the file has no START_THREAD (0x06) records.
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
    // sub-records and update synthetic threads' object_id.
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

    // Correlate GC_ROOT_JAVA_FRAME roots with stack traces.
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

    // Cross-reference thread_object_ids with sorted offsets.
    for &obj_id in ctx.result.index.thread_object_ids.values() {
        if let Some(offset) = lookup_offset(&ctx.all_offsets, obj_id) {
            ctx.result.index.instance_offsets.insert(obj_id, offset);
        }
    }

    // Follow transitive references so offset-based reads
    // cover the entire chain.
    resolve_thread_transitive_offsets(
        ctx.data,
        ctx.id_size,
        &mut ctx.result.index,
        &ctx.all_offsets,
    );
    ctx.all_offsets = Vec::new();
}

/// Binary-searches a sorted [`ObjectOffset`] slice by
/// `object_id`.
pub(super) fn lookup_offset(sorted: &[super::ObjectOffset], id: u64) -> Option<u64> {
    sorted
        .binary_search_by_key(&id, |o| o.object_id)
        .ok()
        .map(|i| sorted[i].offset)
}

/// Reads an `INSTANCE_DUMP` sub-record at `offset` in
/// `data`, returning `(class_object_id, field_data_slice)`.
fn read_raw_instance_at(data: &[u8], offset: u64, id_size: u32) -> Option<(u64, &[u8])> {
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
    Some((class_object_id, &slice[pos..pos + num_bytes]))
}

/// Extracts `ObjectRef` (type 2) field values by name from
/// raw instance field data.
fn extract_obj_refs(
    field_data: &[u8],
    class_object_id: u64,
    target_names: &[&str],
    index: &PreciseIndex,
    id_size: u32,
    records_data: &[u8],
) -> Vec<(String, u64)> {
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
                        results.push((name, ref_id));
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

/// Populates `instance_offsets` with transitive references
/// from thread objects.
fn resolve_thread_transitive_offsets(
    data: &[u8],
    id_size: u32,
    index: &mut PreciseIndex,
    all_offsets: &[super::ObjectOffset],
) {
    let thread_offsets: Vec<u64> = index.instance_offsets.values().copied().collect();

    let mut extra: Vec<(u64, u64)> = Vec::new();
    let mut string_ids: Vec<(u64, u64)> = Vec::new();

    for offset in thread_offsets {
        let Some((class_id, field_data)) = read_raw_instance_at(data, offset, id_size) else {
            continue;
        };
        let refs = extract_obj_refs(
            field_data,
            class_id,
            &["name", "holder"],
            index,
            id_size,
            data,
        );
        for (name, ref_id) in &refs {
            if let Some(off) = lookup_offset(all_offsets, *ref_id) {
                extra.push((*ref_id, off));
                if name == "name" {
                    string_ids.push((*ref_id, off));
                }
            }
        }
    }

    for (_, str_offset) in string_ids {
        let Some((str_class_id, str_data)) = read_raw_instance_at(data, str_offset, id_size) else {
            continue;
        };
        let refs = extract_obj_refs(str_data, str_class_id, &["value"], index, id_size, data);
        for (_, ref_id) in &refs {
            if let Some(off) = lookup_offset(all_offsets, *ref_id) {
                extra.push((*ref_id, off));
            }
        }
    }

    for (id, off) in extra {
        index.instance_offsets.insert(id, off);
    }
}
