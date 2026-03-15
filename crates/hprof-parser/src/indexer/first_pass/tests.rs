//! Tests for the first-pass indexer.

#[cfg(feature = "test-utils")]
use std::io::Cursor;

use byteorder::{BigEndian, WriteBytesExt};
use hprof_api::{MemoryBudget, NullProgressObserver, ProgressNotifier};

#[cfg(feature = "test-utils")]
use byteorder::ReadBytesExt;

#[cfg(feature = "test-utils")]
use super::heap_extraction::{compute_batch_ranges, extract_heap_segment, merge_segment_result};
#[cfg(feature = "test-utils")]
use super::hprof_primitives::{
    PARALLEL_THRESHOLD, PROGRESS_REPORT_INTERVAL, gc_root_skip_size, parse_class_dump,
    primitive_element_size, skip_n,
};
use super::thread_resolution::lookup_offset;
use super::*;
#[cfg(feature = "test-utils")]
use crate::tags::HeapSubTag;
#[cfg(feature = "test-utils")]
use crate::{ClassDumpInfo, read_id};

/// Runs `run_first_pass` with a `NullProgressObserver`.
fn run_fp(data: &[u8], id_size: u32) -> IndexResult {
    let mut obs = NullProgressObserver;
    let mut notifier = ProgressNotifier::new(&mut obs);
    run_first_pass(data, id_size, 0, &mut notifier, MemoryBudget::Unlimited)
}

#[cfg(feature = "test-utils")]
fn run_fp_with_test_observer(data: &[u8], id_size: u32) -> (IndexResult, hprof_api::TestObserver) {
    let mut obs = hprof_api::TestObserver::default();
    let result = {
        let mut notifier = ProgressNotifier::new(&mut obs);
        run_first_pass(data, id_size, 0, &mut notifier, MemoryBudget::Unlimited)
    };
    (result, obs)
}

/// Segments larger than this are sub-divided at sub-record
/// boundaries for finer parallel load-balancing.
#[cfg(feature = "test-utils")]
const SUB_DIVIDE_THRESHOLD: u64 = 16 * 1024 * 1024;

/// Skips a variable-length heap object sub-record body
/// (`InstanceDump`, `ObjectArrayDump`, `PrimArrayDump`)
/// without extracting data. Returns `false` on read failure.
#[cfg(feature = "test-utils")]
fn skip_heap_object(cursor: &mut Cursor<&[u8]>, sub_tag: HeapSubTag, id_size: u32) -> Option<bool> {
    match sub_tag {
        HeapSubTag::InstanceDump => {
            let Ok(_) = read_id(cursor, id_size) else {
                return Some(false);
            };
            let Ok(_) = cursor.read_u32::<BigEndian>() else {
                return Some(false);
            };
            let Ok(_) = read_id(cursor, id_size) else {
                return Some(false);
            };
            let Ok(n) = cursor.read_u32::<BigEndian>() else {
                return Some(false);
            };
            Some(skip_n(cursor, n as usize))
        }
        HeapSubTag::ObjectArrayDump => {
            let Ok(_) = read_id(cursor, id_size) else {
                return Some(false);
            };
            let Ok(_) = cursor.read_u32::<BigEndian>() else {
                return Some(false);
            };
            let Ok(n) = cursor.read_u32::<BigEndian>() else {
                return Some(false);
            };
            let Ok(_) = read_id(cursor, id_size) else {
                return Some(false);
            };
            Some(skip_n(cursor, n as usize * id_size as usize))
        }
        HeapSubTag::PrimArrayDump => {
            let Ok(_) = read_id(cursor, id_size) else {
                return Some(false);
            };
            let Ok(_) = cursor.read_u32::<BigEndian>() else {
                return Some(false);
            };
            let Ok(n) = cursor.read_u32::<BigEndian>() else {
                return Some(false);
            };
            let Ok(et) = cursor.read_u8() else {
                return Some(false);
            };
            let es = primitive_element_size(et);
            if es == 0 {
                return Some(false);
            }
            Some(skip_n(cursor, n as usize * es))
        }
        _ => None,
    }
}

/// Scans a heap segment payload extracting only
/// `CLASS_DUMP` (0x20) sub-records.
#[cfg(feature = "test-utils")]
fn extract_class_dumps_only(payload: &[u8], id_size: u32) -> Vec<(u64, ClassDumpInfo)> {
    let mut cursor = Cursor::new(payload);
    let mut results = Vec::new();

    while let Ok(raw) = cursor.read_u8() {
        let sub_tag = HeapSubTag::from(raw);
        let ok = match sub_tag {
            HeapSubTag::ClassDump => match parse_class_dump(&mut cursor, id_size) {
                Some(info) => {
                    let id = info.class_object_id;
                    results.push((id, info));
                    true
                }
                None => false,
            },
            t if gc_root_skip_size(t, id_size).is_some() => {
                skip_n(&mut cursor, gc_root_skip_size(t, id_size).unwrap())
            }
            t => match skip_heap_object(&mut cursor, t, id_size) {
                Some(ok) => ok,
                None => break,
            },
        };
        if !ok {
            break;
        }
    }
    results
}

/// Splits a segment larger than `threshold` at sub-record
/// boundaries.
#[cfg(feature = "test-utils")]
fn subdivide_segment(
    data: &[u8],
    offset: u64,
    len: u64,
    id_size: u32,
    threshold: u64,
) -> Vec<(u64, u64)> {
    if len <= threshold {
        return vec![(offset, len)];
    }
    let payload = &data[offset as usize..(offset + len) as usize];
    let mut cursor = Cursor::new(payload);
    let mut chunks = Vec::new();
    let mut chunk_start: u64 = 0;

    while cursor.position() < len {
        let pos_before = cursor.position();
        if pos_before - chunk_start >= threshold {
            chunks.push((offset + chunk_start, pos_before - chunk_start));
            chunk_start = pos_before;
        }
        let Ok(raw) = cursor.read_u8() else {
            break;
        };
        let sub_tag = HeapSubTag::from(raw);
        let ok = match sub_tag {
            HeapSubTag::ClassDump => parse_class_dump(&mut cursor, id_size).is_some(),
            t if gc_root_skip_size(t, id_size).is_some() => {
                skip_n(&mut cursor, gc_root_skip_size(t, id_size).unwrap())
            }
            t => match skip_heap_object(&mut cursor, t, id_size) {
                Some(ok) => ok,
                None => break,
            },
        };
        if !ok {
            break;
        }
    }

    let remaining = len - chunk_start;
    if remaining > 0 {
        chunks.push((offset + chunk_start, remaining));
    }
    chunks
}

// =====================================================
// mod tests — raw record-level tests (no test-utils)
// =====================================================

/// Resolves an `HprofStringRef` from a data slice for
/// test assertions.
fn resolve(sref: &crate::HprofStringRef, data: &[u8]) -> String {
    let start = sref.offset as usize;
    let end = start + sref.len as usize;
    String::from_utf8_lossy(&data[start..end]).into_owned()
}

fn make_record(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut rec = Vec::new();
    rec.write_u8(tag).unwrap();
    rec.write_u32::<BigEndian>(0).unwrap();
    rec.write_u32::<BigEndian>(payload.len() as u32).unwrap();
    rec.extend_from_slice(payload);
    rec
}

