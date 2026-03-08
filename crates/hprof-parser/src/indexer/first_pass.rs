//! Single sequential pass over hprof record bytes, building an [`IndexResult`].
//!
//! [`run_first_pass`] iterates over every record in `data` (a slice starting
//! immediately after the file header), parsing known record types into the
//! index and collecting any non-fatal errors as warnings. The function is
//! infallible — it always returns an [`IndexResult`].
//!
//! Progress is reported via the `progress_fn` callback, which receives the
//! current byte offset (relative to `data`) every [`PROGRESS_REPORT_INTERVAL`]
//! bytes, at least every [`PROGRESS_REPORT_MAX_INTERVAL`] during long scans,
//! and once unconditionally after the loop.

use std::collections::HashMap;
use std::io::Cursor;
use std::time::{Duration, Instant};

use byteorder::{BigEndian, ReadBytesExt};

use crate::indexer::IndexResult;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::SegmentFilterBuilder;
use crate::{
    ClassDumpInfo, FieldDef, HprofThread, parse_load_class, parse_record_header, parse_stack_frame,
    parse_stack_trace, parse_start_thread, parse_string_record, read_id,
};

/// Minimum bytes between consecutive [`progress_fn`] calls inside the loop.
pub(crate) const PROGRESS_REPORT_INTERVAL: usize = 4 * 1024 * 1024;

/// Maximum time between consecutive [`progress_fn`] calls during the loop.
pub(crate) const PROGRESS_REPORT_MAX_INTERVAL: Duration = Duration::from_secs(1);

/// Maximum number of distinct warning strings kept in [`IndexResult::warnings`].
///
/// Warnings beyond this cap are counted but not stored to prevent memory
/// saturation on heavily-corrupted large heap dumps.
pub(crate) const MAX_WARNINGS: usize = 100;

fn maybe_report_progress(
    pos: usize,
    last_progress_bytes: &mut usize,
    last_progress_at: &mut Instant,
    progress_fn: &mut impl FnMut(u64),
) {
    let now = Instant::now();
    let enough_bytes = pos.saturating_sub(*last_progress_bytes) >= PROGRESS_REPORT_INTERVAL;
    let enough_time = now.duration_since(*last_progress_at) >= PROGRESS_REPORT_MAX_INTERVAL;
    if enough_bytes || enough_time {
        progress_fn(pos as u64);
        *last_progress_bytes = pos;
        *last_progress_at = now;
    }
}

/// Pushes `msg` to `warnings` unless the cap is reached.
///
/// Returns `true` if the warning was stored, `false` if suppressed.
/// When the cap is hit exactly, a sentinel "N more warnings suppressed" message
/// is NOT pushed here — the caller is responsible for appending a summary at
/// the end of the pass via [`push_suppressed_summary`].
fn push_warning(warnings: &mut Vec<String>, suppressed: &mut u64, msg: String) {
    if warnings.len() < MAX_WARNINGS {
        warnings.push(msg);
    } else {
        *suppressed += 1;
    }
}

/// If any warnings were suppressed, appends a single summary entry to `warnings`.
fn push_suppressed_summary(warnings: &mut Vec<String>, suppressed: u64) {
    if suppressed > 0 {
        warnings.push(format!(
            "... {suppressed} additional warning(s) suppressed \
             (only first {MAX_WARNINGS} shown)"
        ));
    }
}

