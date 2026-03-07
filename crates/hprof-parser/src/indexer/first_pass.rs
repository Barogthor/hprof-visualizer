//! Single sequential pass over hprof record bytes, building an [`IndexResult`].
//!
//! [`run_first_pass`] iterates over every record in `data` (a slice starting
//! immediately after the file header), parsing known record types into the
//! index and collecting any non-fatal errors as warnings. The function is
//! infallible — it always returns an [`IndexResult`].

use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt};

use crate::indexer::IndexResult;
use crate::indexer::precise::PreciseIndex;
use crate::indexer::segment::SegmentFilterBuilder;
use crate::{
    parse_load_class, parse_record_header, parse_stack_frame, parse_stack_trace,
    parse_start_thread, parse_string_record, read_id,
};

/// Scans all records in `data` and returns a populated [`IndexResult`].
///
/// Non-fatal errors (corrupted payloads, size mismatches) are collected in
/// [`IndexResult::warnings`]. Fatal truncations (mid-header EOF, payload
/// window exceeds file) stop iteration and are also recorded as warnings.
///
/// ## Parameters
/// - `data`: raw bytes starting at the first record (immediately after the
///   hprof file header).
/// - `id_size`: byte width of object IDs, taken from the hprof file header
///   (4 or 8).
pub(crate) fn run_first_pass(data: &[u8], id_size: u32) -> IndexResult {
    let mut cursor = Cursor::new(data);
    let mut seg_builder = SegmentFilterBuilder::new();
    let mut result = IndexResult {
        index: PreciseIndex::new(),
        warnings: Vec::new(),
        records_attempted: 0,
        records_indexed: 0,
        segment_filters: Vec::new(),
    };

    while (cursor.position() as usize) < data.len() {
        let header = match parse_record_header(&mut cursor) {
            Ok(h) => h,
            Err(e) => {
                result.warnings.push(format!("EOF mid-header: {e}"));
                break;
            }
        };

        let payload_start = cursor.position() as usize;
        let payload_end = match payload_start.checked_add(header.length as usize) {
            Some(end) if end <= data.len() => end,
            Some(end) => {
                result.warnings.push(format!(
                    "record 0x{:02X} payload end {end} exceeds file size {}",
                    header.tag,
                    data.len()
                ));
                break;
            }
            None => {
                result.warnings.push(format!(
                    "record 0x{:02X} payload length overflow: {}",
                    header.tag, header.length
                ));
                break;
            }
        };

        if matches!(header.tag, 0x0C | 0x1C) {
            extract_heap_object_ids(
                &data[payload_start..payload_end],
                payload_start,
                id_size,
                &mut seg_builder,
            );
            cursor.set_position(payload_end as u64);
            continue;
        }

        if !matches!(header.tag, 0x01 | 0x02 | 0x04 | 0x05 | 0x06) {
            cursor.set_position(payload_end as u64);
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
                    (Ok(_), false) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} consumed {} of {} bytes — skipping",
                            header.tag,
                            payload_cursor.position(),
                            header.length
                        ));
                        false
                    }
                    (Err(e), _) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} at offset {payload_start}: {e}",
                            header.tag
                        ));
                        false
                    }
                }
            }
            0x02 => {
                let parsed = parse_load_class(&mut payload_cursor, id_size);
                let consumed = payload_cursor.position() as usize == header.length as usize;
                match (parsed, consumed) {
                    (Ok(c), true) => {
                        result.index.classes.insert(c.class_serial, c);
                        true
                    }
                    (Ok(_), false) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} consumed {} of {} bytes — skipping",
                            header.tag,
                            payload_cursor.position(),
                            header.length
                        ));
                        false
                    }
                    (Err(e), _) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} at offset {payload_start}: {e}",
                            header.tag
                        ));
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
                    (Ok(_), false) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} consumed {} of {} bytes — skipping",
                            header.tag,
                            payload_cursor.position(),
                            header.length
                        ));
                        false
                    }
                    (Err(e), _) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} at offset {payload_start}: {e}",
                            header.tag
                        ));
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
                    (Ok(_), false) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} consumed {} of {} bytes — skipping",
                            header.tag,
                            payload_cursor.position(),
                            header.length
                        ));
                        false
                    }
                    (Err(e), _) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} at offset {payload_start}: {e}",
                            header.tag
                        ));
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
                    (Ok(_), false) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} consumed {} of {} bytes — skipping",
                            header.tag,
                            payload_cursor.position(),
                            header.length
                        ));
                        false
                    }
                    (Err(e), _) => {
                        result.warnings.push(format!(
                            "record 0x{:02X} at offset {payload_start}: {e}",
                            header.tag
                        ));
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
    }

    result.segment_filters = seg_builder.build();
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