fn make_record_with_declared_length(tag: u8, declared_length: u32, payload: &[u8]) -> Vec<u8> {
    let mut rec = Vec::new();
    rec.write_u8(tag).unwrap();
    rec.write_u32::<BigEndian>(0).unwrap();
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

fn make_instance_sub(obj_id: u64, class_id: u64) -> Vec<u8> {
    let mut p = Vec::new();
    p.write_u8(0x21).unwrap();
    p.write_u64::<BigEndian>(obj_id).unwrap();
    p.write_u32::<BigEndian>(0).unwrap();
    p.write_u64::<BigEndian>(class_id).unwrap();
    p.write_u32::<BigEndian>(0).unwrap();
    p
}

// --- Progress observer tests ---

/// Collects `BytesScanned` positions from a
/// `TestObserver`.
#[cfg(feature = "test-utils")]
fn bytes_scanned_positions(obs: &hprof_api::TestObserver) -> Vec<u64> {
    obs.events
        .iter()
        .filter_map(|e| match e {
            hprof_api::ProgressEvent::BytesScanned(p) => Some(*p),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "test-utils")]
fn segment_completed_events(obs: &hprof_api::TestObserver) -> Vec<(usize, usize)> {
    obs.events
        .iter()
        .filter_map(|e| match e {
            hprof_api::ProgressEvent::SegmentCompleted { done, total } => Some((*done, *total)),
            _ => None,
        })
        .collect()
}

/// Progress observer callbacks during scanning.
mod progress_tests {
    use super::*;

    #[test]
    fn progress_observer_called_once_with_zero_for_empty_data() {
        let mut obs = NullProgressObserver;
        let mut notifier = ProgressNotifier::new(&mut obs);
        let _result = run_first_pass(&[], 8, 0, &mut notifier, MemoryBudget::Unlimited);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn progress_observer_called_for_single_record() {
        let payload = make_string_payload(1, "hello", 8);
        let data = make_record(0x01, &payload);
        let mut obs = hprof_api::TestObserver::default();
        let mut notifier = ProgressNotifier::new(&mut obs);
        run_first_pass(&data, 8, 0, &mut notifier, MemoryBudget::Unlimited);
        let calls = bytes_scanned_positions(&obs);
        assert!(!calls.is_empty(), "observer must be called at least once");
        assert_eq!(
            *calls.last().unwrap(),
            data.len() as u64,
            "final call must report full data length"
        );
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn scan_phase_reports_monotonic_bytes() {
        const FIVE_MIB: usize = 5 * 1024 * 1024;
        let mut data: Vec<u8> = Vec::with_capacity(FIVE_MIB + 16);
        while data.len() < FIVE_MIB {
            data.write_u8(0xFF).unwrap();
            data.write_u32::<BigEndian>(0).unwrap();
            data.write_u32::<BigEndian>(0).unwrap();
        }
        let mut obs = hprof_api::TestObserver::default();
        let mut notifier = ProgressNotifier::new(&mut obs);
        run_first_pass(&data, 8, 0, &mut notifier, MemoryBudget::Unlimited);
        let values = bytes_scanned_positions(&obs);
        assert!(
            values.len() > 1,
            "large data must trigger more than one call"
        );
        for w in values.windows(2) {
            assert!(w[1] > w[0], "values must be strictly increasing");
        }
    }

    #[cfg(feature = "test-utils")]
    #[test]
    fn progress_observer_reports_partial_position_for_truncated_data() {
        const DECLARED_PAYLOAD: u32 = 1000;
        let mut data: Vec<u8> = Vec::new();
        data.write_u8(0x01).unwrap();
        data.write_u32::<BigEndian>(0).unwrap();
        data.write_u32::<BigEndian>(DECLARED_PAYLOAD).unwrap();
        data.extend_from_slice(&[0u8; 50]);
        let mut obs = hprof_api::TestObserver::default();
        let mut notifier = ProgressNotifier::new(&mut obs);
        run_first_pass(&data, 8, 0, &mut notifier, MemoryBudget::Unlimited);
        let calls = bytes_scanned_positions(&obs);
        let reported = *calls.last().expect("observer must be called at least once");
        let declared_end = 9u64 + DECLARED_PAYLOAD as u64;
        assert!(
            reported < declared_end,
            "cursor ({reported}) must be before \
             declared end ({declared_end})"
        );
        assert!(
            reported <= data.len() as u64,
            "cursor ({reported}) must not exceed \
             actual data length ({})",
            data.len()
        );
    }
}

/// Heap segment parsing: GC roots, class dumps, and instance layout.
mod heap_parsing_tests {
    use super::*;

    #[test]
    fn heap_dump_0x0c_record_produces_segment_filter() {
        let obj_id: u64 = 0x1234;
        let data = make_record(0x0C, &make_instance_sub(obj_id, 100));
        let result = run_fp(&data, 8);
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
        let mut payload = make_instance_sub(obj_id1, 100);
        payload.write_u8(0x21).unwrap();
        payload.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let data = make_record(0x1C, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id1));
    }

    #[test]
    fn gc_root_before_instance_dump_cursor_advances_correctly() {
        let gc_root_id: u64 = 0x5555;
        let obj_id: u64 = 0x6666;
        let mut payload = Vec::new();
        payload.write_u8(0x01).unwrap();
        payload.write_u64::<BigEndian>(gc_root_id).unwrap();
        payload.extend_from_slice(&make_instance_sub(obj_id, 200));
        let data = make_record(0x1C, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id));
    }

    #[test]
    fn truncated_gc_root_java_frame_sub_record_adds_warning() {
        let mut payload = Vec::new();
        payload.write_u8(0x03).unwrap();
        payload.write_u64::<BigEndian>(0xABCD).unwrap();
        payload.extend_from_slice(&[0x00, 0x01]);
        let data = make_record(0x1C, &payload);

        let result = run_fp(&data, 8);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("GC_ROOT_JAVA_FRAME")),
            "expected GC_ROOT_JAVA_FRAME warning, \
             got {:?}",
            result.warnings
        );
    }

    #[test]
    fn class_dump_before_instance_dump_cursor_advances_correctly() {
        let class_id: u64 = 0x1111;
        let obj_id: u64 = 0x2222;
        let mut payload = Vec::new();
        payload.write_u8(0x20).unwrap();
        payload.write_u64::<BigEndian>(class_id).unwrap();
        payload.write_u32::<BigEndian>(0).unwrap();
        for _ in 0..6u8 {
            payload.write_u64::<BigEndian>(0).unwrap();
        }
        payload.write_u32::<BigEndian>(16).unwrap();
        payload.write_u16::<BigEndian>(0).unwrap();
        payload.write_u16::<BigEndian>(0).unwrap();
        payload.write_u16::<BigEndian>(0).unwrap();
        payload.extend_from_slice(&make_instance_sub(obj_id, class_id));
        let data = make_record(0x1C, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(obj_id));
    }
}

/// Tolerant behaviour on malformed, truncated, or oversized records.
mod error_handling_tests {
    use super::*;

    #[test]
    fn eof_mid_header_produces_warning_and_no_records() {
        let data = &[0x01u8];
        let result = run_fp(data, 8);
        assert!(
            !result.warnings.is_empty(),
            "expected EOF mid-header warning"
        );
        assert_eq!(result.records_indexed, 0);
        assert_eq!(result.records_attempted, 0);
    }

    #[test]
    fn payload_end_exceeds_data_produces_warning_and_stops() {
        let str_payload = make_string_payload(1, "ok", 8);
        let mut data = make_record(0x01, &str_payload);
        data.push(0x01);
        data.write_u32::<BigEndian>(0).unwrap();
        data.write_u32::<BigEndian>(1000).unwrap();
        let result = run_fp(&data, 8);
        assert!(
            !result.warnings.is_empty(),
            "expected out-of-bounds warning"
        );
        assert_eq!(result.records_indexed, 1);
    }

    #[test]
    fn corrupted_payload_within_window_produces_warning_and_continues() {
        let mut st_payload = Vec::new();
        st_payload.write_u32::<BigEndian>(3).unwrap();
        st_payload.write_u32::<BigEndian>(1).unwrap();
        st_payload.write_u32::<BigEndian>(1).unwrap();
        let mut data = make_record(0x05, &st_payload);
        let str_payload = make_string_payload(7, "next", 8);
        data.extend(make_record(0x01, &str_payload));
        let result = run_fp(&data, 8);
        assert!(!result.warnings.is_empty(), "expected parse warning");
        assert_eq!(
            result.records_indexed, 1,
            "next string record must be indexed"
        );
        assert!(result.index.strings.contains_key(&7));
    }

    #[test]
    fn two_records_first_corrupt_second_valid_gives_one_indexed() {
        let mut st_payload = Vec::new();
        st_payload.write_u32::<BigEndian>(3).unwrap();
        st_payload.write_u32::<BigEndian>(1).unwrap();
        st_payload.write_u32::<BigEndian>(1).unwrap();
        let mut data = make_record(0x05, &st_payload);
        let str_payload = make_string_payload(42, "good", 8);
        data.extend(make_record(0x01, &str_payload));
        let result = run_fp(&data, 8);
        assert_eq!(result.records_indexed, 1);
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn valid_single_record_no_warnings() {
        let payload = make_string_payload(7, "main", 8);
        let data = make_record(0x01, &payload);
        let result = run_fp(&data, 8);
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_attempted, 1);
        assert_eq!(result.records_indexed, 1);
    }

    #[test]
    fn empty_data_no_warnings_no_records() {
        let result = run_fp(&[], 8);
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_indexed, 0);
        assert_eq!(result.records_attempted, 0);
    }

    #[test]
    fn too_short_declared_length_stops_with_warning() {
        let payload = make_load_class_payload(1, 100, 0, 200, 8);
        let data = make_record_with_declared_length(0x02, 4, &payload);
        let result = run_fp(&data, 8);
        assert!(
            !result.warnings.is_empty(),
            "expected a warning for short declared length"
        );
    }

    #[test]
    fn extra_payload_bytes_produces_warning_and_continues() {
        let mut payload = make_stack_trace_payload(3, 1, &[10], 8);
        payload.extend_from_slice(&[0xEE; 8]);
        let mut data = make_record(0x05, &payload);
        let str_payload = make_string_payload(99, "after", 8);
        data.extend(make_record(0x01, &str_payload));
        let result = run_fp(&data, 8);
        assert!(
            !result.warnings.is_empty(),
            "expected size-mismatch warning"
        );
        assert!(
            result.index.stack_traces.contains_key(&3),
            "stack trace with extra bytes must still \
             be indexed"
        );
        assert!(
            result.index.strings.contains_key(&99),
            "subsequent record must be indexed"
        );
    }

    #[test]
    fn start_thread_with_extra_bytes_is_indexed_with_warning() {
        let mut payload = make_start_thread_payload(7, 0xBEEF, 5, 42, 0, 0, 8);
        payload.extend_from_slice(&[0xFF; 4]);
        let data = make_record(0x06, &payload);

        let result = run_fp(&data, 8);
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
    fn string_declared_length_smaller_than_id_size_stops_with_warning() {
        let payload = 1u64.to_be_bytes();
        let data = make_record_with_declared_length(0x01, 4, &payload);
        let result = run_fp(&data, 8);
        assert!(!result.warnings.is_empty(), "expected truncation warning");
    }
}

/// Happy-path indexing of string, class, thread, frame, and stack-trace records.
mod record_parsing_tests {
    use super::*;

    #[test]
    fn empty_data_returns_empty_index() {
        let result = run_fp(&[], 8);
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
        let result = run_fp(&data, 8);
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(resolve(&result.index.strings[&7], &data), "main");
    }

    #[test]
    fn single_load_class_indexed() {
        let payload = make_load_class_payload(1, 100, 0, 200, 8);
        let data = make_record(0x02, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.classes[&1].class_object_id, 100);
    }

    #[test]
    fn load_class_populates_class_names_by_id_with_dot_notation() {
        let str_payload = make_string_payload(5, "java/util/HashMap", 8);
        let cls_payload = make_load_class_payload(1, 100, 0, 5, 8);
        let mut data = Vec::new();
        data.extend(make_record(0x01, &str_payload));
        data.extend(make_record(0x02, &cls_payload));
        let result = run_fp(&data, 8);
        assert_eq!(
            result.index.class_names_by_id.get(&100),
            Some(&"java.util.HashMap".to_string())
        );
    }