/// Scans all records in `data` and returns a populated [`IndexResult`].
///
/// Non-fatal errors (corrupted payloads, size mismatches) are collected in
/// [`IndexResult::warnings`]. Fatal truncations (mid-header EOF, payload
/// window exceeds file) stop iteration and are also recorded as warnings.
///
/// `progress_fn` is called with the current byte offset after every
/// [`PROGRESS_REPORT_INTERVAL`] bytes processed, at least every
/// [`PROGRESS_REPORT_MAX_INTERVAL`] during long scans, and once after the loop
/// (reporting the final cursor position).
///
/// ## Parameters
/// - `data`: raw bytes starting at the first record (immediately after the
///   hprof file header).
/// - `id_size`: byte width of object IDs, taken from the hprof file header
///   (4 or 8).
/// - `progress_fn`: callback receiving the current cursor offset as `u64`.
pub(crate) fn run_first_pass(
    data: &[u8],
    id_size: u32,
    mut progress_fn: impl FnMut(u64),
) -> IndexResult {
    let mut cursor = Cursor::new(data);
    let mut last_progress_bytes: usize = 0;
    let mut last_progress_at = Instant::now();
    let mut seg_builder = SegmentFilterBuilder::new();
    let mut suppressed_warnings: u64 = 0;
    // Intermediate collection for GC roots; correlated AFTER main loop.
    let mut raw_frame_roots: Vec<(u64, u32, i32)> = Vec::new();
    let mut raw_thread_objects: Vec<(u64, u32)> = Vec::new();
    // Temporary map: object_id → records-section offset for ALL instances
    // and prim arrays. Filtered to thread-related objects after the loop.
    let mut all_offsets: HashMap<u64, u64> = HashMap::new();
    let mut result = IndexResult {
        index: PreciseIndex::new(),
        warnings: Vec::new(),
        records_attempted: 0,
        records_indexed: 0,
        segment_filters: Vec::new(),
        heap_record_ranges: Vec::new(),
    };

    while (cursor.position() as usize) < data.len() {
        let header = match parse_record_header(&mut cursor) {
            Ok(h) => h,
            Err(e) => {
                push_warning(
                    &mut result.warnings,
                    &mut suppressed_warnings,
                    format!("EOF mid-header: {e}"),
                );
                break;
            }
        };

        let payload_start = cursor.position() as usize;
        let payload_end = match payload_start.checked_add(header.length as usize) {
            Some(end) if end <= data.len() => end,
            Some(end) => {
                push_warning(
                    &mut result.warnings,
                    &mut suppressed_warnings,
                    format!(
                        "record 0x{:02X} payload end {end} exceeds file size {}",
                        header.tag,
                        data.len()
                    ),
                );
                break;
            }
            None => {
                push_warning(
                    &mut result.warnings,
                    &mut suppressed_warnings,
                    format!(
                        "record 0x{:02X} payload length overflow: {}",
                        header.tag, header.length
                    ),
                );
                break;
            }
        };

        if matches!(header.tag, 0x0C | 0x1C) {
            result
                .heap_record_ranges
                .push((payload_start as u64, header.length as u64));
            extract_heap_object_ids(
                &data[payload_start..payload_end],
                payload_start,
                id_size,
                &mut seg_builder,
                &mut result.index,
                &mut raw_frame_roots,
                &mut raw_thread_objects,
                &mut all_offsets,
                &mut result.warnings,
                &mut suppressed_warnings,
                &mut last_progress_bytes,
                &mut last_progress_at,
                &mut progress_fn,
            );
            cursor.set_position(payload_end as u64);
            let pos = cursor.position() as usize;
            maybe_report_progress(
                pos,
                &mut last_progress_bytes,
                &mut last_progress_at,
                &mut progress_fn,
            );
            continue;
        }

        if !matches!(header.tag, 0x01 | 0x02 | 0x04 | 0x05 | 0x06) {
            cursor.set_position(payload_end as u64);
            let pos = cursor.position() as usize;
            maybe_report_progress(
                pos,
                &mut last_progress_bytes,
                &mut last_progress_at,
                &mut progress_fn,
            );
            continue;
        }

        result.records_attempted += 1;

        let mut payload_cursor = Cursor::new(&data[payload_start..payload_end]);
        let inserted = match header.tag {
            0x01 => {
                let parsed = parse_string_record(&mut payload_cursor, id_size, header.length);
                let consumed = payload_cursor.position() as usize == header.length as usize;
                match (parsed, consumed) {
                    (Ok(s), true) => {
                        result.index.strings.insert(s.id, s);
                        true
                    }
                    (Ok(s), false) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!(
                                "record 0x{:02X} at offset {payload_start}: parsed OK but \
                                 consumed {} of {} bytes (extra bytes ignored)",
                                header.tag,
                                payload_cursor.position(),
                                header.length
                            ),
                        );
                        result.index.strings.insert(s.id, s);
                        true
                    }
                    (Err(e), _) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!("record 0x{:02X} at offset {payload_start}: {e}", header.tag),
                        );
                        false
                    }
                }
            }
            0x02 => {
                let parsed = parse_load_class(&mut payload_cursor, id_size);
                let consumed = payload_cursor.position() as usize == header.length as usize;
                match (parsed, consumed) {
                    (Ok(c), true) => {
                        // Class name resolved from strings snapshot at parse time.
                        // JVM-generated hprof files always emit STRING records before
                        // LOAD_CLASS records, so this is safe in practice. Files that
                        // violate this ordering will produce an empty class name here.
                        let java_name = result
                            .index
                            .strings
                            .get(&c.class_name_string_id)
                            .map(|s| s.value.replace('/', "."))
                            .unwrap_or_default();
                        result
                            .index
                            .class_names_by_id
                            .insert(c.class_object_id, java_name);
                        result.index.classes.insert(c.class_serial, c);
                        true
                    }
                    (Ok(c), false) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!(
                                "record 0x{:02X} at offset {payload_start}: parsed OK but \
                                 consumed {} of {} bytes (extra bytes ignored)",
                                header.tag,
                                payload_cursor.position(),
                                header.length
                            ),
                        );
                        let java_name = result
                            .index
                            .strings
                            .get(&c.class_name_string_id)
                            .map(|s| s.value.replace('/', "."))
                            .unwrap_or_default();
                        result
                            .index
                            .class_names_by_id
                            .insert(c.class_object_id, java_name);
                        result.index.classes.insert(c.class_serial, c);
                        true
                    }
                    (Err(e), _) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!("record 0x{:02X} at offset {payload_start}: {e}", header.tag),
                        );
                        false
                    }
                }
            }
            0x04 => {
                let parsed = parse_stack_frame(&mut payload_cursor, id_size);
                let consumed = payload_cursor.position() as usize == header.length as usize;
                match (parsed, consumed) {
                    (Ok(f), true) => {
                        result.index.stack_frames.insert(f.frame_id, f);
                        true
                    }
                    (Ok(f), false) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!(
                                "record 0x{:02X} at offset {payload_start}: parsed OK but \
                                 consumed {} of {} bytes (extra bytes ignored)",
                                header.tag,
                                payload_cursor.position(),
                                header.length
                            ),
                        );
                        result.index.stack_frames.insert(f.frame_id, f);
                        true
                    }
                    (Err(e), _) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!("record 0x{:02X} at offset {payload_start}: {e}", header.tag),
                        );
                        false
                    }
                }
            }
            0x05 => {
                let parsed = parse_stack_trace(&mut payload_cursor, id_size);
                let consumed = payload_cursor.position() as usize == header.length as usize;
                match (parsed, consumed) {
                    (Ok(t), true) => {
                        result.index.stack_traces.insert(t.stack_trace_serial, t);
                        true
                    }
                    (Ok(t), false) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!(
                                "record 0x{:02X} at offset {payload_start}: parsed OK but \
                                 consumed {} of {} bytes (extra bytes ignored)",
                                header.tag,
                                payload_cursor.position(),
                                header.length
                            ),
                        );
                        result.index.stack_traces.insert(t.stack_trace_serial, t);
                        true
                    }
                    (Err(e), _) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!("record 0x{:02X} at offset {payload_start}: {e}", header.tag),
                        );
                        false
                    }
                }
            }
            0x06 => {
                let parsed = parse_start_thread(&mut payload_cursor, id_size);
                let consumed = payload_cursor.position() as usize == header.length as usize;
                match (parsed, consumed) {
                    (Ok(t), true) => {
                        result.index.threads.insert(t.thread_serial, t);
                        true
                    }
                    (Ok(t), false) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!(
                                "record 0x{:02X} at offset {payload_start}: parsed OK but \
                                 consumed {} of {} bytes (extra bytes ignored)",
                                header.tag,
                                payload_cursor.position(),
                                header.length
                            ),
                        );
                        result.index.threads.insert(t.thread_serial, t);
                        true
                    }
                    (Err(e), _) => {
                        push_warning(
                            &mut result.warnings,
                            &mut suppressed_warnings,
                            format!("record 0x{:02X} at offset {payload_start}: {e}", header.tag),
                        );
                        false
                    }
                }
            }
            _ => unreachable!(),
        };

        if inserted {
            result.records_indexed += 1;
        }

        cursor.set_position(payload_end as u64);
        let pos = cursor.position() as usize;
        maybe_report_progress(
            pos,
            &mut last_progress_bytes,
            &mut last_progress_at,
            &mut progress_fn,
        );
    }

    // Synthesise thread entries from STACK_TRACE records when the file has no
    // START_THREAD (0x06) records (e.g. jvisualvm heap dumps). A STACK_TRACE
    // with thread_serial > 0 that is not already covered by a real thread entry
    // gets a synthetic placeholder with name_string_id = 0.
    // This MUST happen before frame root correlation so synthetic threads are
    // available for lookup.
    let traces: Vec<_> = result
        .index
        .stack_traces
        .values()
        .filter(|t| t.thread_serial > 0)
        .map(|t| (t.thread_serial, t.stack_trace_serial))
        .collect();
    for (thread_serial, stack_trace_serial) in traces {
        result
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

    // Populate thread_object_ids from ROOT_THREAD_OBJ sub-records and
    // update synthetic threads' object_id when a match exists.
    for (object_id, thread_serial) in raw_thread_objects {
        result
            .index
            .thread_object_ids
            .insert(thread_serial, object_id);
        if let Some(thread) = result.index.threads.get_mut(&thread_serial)
            && thread.object_id == 0
        {
            thread.object_id = object_id;
        }
    }

    // Correlate GC_ROOT_JAVA_FRAME roots with stack traces to populate java_frame_roots.
    // O(1) per root: thread_serial → stack_trace_serial → frame_ids[frame_number].
    for (object_id, thread_serial, frame_number) in raw_frame_roots {
        if frame_number < 0 {
            continue;
        }
        let Some(thread) = result.index.threads.get(&thread_serial) else {
            continue;
        };
        let Some(trace) = result.index.stack_traces.get(&thread.stack_trace_serial) else {
            continue;
        };
        let idx = frame_number as usize;
        let Some(&frame_id) = trace.frame_ids.get(idx) else {
            continue;
        };
        result
            .index
            .java_frame_roots
            .entry(frame_id)
            .or_default()
            .push(object_id);
    }

    // Cross-reference thread_object_ids with the full offset map to keep
    // only thread-related offsets. This is O(threads) — typically < 200.
    for &obj_id in result.index.thread_object_ids.values() {
        if let Some(&offset) = all_offsets.get(&obj_id) {
            result.index.instance_offsets.insert(obj_id, offset);
        }
    }

    // Follow transitive references so offset-based reads cover the
    // entire chain: Thread → name (String) → value (char[]/byte[]),
    // and Thread → holder (FieldHolder) for JDK 19+ threadStatus.
    resolve_thread_transitive_offsets(data, id_size, &mut result.index, &all_offsets);
    drop(all_offsets);

    progress_fn(cursor.position());
    push_suppressed_summary(&mut result.warnings, suppressed_warnings);
    let (filters, filter_warnings) = seg_builder.finish();
    result.segment_filters = filters;
    result.warnings.extend(filter_warnings);
    result
}