/// Skips a `CLASS_DUMP` sub-record body (after the sub-tag byte), returning
/// `false` on any read failure.
fn skip_class_dump(cursor: &mut Cursor<&[u8]>, id_size: u32) -> bool {
    // class_id + stack_trace_serial(4) + super_class_id + classloader_id +
    // signers_id + protection_domain_id + reserved1 + reserved2 + instance_size(4)
    // = 7 * id_size + 8
    if !skip_n(cursor, 7 * id_size as usize + 8) {
        return false;
    }

    // constant pool
    let Ok(cp_count) = cursor.read_u16::<BigEndian>() else {
        return false;
    };
    for _ in 0..cp_count {
        if cursor.read_u16::<BigEndian>().is_err() {
            return false;
        }
        let Ok(elem_type) = cursor.read_u8() else {
            return false;
        };
        let size = if elem_type == 2 {
            id_size as usize
        } else {
            let s = primitive_element_size(elem_type);
            if s == 0 {
                return false;
            }
            s
        };
        if !skip_n(cursor, size) {
            return false;
        }
    }

    // static fields
    let Ok(sf_count) = cursor.read_u16::<BigEndian>() else {
        return false;
    };
    for _ in 0..sf_count {
        if read_id(cursor, id_size).is_err() {
            return false;
        }
        let Ok(field_type) = cursor.read_u8() else {
            return false;
        };
        let size = if field_type == 2 {
            id_size as usize
        } else {
            let s = primitive_element_size(field_type);
            if s == 0 {
                return false;
            }
            s
        };
        if !skip_n(cursor, size) {
            return false;
        }
    }

    // instance fields: count(u16) + [name_string_id(id_size) + type(u8)]
    let Ok(if_count) = cursor.read_u16::<BigEndian>() else {
        return false;
    };
    skip_n(cursor, if_count as usize * (id_size as usize + 1))
}