    #[test]
    fn load_class_with_unknown_string_id_inserts_empty_name() {
        let payload = make_load_class_payload(1, 42, 0, 99, 8);
        let data = make_record(0x02, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(
            result.index.class_names_by_id.get(&42),
            Some(&String::new())
        );
    }

    #[test]
    fn single_start_thread_indexed() {
        let payload = make_start_thread_payload(2, 300, 0, 1, 2, 3, 8);
        let data = make_record(0x06, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.index.threads.len(), 1);
        assert_eq!(result.index.threads[&2].object_id, 300);
    }

    #[test]
    fn single_stack_frame_indexed() {
        let payload = make_stack_frame_payload(10, 1, 2, 3, 5, 42, 8);
        let data = make_record(0x04, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.index.stack_frames.len(), 1);
        assert_eq!(result.index.stack_frames[&10].line_number, 42);
    }

    #[test]
    fn single_stack_trace_indexed() {
        let payload = make_stack_trace_payload(3, 1, &[10, 20], 8);
        let data = make_record(0x05, &payload);
        let result = run_fp(&data, 8);
        assert_eq!(result.index.stack_traces.len(), 1);
        assert_eq!(result.index.stack_traces[&3].frame_ids, vec![10u64, 20u64]);
    }

    #[test]
    fn unknown_tag_skipped_index_empty() {
        let data = make_record(0xFF, &[0u8; 4]);
        let result = run_fp(&data, 8);
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
        let result = run_fp(&data, 8);
        assert_eq!(result.index.strings.len(), 3);
        assert_eq!(resolve(&result.index.strings[&1], &data), "a");
        assert_eq!(resolve(&result.index.strings[&2], &data), "b");
        assert_eq!(resolve(&result.index.strings[&3], &data), "c");
    }

    #[test]
    fn id_size_4_string_and_class_both_indexed() {
        let mut data = Vec::new();
        let str_payload = make_string_payload(5, "foo", 4);
        data.extend(make_record(0x01, &str_payload));
        let cls_payload = make_load_class_payload(1, 50, 0, 5, 4);
        data.extend(make_record(0x02, &cls_payload));
        let result = run_fp(&data, 4);
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(resolve(&result.index.strings[&5], &data), "foo");
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.classes[&1].class_object_id, 50);
    }

    #[test]
    fn zgc_high_bit_ids_indexed_through_first_pass() {
        let base: u64 = 0xFFFF_0000_0000_0000;
        let count = 50u64;
        let class_id: u64 = base | 0xFFFF;
        let mut heap_payload = Vec::new();
        for i in 0..count {
            heap_payload.extend_from_slice(&make_instance_sub(base | i, class_id));
        }
        let data = make_record(0x1C, &heap_payload);
        let result = run_fp(&data, 8);
        assert_eq!(
            result.segment_filters.len(),
            1,
            "one segment filter expected"
        );
        for i in 0..count {
            assert!(
                result.segment_filters[0].contains(base | i),
                "ZGC-style ID {:#X} not found in filter",
                base | i
            );
        }
    }
}

/// Thread synthesis from STACK_TRACE records and offset lookup correctness.
mod thread_resolution_tests {
    use super::*;

    #[test]
    fn stack_trace_without_start_thread_synthesises_thread_entry() {
        let payload = make_stack_trace_payload(5, 3, &[10], 8);
        let data = make_record(0x05, &payload);
        let result = run_fp(&data, 8);
        assert!(
            result.index.threads.contains_key(&3),
            "synthetic thread must be created from \
             STACK_TRACE thread_serial"
        );
        let t = &result.index.threads[&3];
        assert_eq!(t.thread_serial, 3);
        assert_eq!(t.stack_trace_serial, 5);
        assert_eq!(t.name_string_id, 0, "synthetic thread has no name string");
    }

    #[test]
    fn stack_trace_thread_serial_zero_does_not_synthesise_thread() {
        let payload = make_stack_trace_payload(1, 0, &[], 8);
        let data = make_record(0x05, &payload);
        let result = run_fp(&data, 8);
        assert!(
            !result.index.threads.contains_key(&0),
            "thread_serial=0 must not be synthesised"
        );
    }

    #[test]
    fn start_thread_record_takes_priority_over_synthetic_thread() {
        let str_payload = make_string_payload(10, "main", 8);
        let thread_payload = make_start_thread_payload(1, 100, 7, 10, 0, 0, 8);
        let trace_payload = make_stack_trace_payload(7, 1, &[50], 8);
        let mut data = make_record(0x01, &str_payload);
        data.extend(make_record(0x06, &thread_payload));
        data.extend(make_record(0x05, &trace_payload));
        let result = run_fp(&data, 8);
        assert_eq!(result.index.threads.len(), 1);
        let t = &result.index.threads[&1];
        assert_eq!(
            t.name_string_id, 10,
            "real START_THREAD must not be overwritten"
        );
    }

    #[test]
    fn lookup_offset_finds_all_inserted_entries() {
        use super::ObjectOffset;
        let mut vec: Vec<ObjectOffset> = (0..1000)
            .map(|i| ObjectOffset {
                object_id: i * 7 + 3,
                offset: i * 100,
            })
            .collect();
        vec.sort_unstable_by_key(|o| o.object_id);
        for i in 0..1000u64 {
            let id = i * 7 + 3;
            assert_eq!(
                lookup_offset(&vec, id),
                Some(i * 100),
                "lookup failed for id {id}"
            );
        }
    }

    #[test]
    fn lookup_offset_returns_none_for_missing_id() {
        use super::ObjectOffset;
        let vec: Vec<ObjectOffset> = vec![
            ObjectOffset {
                object_id: 10,
                offset: 100,
            },
            ObjectOffset {
                object_id: 20,
                offset: 200,
            },
            ObjectOffset {
                object_id: 30,
                offset: 300,
            },
        ];
        assert_eq!(lookup_offset(&vec, 15), None);
        assert_eq!(lookup_offset(&vec, 0), None);
        assert_eq!(lookup_offset(&vec, 999), None);
    }

    #[test]
    fn lookup_offset_empty_vec_returns_none() {
        use super::ObjectOffset;
        let vec: Vec<ObjectOffset> = Vec::new();
        assert_eq!(lookup_offset(&vec, 42), None);
    }

    #[test]
    fn scan_records_parse_failure_emits_warning() {
        // STRING record (tag=0x01) with header_length=1 but
        // id_size=4 — parser can't read the 4-byte string ID.
        let mut data = Vec::new();
        data.push(0x01u8); // STRING_IN_UTF8 tag
        data.extend_from_slice(&0u32.to_be_bytes()); // timestamp
        data.extend_from_slice(&1u32.to_be_bytes()); // length = 1
        data.push(0xFFu8); // 1-byte payload, insufficient for 4-byte ID

        let result = run_fp(&data, 4);
        assert!(
            result.warnings.iter().any(|w| w.contains("at offset")),
            "expected parse-failure warning, got: {:?}",
            result.warnings
        );
    }

    #[test]
    fn scan_records_partial_consumption_emits_warning() {
        // STACK_FRAME record (tag=0x04) with id_size=4.
        // parse_stack_frame reads exactly 24 bytes; we set
        // header_length=28 (4 extra bytes) to trigger the
        // "parsed OK but consumed X of Y bytes" warning.
        let id_size = 4u32;
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_be_bytes()); // frame_id
        payload.extend_from_slice(&2u32.to_be_bytes()); // method_name_string_id
        payload.extend_from_slice(&3u32.to_be_bytes()); // method_signature_string_id
        payload.extend_from_slice(&4u32.to_be_bytes()); // source_file_name_id
        payload.extend_from_slice(&5u32.to_be_bytes()); // class_serial_num
        payload.extend_from_slice(&0i32.to_be_bytes()); // line_number
        payload.extend_from_slice(&[0xBE, 0xEF, 0xCA, 0xFE]); // 4 extra bytes

        let mut data = Vec::new();
        data.push(0x04u8); // STACK_FRAME tag
        data.extend_from_slice(&0u32.to_be_bytes()); // timestamp
        data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        data.extend_from_slice(&payload);

        let result = run_fp(&data, id_size);
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("extra bytes ignored")),
            "expected partial-consumption warning, got: {:?}",
            result.warnings
        );
    }
}

// =====================================================
// builder_tests — tests using HprofTestBuilder
// =====================================================

/// End-to-end indexing tests using `HprofTestBuilder`.
#[cfg(feature = "test-utils")]
mod builder_tests {
    use super::*;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    fn resolve(sref: &crate::HprofStringRef, data: &[u8]) -> String {
        let start = sref.offset as usize;
        let end = start + sref.len as usize;
        String::from_utf8_lossy(&data[start..end]).into_owned()
    }

    // --- Segment filter tests ---