/// Advances the cursor by `n` bytes, returning `false` if out of bounds.
fn skip_n(cursor: &mut Cursor<&[u8]>, n: usize) -> bool {
    let pos = cursor.position() as usize;
    let new_pos = pos.saturating_add(n);
    if new_pos > cursor.get_ref().len() {
        return false;
    }
    cursor.set_position(new_pos as u64);
    true
}

/// Returns the byte size of a primitive hprof type code, or 0 for unknown.
fn primitive_element_size(type_byte: u8) -> usize {
    match type_byte {
        4 => 1,  // boolean
        5 => 2,  // char
        6 => 4,  // float
        7 => 8,  // double
        8 => 1,  // byte
        9 => 2,  // short
        10 => 4, // int
        11 => 8, // long
        _ => 0,
    }
}

/// Returns the byte size of a value with the given hprof type code.
pub(crate) fn value_byte_size(type_code: u8, id_size: u32) -> usize {
    match type_code {
        2 => id_size as usize,
        4 | 8 => 1,
        5 | 9 => 2,
        6 | 10 => 4,
        7 | 11 => 8,
        _ => 0,
    }
}

/// Parses a `CLASS_DUMP` sub-record body (after the sub-tag byte), returning
/// `None` on any read failure.
pub(crate) fn parse_class_dump(cursor: &mut Cursor<&[u8]>, id_size: u32) -> Option<ClassDumpInfo> {
    let class_object_id = read_id(cursor, id_size).ok()?;
    let _stack_trace_serial = cursor.read_u32::<BigEndian>().ok()?;
    let super_class_id = read_id(cursor, id_size).ok()?;
    // skip classloader_id, signers_id, protection_domain_id, reserved1, reserved2
    if !skip_n(cursor, 5 * id_size as usize) {
        return None;
    }
    let instance_size = cursor.read_u32::<BigEndian>().ok()?;

    // Skip constant pool
    let cp_count = cursor.read_u16::<BigEndian>().ok()?;
    for _ in 0..cp_count {
        let _index = cursor.read_u16::<BigEndian>().ok()?;
        let cp_type = cursor.read_u8().ok()?;
        let val_size = value_byte_size(cp_type, id_size);
        if !skip_n(cursor, val_size) {
            return None;
        }
    }

    // Skip static fields
    let static_count = cursor.read_u16::<BigEndian>().ok()?;
    for _ in 0..static_count {
        if !skip_n(cursor, id_size as usize) {
            return None;
        }
        let field_type = cursor.read_u8().ok()?;
        let val_size = value_byte_size(field_type, id_size);
        if !skip_n(cursor, val_size) {
            return None;
        }
    }

    // Parse instance fields
    let field_count = cursor.read_u16::<BigEndian>().ok()?;
    let mut instance_fields = Vec::with_capacity(field_count as usize);
    for _ in 0..field_count {
        let name_string_id = read_id(cursor, id_size).ok()?;
        let field_type = cursor.read_u8().ok()?;
        instance_fields.push(FieldDef {
            name_string_id,
            field_type,
        });
    }

    Some(ClassDumpInfo {
        class_object_id,
        super_class_id,
        instance_size,
        instance_fields,
    })
}

/// Extracts object IDs from a `HEAP_DUMP` (0x0C) or `HEAP_DUMP_SEGMENT`
/// (0x1C) payload, registering each ID with `builder` at `data_offset`.
///
/// `raw_frame_roots` is populated with `(object_id, thread_serial, frame_number)`
/// tuples for `GC_ROOT_JAVA_FRAME` (sub-tag `0x03`) sub-records, to be
/// correlated with stack traces after the main loop.
///
/// `progress_fn` is called periodically via [`maybe_report_progress`] to
/// prevent the progress bar from freezing on large heap segments. The absolute
/// position reported is `data_offset + sub-record cursor offset`.
///
/// All read errors silently break the sub-record loop (tolerant parsing).
#[allow(clippy::too_many_arguments)]
fn extract_heap_object_ids(
    payload: &[u8],
    data_offset: usize,
    id_size: u32,
    builder: &mut SegmentFilterBuilder,
    index: &mut PreciseIndex,
    raw_frame_roots: &mut Vec<(u64, u32, i32)>,
    raw_thread_objects: &mut Vec<(u64, u32)>,
    all_offsets: &mut HashMap<u64, u64>,
    warnings: &mut Vec<String>,
    suppressed_warnings: &mut u64,
    last_progress_bytes: &mut usize,
    last_progress_at: &mut Instant,
    progress_fn: &mut impl FnMut(u64),
) {
    let mut cursor = Cursor::new(payload);

    while let Ok(sub_tag) = cursor.read_u8() {
        let sub_record_start = data_offset + cursor.position() as usize;

        let ok = match sub_tag {
            0x01 => skip_n(&mut cursor, id_size as usize),
            0x02 => skip_n(&mut cursor, 2 * id_size as usize),
            0x03 => {
                let Ok(object_id) = read_id(&mut cursor, id_size) else {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        format!(
                            "GC_ROOT_JAVA_FRAME at offset {sub_record_start}: truncated object_id"
                        ),
                    );
                    break;
                };
                let Ok(thread_serial) = cursor.read_u32::<BigEndian>() else {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        format!(
                            "GC_ROOT_JAVA_FRAME at offset {sub_record_start}: truncated thread_serial"
                        ),
                    );
                    break;
                };
                let Ok(frame_number) = cursor.read_i32::<BigEndian>() else {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        format!(
                            "GC_ROOT_JAVA_FRAME at offset {sub_record_start}: truncated frame_number"
                        ),
                    );
                    break;
                };
                raw_frame_roots.push((object_id, thread_serial, frame_number));
                true
            }
            0x04 => skip_n(&mut cursor, id_size as usize + 8),
            0x05 => skip_n(&mut cursor, id_size as usize + 4),
            0x06 => skip_n(&mut cursor, id_size as usize),
            0x07 => skip_n(&mut cursor, id_size as usize + 4),
            0x08 => {
                let Ok(object_id) = read_id(&mut cursor, id_size) else {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        format!(
                            "ROOT_THREAD_OBJ at offset \
                             {sub_record_start}: truncated object_id"
                        ),
                    );
                    break;
                };
                let Ok(thread_serial) = cursor.read_u32::<BigEndian>() else {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        format!(
                            "ROOT_THREAD_OBJ at offset \
                             {sub_record_start}: truncated \
                             thread_serial"
                        ),
                    );
                    break;
                };
                let Ok(_stack_trace_serial) = cursor.read_u32::<BigEndian>() else {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        format!(
                            "ROOT_THREAD_OBJ at offset \
                             {sub_record_start}: truncated \
                             stack_trace_serial"
                        ),
                    );
                    break;
                };
                raw_thread_objects.push((object_id, thread_serial));
                true
            }
            0x09 => skip_n(&mut cursor, id_size as usize + 8),

            0x20 => match parse_class_dump(&mut cursor, id_size) {
                Some(info) => {
                    index.class_dumps.insert(info.class_object_id, info);
                    true
                }
                None => {
                    push_warning(
                        warnings,
                        suppressed_warnings,
                        "truncated CLASS_DUMP sub-record — skipping".to_string(),
                    );
                    false
                }
            },

            0x21 => {
                let Ok(obj_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                builder.add(sub_record_start, obj_id);
                all_offsets.insert(obj_id, (sub_record_start - 1) as u64);
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

            0x22 => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                builder.add(sub_record_start, arr_id);
                // OBJECT_ARRAY_DUMP offsets are not needed for
                // thread resolution — skip to save memory.
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

            0x23 => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                builder.add(sub_record_start, arr_id);
                all_offsets.insert(arr_id, (sub_record_start - 1) as u64);
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

            _ => break,
        };

        if !ok {
            break;
        }

        let abs_pos = data_offset + cursor.position() as usize;
        maybe_report_progress(abs_pos, last_progress_bytes, last_progress_at, progress_fn);
    }
}