/// Extracts object IDs from a `HEAP_DUMP` (0x0C) or `HEAP_DUMP_SEGMENT`
/// (0x1C) payload, registering each ID with `builder` at `data_offset`.
///
/// All read errors silently break the sub-record loop (tolerant parsing).
fn extract_heap_object_ids(
    payload: &[u8],
    data_offset: usize,
    id_size: u32,
    builder: &mut SegmentFilterBuilder,
) {
    let mut cursor = Cursor::new(payload);

    while let Ok(sub_tag) = cursor.read_u8() {
        let sub_record_start = data_offset + cursor.position() as usize;

        let ok = match sub_tag {
            0x01 => skip_n(&mut cursor, id_size as usize),
            0x02 => skip_n(&mut cursor, 2 * id_size as usize),
            0x03 => skip_n(&mut cursor, id_size as usize + 8),
            0x04 => skip_n(&mut cursor, id_size as usize + 8),
            0x05 => skip_n(&mut cursor, id_size as usize + 4),
            0x06 => skip_n(&mut cursor, id_size as usize),
            0x07 => skip_n(&mut cursor, id_size as usize + 4),
            0x08 => skip_n(&mut cursor, id_size as usize),
            0x09 => skip_n(&mut cursor, id_size as usize + 8),

            0x20 => skip_class_dump(&mut cursor, id_size),

            0x21 => {
                let Ok(obj_id) = read_id(&mut cursor, id_size) else {
                    break;
                };
                builder.add(sub_record_start, obj_id);
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

    #[test]
    fn heap_dump_0x0c_record_produces_segment_filter() {
        let obj_id: u64 = 0x1234;
        let data = make_record(0x0C, &make_instance_sub(obj_id, 100));
        let result = run_first_pass(&data, 8);
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
        let result = run_first_pass(&data, 8);
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
        let result = run_first_pass(&data, 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id));
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
        let result = run_first_pass(&data, 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id));
    }

    // --- NEW RED tests for tolerant behaviour ---

    #[test]
    fn eof_mid_header_produces_warning_and_no_records() {
        // Only 1 byte — cannot form a 9-byte record header.
        let data = &[0x01u8];
        let result = run_first_pass(data, 8);
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

        let result = run_first_pass(&data, 8);
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

        let result = run_first_pass(&data, 8);
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

        let result = run_first_pass(&data, 8);
        assert_eq!(result.records_indexed, 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn valid_single_record_no_warnings() {
        let payload = make_string_payload(7, "main", 8);
        let data = make_record(0x01, &payload);
        let result = run_first_pass(&data, 8);
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_attempted, 1);
        assert_eq!(result.records_indexed, 1);
    }

    #[test]
    fn empty_data_no_warnings_no_records() {
        let result = run_first_pass(&[], 8);
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
        let result = run_first_pass(&data, 8);
        assert!(
            !result.warnings.is_empty(),
            "expected a warning for short declared length"
        );
    }

    #[test]
    fn extra_payload_bytes_produces_warning_and_continues() {
        // STACK_TRACE with one frame plus trailing junk bytes in the same record.
        // The size mismatch should produce a warning; a subsequent record is indexed.
        let mut payload = make_stack_trace_payload(3, 1, &[10], 8);
        payload.extend_from_slice(&[0xEE; 8]);
        let mut data = make_record(0x05, &payload);
        // Append a valid STRING record that must be indexed despite the mismatch.
        let str_payload = make_string_payload(99, "after", 8);
        data.extend(make_record(0x01, &str_payload));

        let result = run_first_pass(&data, 8);
        assert!(
            !result.warnings.is_empty(),
            "expected size-mismatch warning"
        );
        assert!(
            result.index.strings.contains_key(&99),
            "subsequent record must be indexed"
        );
    }

    #[test]
    fn string_declared_length_smaller_than_id_size_stops_with_warning() {
        // Declared payload length is invalid for id_size=8.
        let payload = 1u64.to_be_bytes();
        let data = make_record_with_declared_length(0x01, 4, &payload);
        let result = run_first_pass(&data, 8);
        assert!(!result.warnings.is_empty(), "expected truncation warning");
    }

    // --- Unchanged happy-path tests (updated call sites) ---

    #[test]
    fn empty_data_returns_empty_index() {
        let result = run_first_pass(&[], 8);
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
        let result = run_first_pass(&data, 8);
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(result.index.strings[&7].value, "main");
    }

    #[test]
    fn single_load_class_indexed() {
        let payload = make_load_class_payload(1, 100, 0, 200, 8);
        let data = make_record(0x02, &payload);
        let result = run_first_pass(&data, 8);
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.classes[&1].class_object_id, 100);
    }

    #[test]
    fn single_start_thread_indexed() {
        let payload = make_start_thread_payload(2, 300, 0, 1, 2, 3, 8);
        let data = make_record(0x06, &payload);
        let result = run_first_pass(&data, 8);
        assert_eq!(result.index.threads.len(), 1);
        assert_eq!(result.index.threads[&2].object_id, 300);
    }

    #[test]
    fn single_stack_frame_indexed() {
        let payload = make_stack_frame_payload(10, 1, 2, 3, 5, 42, 8);
        let data = make_record(0x04, &payload);
        let result = run_first_pass(&data, 8);
        assert_eq!(result.index.stack_frames.len(), 1);
        assert_eq!(result.index.stack_frames[&10].line_number, 42);
    }

    #[test]
    fn single_stack_trace_indexed() {
        let payload = make_stack_trace_payload(3, 1, &[10, 20], 8);
        let data = make_record(0x05, &payload);
        let result = run_first_pass(&data, 8);
        assert_eq!(result.index.stack_traces.len(), 1);
        assert_eq!(result.index.stack_traces[&3].frame_ids, vec![10u64, 20u64]);
    }

    #[test]
    fn unknown_tag_skipped_index_empty() {
        let data = make_record(0xFF, &[0u8; 4]);
        let result = run_first_pass(&data, 8);
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
        let result = run_first_pass(&data, 8);
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
        let result = run_first_pass(&data, 4);
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
        let result = run_first_pass(&bytes[start..], 8);
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
        let result = run_first_pass(&truncated[start..], 8);
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
        let result = run_first_pass(&bytes[start..], 8);
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
        let result = run_first_pass(&bytes[start..], 8);
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
        let result = run_first_pass(&bytes[start..], 8);
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
        let result = run_first_pass(&bytes[start..], 8);
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
        let result = run_first_pass(&bytes[start..], 8);
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
        let result = run_first_pass(&truncated[start..], 8);
        assert!(!result.warnings.is_empty(), "expected truncation warning");
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(result.index.strings[&1].value, "main");
    }
}