    #[test]
    fn heap_dump_segment_instance_dump_produces_one_filter_containing_id() {
        let object_id = 0xABCD_u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(object_id, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
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
        let truncated = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(id1, 0, 100, &[])
            .add_instance(id2, 0, 100, &[])
            .truncate_at(full.len() - 5)
            .build();
        let start = advance_past_header(&truncated);
        let result = run_fp(&truncated[start..], 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(id1));
    }

    #[test]
    fn no_heap_dump_records_produces_empty_segment_filters() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "hello")
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
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
        let result = run_fp(&bytes[start..], 8);
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
        let result = run_fp(&bytes[start..], 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.segment_filters[0].contains(array_id));
    }

    #[test]
    fn prim_array_in_heap_dump_segment_filter_contains_array_id() {
        let array_id = 0xCAFE_u64;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_prim_array(array_id, 0, 2, 8, &[0x01, 0x02])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
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
        let result = run_fp(&bytes[start..], 8);
        assert!(result.warnings.is_empty());
        assert_eq!(result.records_indexed, result.records_attempted);
        assert_eq!(result.index.strings.len(), 2);
        assert_eq!(result.index.classes.len(), 1);
        assert_eq!(result.index.threads.len(), 2);
        assert_eq!(result.index.stack_frames.len(), 1);
        assert_eq!(result.index.stack_traces.len(), 1);
        let records = &bytes[start..];
        assert_eq!(resolve(&result.index.strings[&1], records), "main");
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
        let result = run_fp(&truncated[start..], 8);
        assert!(!result.warnings.is_empty(), "expected truncation warning");
        assert_eq!(result.index.strings.len(), 1);
        let records = &truncated[start..];
        assert_eq!(resolve(&result.index.strings[&1], records), "main");
    }

    // --- GC_ROOT_JAVA_FRAME indexing tests ---

    #[test]
    fn java_frame_root_at_frame_number_0_is_indexed_to_correct_frame_id() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 100, 10, 1, 0, 0)
            .add_java_frame_root(99, 1, 0)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        let roots = result.index.java_frame_roots.get(&50);
        assert!(roots.is_some(), "frame_id=50 must have roots");
        assert_eq!(roots.unwrap(), &vec![99u64]);
    }

    #[test]
    fn java_frame_root_with_negative_frame_number_is_not_stored() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 100, 10, 1, 0, 0)
            .add_java_frame_root(99, 1, -1)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        assert!(
            result.index.java_frame_roots.is_empty(),
            "root with frame_number=-1 must not \
             be stored"
        );
    }

    #[test]
    fn class_dump_parsed_into_index_class_dumps() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(100, 0, 16, &[(10, 10u8)])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        assert_eq!(result.index.class_dumps.len(), 1);
        let info = result.index.class_dumps.get(&100).unwrap();
        assert_eq!(info.class_object_id, 100);
        assert_eq!(info.super_class_id, 0);
        assert_eq!(info.instance_size, 16);
        assert_eq!(info.instance_fields.len(), 1);
    }

    #[test]
    fn class_dump_two_fields_correctly_parsed() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class_dump(200, 50, 24, &[(20, 10u8), (21, 8u8)])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        let info = result.index.class_dumps.get(&200).unwrap();
        assert_eq!(info.instance_fields.len(), 2);
        assert_eq!(info.instance_fields[0].name_string_id, 20);
        assert_eq!(info.instance_fields[0].field_type, 10);
        assert_eq!(info.instance_fields[1].name_string_id, 21);
        assert_eq!(info.instance_fields[1].field_type, 8);
    }

    #[test]
    fn heap_record_ranges_populated_for_heap_dump_segment() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        assert_eq!(result.heap_record_ranges.len(), 1);
    }

    #[test]
    fn root_thread_obj_populates_thread_object_ids() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_trace(10, 1, &[])
            .add_root_thread_obj(0xBEEF, 1, 10)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        assert_eq!(
            result.index.thread_object_ids.get(&1),
            Some(&0xBEEF),
            "ROOT_THREAD_OBJ must populate \
             thread_object_ids"
        );
    }

    #[test]
    fn root_thread_obj_updates_synthetic_thread_object_id() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_trace(10, 1, &[])
            .add_root_thread_obj(0xCAFE, 1, 10)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        let thread = result.index.threads.get(&1).unwrap();
        assert_eq!(
            thread.object_id, 0xCAFE,
            "synthetic thread must get object_id \
             from ROOT_THREAD_OBJ"
        );
    }

    #[test]
    fn synthetic_thread_enables_frame_root_correlation() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_java_frame_root(99, 1, 0)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        let roots = result.index.java_frame_roots.get(&50);
        assert!(
            roots.is_some(),
            "frame_id=50 must have roots via \
             synthetic thread"
        );
        assert_eq!(roots.unwrap(), &vec![99u64]);
    }

    #[test]
    fn java_frame_root_with_out_of_range_frame_number_is_not_stored() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_stack_frame(50, 1, 2, 3, 1, 42)
            .add_stack_trace(10, 1, &[50])
            .add_thread(1, 100, 10, 1, 0, 0)
            .add_java_frame_root(99, 1, 1)
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        assert!(
            result.index.java_frame_roots.is_empty(),
            "root with out-of-range frame_number \
             must not be stored"
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
        let result = run_fp(&bytes[start..], 8);
        assert!(
            result.index.instance_offsets.contains_key(&thread_obj_id),
            "thread object must have a recorded offset"
        );
        let offset = result.index.instance_offsets[&thread_obj_id];
        assert!(
            offset > 0,
            "offset must be > 0 (past the heap \
             record header)"
        );
    }

    // ---- Parallel heap extraction tests ----

    fn make_class_dump_sub_record(
        class_object_id: u64,
        super_class_id: u64,
        instance_size: u32,
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x20).unwrap();
        write_id(&mut p, class_object_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap();
        write_id(&mut p, super_class_id, id_size);
        for _ in 0..5 {
            write_id(&mut p, 0, id_size);
        }
        p.write_u32::<BigEndian>(instance_size).unwrap();
        p.write_u16::<BigEndian>(0).unwrap();
        p.write_u16::<BigEndian>(0).unwrap();
        p.write_u16::<BigEndian>(0).unwrap();
        p
    }

    fn make_instance_sub_record(
        obj_id: u64,
        class_id: u64,
        field_data: &[u8],
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x21).unwrap();
        write_id(&mut p, obj_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap();
        write_id(&mut p, class_id, id_size);
        p.write_u32::<BigEndian>(field_data.len() as u32).unwrap();
        p.extend_from_slice(field_data);
        p
    }

    fn make_prim_array_sub_record(
        arr_id: u64,
        elem_type: u8,
        elements: &[u8],
        id_size: u32,
    ) -> Vec<u8> {
        let elem_size = primitive_element_size(elem_type);
        let num_elements = if elem_size > 0 {
            elements.len() / elem_size
        } else {
            0
        };
        let mut p = Vec::new();
        p.write_u8(0x23).unwrap();
        write_id(&mut p, arr_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap();
        p.write_u32::<BigEndian>(num_elements as u32).unwrap();
        p.write_u8(elem_type).unwrap();
        p.extend_from_slice(elements);
        p
    }

    fn make_obj_array_sub_record(
        arr_id: u64,
        class_id: u64,
        element_ids: &[u64],
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x22).unwrap();
        write_id(&mut p, arr_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap();
        p.write_u32::<BigEndian>(element_ids.len() as u32).unwrap();
        write_id(&mut p, class_id, id_size);
        for &eid in element_ids {
            write_id(&mut p, eid, id_size);
        }
        p
    }

    fn write_id(buf: &mut Vec<u8>, id: u64, id_size: u32) {
        if id_size == 8 {
            buf.write_u64::<BigEndian>(id).unwrap();
        } else {
            buf.write_u32::<BigEndian>(id as u32).unwrap();
        }
    }

    #[test]
    fn extract_class_dumps_only_returns_class_dumps() {
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.extend(make_class_dump_sub_record(100, 0, 16, id_size));
        payload.extend(make_instance_sub_record(200, 100, &[0; 16], id_size));
        payload.extend(make_class_dump_sub_record(300, 100, 8, id_size));

        let results = extract_class_dumps_only(&payload, id_size);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 100);
        assert_eq!(results[0].1.instance_size, 16);
        assert_eq!(results[1].0, 300);
        assert_eq!(results[1].1.super_class_id, 100);
    }

    #[test]
    fn extract_heap_segment_skips_class_dump() {
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.extend(make_class_dump_sub_record(100, 0, 16, id_size));
        payload.extend(make_instance_sub_record(200, 100, &[0; 16], id_size));
        payload.extend(make_obj_array_sub_record(300, 100, &[400, 500], id_size));
        payload.extend(make_prim_array_sub_record(600, 10, &[1, 0, 0, 0], id_size));

        let data_offset = 100;
        let parsing_result = extract_heap_segment(&payload, data_offset, id_size, usize::MAX);
        // Single chunk (no chunking)
        assert_eq!(parsing_result.chunks.len(), 1);
        let result = &parsing_result.chunks[0];

        // all_offsets: INSTANCE_DUMP + PRIM_ARRAY
        assert_eq!(result.all_offsets.len(), 2);
        let ids: Vec<u64> = result.all_offsets.iter().map(|o| o.object_id).collect();
        assert!(ids.contains(&200));
        assert!(ids.contains(&600));

        // filter_ids: INSTANCE + OBJ_ARRAY + PRIM_ARRAY
        assert_eq!(result.filter_ids.len(), 3);
        let fids: Vec<u64> = result.filter_ids.iter().map(|e| e.object_id).collect();
        assert!(fids.contains(&200));
        assert!(fids.contains(&300));
        assert!(fids.contains(&600));

        assert!(result.warnings.is_empty());
    }

    #[test]
    fn subdivide_segment_no_split_below_threshold() {
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.extend(make_instance_sub_record(1, 0, &[0; 8], id_size));
        let offset = 0u64;
        let len = payload.len() as u64;
        let chunks = subdivide_segment(&payload, offset, len, id_size, SUB_DIVIDE_THRESHOLD);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], (0, len));
    }

    #[test]
    fn subdivide_segment_splits_at_sub_record_boundaries() {
        let id_size = 8u32;
        let single = make_instance_sub_record(1, 0, &[0; 8], id_size);
        let record_size = single.len();
        let threshold = record_size as u64;
        let count = 5;
        let mut payload = Vec::new();
        for i in 0..count {
            payload.extend(make_instance_sub_record(i + 1, 0, &[0; 8], id_size));
        }
        let len = payload.len() as u64;
        let chunks = subdivide_segment(&payload, 0, len, id_size, threshold);

        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );

        let total: u64 = chunks.iter().map(|(_, l)| *l).sum();
        assert_eq!(total, len);

        let mut prev_end = 0u64;
        for (off, clen) in &chunks {
            assert_eq!(*off, prev_end, "chunks must be contiguous");
            prev_end = off + clen;
        }
    }

    #[test]
    fn parallel_path_produces_correct_results() {
        let id_size = 8u32;
        let big_data = vec![0u8; 33 * 1024 * 1024];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_class_dump(100, 0, big_data.len() as u32, &[])
            .add_instance(42, 0, 100, &big_data)
            .build();

        let start = advance_past_header(&bytes);
        let data = &bytes[start..];

        assert!(
            (data.len() as u64) >= PARALLEL_THRESHOLD,
            "test data must exceed parallel threshold"
        );

        let result = run_fp(data, id_size);

        assert!(
            result.index.class_dumps.contains_key(&100),
            "class_dumps must contain class 100"
        );

        assert!(
            result.heap_record_ranges.len() >= 2,
            "need at least 2 heap segments"
        );

        assert!(
            !result.segment_filters.is_empty(),
            "segment filters must be built"
        );

        assert!(
            result.warnings.is_empty(),
            "no warnings expected: {:?}",
            result.warnings
        );
    }

    #[test]
    fn small_file_uses_sequential_path() {
        let id_size = 8u32;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_instance(42, 0, 100, &[1, 2, 3, 4])
            .build();

        assert!(
            (bytes.len() as u64) < PARALLEL_THRESHOLD,
            "test fixture must be below threshold"
        );

        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], id_size);
        assert!(
            !result.index.class_dumps.is_empty() || !result.heap_record_ranges.is_empty(),
            "should produce valid results via \
             sequential path"
        );
    }

    #[test]
    fn sequential_path_reports_all_segments() {
        let id_size = 8u32;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_instance(1, 0, 100, &[1])
            .add_instance(2, 0, 100, &[2])
            .add_instance(3, 0, 100, &[3])
            .build();

        assert!(
            (bytes.len() as u64) < PARALLEL_THRESHOLD,
            "fixture must stay on sequential path"
        );

        let start = advance_past_header(&bytes);
        let (result, obs) = run_fp_with_test_observer(&bytes[start..], id_size);
        let events = segment_completed_events(&obs);
        let expected_total = result.heap_record_ranges.len();

        assert!(expected_total > 0, "expected at least one heap segment");
        assert_eq!(events.len(), expected_total);

        for (idx, (done, total)) in events.iter().enumerate() {
            assert_eq!(*done, idx + 1, "done must be 1..N");
            assert_eq!(*total, expected_total, "total must stay constant");
            assert!(*done <= *total, "done must not exceed total");
        }
    }

    #[test]
    fn parallel_path_reports_all_segments() {
        let id_size = 8u32;
        let big_data = vec![0u8; 33 * 1024 * 1024];
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_class_dump(100, 0, big_data.len() as u32, &[])
            .add_instance(42, 0, 100, &big_data)
            .build();

        let start = advance_past_header(&bytes);
        let data = &bytes[start..];
        assert!(
            (data.len() as u64) >= PARALLEL_THRESHOLD,
            "fixture must trigger parallel path"
        );

        let (result, obs) = run_fp_with_test_observer(data, id_size);
        let events = segment_completed_events(&obs);
        let expected_total = result.heap_record_ranges.len();

        assert!(expected_total > 0, "expected at least one heap segment");
        assert_eq!(events.len(), expected_total);

        for (idx, (done, total)) in events.iter().enumerate() {
            assert_eq!(*done, idx + 1, "done must be 1..N");
            assert_eq!(*total, expected_total, "total must stay constant");
            assert!(*done <= *total, "done must not exceed total");
        }
    }

    #[test]
    fn segment_done_never_exceeds_total() {
        let id_size = 8u32;
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
            .add_instance(10, 0, 100, &[1])
            .add_instance(11, 0, 100, &[2])
            .add_instance(12, 0, 100, &[3])
            .build();

        let start = advance_past_header(&bytes);
        let (_result, obs) = run_fp_with_test_observer(&bytes[start..], id_size);
        let events = segment_completed_events(&obs);

        assert!(!events.is_empty(), "expected segment progress events");
        for (done, total) in events {
            assert!(done <= total, "done must not exceed total");
        }
    }

    // =====================================================
    // Story 10.1 — Progress after heap segment skip
    // =====================================================

    /// Builds a record header (9 bytes): tag + timestamp(0)
    /// + payload length.
    #[cfg(feature = "test-utils")]
    fn build_record_header(buf: &mut Vec<u8>, tag: u8, payload_len: u32) {
        buf.write_u8(tag).unwrap();
        buf.write_u32::<BigEndian>(0).unwrap(); // timestamp
        buf.write_u32::<BigEndian>(payload_len).unwrap();
    }

    #[cfg(feature = "test-utils")]
    const RECORD_HEADER_SIZE: usize = 9;

    /// Test 2.1: STRING + large HeapDumpSegment + STRING.
    /// Progress must include a position after the segment
    /// skip, not just string positions.
    #[test]
    #[cfg(feature = "test-utils")]
    fn progress_reports_after_heap_segment_skip() {
        let id_size = 8u32;
        let seg_payload_len = PROGRESS_REPORT_INTERVAL as u32 + 1024;

        // STRING record: id(8 bytes) + "hi"(2 bytes)
        let str_payload = {
            let mut p = Vec::new();
            p.write_u64::<BigEndian>(1).unwrap();
            p.extend_from_slice(b"hi");
            p
        };

        let total_size = RECORD_HEADER_SIZE
            + str_payload.len()
            + RECORD_HEADER_SIZE
            + seg_payload_len as usize
            + RECORD_HEADER_SIZE
            + str_payload.len();

        let mut data = vec![0u8; total_size];
        let mut offset = 0;

        // STRING #1
        {
            let mut hdr = Vec::new();
            build_record_header(&mut hdr, 0x01, str_payload.len() as u32);
            data[offset..offset + hdr.len()].copy_from_slice(&hdr);
            offset += hdr.len();
            data[offset..offset + str_payload.len()].copy_from_slice(&str_payload);
            offset += str_payload.len();
        }

        // HeapDumpSegment (0x1C)
        let heap_seg_start = offset;
        {
            let mut hdr = Vec::new();
            build_record_header(&mut hdr, 0x1C, seg_payload_len);
            data[offset..offset + hdr.len()].copy_from_slice(&hdr);
            offset += hdr.len();
            // payload is zeros (already allocated)
            offset += seg_payload_len as usize;
        }
        let heap_seg_end = (heap_seg_start + RECORD_HEADER_SIZE + seg_payload_len as usize) as u64;

        // STRING #2
        {
            let mut hdr = Vec::new();
            build_record_header(&mut hdr, 0x01, str_payload.len() as u32);
            data[offset..offset + hdr.len()].copy_from_slice(&hdr);
            offset += hdr.len();
            // Reuse str_payload but with different id
            let mut p2 = Vec::new();
            p2.write_u64::<BigEndian>(2).unwrap();
            p2.extend_from_slice(b"hi");
            data[offset..offset + p2.len()].copy_from_slice(&p2);
        }

        let (_result, obs) = run_fp_with_test_observer(&data, id_size);
        let positions = bytes_scanned_positions(&obs);

        let has_post_segment = positions.iter().any(|&p| p >= heap_seg_end);
        assert!(
            has_post_segment,
            "expected BytesScanned position >= {heap_seg_end} \
             (after heap segment skip), got: {positions:?}"
        );
    }

    /// Test 2.2: Two heap segments only (no structural
    /// records). Must produce exactly 2 BytesScanned events.
    ///
    /// Relies on synchronous execution: each segment
    /// exceeds `PROGRESS_REPORT_INTERVAL` (4 MB) so the
    /// bytes throttle fires once per segment. The time
    /// throttle (1 s) never triggers because the scan loop
    /// is CPU-bound with no real I/O delay.
    #[test]
    #[cfg(feature = "test-utils")]
    fn progress_heap_only_dump_two_segments() {
        let id_size = 8u32;
        let seg_payload_len = PROGRESS_REPORT_INTERVAL as u32 + 1024;

        let total_size = 2 * (RECORD_HEADER_SIZE + seg_payload_len as usize);
        let mut data = vec![0u8; total_size];
        let mut offset = 0;

        for _ in 0..2 {
            let mut hdr = Vec::new();
            build_record_header(&mut hdr, 0x1C, seg_payload_len);
            data[offset..offset + hdr.len()].copy_from_slice(&hdr);
            offset += hdr.len() + seg_payload_len as usize;
        }

        let (_result, obs) = run_fp_with_test_observer(&data, id_size);
        let positions = bytes_scanned_positions(&obs);

        assert_eq!(
            positions.len(),
            2,
            "expected exactly 2 BytesScanned events \
             (one per segment), got: {positions:?}"
        );
    }

    /// Test 2.3: Small heap segment (1-byte payload) — no
    /// regression. Only the catch-all event fires.
    #[test]
    #[cfg(feature = "test-utils")]
    fn progress_small_heap_segment_no_extra_event() {
        let id_size = 8u32;
        let seg_payload_len = 1u32;
        let total_size = RECORD_HEADER_SIZE + seg_payload_len as usize;
        let mut data = vec![0u8; total_size];
        let mut hdr = Vec::new();
        build_record_header(&mut hdr, 0x1C, seg_payload_len);
        data[..hdr.len()].copy_from_slice(&hdr);

        let (_result, obs) = run_fp_with_test_observer(&data, id_size);
        let positions = bytes_scanned_positions(&obs);

        assert_eq!(
            positions.len(),
            1,
            "expected exactly 1 BytesScanned event \
             (catch-all only), got: {positions:?}"
        );
        assert_eq!(
            positions[0], total_size as u64,
            "catch-all should report end of blob"
        );
    }

    /// Test 2.4: Multiple heap segments interleaved with
    /// structural records. BytesScanned positions must be
    /// monotonically increasing and include post-segment
    /// positions.
    #[test]
    #[cfg(feature = "test-utils")]
    fn progress_interleaved_segments_monotonic() {
        let id_size = 8u32;
        let seg_payload_len = PROGRESS_REPORT_INTERVAL as u32 + 1024;
        let str_payload = {
            let mut p = Vec::new();
            p.write_u64::<BigEndian>(1).unwrap();
            p.extend_from_slice(b"ab");
            p
        };

        // Layout: SEG + STRING + SEG + STRING
        let total_size = 2 * (RECORD_HEADER_SIZE + seg_payload_len as usize)
            + 2 * (RECORD_HEADER_SIZE + str_payload.len());
        let mut data = vec![0u8; total_size];
        let mut offset = 0;
        let mut seg_ends = Vec::new();

        for i in 0..2 {
            // Heap segment
            let mut hdr = Vec::new();
            build_record_header(&mut hdr, 0x1C, seg_payload_len);
            data[offset..offset + hdr.len()].copy_from_slice(&hdr);
            offset += hdr.len() + seg_payload_len as usize;
            seg_ends.push(offset as u64);

            // STRING record
            let mut shdr = Vec::new();
            build_record_header(&mut shdr, 0x01, str_payload.len() as u32);
            data[offset..offset + shdr.len()].copy_from_slice(&shdr);
            offset += shdr.len();
            let mut sp = Vec::new();
            sp.write_u64::<BigEndian>((i + 10) as u64).unwrap();
            sp.extend_from_slice(b"ab");
            data[offset..offset + sp.len()].copy_from_slice(&sp);
            offset += sp.len();
        }

        let (_result, obs) = run_fp_with_test_observer(&data, id_size);
        let positions = bytes_scanned_positions(&obs);

        // Monotonically increasing
        for w in positions.windows(2) {
            assert!(w[1] > w[0], "positions not monotonic: {positions:?}");
        }

        // Each segment end is covered
        for seg_end in &seg_ends {
            let has = positions.iter().any(|&p| p >= *seg_end);
            assert!(
                has,
                "no BytesScanned >= segment end \
                 {seg_end}, got: {positions:?}"
            );
        }
    }

    /// Test 2.5: HeapDump tag (0x0C) variant also triggers
    /// progress reporting after skip.
    #[test]
    #[cfg(feature = "test-utils")]
    fn progress_heap_dump_tag_variant() {
        let id_size = 8u32;
        let seg_payload_len = PROGRESS_REPORT_INTERVAL as u32 + 1024;
        let total_size = RECORD_HEADER_SIZE + seg_payload_len as usize;
        let mut data = vec![0u8; total_size];
        let mut hdr = Vec::new();
        build_record_header(
            &mut hdr,
            0x0C, // HeapDump tag
            seg_payload_len,
        );
        data[..hdr.len()].copy_from_slice(&hdr);

        let (_result, obs) = run_fp_with_test_observer(&data, id_size);
        let positions = bytes_scanned_positions(&obs);

        // Should have at least one event from the new code
        // path (segment skip) — the payload exceeds the
        // 4 MB throttle.
        assert!(
            !positions.is_empty(),
            "expected at least one BytesScanned event"
        );
        assert_eq!(
            positions.last().copied().unwrap(),
            total_size as u64,
            "last position should be end of blob"
        );
    }
}

