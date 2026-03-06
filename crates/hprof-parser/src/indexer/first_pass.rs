//! Single sequential pass over hprof record bytes, building a [`PreciseIndex`].
//!
//! [`run_first_pass`] iterates over every record in `data` (a slice starting
//! immediately after the file header), parsing known record types into the
//! index and skipping unknown ones.  Any parse error propagates as
//! [`HprofError`].

use std::io::Cursor;

use crate::indexer::precise::PreciseIndex;
use crate::{
    HprofError, parse_load_class, parse_record_header, parse_stack_frame, parse_stack_trace,
    parse_start_thread, parse_string_record, skip_record,
};

/// Scans all records in `data` and returns a populated [`PreciseIndex`].
///
/// ## Parameters
/// - `data`: raw bytes starting at the first record (immediately after the
///   hprof file header).
/// - `id_size`: byte width of object IDs, taken from the hprof file header
///   (4 or 8).
///
/// ## Errors
/// Returns [`HprofError`] if a record header or payload is truncated or
/// contains invalid data. Unknown record tags are silently skipped.
pub(crate) fn run_first_pass(data: &[u8], id_size: u32) -> Result<PreciseIndex, HprofError> {
    let mut cursor = Cursor::new(data);
    let mut index = PreciseIndex::new();

    while (cursor.position() as usize) < data.len() {
        let header = parse_record_header(&mut cursor)?;
        match header.tag {
            0x01 => {
                let s = parse_string_record(&mut cursor, id_size, header.length)?;
                index.strings.insert(s.id, s);
            }
            0x02 => {
                let c = parse_load_class(&mut cursor, id_size)?;
                index.classes.insert(c.class_serial, c);
            }
            0x04 => {
                let f = parse_stack_frame(&mut cursor, id_size)?;
                index.stack_frames.insert(f.frame_id, f);
            }
            0x05 => {
                let t = parse_stack_trace(&mut cursor, id_size)?;
                index.stack_traces.insert(t.stack_trace_serial, t);
            }
            0x06 => {
                let t = parse_start_thread(&mut cursor, id_size)?;
                index.threads.insert(t.thread_serial, t);
            }
            _ => {
                skip_record(&mut cursor, &header)?;
            }
        }
    }

    Ok(index)
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

    #[test]
    fn empty_data_returns_empty_index() {
        let index = run_first_pass(&[], 8).unwrap();
        assert!(index.strings.is_empty());
        assert!(index.classes.is_empty());
        assert!(index.threads.is_empty());
        assert!(index.stack_frames.is_empty());
        assert!(index.stack_traces.is_empty());
    }

    #[test]
    fn single_string_record_indexed() {
        let payload = make_string_payload(7, "main", 8);
        let data = make_record(0x01, &payload);
        let index = run_first_pass(&data, 8).unwrap();
        assert_eq!(index.strings.len(), 1);
        assert_eq!(index.strings[&7].value, "main");
    }

    #[test]
    fn single_load_class_indexed() {
        let payload = make_load_class_payload(1, 100, 0, 200, 8);
        let data = make_record(0x02, &payload);
        let index = run_first_pass(&data, 8).unwrap();
        assert_eq!(index.classes.len(), 1);
        assert_eq!(index.classes[&1].class_object_id, 100);
    }

    #[test]
    fn single_start_thread_indexed() {
        let payload = make_start_thread_payload(2, 300, 0, 1, 2, 3, 8);
        let data = make_record(0x06, &payload);
        let index = run_first_pass(&data, 8).unwrap();
        assert_eq!(index.threads.len(), 1);
        assert_eq!(index.threads[&2].object_id, 300);
    }

    #[test]
    fn single_stack_frame_indexed() {
        let payload = make_stack_frame_payload(10, 1, 2, 3, 5, 42, 8);
        let data = make_record(0x04, &payload);
        let index = run_first_pass(&data, 8).unwrap();
        assert_eq!(index.stack_frames.len(), 1);
        assert_eq!(index.stack_frames[&10].line_number, 42);
    }

    #[test]
    fn single_stack_trace_indexed() {
        let payload = make_stack_trace_payload(3, 1, &[10, 20], 8);
        let data = make_record(0x05, &payload);
        let index = run_first_pass(&data, 8).unwrap();
        assert_eq!(index.stack_traces.len(), 1);
        assert_eq!(index.stack_traces[&3].frame_ids, vec![10u64, 20u64]);
    }

    #[test]
    fn unknown_tag_skipped_index_empty() {
        // tag=0xFF with 4-byte payload of zeros
        let data = make_record(0xFF, &[0u8; 4]);
        let index = run_first_pass(&data, 8).unwrap();
        assert!(index.strings.is_empty());
        assert!(index.classes.is_empty());
        assert!(index.threads.is_empty());
        assert!(index.stack_frames.is_empty());
        assert!(index.stack_traces.is_empty());
    }

    #[test]
    fn three_string_records_all_indexed() {
        let mut data = Vec::new();
        for (id, s) in [(1u64, "a"), (2, "b"), (3, "c")] {
            let payload = make_string_payload(id, s, 8);
            data.extend(make_record(0x01, &payload));
        }
        let index = run_first_pass(&data, 8).unwrap();
        assert_eq!(index.strings.len(), 3);
        assert_eq!(index.strings[&1].value, "a");
        assert_eq!(index.strings[&2].value, "b");
        assert_eq!(index.strings[&3].value, "c");
    }

    #[test]
    fn id_size_4_string_and_class_both_indexed() {
        let mut data = Vec::new();
        let str_payload = make_string_payload(5, "foo", 4);
        data.extend(make_record(0x01, &str_payload));
        let cls_payload = make_load_class_payload(1, 50, 0, 5, 4);
        data.extend(make_record(0x02, &cls_payload));
        let index = run_first_pass(&data, 4).unwrap();
        assert_eq!(index.strings.len(), 1);
        assert_eq!(index.strings[&5].value, "foo");
        assert_eq!(index.classes.len(), 1);
        assert_eq!(index.classes[&1].class_object_id, 50);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    #[test]
    fn full_index_round_trip() {
        // 2 strings + 1 class + 2 threads + 1 stack_frame + 1 stack_trace = 7 entries
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
        let index = run_first_pass(&bytes[start..], 8).unwrap();
        assert_eq!(index.strings.len(), 2);
        assert_eq!(index.classes.len(), 1);
        assert_eq!(index.threads.len(), 2);
        assert_eq!(index.stack_frames.len(), 1);
        assert_eq!(index.stack_traces.len(), 1);
        assert_eq!(index.strings[&1].value, "main");
        assert_eq!(index.classes[&10].class_object_id, 200);
        assert_eq!(index.threads[&1].object_id, 300);
        assert_eq!(index.threads[&2].object_id, 400);
        assert_eq!(index.stack_frames[&50].line_number, 42);
        assert_eq!(index.stack_traces[&100].frame_ids, vec![50u64]);
    }
}