/// Reads an `INSTANCE_DUMP` sub-record at `offset` in `data`,
/// returning `(class_object_id, field_data_slice)`.
///
/// `offset` is relative to the records section and must point to
/// the sub-tag byte (0x21).
fn read_raw_instance_at(data: &[u8], offset: u64, id_size: u32) -> Option<(u64, &[u8])> {
    let start = offset as usize;
    if start >= data.len() {
        return None;
    }
    let slice = &data[start..];
    let mut cursor = Cursor::new(slice);
    if cursor.read_u8().ok()? != 0x21 {
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

/// Extracts `ObjectRef` (type 2) field values by name from raw
/// instance field data.
///
/// Walks the class hierarchy in leaf-first order (HotSpot layout)
/// and returns `(field_name, referenced_object_id)` for each match.
fn extract_obj_refs(
    field_data: &[u8],
    class_object_id: u64,
    target_names: &[&str],
    index: &PreciseIndex,
    id_size: u32,
) -> Vec<(String, u64)> {
    let mut chain: Vec<u64> = Vec::new();
    let mut visited = std::collections::HashSet::new();
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
            if field.field_type == 2 {
                let Ok(ref_id) = read_id(&mut cursor, id_size) else {
                    return results;
                };
                if ref_id != 0 {
                    let name = index
                        .strings
                        .get(&field.name_string_id)
                        .map(|s| s.value.as_str())
                        .unwrap_or("");
                    if target_names.contains(&name) {
                        results.push((name.to_string(), ref_id));
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

/// Populates `instance_offsets` with transitive references from
/// thread objects: `Thread.name` → `String.value` → `char[]/byte[]`,
/// and `Thread.holder` → `FieldHolder` (JDK 19+).
fn resolve_thread_transitive_offsets(
    data: &[u8],
    id_size: u32,
    index: &mut PreciseIndex,
    all_offsets: &HashMap<u64, u64>,
) {
    let thread_offsets: Vec<u64> = index.instance_offsets.values().copied().collect();

    let mut extra: Vec<(u64, u64)> = Vec::new();
    let mut string_ids: Vec<(u64, u64)> = Vec::new();

    for offset in thread_offsets {
        let Some((class_id, field_data)) = read_raw_instance_at(data, offset, id_size) else {
            continue;
        };
        let refs = extract_obj_refs(field_data, class_id, &["name", "holder"], index, id_size);
        for (name, ref_id) in &refs {
            if let Some(&off) = all_offsets.get(ref_id) {
                extra.push((*ref_id, off));
                if name == "name" {
                    string_ids.push((*ref_id, off));
                }
            }
        }
    }

    // Follow String.value → char[]/byte[] array
    for (_, str_offset) in string_ids {
        let Some((str_class_id, str_data)) = read_raw_instance_at(data, str_offset, id_size) else {
            continue;
        };
        let refs = extract_obj_refs(str_data, str_class_id, &["value"], index, id_size);
        for (_, ref_id) in &refs {
            if let Some(&off) = all_offsets.get(ref_id) {
                extra.push((*ref_id, off));
            }
        }
    }

    for (id, off) in extra {
        index.instance_offsets.insert(id, off);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{BigEndian, WriteBytesExt};

    fn make_record(tag: u8, payload: &[u8]) -> Vec<u8> {
        let mut rec = Vec::new();
        rec.write_u8(tag).unwrap();
        rec.write_u32::<BigEndian>(0).unwrap(); // time_offset
        rec.write_u32::<BigEndian>(payload.len() as u32).unwrap();
        rec.extend_from_slice(payload);
        rec
    }

    fn make_record_with_declared_length(tag: u8, declared_length: u32, payload: &[u8]) -> Vec<u8> {
        let mut rec = Vec::new();
        rec.write_u8(tag).unwrap();
        rec.write_u32::<BigEndian>(0).unwrap(); // time_offset
        rec.write_u32::<BigEndian>(declared_length).unwrap();
        rec.extend_from_slice(payload);
        rec
    }

    fn make_string_payload(id: u64, content: &str, id_size: u32) -> Vec<u8> {
        let mut p = Vec::new();
        if id_size == 8 {
            p.write_u64::<BigEndian>(id).unwrap();
        } else {
            p.write_u32::<BigEndian>(id as u32).unwrap();
        }
        p.extend_from_slice(content.as_bytes());
        p
    }

    fn make_load_class_payload(
        class_serial: u32,
        class_object_id: u64,
        stack_trace_serial: u32,
        class_name_string_id: u64,
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u32::<BigEndian>(class_serial).unwrap();
        if id_size == 8 {
            p.write_u64::<BigEndian>(class_object_id).unwrap();
        } else {
            p.write_u32::<BigEndian>(class_object_id as u32).unwrap();
        }
        p.write_u32::<BigEndian>(stack_trace_serial).unwrap();
        if id_size == 8 {
            p.write_u64::<BigEndian>(class_name_string_id).unwrap();
        } else {
            p.write_u32::<BigEndian>(class_name_string_id as u32)
                .unwrap();
        }
        p
    }

    fn make_start_thread_payload(
        thread_serial: u32,
        object_id: u64,
        stack_trace_serial: u32,
        name_string_id: u64,
        group_name_string_id: u64,
        group_parent_name_string_id: u64,
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u32::<BigEndian>(thread_serial).unwrap();
        if id_size == 8 {
            p.write_u64::<BigEndian>(object_id).unwrap();
            p.write_u32::<BigEndian>(stack_trace_serial).unwrap();
            p.write_u64::<BigEndian>(name_string_id).unwrap();
            p.write_u64::<BigEndian>(group_name_string_id).unwrap();
            p.write_u64::<BigEndian>(group_parent_name_string_id)
                .unwrap();
        } else {
            p.write_u32::<BigEndian>(object_id as u32).unwrap();
            p.write_u32::<BigEndian>(stack_trace_serial).unwrap();
            p.write_u32::<BigEndian>(name_string_id as u32).unwrap();
            p.write_u32::<BigEndian>(group_name_string_id as u32)
                .unwrap();
            p.write_u32::<BigEndian>(group_parent_name_string_id as u32)
                .unwrap();
        }
        p
    }

    fn make_stack_frame_payload(
        frame_id: u64,
        method_name_id: u64,
        method_sig_id: u64,
        source_file_id: u64,
        class_serial: u32,
        line_number: i32,
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        if id_size == 8 {
            p.write_u64::<BigEndian>(frame_id).unwrap();
            p.write_u64::<BigEndian>(method_name_id).unwrap();
            p.write_u64::<BigEndian>(method_sig_id).unwrap();
            p.write_u64::<BigEndian>(source_file_id).unwrap();
        } else {
            p.write_u32::<BigEndian>(frame_id as u32).unwrap();
            p.write_u32::<BigEndian>(method_name_id as u32).unwrap();
            p.write_u32::<BigEndian>(method_sig_id as u32).unwrap();
            p.write_u32::<BigEndian>(source_file_id as u32).unwrap();
        }
        p.write_u32::<BigEndian>(class_serial).unwrap();
        p.write_i32::<BigEndian>(line_number).unwrap();
        p
    }

    fn make_stack_trace_payload(
        stack_trace_serial: u32,
        thread_serial: u32,
        frame_ids: &[u64],
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u32::<BigEndian>(stack_trace_serial).unwrap();
        p.write_u32::<BigEndian>(thread_serial).unwrap();
        p.write_u32::<BigEndian>(frame_ids.len() as u32).unwrap();
        for &fid in frame_ids {
            if id_size == 8 {
                p.write_u64::<BigEndian>(fid).unwrap();
            } else {
                p.write_u32::<BigEndian>(fid as u32).unwrap();
            }
        }
        p
    }

    // --- Segment filter raw tests (no test-utils feature) ---

    fn make_instance_sub(obj_id: u64, class_id: u64) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x21).unwrap(); // INSTANCE_DUMP
        p.write_u64::<BigEndian>(obj_id).unwrap();
        p.write_u32::<BigEndian>(0).unwrap(); // stack_trace_serial
        p.write_u64::<BigEndian>(class_id).unwrap();
        p.write_u32::<BigEndian>(0).unwrap(); // num_bytes = 0
        p
    }

    // --- Progress callback tests ---

    #[test]
    fn progress_callback_called_once_with_zero_for_empty_data() {
        let mut calls: Vec<u64> = Vec::new();
        run_first_pass(&[], 8, |bytes| calls.push(bytes));
        assert_eq!(
            calls,
            vec![0],
            "empty data must still trigger a final callback with offset 0"
        );
    }

    #[test]
    fn progress_callback_called_once_for_single_record_with_final_position() {
        let payload = make_string_payload(1, "hello", 8);
        let data = make_record(0x01, &payload);
        let mut calls: Vec<u64> = Vec::new();
        run_first_pass(&data, 8, |bytes| calls.push(bytes));
        assert_eq!(
            calls.len(),
            1,
            "single record must trigger exactly one callback"
        );
        assert_eq!(
            calls[0],
            data.len() as u64,
            "final callback must report full data length"
        );
    }

    #[test]
    fn progress_callback_fires_more_than_once_for_large_data() {
        use byteorder::WriteBytesExt;
        // Build 5 MiB of unknown-tag (0xFF) records, each 9 bytes (tag + 4 time + 4 len=0).
        const FIVE_MIB: usize = 5 * 1024 * 1024;
        let mut data: Vec<u8> = Vec::with_capacity(FIVE_MIB + 16);
        while data.len() < FIVE_MIB {
            data.write_u8(0xFF).unwrap();
            data.write_u32::<BigEndian>(0).unwrap();
            data.write_u32::<BigEndian>(0).unwrap();
        }
        let mut values: Vec<u64> = Vec::new();
        run_first_pass(&data, 8, |bytes| values.push(bytes));
        assert!(
            values.len() > 1,
            "large data must trigger more than one callback"
        );
        for w in values.windows(2) {
            assert!(
                w[1] >= w[0],
                "callback values must be monotonically increasing"
            );
        }
    }

    #[test]
    fn progress_callback_reports_partial_position_for_truncated_data() {
        use byteorder::WriteBytesExt;
        const DECLARED_PAYLOAD: u32 = 1000;
        // Build: 9-byte header declaring 1000-byte payload + only 50 actual payload bytes.
        // Cursor stops after the header (position 9) when the payload window check fails.
        let mut data: Vec<u8> = Vec::new();
        data.write_u8(0x01).unwrap();
        data.write_u32::<BigEndian>(0).unwrap();
        data.write_u32::<BigEndian>(DECLARED_PAYLOAD).unwrap();
        data.extend_from_slice(&[0u8; 50]); // only 50 of the declared 1000 bytes
        // data.len() = 59; declared payload end = 1009.
        let mut final_pos: Option<u64> = None;
        run_first_pass(&data, 8, |bytes| final_pos = Some(bytes));
        let reported = final_pos.expect("callback must be called at least once");
        let declared_end = 9u64 + DECLARED_PAYLOAD as u64;
        // Cursor must stop before the declared payload end (non-trivial bound).
        assert!(
            reported < declared_end,
            "cursor ({reported}) must be before declared end ({declared_end})"
        );
        // Cursor must remain within actual data (confirms no over-run).
        assert!(
            reported <= data.len() as u64,
            "cursor ({reported}) must not exceed actual data length ({})",
            data.len()
        );
    }

    #[test]
    fn heap_dump_0x0c_record_produces_segment_filter() {
        let obj_id: u64 = 0x1234;
        let data = make_record(0x0C, &make_instance_sub(obj_id, 100));
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(
            result.segment_filters.len(),
            1,
            "expected one filter from 0x0C record"
        );
        assert!(result.segment_filters[0].contains(obj_id));
    }

    #[test]
    fn truncated_heap_dump_segment_mid_sub_record_partial_filter_built() {
        let obj_id1: u64 = 0xAAAA_BBBB;
        // Build payload with one complete sub-record + one truncated sub-record
        let mut payload = make_instance_sub(obj_id1, 100);
        payload.write_u8(0x21).unwrap(); // sub-tag of truncated record
        payload.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // only 4 of 8 id bytes
        let data = make_record(0x1C, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        // First sub-record was fully parsed → filter exists
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id1));
    }

    #[test]
    fn gc_root_before_instance_dump_cursor_advances_correctly() {
        let gc_root_id: u64 = 0x5555;
        let obj_id: u64 = 0x6666;
        let mut payload = Vec::new();
        // GC_ROOT_UNKNOWN (0x01): object_id only
        payload.write_u8(0x01).unwrap();
        payload.write_u64::<BigEndian>(gc_root_id).unwrap();
        // INSTANCE_DUMP (0x21)
        payload.extend_from_slice(&make_instance_sub(obj_id, 200));
        let data = make_record(0x1C, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id));
    }

    #[test]
    fn truncated_gc_root_java_frame_sub_record_adds_warning() {
        let mut payload = Vec::new();
        payload.write_u8(0x03).unwrap(); // GC_ROOT_JAVA_FRAME
        payload.write_u64::<BigEndian>(0xABCD).unwrap();
        payload.extend_from_slice(&[0x00, 0x01]); // partial thread_serial (needs 4 bytes)
        let data = make_record(0x1C, &payload);

        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("GC_ROOT_JAVA_FRAME")),
            "expected GC_ROOT_JAVA_FRAME warning, got {:?}",
            result.warnings
        );
    }

    #[test]
    fn class_dump_before_instance_dump_cursor_advances_correctly() {
        let class_id: u64 = 0x1111;
        let obj_id: u64 = 0x2222;
        let mut payload = Vec::new();
        // CLASS_DUMP (0x20): class_id(8) + stack_serial(4) + 6*id(8) + instance_size(4)
        //                    + cp_count(2)=0 + static_count(2)=0 + instance_count(2)=0
        payload.write_u8(0x20).unwrap();
        payload.write_u64::<BigEndian>(class_id).unwrap();
        payload.write_u32::<BigEndian>(0).unwrap(); // stack_serial
        for _ in 0..6u8 {
            payload.write_u64::<BigEndian>(0).unwrap(); // super+loader+signers+prot+res1+res2
        }
        payload.write_u32::<BigEndian>(16).unwrap(); // instance_size
        payload.write_u16::<BigEndian>(0).unwrap(); // cp_count
        payload.write_u16::<BigEndian>(0).unwrap(); // static_fields
        payload.write_u16::<BigEndian>(0).unwrap(); // instance_fields
        // INSTANCE_DUMP (0x21)
        payload.extend_from_slice(&make_instance_sub(obj_id, class_id));
        let data = make_record(0x1C, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id));
    }

    // --- NEW RED tests for tolerant behaviour ---

    #[test]
    fn eof_mid_header_produces_warning_and_no_records() {
        // Only 1 byte — cannot form a 9-byte record header.
        let data = &[0x01u8];
        let result = run_first_pass(data, 8, |_| {});
        assert!(
            !result.warnings.is_empty(),
            "expected EOF mid-header warning"
        );
        assert_eq!(result.records_indexed, 0);
        assert_eq!(result.records_attempted, 0);
    }

    #[test]
    fn payload_end_exceeds_data_produces_warning_and_stops() {
        // One valid string record followed by a record whose declared payload
        // extends beyond the end of the data slice.
        let str_payload = make_string_payload(1, "ok", 8);
        let mut data = make_record(0x01, &str_payload);

        // Append a record header with length=1000 but no actual payload bytes.
        data.push(0x01);
        data.write_u32::<BigEndian>(0).unwrap();
        data.write_u32::<BigEndian>(1000).unwrap();

        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            !result.warnings.is_empty(),
            "expected out-of-bounds warning"
        );
        // The first (valid) record must still be indexed.
        assert_eq!(result.records_indexed, 1);
    }

    #[test]
    fn corrupted_payload_within_window_produces_warning_and_continues() {
        // STACK_TRACE that claims 1 frame but provides zero frame ID bytes.
        // declared_length = 12 (serial+thread_serial+num_frames), so the window
        // is exactly 12 bytes — cursor advances correctly after the parse error.
        // The next STRING record must still be indexed.
        let mut st_payload = Vec::new();
        st_payload.write_u32::<BigEndian>(3).unwrap(); // stack_trace_serial
        st_payload.write_u32::<BigEndian>(1).unwrap(); // thread_serial
        st_payload.write_u32::<BigEndian>(1).unwrap(); // num_frames=1 — no IDs follow
        let mut data = make_record(0x05, &st_payload); // declared_length == 12

        let str_payload = make_string_payload(7, "next", 8);
        data.extend(make_record(0x01, &str_payload));

        let result = run_first_pass(&data, 8, |_| {});
        assert!(!result.warnings.is_empty(), "expected parse warning");
        assert_eq!(
            result.records_indexed, 1,
            "next string record must be indexed"
        );
        assert!(result.index.strings.contains_key(&7));
    }

    #[test]
    fn two_records_first_corrupt_second_valid_gives_one_indexed() {
        // STACK_TRACE that claims 1 frame but provides zero frame ID bytes.
        // Window is exactly 12 bytes — cursor lands at the start of the next record.
        let mut st_payload = Vec::new();
        st_payload.write_u32::<BigEndian>(3).unwrap();
        st_payload.write_u32::<BigEndian>(1).unwrap();
        st_payload.write_u32::<BigEndian>(1).unwrap(); // num_frames=1, no IDs
        let mut data = make_record(0x05, &st_payload);

        let str_payload = make_string_payload(42, "good", 8);
        data.extend(make_record(0x01, &str_payload));

        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.records_indexed, 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn valid_single_record_no_warnings() {
        let payload = make_string_payload(7, "main", 8);
        let data = make_record(0x01, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_attempted, 1);
        assert_eq!(result.records_indexed, 1);
    }

    #[test]
    fn empty_data_no_warnings_no_records() {
        let result = run_first_pass(&[], 8, |_| {});
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_indexed, 0);
        assert_eq!(result.records_attempted, 0);
    }

    // --- Updated tests (previously checked for Err, now check warnings) ---

    #[test]
    fn too_short_declared_length_stops_with_warning() {
        // Declared length is 4, but LOAD_CLASS needs more bytes.
        let payload = make_load_class_payload(1, 100, 0, 200, 8);
        let data = make_record_with_declared_length(0x02, 4, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            !result.warnings.is_empty(),
            "expected a warning for short declared length"
        );
    }

    #[test]
    fn extra_payload_bytes_produces_warning_and_continues() {
        // STACK_TRACE with one frame plus trailing junk bytes in the same record.
        // The size mismatch should produce a warning; the record is still indexed,
        // and a subsequent record is also indexed.
        let mut payload = make_stack_trace_payload(3, 1, &[10], 8);
        payload.extend_from_slice(&[0xEE; 8]);
        let mut data = make_record(0x05, &payload);
        // Append a valid STRING record that must be indexed despite the mismatch.
        let str_payload = make_string_payload(99, "after", 8);
        data.extend(make_record(0x01, &str_payload));

        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            !result.warnings.is_empty(),
            "expected size-mismatch warning"
        );
        assert!(
            result.index.stack_traces.contains_key(&3),
            "stack trace with extra bytes must still be indexed"
        );
        assert!(
            result.index.strings.contains_key(&99),
            "subsequent record must be indexed"
        );
    }

    #[test]
    fn start_thread_with_extra_bytes_is_indexed_with_warning() {
        // START_THREAD payload with 4 trailing junk bytes appended.
        let mut payload = make_start_thread_payload(7, 0xBEEF, 5, 42, 0, 0, 8);
        payload.extend_from_slice(&[0xFF; 4]);
        let data = make_record(0x06, &payload);

        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            !result.warnings.is_empty(),
            "expected warning for extra bytes"
        );
        assert!(
            result.index.threads.contains_key(&7),
            "thread with extra bytes must still be indexed"
        );
        assert_eq!(result.records_indexed, 1);
    }

    #[test]
    fn stack_trace_without_start_thread_synthesises_thread_entry() {
        // A STACK_TRACE record with thread_serial=3 and no START_THREAD record.
        // After the pass, a synthetic HprofThread must exist for serial 3.
        let payload = make_stack_trace_payload(5, 3, &[10], 8);
        let data = make_record(0x05, &payload);

        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            result.index.threads.contains_key(&3),
            "synthetic thread must be created from STACK_TRACE thread_serial"
        );
        let t = &result.index.threads[&3];
        assert_eq!(t.thread_serial, 3);
        assert_eq!(t.stack_trace_serial, 5);
        assert_eq!(t.name_string_id, 0, "synthetic thread has no name string");
    }

    #[test]
    fn stack_trace_thread_serial_zero_does_not_synthesise_thread() {
        // STACK_TRACE with thread_serial=0 is a system trace; must not become a thread.
        let payload = make_stack_trace_payload(1, 0, &[], 8);
        let data = make_record(0x05, &payload);

        let result = run_first_pass(&data, 8, |_| {});
        assert!(
            !result.index.threads.contains_key(&0),
            "thread_serial=0 must not be synthesised"
        );
    }

    #[test]
    fn start_thread_record_takes_priority_over_synthetic_thread() {
        // A file with both a START_THREAD (thread_serial=1) and a STACK_TRACE
        // (thread_serial=1). The real START_THREAD name_string_id must be kept.
        let str_payload = make_string_payload(10, "main", 8);
        let thread_payload = make_start_thread_payload(1, 100, 7, 10, 0, 0, 8);
        let trace_payload = make_stack_trace_payload(7, 1, &[50], 8);
        let mut data = make_record(0x01, &str_payload);
        data.extend(make_record(0x06, &thread_payload));
        data.extend(make_record(0x05, &trace_payload));

        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.threads.len(), 1);
        let t = &result.index.threads[&1];
        assert_eq!(
            t.name_string_id, 10,
            "real START_THREAD must not be overwritten"
        );
    }

    #[test]
    fn string_declared_length_smaller_than_id_size_stops_with_warning() {
        // Declared payload length is invalid for id_size=8.
        let payload = 1u64.to_be_bytes();
        let data = make_record_with_declared_length(0x01, 4, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert!(!result.warnings.is_empty(), "expected truncation warning");
    }

    // --- Unchanged happy-path tests (updated call sites) ---

    #[test]
    fn empty_data_returns_empty_index() {
        let result = run_first_pass(&[], 8, |_| {});
        assert!(result.index.strings.is_empty());
        assert!(result.index.classes.is_empty());
        assert!(result.index.threads.is_empty());
        assert!(result.index.stack_frames.is_empty());
        assert!(result.index.stack_traces.is_empty());
    }

    #[test]
    fn single_string_record_indexed() {
        let payload = make_string_payload(7, "main", 8);
        let data = make_record(0x01, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(result.index.strings[&7].value, "main");
    }

    #[test]
    fn single_load_class_indexed() {
        let payload = make_load_class_payload(1, 100, 0, 200, 8);
        let data = make_record(0x02, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.classes[&1].class_object_id, 100);
    }

    #[test]
    fn load_class_populates_class_names_by_id_with_dot_notation() {
        // String id=5 = "java/util/HashMap", LOAD_CLASS with class_object_id=100, name_string_id=5
        let str_payload = make_string_payload(5, "java/util/HashMap", 8);
        let cls_payload = make_load_class_payload(1, 100, 0, 5, 8);
        let mut data = Vec::new();
        data.extend(make_record(0x01, &str_payload));
        data.extend(make_record(0x02, &cls_payload));
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(
            result.index.class_names_by_id.get(&100),
            Some(&"java.util.HashMap".to_string())
        );
    }

    #[test]
    fn load_class_with_unknown_string_id_inserts_empty_name() {
        // LOAD_CLASS with name_string_id=99 but no string record for 99
        let payload = make_load_class_payload(1, 42, 0, 99, 8);
        let data = make_record(0x02, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        // class_names_by_id entry exists but is empty
        assert_eq!(
            result.index.class_names_by_id.get(&42),
            Some(&String::new())
        );
    }

    #[test]
    fn single_start_thread_indexed() {
        let payload = make_start_thread_payload(2, 300, 0, 1, 2, 3, 8);
        let data = make_record(0x06, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.threads.len(), 1);
        assert_eq!(result.index.threads[&2].object_id, 300);
    }

    #[test]
    fn single_stack_frame_indexed() {
        let payload = make_stack_frame_payload(10, 1, 2, 3, 5, 42, 8);
        let data = make_record(0x04, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.stack_frames.len(), 1);
        assert_eq!(result.index.stack_frames[&10].line_number, 42);
    }

    #[test]
    fn single_stack_trace_indexed() {
        let payload = make_stack_trace_payload(3, 1, &[10, 20], 8);
        let data = make_record(0x05, &payload);
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.stack_traces.len(), 1);
        assert_eq!(result.index.stack_traces[&3].frame_ids, vec![10u64, 20u64]);
    }

    #[test]
    fn unknown_tag_skipped_index_empty() {
        let data = make_record(0xFF, &[0u8; 4]);
        let result = run_first_pass(&data, 8, |_| {});
        assert!(result.index.strings.is_empty());
        assert!(result.index.classes.is_empty());
        assert!(result.index.threads.is_empty());
        assert!(result.index.stack_frames.is_empty());
        assert!(result.index.stack_traces.is_empty());
    }

    #[test]
    fn three_string_records_all_indexed() {
        let mut data = Vec::new();
        for (id, s) in [(1u64, "a"), (2, "b"), (3, "c")] {
            let payload = make_string_payload(id, s, 8);
            data.extend(make_record(0x01, &payload));
        }
        let result = run_first_pass(&data, 8, |_| {});
        assert_eq!(result.index.strings.len(), 3);
        assert_eq!(result.index.strings[&1].value, "a");
        assert_eq!(result.index.strings[&2].value, "b");
        assert_eq!(result.index.strings[&3].value, "c");
    }

    #[test]
    fn id_size_4_string_and_class_both_indexed() {
        let mut data = Vec::new();
        let str_payload = make_string_payload(5, "foo", 4);
        data.extend(make_record(0x01, &str_payload));
        let cls_payload = make_load_class_payload(1, 50, 0, 5, 4);
        data.extend(make_record(0x02, &cls_payload));
        let result = run_first_pass(&data, 4, |_| {});
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(result.index.strings[&5].value, "foo");
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.classes[&1].class_object_id, 50);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    // --- Segment filter tests ---

    #[test]
    fn heap_dump_segment_instance_dump_produces_one_filter_containing_id() {
        let object_id = 0xABCD_u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(object_id, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(result.segment_filters.len(), 1);
        assert_eq!(
            result.segment_filters[0].segment_index, 0,
            "AC#3: small file must be segment 0"
        );
        assert!(result.segment_filters[0].contains(object_id));
    }

    #[test]
    fn truncated_second_heap_dump_segment_partial_filter_has_first_id() {
        let id1 = 0x1111_u64;
        let id2 = 0x2222_u64;
        let full = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(id1, 0, 100, &[])
            .add_instance(id2, 0, 100, &[])
            .build();
        // Truncate 5 bytes from end: second record's outer header is present but
        // payload window exceeds available bytes → outer-level tolerance kicks in.
        let truncated = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(id1, 0, 100, &[])
            .add_instance(id2, 0, 100, &[])
            .truncate_at(full.len() - 5)
            .build();
        let start = advance_past_header(&truncated);
        let result = run_first_pass(&truncated[start..], 8, |_| {});
        // id1 was in a complete record → filter must exist and contain id1
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(id1));
    }

    #[test]
    fn no_heap_dump_records_produces_empty_segment_filters() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "hello")
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert!(result.segment_filters.is_empty());
    }

    #[test]
    fn two_instances_same_small_file_one_segment_containing_both() {
        let id1 = 111_u64;
        let id2 = 222_u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(id1, 0, 100, &[])
            .add_instance(id2, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(result.segment_filters.len(), 1);
        assert_eq!(result.segment_filters[0].segment_index, 0);
        assert!(result.segment_filters[0].contains(id1));
        assert!(result.segment_filters[0].contains(id2));
    }

    #[test]
    fn object_array_in_heap_dump_segment_filter_contains_array_id() {
        let array_id = 0xBEEF_u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_object_array(array_id, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(array_id));
    }

    #[test]
    fn prim_array_in_heap_dump_segment_filter_contains_array_id() {
        let array_id = 0xCAFE_u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            // element_type=8 (byte), 2 byte elements
            .add_prim_array(array_id, 0, 2, 8, &[0x01, 0x02])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(array_id));
    }

    // --- Existing tests ---

    #[test]
    fn full_index_round_trip() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .add_string(2, "java.lang.Thread")
            .add_class(10, 200, 0, 2)
            .add_thread(1, 300, 0, 1, 0, 0)
            .add_thread(2, 400, 0, 1, 0, 0)
            .add_stack_frame(50, 1, 2, 1, 10, 42)
            .add_stack_trace(100, 1, &[50])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_indexed, result.records_attempted);
        assert_eq!(result.index.strings.len(), 2);
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.threads.len(), 2);
        assert_eq!(result.index.stack_frames.len(), 1);
        assert_eq!(result.index.stack_traces.len(), 1);
        assert_eq!(result.index.strings[&1].value, "main");
        assert_eq!(result.index.classes[&10].class_object_id, 200);
        assert_eq!(result.index.threads[&1].object_id, 300);
        assert_eq!(result.index.threads[&2].object_id, 400);
        assert_eq!(result.index.stack_frames[&50].line_number, 42);
        assert_eq!(result.index.stack_traces[&100].frame_ids, vec![50u64]);
    }

    #[test]
    fn truncated_file_returns_partial_index_with_warning() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .add_string(2, "worker")
            .build();
        let truncated = &bytes[..bytes.len() - 4];
        let start = advance_past_header(truncated);
        let result = run_first_pass(&truncated[start..], 8, |_| {});
        assert!(!result.warnings.is_empty(), "expected truncation warning");
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(result.index.strings[&1].value, "main");
    }

    // --- GC_ROOT_JAVA_FRAME indexing tests ---

    #[cfg(feature = "test-utils")]
    #[test]
    fn java_frame_root_at_frame_number_0_is_indexed_to_correct_frame_id() {
        // Setup: thread serial=1, stack trace serial=10, one frame with id=50.
        // GC root: object_id=99, thread_serial=1, frame_number=0 → frame_id=50.
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 100, 10, 1, 0, 0)
            .add_java_frame_root(99, 1, 0)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        let roots = result.index.java_frame_roots.get(&50);
        assert!(roots.is_some(), "frame_id=50 must have roots");
        assert_eq!(roots.unwrap(), &vec![99u64]);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn java_frame_root_with_negative_frame_number_is_not_stored() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 100, 10, 1, 0, 0)
            .add_java_frame_root(99, 1, -1)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert!(
            result.index.java_frame_roots.is_empty(),
            "root with frame_number=-1 must not be stored"
        );
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn class_dump_parsed_into_index_class_dumps() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(100, 0, 16, &[(10, 10u8)])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(result.index.class_dumps.len(), 1);
        let info = result.index.class_dumps.get(&100).unwrap();
        assert_eq!(info.class_object_id, 100);
        assert_eq!(info.super_class_id, 0);
        assert_eq!(info.instance_size, 16);
        assert_eq!(info.instance_fields.len(), 1);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn class_dump_two_fields_correctly_parsed() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(200, 50, 24, &[(20, 10u8), (21, 8u8)])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        let info = result.index.class_dumps.get(&200).unwrap();
        assert_eq!(info.instance_fields.len(), 2);
        assert_eq!(info.instance_fields[0].name_string_id, 20);
        assert_eq!(info.instance_fields[0].field_type, 10);
        assert_eq!(info.instance_fields[1].name_string_id, 21);
        assert_eq!(info.instance_fields[1].field_type, 8);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn heap_record_ranges_populated_for_heap_dump_segment() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(result.heap_record_ranges.len(), 1);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn root_thread_obj_populates_thread_object_ids() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_trace(10, 1, &[])
            .add_root_thread_obj(0xBEEF, 1, 10)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert_eq!(
            result.index.thread_object_ids.get(&1),
            Some(&0xBEEF),
            "ROOT_THREAD_OBJ must populate thread_object_ids"
        );
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn root_thread_obj_updates_synthetic_thread_object_id() {
        // Synthetic thread (from STACK_TRACE) starts with object_id=0.
        // ROOT_THREAD_OBJ should update it to the real object_id.
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_trace(10, 1, &[])
            .add_root_thread_obj(0xCAFE, 1, 10)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        let thread = result.index.threads.get(&1).unwrap();
        assert_eq!(
            thread.object_id, 0xCAFE,
            "synthetic thread must get object_id from ROOT_THREAD_OBJ"
        );
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn synthetic_thread_enables_frame_root_correlation() {
        // STACK_TRACE (thread_serial=1) + GC_ROOT_JAVA_FRAME
        // (thread_serial=1, frame_number=0), but NO START_THREAD.
        // The synthetic thread must be created before correlation so
        // the frame root is indexed.
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_java_frame_root(99, 1, 0)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        let roots = result.index.java_frame_roots.get(&50);
        assert!(
            roots.is_some(),
            "frame_id=50 must have roots via synthetic thread"
        );
        assert_eq!(roots.unwrap(), &vec![99u64]);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn java_frame_root_with_out_of_range_frame_number_is_not_stored() {
        // Stack trace has only 1 frame (index 0), so frame_number=1 is out of range.
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 100, 10, 1, 0, 0)
            .add_java_frame_root(99, 1, 1)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert!(
            result.index.java_frame_roots.is_empty(),
            "root with out-of-range frame_number must not be stored"
        );
    }

    #[test]
    fn instance_offsets_contains_thread_object_after_first_pass() {
        let thread_obj_id = 0xBEEF_u64;
        let thread_serial = 1_u32;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_root_thread_obj(thread_obj_id, thread_serial, 0)
            .add_instance(thread_obj_id, 0, 100, &[1, 2, 3, 4])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_first_pass(&bytes[start..], 8, |_| {});
        assert!(
            result.index.instance_offsets.contains_key(&thread_obj_id),
            "thread object must have a recorded offset"
        );
        let offset = result.index.instance_offsets[&thread_obj_id];
        assert!(
            offset > 0,
            "offset must be > 0 (past the heap record header)"
        );
    }
}