// ---- Story 10.2: Chunked heap extraction tests ----

#[cfg(feature = "test-utils")]
mod chunked_extraction_tests {
    use super::*;
    use byteorder::{BigEndian, WriteBytesExt};

    fn write_id(buf: &mut Vec<u8>, id: u64, id_size: u32) {
        if id_size == 8 {
            buf.write_u64::<BigEndian>(id).unwrap();
        } else {
            buf.write_u32::<BigEndian>(id as u32).unwrap();
        }
    }

    fn make_instance_sub(obj_id: u64, class_id: u64, field_data: &[u8], id_size: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x21).unwrap(); // InstanceDump
        write_id(&mut p, obj_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap(); // stack
        write_id(&mut p, class_id, id_size);
        p.write_u32::<BigEndian>(field_data.len() as u32).unwrap();
        p.extend_from_slice(field_data);
        p
    }

    fn make_prim_array_sub(arr_id: u64, elem_type: u8, elements: &[u8], id_size: u32) -> Vec<u8> {
        let elem_size = primitive_element_size(elem_type);
        let num_elements = if elem_size > 0 {
            elements.len() / elem_size
        } else {
            0
        };
        let mut p = Vec::new();
        p.write_u8(0x23).unwrap(); // PrimArrayDump
        write_id(&mut p, arr_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap(); // stack
        p.write_u32::<BigEndian>(num_elements as u32).unwrap();
        p.write_u8(elem_type).unwrap();
        p.extend_from_slice(elements);
        p
    }

    fn make_class_dump_sub(
        class_object_id: u64,
        super_class_id: u64,
        instance_size: u32,
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x20).unwrap(); // ClassDump
        write_id(&mut p, class_object_id, id_size);
        p.write_u32::<BigEndian>(0).unwrap();
        write_id(&mut p, super_class_id, id_size);
        for _ in 0..5 {
            write_id(&mut p, 0, id_size);
        }
        p.write_u32::<BigEndian>(instance_size).unwrap();
        p.write_u16::<BigEndian>(0).unwrap(); // const
        p.write_u16::<BigEndian>(0).unwrap(); // static
        p.write_u16::<BigEndian>(0).unwrap(); // inst
        p
    }

    fn make_gc_root_java_frame_sub(
        object_id: u64,
        thread_serial: u32,
        frame_number: i32,
        id_size: u32,
    ) -> Vec<u8> {
        let mut p = Vec::new();
        p.write_u8(0x03).unwrap(); // GcRootJavaFrame
        write_id(&mut p, object_id, id_size);
        p.write_u32::<BigEndian>(thread_serial).unwrap();
        p.write_i32::<BigEndian>(frame_number).unwrap();
        p
    }

    /// 5.1: 6 InstanceDumps, chunk to hold ~2 each
    #[test]
    fn chunked_6_instances_3_chunks() {
        let id_size = 8u32;
        let field_data = [0u8; 8];
        let mut payload = Vec::new();
        for i in 1..=6u64 {
            payload.extend(make_instance_sub(i, 100, &field_data, id_size));
        }
        // Each InstanceDump: 1+8+4+8+4+8 = 33 bytes
        let record_size = 33usize;
        // Chunk boundary exactly at 2 records = 66 bytes
        // (cursor >= 66 triggers flush after 2nd record)
        let max_chunk = record_size * 2;

        let result = extract_heap_segment(&payload, 0, id_size, max_chunk);
        assert_eq!(result.chunks.len(), 3, "6 records / 2 per chunk = 3 chunks");
        for chunk in &result.chunks {
            assert_eq!(
                chunk.all_offsets.len(),
                2,
                "each chunk should have 2 offsets"
            );
        }
    }

    /// 5.2: max_chunk_bytes > payload → single chunk
    #[test]
    fn no_chunking_when_max_exceeds_payload() {
        let id_size = 8u32;
        let field_data = [0u8; 8];
        let mut payload = Vec::new();
        for i in 1..=6u64 {
            payload.extend(make_instance_sub(i, 100, &field_data, id_size));
        }
        let single = extract_heap_segment(&payload, 0, id_size, usize::MAX);
        assert_eq!(single.chunks.len(), 1, "large max → single chunk");
        assert_eq!(single.chunks[0].all_offsets.len(), 6);
    }

    /// 5.3: chunk boundary exactly at sub-record end
    #[test]
    fn chunk_boundary_at_exact_record_end() {
        let id_size = 8u32;
        let field_data = [0u8; 8];
        let mut payload = Vec::new();
        for i in 1..=4u64 {
            payload.extend(make_instance_sub(i, 100, &field_data, id_size));
        }
        // Each record: 1+8+4+8+4+8 = 33 bytes
        // Set chunk exactly at 2 records = 66 bytes
        let max_chunk = 66;
        let result = extract_heap_segment(&payload, 0, id_size, max_chunk);
        assert_eq!(result.chunks.len(), 2);
        assert_eq!(result.chunks[0].all_offsets.len(), 2);
        assert_eq!(result.chunks[1].all_offsets.len(), 2);
    }

    /// 5.4: mixed sub-records; single vs multi-chunk
    /// merged results must be identical
    #[test]
    fn mixed_records_single_vs_multi_chunk_identical() {
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.extend(make_instance_sub(1, 100, &[0u8; 8], id_size));
        payload.extend(make_prim_array_sub(
            2, 10, // int
            &[0u8; 16], id_size,
        ));
        payload.extend(make_class_dump_sub(3, 0, 32, id_size));
        payload.extend(make_gc_root_java_frame_sub(4, 1, 0, id_size));

        // Single chunk
        let single = extract_heap_segment(&payload, 0, id_size, usize::MAX);
        // Multi chunk (small boundary)
        let multi = extract_heap_segment(&payload, 0, id_size, 40);
        assert!(multi.chunks.len() > 1, "must produce multiple chunks");

        // Merge both into separate contexts and compare
        let mut ctx_s = FirstPassContext::new(&[], 8, 0, MemoryBudget::Unlimited);
        single.merge_into(&mut ctx_s);

        let mut ctx_m = FirstPassContext::new(&[], 8, 0, MemoryBudget::Unlimited);
        multi.merge_into(&mut ctx_m);

        // Sort for comparison
        let mut s_offsets: Vec<u64> = ctx_s.all_offsets.iter().map(|o| o.object_id).collect();
        s_offsets.sort_unstable();
        let mut m_offsets: Vec<u64> = ctx_m.all_offsets.iter().map(|o| o.object_id).collect();
        m_offsets.sort_unstable();
        assert_eq!(s_offsets, m_offsets, "all_offsets object_ids must match");

        let s_frame_roots = ctx_s.raw_frame_roots.len();
        let m_frame_roots = ctx_m.raw_frame_roots.len();
        let s_class_dumps = ctx_s.result.index.class_dumps.len();
        let m_class_dumps = ctx_m.result.index.class_dumps.len();
        assert_eq!(
            s_frame_roots, m_frame_roots,
            "raw_frame_roots count must match"
        );
        assert_eq!(s_class_dumps, m_class_dumps, "class_dumps count must match");

        // Verify filter_ids equivalence via segment_filters (AC3)
        let idx_s = ctx_s.finish();
        let idx_m = ctx_m.finish();
        assert_eq!(
            idx_s.segment_filters.len(),
            idx_m.segment_filters.len(),
            "segment_filter count must match (validates filter_ids identity)"
        );
    }

    /// 5.5: full plumbing through `run_first_pass`
    /// with MemoryBudget::Bytes(512)
    #[test]
    fn run_first_pass_with_budget_bytes() {
        use crate::test_utils::HprofTestBuilder;

        // Build a blob with many InstanceDumps
        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=10u64 {
            builder = builder.add_instance(i, 0, 100, &[0u8; 16]);
        }
        let bytes = builder.build();
        let start = crate::test_utils::advance_past_header(&bytes);

        // With budget
        let mut obs = NullProgressObserver;
        let mut notifier = ProgressNotifier::new(&mut obs);
        let result_budgeted = run_first_pass(
            &bytes[start..],
            8,
            start as u64,
            &mut notifier,
            MemoryBudget::Bytes(512),
        );

        // Without budget
        let mut obs2 = NullProgressObserver;
        let mut notifier2 = ProgressNotifier::new(&mut obs2);
        let result_none = run_first_pass(
            &bytes[start..],
            8,
            start as u64,
            &mut notifier2,
            MemoryBudget::Unlimited,
        );

        // Same segment filters and records indexed
        assert_eq!(
            result_budgeted.segment_filters.len(),
            result_none.segment_filters.len(),
        );
        assert_eq!(result_budgeted.records_indexed, result_none.records_indexed,);
        assert_eq!(result_budgeted.warnings.len(), result_none.warnings.len(),);

        // Value-for-value: instance_offsets keys and offsets
        // (derived from all_offsets — verifies plumbing through
        // FirstPassContext → extract_all → merge_into).
        let mut budgeted_ids: Vec<u64> = result_budgeted
            .index
            .instance_offsets
            .keys()
            .cloned()
            .collect();
        budgeted_ids.sort_unstable();
        let mut none_ids: Vec<u64> = result_none.index.instance_offsets.keys().cloned().collect();
        none_ids.sort_unstable();
        assert_eq!(
            budgeted_ids, none_ids,
            "instance_offsets keys must match value-for-value"
        );
        for id in &none_ids {
            assert_eq!(
                result_budgeted.index.instance_offsets.get(id),
                result_none.index.instance_offsets.get(id),
                "instance_offset for id {id:#X} must match"
            );
        }
    }

    /// 5.6: run_first_pass with budget_bytes = None
    /// is identical to current behavior
    #[test]
    fn run_first_pass_none_budget_regression() {
        use crate::test_utils::HprofTestBuilder;

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[1, 2, 3, 4])
            .build();
        let start = crate::test_utils::advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        assert_eq!(
            result.segment_filters.len(),
            1,
            "must produce one segment filter"
        );
        assert!(result.warnings.is_empty());
        assert_eq!(
            result.heap_record_ranges.len(),
            1,
            "must have one heap record range"
        );
    }

    /// 5.7: truncated sub-record at end of payload
    #[test]
    fn chunked_extraction_truncated_sub_record() {
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.extend(make_instance_sub(1, 100, &[0u8; 8], id_size));
        payload.extend(make_instance_sub(2, 100, &[0u8; 8], id_size));
        // Add truncated record: sub-tag + partial id
        payload.push(0x21); // InstanceDump tag
        payload.extend(&[0u8; 4]); // partial id

        let chunked = extract_heap_segment(&payload, 0, id_size, 40);
        let unchunked = extract_heap_segment(&payload, 0, id_size, usize::MAX);

        // Both must find exactly 2 records (break on 3rd)
        let c_total: usize = chunked.chunks.iter().map(|c| c.all_offsets.len()).sum();
        let u_total: usize = unchunked.chunks.iter().map(|c| c.all_offsets.len()).sum();
        assert_eq!(c_total, 2);
        assert_eq!(u_total, 2);
    }

    /// 5.8: data_offset correctness across chunks
    #[test]
    fn data_offset_correctness_across_chunks() {
        let id_size = 8u32;
        let field_data = [0u8; 8];
        let data_offset = 500usize;
        let mut payload = Vec::new();
        for i in 1..=4u64 {
            payload.extend(make_instance_sub(i, 100, &field_data, id_size));
        }
        // Also add a PrimArrayDump
        payload.extend(make_prim_array_sub(
            5, 10, // int
            &[0u8; 16], id_size,
        ));

        let result = extract_heap_segment(&payload, data_offset, id_size, 40);
        assert!(result.chunks.len() > 1, "must produce multiple chunks");

        // Verify every ObjectOffset.offset and
        // FilterEntry.data_offset uses absolute offsets
        for chunk in &result.chunks {
            for o in &chunk.all_offsets {
                assert!(
                    o.offset >= data_offset as u64,
                    "offset {} must be >= data_offset {}",
                    o.offset,
                    data_offset,
                );
            }
            for f in &chunk.filter_ids {
                assert!(
                    f.data_offset >= data_offset,
                    "filter data_offset {} must be \
                     >= data_offset {}",
                    f.data_offset,
                    data_offset,
                );
            }
        }
    }

    /// 5.9: single-chunk HeapSegmentParsingResult.
    /// merge_into produces same state as direct
    /// merge_segment_result call.
    #[test]
    fn single_chunk_merge_same_as_direct() {
        let id_size = 8u32;
        let mut payload = Vec::new();
        payload.extend(make_instance_sub(1, 100, &[0u8; 8], id_size));
        payload.extend(make_instance_sub(2, 100, &[0u8; 8], id_size));

        // Direct extraction (usize::MAX = single chunk)
        let parsing_result = extract_heap_segment(&payload, 0, id_size, usize::MAX);
        assert_eq!(parsing_result.chunks.len(), 1);

        // Get the single result for direct merge
        let direct_result = extract_heap_segment(&payload, 0, id_size, usize::MAX);

        // Merge via HeapSegmentParsingResult
        let mut ctx_wrapper = FirstPassContext::new(&[], 8, 0, MemoryBudget::Unlimited);
        parsing_result.merge_into(&mut ctx_wrapper);

        // Merge via direct merge_segment_result
        let mut ctx_direct = FirstPassContext::new(&[], 8, 0, MemoryBudget::Unlimited);
        for chunk in direct_result.chunks {
            merge_segment_result(&mut ctx_direct, chunk);
        }

        assert_eq!(ctx_wrapper.all_offsets.len(), ctx_direct.all_offsets.len(),);
        for (w, d) in ctx_wrapper
            .all_offsets
            .iter()
            .zip(ctx_direct.all_offsets.iter())
        {
            assert_eq!(w.object_id, d.object_id);
            assert_eq!(w.offset, d.offset);
        }
    }

    /// 5.10: empty payload → zero chunks
    #[test]
    fn empty_payload_zero_chunks() {
        let result = extract_heap_segment(&[], 0, 8, 100);
        assert!(
            result.chunks.is_empty(),
            "empty payload should produce no chunks"
        );

        // merge_into is a no-op
        let mut ctx = FirstPassContext::new(&[], 8, 0, MemoryBudget::Unlimited);
        result.merge_into(&mut ctx);
        assert!(ctx.all_offsets.is_empty());
        assert!(ctx.raw_frame_roots.is_empty());
    }
}

// ---- Story 10.3: Budget-aware batching tests ----

#[cfg(feature = "test-utils")]
mod budget_batching_tests {
    use super::*;
    use crate::indexer::HeapRecordRange;

    // ---- compute_batch_ranges unit tests ----

    fn make_ranges(sizes: &[u64]) -> Vec<HeapRecordRange> {
        let mut offset = 0u64;
        sizes
            .iter()
            .map(|&len| {
                let r = HeapRecordRange {
                    payload_start: offset,
                    payload_length: len,
                };
                offset += len;
                r
            })
            .collect()
    }

    /// 10.3-unit-1: all segments fit in one batch
    #[test]
    fn batch_all_fit_single_batch() {
        let ranges = make_ranges(&[100, 200, 300]);
        let batches = compute_batch_ranges(&ranges, 1000);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], 0..3);
    }

    /// 10.3-unit-2: segments split across two batches
    #[test]
    fn batch_split_across_two() {
        // 300 + 300 = 600 > 500 → first batch [0..1],
        // second batch [1..3]
        let ranges = make_ranges(&[300, 300, 200]);
        let batches = compute_batch_ranges(&ranges, 500);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0], 0..1);
        assert_eq!(batches[1], 1..3);
    }

    /// 10.3-unit-3: single oversized segment → solo batch
    #[test]
    fn batch_single_oversized_segment() {
        let ranges = make_ranges(&[1000, 100, 100]);
        let batches = compute_batch_ranges(&ranges, 500);
        assert_eq!(batches.len(), 2);
        // First segment alone (oversized)
        assert_eq!(batches[0], 0..1);
        // Remaining fit together
        assert_eq!(batches[1], 1..3);
    }

    /// 10.3-unit-4: all segments exceed budget →
    /// each in its own batch
    #[test]
    fn batch_all_oversized() {
        let ranges = make_ranges(&[600, 700, 800]);
        let batches = compute_batch_ranges(&ranges, 500);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0], 0..1);
        assert_eq!(batches[1], 1..2);
        assert_eq!(batches[2], 2..3);
    }

    /// 10.3-unit-5: empty ranges → empty batches
    #[test]
    fn batch_empty_ranges() {
        let ranges: Vec<HeapRecordRange> = Vec::new();
        let batches = compute_batch_ranges(&ranges, 500);
        assert!(batches.is_empty());
    }

    /// 10.3-unit-6: exact fit → single batch
    #[test]
    fn batch_exact_fit() {
        let ranges = make_ranges(&[250, 250]);
        let batches = compute_batch_ranges(&ranges, 500);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], 0..2);
    }

    /// 10.3-unit-7: u64::MAX budget → single batch
    #[test]
    fn batch_unlimited_budget() {
        let ranges = make_ranges(&[100, 200, 300, 400]);
        let batches = compute_batch_ranges(&ranges, u64::MAX);
        assert_eq!(batches.len(), 1);
    }

    /// 10.3-unit-8: multiple batches with mixed sizes
    #[test]
    fn batch_realistic_distribution() {
        // budget = 800
        // [400, 400] = 800 ≤ 800 → batch 1
        // [400, 200, 200] = 800 ≤ 800 → batch 2
        // [100] → batch 3
        let ranges = make_ranges(&[400, 400, 400, 200, 200, 100]);
        let batches = compute_batch_ranges(&ranges, 800);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0], 0..2);
        assert_eq!(batches[1], 2..5);
        assert_eq!(batches[2], 5..6);
    }

    // ---- Integration tests via run_first_pass ----

    /// Helper: run first pass with budget, returning both
    /// result and observer events.
    fn run_fp_with_budget(
        data: &[u8],
        id_size: u32,
        budget: MemoryBudget,
    ) -> (IndexResult, hprof_api::TestObserver) {
        let mut obs = hprof_api::TestObserver::default();
        let result = {
            let mut notifier = ProgressNotifier::new(&mut obs);
            run_first_pass(data, id_size, 0, &mut notifier, budget)
        };
        (result, obs)
    }

    /// 3.1a: batching occurs — progress events
    /// segment_completed are cumulative.
    #[test]
    fn budget_batching_progress_events_cumulative() {
        use crate::test_utils::HprofTestBuilder;
        use hprof_api::ProgressEvent;

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=5u64 {
            builder = builder.add_instance(i, 0, 100, &[0u8; 16]);
        }
        let bytes = builder.build();
        let start = crate::test_utils::advance_past_header(&bytes);

        let (_, obs) = run_fp_with_budget(&bytes[start..], 8, MemoryBudget::Bytes(64));

        let seg_events: Vec<(usize, usize)> = obs
            .events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::SegmentCompleted { done, total } => Some((*done, *total)),
                _ => None,
            })
            .collect();

        assert_eq!(seg_events.len(), 5);
        // Final event must be (5, 5)
        assert_eq!(
            *seg_events.last().unwrap(),
            (5, 5),
            "final segment_completed must be \
             (total, total)"
        );
        // Events must be monotonically increasing
        for w in seg_events.windows(2) {
            assert!(w[1].0 > w[0].0, "done must be strictly increasing");
        }
        // All report against total_segments = 5
        for (_, total) in &seg_events {
            assert_eq!(*total, 5);
        }
    }

    /// 3.1b: results identical with budget vs without.
    #[test]
    fn budget_batching_results_identical() {
        use crate::test_utils::HprofTestBuilder;

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=8u64 {
            builder = builder.add_instance(i, 0, 100, &[0u8; 32]);
        }
        let bytes = builder.build();
        let start = crate::test_utils::advance_past_header(&bytes);

        let (result_budget, _) = run_fp_with_budget(&bytes[start..], 8, MemoryBudget::Bytes(128));
        let (result_none, _) = run_fp_with_budget(&bytes[start..], 8, MemoryBudget::Unlimited);

        // Same segment filters
        assert_eq!(
            result_budget.segment_filters.len(),
            result_none.segment_filters.len(),
        );

        // Same records indexed
        assert_eq!(result_budget.records_indexed, result_none.records_indexed,);

        // Same heap record ranges
        assert_eq!(
            result_budget.heap_record_ranges.len(),
            result_none.heap_record_ranges.len(),
        );

        // Same warnings
        assert_eq!(result_budget.warnings.len(), result_none.warnings.len(),);
    }

    /// 3.2: regression — unlimited budget matches
    /// existing behavior.
    #[test]
    fn budget_none_regression() {
        use crate::test_utils::HprofTestBuilder;

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xDEAD, 0, 100, &[1, 2, 3, 4])
            .build();
        let start = crate::test_utils::advance_past_header(&bytes);

        let result = run_fp(&bytes[start..], 8);
        assert_eq!(result.segment_filters.len(), 1);
        assert!(result.warnings.is_empty());
        assert_eq!(result.heap_record_ranges.len(), 1);
    }

    /// 3.3: single segment larger than budget →
    /// processed alone (not skipped).
    #[test]
    fn budget_single_oversized_segment_processed() {
        use crate::test_utils::HprofTestBuilder;

        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(0xBEEF, 0, 42, &[0u8; 256])
            .build();
        let start = crate::test_utils::advance_past_header(&bytes);

        // Budget smaller than segment payload
        let (result, _) = run_fp_with_budget(&bytes[start..], 8, MemoryBudget::Bytes(16));
        assert_eq!(
            result.segment_filters.len(),
            1,
            "oversized segment must produce filters"
        );
        assert_eq!(result.heap_record_ranges.len(), 1);
    }

    /// 3.4: HprofFile::from_path still compiles and works
    /// without budget_bytes.
    #[test]
    fn hprof_file_from_path_no_budget_compiles() {
        let bytes = crate::test_utils::HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(1, 0, 100, &[0u8; 4])
            .build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &bytes).unwrap();
        std::io::Write::flush(&mut tmp).unwrap();
        let hfile = crate::HprofFile::from_path(tmp.path()).unwrap();
        assert_eq!(hfile.segment_filters.len(), 1);
        assert_eq!(hfile.heap_record_ranges.len(), 1);
    }

    /// 3.5: budget_bytes = 0 → floor kicks in,
    /// extraction completes normally.
    #[test]
    fn budget_zero_floor_kicks_in() {
        use crate::test_utils::HprofTestBuilder;

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=3u64 {
            builder = builder.add_instance(i, 0, 100, &[0u8; 8]);
        }
        let bytes = builder.build();
        let start = crate::test_utils::advance_past_header(&bytes);

        // budget = 0 → floor = 64 MB
        let (result, _) = run_fp_with_budget(&bytes[start..], 8, MemoryBudget::Bytes(0));
        // 3 HEAP_DUMP_SEGMENT records
        assert_eq!(result.heap_record_ranges.len(), 3);
        assert!(result.warnings.is_empty());
    }

    /// 3.6: all segments exceed budget individually →
    /// each processed in solo batch (verified via
    /// compute_batch_ranges).
    #[test]
    fn budget_all_segments_exceed_individually() {
        // Synthetic ranges: each 100 MB, budget = 80 MB
        let ranges = make_ranges(&[100 * 1024 * 1024, 100 * 1024 * 1024, 100 * 1024 * 1024]);
        let batches = compute_batch_ranges(&ranges, 80 * 1024 * 1024);
        // Each segment exceeds 80 MB → solo batch
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0], 0..1);
        assert_eq!(batches[1], 1..2);
        assert_eq!(batches[2], 2..3);
    }

    /// 3.6b: integration — all segments individually
    /// oversized (but floor dominates for tiny test data).
    #[test]
    fn budget_all_oversized_extraction_succeeds() {
        use crate::test_utils::HprofTestBuilder;

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=4u64 {
            builder = builder.add_instance(i, 0, 100, &[0u8; 512]);
        }
        let bytes = builder.build();
        let start = crate::test_utils::advance_past_header(&bytes);

        let (result, _) = run_fp_with_budget(&bytes[start..], 8, MemoryBudget::Bytes(1));
        // All 4 segments found despite tiny budget
        assert_eq!(result.heap_record_ranges.len(), 4);
        assert!(result.warnings.is_empty());
    }

    /// 3.7: end-to-end plumbing through HprofFile.
    #[test]
    fn budget_e2e_through_hprof_file() {
        use crate::test_utils::HprofTestBuilder;

        let mut builder = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=5u64 {
            builder = builder.add_instance(i, 0, 100, &[0u8; 16]);
        }
        let bytes = builder.build();
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp, &bytes).unwrap();
        std::io::Write::flush(&mut tmp).unwrap();

        // With budget
        let hfile_budget = crate::HprofFile::from_path_with_progress(
            tmp.path(),
            &mut hprof_api::NullProgressObserver,
            MemoryBudget::Bytes(128),
        )
        .unwrap();

        // Without budget
        let hfile_none = crate::HprofFile::from_path(tmp.path()).unwrap();

        assert_eq!(
            hfile_budget.segment_filters.len(),
            hfile_none.segment_filters.len(),
        );
        assert_eq!(hfile_budget.records_indexed, hfile_none.records_indexed,);
        assert_eq!(
            hfile_budget.heap_record_ranges.len(),
            hfile_none.heap_record_ranges.len(),
        );
    }
}
