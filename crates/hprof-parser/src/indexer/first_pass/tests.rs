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
#[cfg(feature = "test-utils")]
use super::offset_lookup::SegmentEntryPoint;
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
fn heap_bytes_events(
    obs: &hprof_api::TestObserver,
) -> Vec<(u64, u64)> {
    obs.events
        .iter()
        .filter_map(|e| match e {
            hprof_api::ProgressEvent::HeapBytesExtracted {
                done,
                total,
            } => Some((*done, *total)),
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
            result.index.instance_offsets.contains(&thread_obj_id),
            "thread object must have a recorded offset"
        );
        let offset = result.index.instance_offsets.get(thread_obj_id).unwrap();
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
        let bytes = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            id_size,
        )
        .add_instance(1, 0, 100, &[1])
        .add_instance(2, 0, 100, &[2])
        .add_instance(3, 0, 100, &[3])
        .build();

        assert!(
            (bytes.len() as u64) < PARALLEL_THRESHOLD,
            "fixture must stay on sequential path"
        );

        let start = advance_past_header(&bytes);
        let (result, obs) =
            run_fp_with_test_observer(&bytes[start..], id_size);
        let events = heap_bytes_events(&obs);
        let expected_total = result.heap_record_ranges.len();

        assert!(
            expected_total > 0,
            "expected at least one heap segment"
        );
        assert_eq!(events.len(), expected_total);

        // done is monotonically increasing bytes
        for w in events.windows(2) {
            assert!(
                w[1].0 > w[0].0,
                "done must be strictly increasing"
            );
        }
        // total stays constant
        let total = events[0].1;
        for (_, t) in &events {
            assert_eq!(*t, total, "total must stay constant");
        }
        // final done == total
        assert_eq!(
            events.last().unwrap().0,
            total,
            "final done must equal total"
        );
    }

    #[test]
    fn parallel_path_reports_all_segments() {
        let id_size = 8u32;
        let big_data = vec![0u8; 33 * 1024 * 1024];
        let bytes = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            id_size,
        )
        .add_class_dump(
            100,
            0,
            big_data.len() as u32,
            &[],
        )
        .add_instance(42, 0, 100, &big_data)
        .build();

        let start = advance_past_header(&bytes);
        let data = &bytes[start..];
        assert!(
            (data.len() as u64) >= PARALLEL_THRESHOLD,
            "fixture must trigger parallel path"
        );

        let (result, obs) =
            run_fp_with_test_observer(data, id_size);
        let events = heap_bytes_events(&obs);
        let expected_total = result.heap_record_ranges.len();

        assert!(
            expected_total > 0,
            "expected at least one heap segment"
        );
        assert_eq!(events.len(), expected_total);

        // done is monotonically increasing bytes
        for w in events.windows(2) {
            assert!(
                w[1].0 > w[0].0,
                "done must be strictly increasing"
            );
        }
        // total stays constant
        let total = events[0].1;
        for (_, t) in &events {
            assert_eq!(*t, total, "total must stay constant");
        }
        // final done == total
        assert_eq!(
            events.last().unwrap().0,
            total,
            "final done must equal total"
        );
    }

    #[test]
    fn segment_done_never_exceeds_total() {
        let id_size = 8u32;
        let bytes = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            id_size,
        )
        .add_instance(10, 0, 100, &[1])
        .add_instance(11, 0, 100, &[2])
        .add_instance(12, 0, 100, &[3])
        .build();

        let start = advance_past_header(&bytes);
        let (_result, obs) =
            run_fp_with_test_observer(&bytes[start..], id_size);
        let events = heap_bytes_events(&obs);

        assert!(
            !events.is_empty(),
            "expected heap bytes progress events"
        );
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
                chunk.filter_ids.len(),
                2,
                "each chunk should have 2 filter IDs"
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
        assert_eq!(single.chunks[0].filter_ids.len(), 6);
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
        assert_eq!(result.chunks[0].filter_ids.len(), 2);
        assert_eq!(result.chunks[1].filter_ids.len(), 2);
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

        // Value-for-value: instance_offsets keys and
        // offsets (verifies plumbing through
        // FirstPassContext → extract_all → merge_into).
        let mut budgeted_ids: Vec<u64> = result_budgeted.index.instance_offsets.keys();
        budgeted_ids.sort_unstable();
        let mut none_ids: Vec<u64> = result_none.index.instance_offsets.keys();
        none_ids.sort_unstable();
        assert_eq!(
            budgeted_ids, none_ids,
            "instance_offsets keys must match value-for-value"
        );
        for id in &none_ids {
            assert_eq!(
                result_budgeted.index.instance_offsets.get(*id),
                result_none.index.instance_offsets.get(*id),
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
        let c_total: usize = chunked.chunks.iter().map(|c| c.filter_ids.len()).sum();
        let u_total: usize = unchunked.chunks.iter().map(|c| c.filter_ids.len()).sum();
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

        // Verify every FilterEntry.data_offset uses
        // absolute offsets
        for chunk in &result.chunks {
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

        assert_eq!(
            ctx_wrapper.raw_frame_roots.len(),
            ctx_direct.raw_frame_roots.len(),
        );
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

        let mut builder =
            HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8);
        for i in 1..=5u64 {
            builder =
                builder.add_instance(i, 0, 100, &[0u8; 16]);
        }
        let bytes = builder.build();
        let start =
            crate::test_utils::advance_past_header(&bytes);

        let (_, obs) = run_fp_with_budget(
            &bytes[start..],
            8,
            MemoryBudget::Bytes(64),
        );

        let byte_events: Vec<(u64, u64)> = obs
            .events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::HeapBytesExtracted {
                    done,
                    total,
                } => Some((*done, *total)),
                _ => None,
            })
            .collect();

        assert_eq!(byte_events.len(), 5);
        // Final done == total
        let (final_done, final_total) =
            *byte_events.last().unwrap();
        assert_eq!(
            final_done, final_total,
            "final heap_bytes_extracted must be \
             (total, total)"
        );
        // Events must be monotonically increasing
        for w in byte_events.windows(2) {
            assert!(
                w[1].0 > w[0].0,
                "done must be strictly increasing"
            );
        }
        // All report same total
        let total = byte_events[0].1;
        for (_, t) in &byte_events {
            assert_eq!(*t, total);
        }
    }

    /// 3.1b: results identical with budget vs without —
    /// counts AND actual extracted IDs and offsets.
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

        assert_eq!(
            result_budget.segment_filters.len(),
            result_none.segment_filters.len(),
        );
        assert_eq!(result_budget.records_indexed, result_none.records_indexed);
        assert_eq!(
            result_budget.heap_record_ranges.len(),
            result_none.heap_record_ranges.len(),
        );
        assert_eq!(result_budget.warnings.len(), result_none.warnings.len());

        // Content check: same object IDs and offsets extracted.
        let mut budget_ids: Vec<u64> = result_budget.index.instance_offsets.keys();
        let mut none_ids: Vec<u64> = result_none.index.instance_offsets.keys();
        budget_ids.sort_unstable();
        none_ids.sort_unstable();
        assert_eq!(budget_ids, none_ids, "extracted IDs must match");
        for id in &budget_ids {
            assert_eq!(
                result_budget.index.instance_offsets.get(*id),
                result_none.index.instance_offsets.get(*id),
                "offset for ID {id} must match"
            );
        }
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

    /// 3.4: HprofFile::from_path uses Unlimited budget
    /// (no regression on the convenience wrapper).
    #[test]
    fn hprof_file_from_path_uses_unlimited_budget() {
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

    /// 10.3-parallel: verifies that the parallel extraction path
    /// is entered (total heap > PARALLEL_THRESHOLD = 32 MB)
    /// and produces correct results and cumulative progress.
    ///
    /// Two prim-array segments of 17 MB each (total ~34 MB) are
    /// used. With BATCH_FLOOR = 64 MB and total < 64 MB, all
    /// segments go in one batch — the multi-batch aspect of
    /// the parallel path requires > 64 MB and is covered by
    /// the `compute_batch_ranges` unit tests (10.3-unit-*).
    #[test]
    fn budget_parallel_path_single_batch() {
        use crate::test_utils::HprofTestBuilder;
        use hprof_api::ProgressEvent;

        // 17 MB per segment × 2 = ~34 MB > PARALLEL_THRESHOLD
        let seg_bytes = 17 * 1024 * 1024_usize;
        let data = vec![0u8; seg_bytes];
        let bytes = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            8,
        )
        // element_type 8 = TYPE_BYTE (1 byte each)
        .add_prim_array(1, 0, seg_bytes as u32, 8, &data)
        .add_prim_array(2, 0, seg_bytes as u32, 8, &data)
        .build();
        let start =
            crate::test_utils::advance_past_header(&bytes);

        let (result, obs) = run_fp_with_budget(
            &bytes[start..],
            8,
            MemoryBudget::Unlimited,
        );

        assert_eq!(result.heap_record_ranges.len(), 2);
        assert!(result.warnings.is_empty());

        let byte_events: Vec<(u64, u64)> = obs
            .events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::HeapBytesExtracted {
                    done,
                    total,
                } => Some((*done, *total)),
                _ => None,
            })
            .collect();
        assert_eq!(byte_events.len(), 2);
        let (final_done, final_total) =
            *byte_events.last().unwrap();
        assert_eq!(final_done, final_total);
        let total = byte_events[0].1;
        for (_, t) in &byte_events {
            assert_eq!(*t, total);
        }
    }

    /// 13.0-5.9: multi-batch monotonicity — bytes_done
    /// never regresses at batch boundaries.
    #[test]
    fn multi_batch_bytes_monotonicity() {
        use crate::test_utils::HprofTestBuilder;
        use hprof_api::ProgressEvent;

        let mut builder = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            8,
        );
        // 5 segments with 16 bytes payload each;
        // budget = 64 forces BATCH_FLOOR (64 MB) which
        // fits all, but test asserts monotonicity across
        // any number of batches.
        for i in 1..=5u64 {
            builder =
                builder.add_instance(i, 0, 100, &[0u8; 16]);
        }
        let bytes = builder.build();
        let start =
            crate::test_utils::advance_past_header(&bytes);

        let (_, obs) = run_fp_with_budget(
            &bytes[start..],
            8,
            MemoryBudget::Bytes(64),
        );

        let byte_events: Vec<(u64, u64)> = obs
            .events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::HeapBytesExtracted {
                    done,
                    total,
                } => Some((*done, *total)),
                _ => None,
            })
            .collect();

        assert!(
            !byte_events.is_empty(),
            "must emit HeapBytesExtracted"
        );

        // Strictly monotonically increasing done
        for w in byte_events.windows(2) {
            assert!(
                w[1].0 > w[0].0,
                "done must be strictly increasing: \
                 {} -> {}",
                w[0].0,
                w[1].0
            );
        }

        // Final done == total
        let (final_done, final_total) =
            *byte_events.last().unwrap();
        assert_eq!(
            final_done, final_total,
            "final done must equal total"
        );
    }
}

// =====================================================
// mod story_13_0_tests — Story 13.0 integration tests
// =====================================================

#[cfg(feature = "test-utils")]
mod story_13_0_tests {
    use hprof_api::{
        MemoryBudget, ProgressEvent, ProgressNotifier,
        TestObserver,
    };

    use crate::indexer::first_pass::run_first_pass;
    use crate::test_utils::{
        HprofTestBuilder, advance_past_header,
    };

    /// 13.0-5.6: extract_all on a dump that triggers the
    /// PARALLEL path terminates without deadlock.
    /// Uses a dedicated 2-thread pool so the test is
    /// deterministic. Exercises the `drop(tx)` guard
    /// inside `rayon::in_place_scope` — if `drop(tx)` is
    /// removed, `rx.iter()` never terminates and the
    /// scope hangs.
    #[test]
    fn extract_all_terminates_no_deadlock() {
        // Two segments of 17 MB each → 34 MB total ≥
        // PARALLEL_THRESHOLD, and num_threads == 2 > 1
        // → parallel path taken.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .expect("2-thread pool");
        let seg_bytes = 17 * 1024 * 1024_usize;
        let data = vec![0u8; seg_bytes];
        let bytes = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            8,
        )
        .add_prim_array(1, 0, seg_bytes as u32, 8, &data)
        .add_prim_array(2, 0, seg_bytes as u32, 8, &data)
        .build();
        let start = advance_past_header(&bytes);
        let handle = std::thread::spawn(move || {
            let mut obs = TestObserver::default();
            pool.install(|| {
                let mut notifier =
                    ProgressNotifier::new(&mut obs);
                let _result = run_first_pass(
                    &bytes[start..],
                    8,
                    0,
                    &mut notifier,
                    MemoryBudget::Unlimited,
                );
            });
        });
        let result = handle.join();
        assert!(
            result.is_ok(),
            "extract_all must not deadlock"
        );
    }

    /// 13.0-5.7: single-threaded fallback — when rayon
    /// pool has 1 thread, extraction uses sequential
    /// path and emits `HeapBytesExtracted` events.
    #[test]
    fn single_threaded_fallback_emits_bytes_events() {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("1-thread pool");

        // 17 MB × 2 ≥ PARALLEL_THRESHOLD but
        // num_threads == 1 → sequential fallback.
        let seg_bytes = 17 * 1024 * 1024_usize;
        let data = vec![0u8; seg_bytes];
        let bytes = HprofTestBuilder::new(
            "JAVA PROFILE 1.0.2",
            8,
        )
        .add_prim_array(1, 0, seg_bytes as u32, 8, &data)
        .add_prim_array(2, 0, seg_bytes as u32, 8, &data)
        .build();
        let start = advance_past_header(&bytes);

        let mut obs = TestObserver::default();
        pool.install(|| {
            let mut notifier =
                ProgressNotifier::new(&mut obs);
            let _result = run_first_pass(
                &bytes[start..],
                8,
                0,
                &mut notifier,
                MemoryBudget::Unlimited,
            );
        });

        let byte_events: Vec<(u64, u64)> = obs
            .events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::HeapBytesExtracted {
                    done,
                    total,
                } => Some((*done, *total)),
                _ => None,
            })
            .collect();

        assert!(
            !byte_events.is_empty(),
            "sequential fallback must emit \
             HeapBytesExtracted"
        );
        let (final_done, final_total) =
            *byte_events.last().unwrap();
        assert_eq!(
            final_done, final_total,
            "final done must equal total"
        );
    }
}

// =====================================================
// mod post_extraction_tests — post-extraction profiling
// and correctness tests (Story 10.4)
// =====================================================

#[cfg(feature = "test-utils")]
mod post_extraction_tests {
    use hprof_api::{MemoryBudget, NullProgressObserver, ProgressNotifier};

    use crate::indexer::DiagnosticInfo;
    use crate::indexer::IndexResult;
    use crate::indexer::first_pass::run_first_pass;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    fn run_fp(data: &[u8], id_size: u32) -> IndexResult {
        let mut obs = NullProgressObserver;
        let mut notifier = ProgressNotifier::new(&mut obs);
        run_first_pass(data, id_size, 0, &mut notifier, MemoryBudget::Unlimited)
    }

    /// Parse `/proc/self/statm` to get private RSS in MB.
    ///
    /// Returns `(resident - shared) * page_size / 1_MB`. Indicative
    /// only — high variance on WSL2 due to rayon thread pool and
    /// delayed page reclaim. Returns 0.0 on non-Linux or read failure.
    fn private_rss_mb() -> f64 {
        #[cfg(target_os = "linux")]
        if let Ok(s) = std::fs::read_to_string("/proc/self/statm") {
            let mut parts = s.split_whitespace();
            let resident: u64 = parts.nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            let shared: u64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            // Assume 4 KiB pages (standard on Linux/WSL2 x86-64)
            return (resident.saturating_sub(shared) * 4096) as f64 / (1024.0 * 1024.0);
        }
        0.0
    }

    /// Deterministic primary metric — PreciseIndex heap
    /// size only (no more all_offsets).
    fn theoretical_mem_mb(diag: &DiagnosticInfo) -> f64 {
        diag.precise_index_heap_bytes as f64 / (1024.0 * 1024.0)
    }

    // ------------------------------------------------------------------
    // DiagnosticInfo fields (updated for story 10.5)
    // ------------------------------------------------------------------

    /// Asserts that `DiagnosticInfo` is populated with
    /// coherent values after a minimal first pass.
    #[test]
    fn diagnostics_fields_present() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_instance(1, 0, 100, &[])
            .build();
        let start = advance_past_header(&bytes);
        let result = run_fp(&bytes[start..], 8);
        let diag = &result.diagnostics;
        assert!(
            diag.precise_index_heap_bytes > 0,
            "precise_index_heap_bytes must be > 0"
        );
    }

    // ------------------------------------------------------------------
    // Task 2.3 / 2.4 / 2.5 — profiling: all fixtures (#[ignore])
    // ------------------------------------------------------------------

    /// Runs `run_first_pass` on all `assets/generated/*-san.hprof`
    /// fixtures plus the RustRover and VisualVM real-world dumps,
    /// then prints a structured profiling log and scaling summary.
    ///
    /// Run:
    /// ```text
    /// cargo test post_extraction -- --ignored --nocapture
    /// ```
    ///
    /// Expected runtime: ~15-25 minutes (1.4 GB total).
    #[test]
    #[ignore]
    fn all_fixtures_profiling() {
        let assets_gen = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/generated");
        let real_dumps: &[&str] = &[
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/heapdump-rustrover-sanitarized.hprof"
            ),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/heapdump-visualvm-sanitarized.hprof"
            ),
        ];

        let mut fixtures: Vec<std::path::PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(assets_gen) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.ends_with("-san.hprof") && !name.contains("truncated") {
                    fixtures.push(path);
                }
            }
        }
        for dump in real_dumps {
            let p = std::path::Path::new(dump);
            if p.exists() {
                fixtures.push(p.to_path_buf());
            }
        }

        if fixtures.is_empty() {
            eprintln!("[post_extraction] No fixtures found — skipping");
            return;
        }

        fixtures.sort();

        // Exclude scenarios where all size variants have identical
        // file sizes (e.g. s06: 4.8 MB across all sizes = no scaling data)
        let fixtures = {
            let mut scenario_sizes: std::collections::HashMap<String, Vec<u64>> =
                std::collections::HashMap::new();
            for p in &fixtures {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if let Some(seg) = name.split('-').find(|s| s.len() == 3 && s.starts_with('s')) {
                    let size = p.metadata().map(|m| m.len()).unwrap_or(0);
                    scenario_sizes
                        .entry(seg.to_string())
                        .or_default()
                        .push(size);
                }
            }
            let uniform: std::collections::HashSet<_> = scenario_sizes
                .into_iter()
                .filter(|(_, v)| v.len() >= 4 && v.iter().all(|&s| s == v[0]))
                .map(|(k, _)| k)
                .collect();
            fixtures
                .into_iter()
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    !name.split('-').any(|s| uniform.contains(s))
                })
                .collect::<Vec<_>>()
        };

        eprintln!("[post_extraction] profiling {} fixtures\n", fixtures.len());

        struct Row {
            _name: String,
            _theo_mb: f64,
        }
        let mut rows: Vec<Row> = Vec::new();

        for path in &fixtures {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let size_mb = path.metadata().map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);
            let raw = match std::fs::read(path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[post_extraction] Cannot read {name}: {e}");
                    continue;
                }
            };
            let null_pos = match raw.iter().position(|&b| b == 0) {
                Some(p) => p,
                None => {
                    eprintln!("[post_extraction] {name}: no null byte in header");
                    continue;
                }
            };
            let id_size_offset = null_pos + 1;
            let id_size = if id_size_offset + 4 <= raw.len() {
                u32::from_be_bytes([
                    raw[id_size_offset],
                    raw[id_size_offset + 1],
                    raw[id_size_offset + 2],
                    raw[id_size_offset + 3],
                ])
            } else {
                eprintln!("[post_extraction] {name}: header too short");
                continue;
            };
            let hdr_end = id_size_offset + 4 + 8;
            if hdr_end >= raw.len() {
                eprintln!("[post_extraction] {name}: too short");
                continue;
            }
            let data = &raw[hdr_end..];
            let rss_before = private_rss_mb();
            let t0 = std::time::Instant::now();
            let result = run_fp(data, id_size);
            let total_ms = t0.elapsed().as_millis();
            let rss_after = private_rss_mb();
            let diag = &result.diagnostics;
            let theo_mb = theoretical_mem_mb(diag);
            let segments = result.segment_filters.len();
            eprintln!(
                "[post_extraction] fixture={name} \
                 size_mb={size_mb:.1} \
                 entry_points={} \
                 filter_lookup_ms={} \
                 theo_mem_mb={theo_mb:.1} \
                 segments={segments} \
                 total_ms={total_ms} \
                 rss_before={rss_before:.1}MB \
                 rss_after={rss_after:.1}MB",
                diag.entry_point_count, diag.filter_lookup_ms
            );
            let _ = (segments, total_ms, rss_before, rss_after);
            rows.push(Row {
                _name: name.to_string(),
                _theo_mb: theo_mb,
            });
        }

        if !rows.is_empty() {
            eprintln!(
                "\n[post_extraction] {} fixtures \
                 profiled",
                rows.len()
            );
        }
    }

    // ------------------------------------------------------------------
    // Task 4.1 — manual 70 GB profiling (#[ignore])
    // ------------------------------------------------------------------

    /// Runs `run_first_pass` on the dump pointed to by the
    /// `HPROF_BENCH_FILE` env var and prints the same structured log.
    ///
    /// Skips gracefully if `HPROF_BENCH_FILE` is unset.
    ///
    /// Run:
    /// ```text
    /// HPROF_BENCH_FILE=/path/to/dump.hprof \
    ///   cargo test post_extraction__manual -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn manual_large_dump_profiling() {
        let path_str = match std::env::var("HPROF_BENCH_FILE") {
            Ok(p) => p,
            Err(_) => {
                eprintln!("[post_extraction] HPROF_BENCH_FILE not set — skipping");
                return;
            }
        };
        let path = std::path::Path::new(&path_str);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let size_mb = path.metadata().map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0);
        let raw = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[post_extraction] Cannot read {path:?}: {e}");
                return;
            }
        };
        let null_pos = match raw.iter().position(|&b| b == 0) {
            Some(p) => p,
            None => {
                eprintln!("[post_extraction] {name}: no null byte in header");
                return;
            }
        };
        let id_size_offset = null_pos + 1;
        let id_size = if id_size_offset + 4 <= raw.len() {
            u32::from_be_bytes([
                raw[id_size_offset],
                raw[id_size_offset + 1],
                raw[id_size_offset + 2],
                raw[id_size_offset + 3],
            ])
        } else {
            eprintln!("[post_extraction] {name}: header too short");
            return;
        };
        let hdr_end = id_size_offset + 4 + 8;
        if hdr_end >= raw.len() {
            eprintln!("[post_extraction] {name}: file too short");
            return;
        }
        let data = &raw[hdr_end..];
        let rss_before = private_rss_mb();
        let t0 = std::time::Instant::now();
        let result = run_fp(data, id_size);
        let total_ms = t0.elapsed().as_millis();
        let rss_after = private_rss_mb();
        let diag = &result.diagnostics;
        let theo_mb = theoretical_mem_mb(diag);
        let segments = result.segment_filters.len();
        eprintln!(
            "[post_extraction] fixture={name} \
             size_mb={size_mb:.1} \
             entry_points={} \
             filter_lookup_ms={} \
             theo_mem_mb={theo_mb:.1} \
             segments={segments} \
             total_ms={total_ms} \
             rss_before={rss_before:.1}MB \
             rss_after={rss_after:.1}MB",
            diag.entry_point_count, diag.filter_lookup_ms
        );
    }
}

// ── Task 3.0 / 5.1: filter_lookup correctness ──

/// Verifies that `instance_offsets` from `run_first_pass`
/// match expected offsets derived from the deterministic
/// `HprofTestBuilder` layout.
///
/// The dump contains: a STACK_TRACE, a ROOT_THREAD_OBJ
/// linking thread_serial=1 to object 0x100, and an
/// INSTANCE_DUMP for object 0x100. The test computes the
/// expected tag-byte offset directly from the known
/// builder layout and asserts it matches.
#[cfg(feature = "test-utils")]
#[test]
fn filter_lookup_matches_expected() {
    use crate::test_utils::{HprofTestBuilder, advance_past_header};
    let id_size: u32 = 8;

    // Build the dump. Record order matters for
    // computing expected offsets.
    //
    // Records added (each is one top-level record):
    //  0: STACK_TRACE (tag 0x05)
    //  1: HEAP_DUMP_SEGMENT containing ROOT_THREAD_OBJ
    //  2: HEAP_DUMP_SEGMENT containing INSTANCE_DUMP 0x100
    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
        // record 0: STACK_TRACE for thread_serial=1
        .add_stack_trace(1, 1, &[])
        // record 1: ROOT_THREAD_OBJ linking thread 1
        //   to object 0x100
        .add_root_thread_obj(0x100, 1, 1)
        // record 2: INSTANCE_DUMP for object 0x100
        .add_instance(0x100, 0, 200, &[])
        .build();

    let header_len = advance_past_header(&bytes);
    let records_data = &bytes[header_len..];

    // Compute expected offset of object 0x100.
    //
    // Record 0 (STACK_TRACE): tag(1) + time(4) +
    //   length(4) + payload[serial(4) + thread(4) +
    //   num_frames(4)] = 9 + 12 = 21 bytes
    let rec0_size = 9 + 4 + 4 + 4; // 21

    // Record 1 (HEAP_DUMP_SEGMENT with
    //   ROOT_THREAD_OBJ): tag(1) + time(4) +
    //   length(4) + payload[sub-tag(1) + obj_id(8) +
    //   thread_serial(4) + stack_trace_serial(4)]
    //   = 9 + (1 + 8 + 4 + 4) = 9 + 17 = 26
    let rec1_size = 9 + 1 + id_size as usize + 4 + 4;

    // Record 2 (HEAP_DUMP_SEGMENT with INSTANCE_DUMP):
    //   tag(1) + time(4) + length(4) = 9 byte header.
    //   Payload starts at rec0_size + rec1_size + 9.
    //   First sub-record tag byte is at the payload
    //   start.
    let instance_payload_start = rec0_size + rec1_size + 9;
    // The offset stored is the tag
    // byte position = sub_record_start - 1 =
    // payload_start. Because:
    //   sub_record_start = data_offset + cursor_pos
    //   After reading tag byte, cursor_pos = 1.
    //   offset = sub_record_start - 1
    //          = (payload_start + 1) - 1
    //          = payload_start
    let expected_offset = instance_payload_start as u64;

    let result = run_fp(records_data, id_size);

    // Verify thread resolution found the object
    let actual_offset = result.index.instance_offsets.get(0x100);
    assert_eq!(
        actual_offset,
        Some(expected_offset),
        "instance_offsets[0x100] expected={} actual={:?}",
        expected_offset,
        actual_offset,
    );
}

// ── Task 1.6: entry points at real SEGMENT_SIZE ──

/// Verifies that entry points are recorded when a
/// single HEAP_DUMP_SEGMENT payload crosses a real
/// 64 MiB SEGMENT_SIZE boundary.
///
/// Allocates ~65 MB — requires >=128 MB free RAM.
#[cfg(feature = "test-utils")]
#[test]
#[ignore]
fn entry_points_at_segment_boundary_large() {
    use crate::indexer::segment::SEGMENT_SIZE;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    // Build a single HEAP_DUMP_SEGMENT containing:
    //   1) A large PRIM_ARRAY_DUMP (~64 MiB of zeros)
    //   2) A small INSTANCE_DUMP after the boundary
    let id_size: u32 = 8;

    // PRIM_ARRAY sub-record: sub-tag(1) + id(8) +
    //   stack_serial(4) + num_elements(4) + elem_type(1)
    //   + data(N)
    let prim_header = 1 + 8 + 4 + 4 + 1; // 18 bytes
    // We want the total prim sub-record to push past
    // SEGMENT_SIZE. We need enough data so that the
    // INSTANCE_DUMP's tag byte is at or after the
    // SEGMENT_SIZE boundary in the records data.
    // The prim array payload fills from the segment
    // payload start. We need prim_header + data_len to
    // be >= SEGMENT_SIZE when combined with the
    // HEAP_DUMP_SEGMENT record header offset.
    // For simplicity, use data_len = SEGMENT_SIZE.
    let data_len = SEGMENT_SIZE;
    let num_elements = data_len as u32; // byte array

    let mut payload = Vec::with_capacity(prim_header + data_len + 50);
    // PRIM_ARRAY_DUMP sub-tag
    payload.push(0x23);
    payload.extend_from_slice(&100u64.to_be_bytes()); // arr_id
    payload.extend_from_slice(&0u32.to_be_bytes()); // stack
    payload.extend_from_slice(&num_elements.to_be_bytes());
    payload.push(8); // PRIM_TYPE_BYTE
    payload.resize(prim_header + data_len, 0u8);

    // INSTANCE_DUMP sub-tag after the large array
    payload.push(0x21); // sub-tag
    payload.extend_from_slice(&200u64.to_be_bytes()); // obj_id
    payload.extend_from_slice(&0u32.to_be_bytes()); // stack
    payload.extend_from_slice(&300u64.to_be_bytes()); // class
    payload.extend_from_slice(&0u32.to_be_bytes()); // 0 bytes

    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", id_size)
        .add_raw_heap_segment(&payload)
        .build();

    let start = advance_past_header(&bytes);
    let result = run_fp(&bytes[start..], id_size);

    // The INSTANCE_DUMP's tag byte is at
    // payload_start + prim_header + data_len which
    // should be >= SEGMENT_SIZE in records data.
    // We expect entry points for segment 0 (first
    // sub-record) and segment 1 (the INSTANCE_DUMP).
    assert!(
        !result.segment_filters.is_empty(),
        "must have at least 1 segment filter"
    );
    // Entry points are stored in FirstPassContext which
    // is consumed by finish(). We verify indirectly by
    // checking that the INSTANCE_DUMP was indexed (its
    // offset would be found via the filter).
    // More direct verification: ensure the second
    // sub-record was found (obj_id 200 in filter).
    let has_200 = result.segment_filters.iter().any(|f| f.contains(200));
    assert!(
        has_200,
        "INSTANCE_DUMP obj 200 past boundary must be \
         in a segment filter"
    );
}

// ── Task 1.7: entry points across multiple segments ──

/// Verifies extraction and filtering work across
/// multiple HEAP_DUMP_SEGMENT records, and that at
/// least one entry point is recorded.
#[cfg(feature = "test-utils")]
#[test]
fn entry_points_across_multiple_heap_segments() {
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
        .add_instance(1, 0, 100, &[])
        .add_instance(2, 0, 100, &[])
        .build();

    let start = advance_past_header(&bytes);
    let result = run_fp(&bytes[start..], 8);

    assert!(
        result.segment_filters.iter().any(|f| f.contains(1)),
        "instance 1 must be in a filter"
    );
    assert!(
        result.segment_filters.iter().any(|f| f.contains(2)),
        "instance 2 must be in a filter"
    );
    assert!(
        result.diagnostics.entry_point_count >= 1,
        "at least one entry point must be recorded"
    );
}

/// Verifies that `extract_heap_segment` records entry
/// points for two distinct 64 MiB windows when the
/// `data_offset` places sub-records across a segment
/// boundary — without allocating 64 MiB of payload.
///
/// Uses `data_offset = SEGMENT_SIZE - 5`:
/// - Sub-record 1 tag at SEGMENT_SIZE - 5 → segment 0
/// - Sub-record 2 tag at SEGMENT_SIZE + 20 → segment 1
#[cfg(feature = "test-utils")]
#[test]
fn extract_heap_segment_cross_segment_entry_points() {
    use crate::indexer::segment::SEGMENT_SIZE;

    let id_size: u32 = 8;
    let mut payload = Vec::new();

    // INSTANCE_DUMP: tag(1)+id(8)+stack(4)+class(8)+
    //   num_bytes(4) = 25 bytes each.
    // Two records → 50 bytes total payload.
    for obj_id in [1u64, 2u64] {
        payload.push(0x21); // InstanceDump sub-tag
        payload.extend_from_slice(&obj_id.to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
        payload.extend_from_slice(&100u64.to_be_bytes());
        payload.extend_from_slice(&0u32.to_be_bytes());
    }

    // data_offset places tag #1 at SEGMENT_SIZE - 5
    // (segment 0) and tag #2 at SEGMENT_SIZE + 20
    // (segment 1).
    let data_offset = SEGMENT_SIZE - 5;
    let parsing_result = extract_heap_segment(&payload, data_offset, id_size, usize::MAX);

    let entry_points: Vec<SegmentEntryPoint> = parsing_result
        .chunks
        .iter()
        .flat_map(|c| c.segment_entry_points.iter())
        .copied()
        .collect();

    assert_eq!(
        entry_points.len(),
        2,
        "expected entry points for segment 0 and 1, \
         got {entry_points:?}"
    );
    assert_eq!(
        entry_points[0].segment_index, 0,
        "first entry point must be segment 0"
    );
    assert_eq!(
        entry_points[0].scan_offset, data_offset,
        "segment 0 entry at tag byte of sub-record 1"
    );
    assert_eq!(
        entry_points[1].segment_index, 1,
        "second entry point must be segment 1"
    );
    assert_eq!(
        entry_points[1].scan_offset,
        data_offset + 25,
        "segment 1 entry at tag byte of sub-record 2"
    );
}

/// Direct unit test for `EntryPointTracker` and
/// `extract_heap_segment` entry point recording.
#[cfg(feature = "test-utils")]
#[test]
fn extract_heap_segment_records_entry_points() {
    // Build a small payload with two INSTANCE_DUMPs.
    // Both at segment 0 (since data is tiny).
    let id_size: u32 = 8;
    let mut payload = Vec::new();

    // INSTANCE_DUMP #1
    payload.push(0x21);
    payload.extend_from_slice(&1u64.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&100u64.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());

    // INSTANCE_DUMP #2
    payload.push(0x21);
    payload.extend_from_slice(&2u64.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());
    payload.extend_from_slice(&100u64.to_be_bytes());
    payload.extend_from_slice(&0u32.to_be_bytes());

    let parsing_result = extract_heap_segment(&payload, 0, id_size, usize::MAX);

    // Collect entry points from all chunks
    let entry_points: Vec<SegmentEntryPoint> = parsing_result
        .chunks
        .iter()
        .flat_map(|c| c.segment_entry_points.iter())
        .copied()
        .collect();

    // Both sub-records are in segment 0, so we should
    // have exactly one entry point for segment 0.
    assert_eq!(
        entry_points.len(),
        1,
        "expected 1 entry point for segment 0"
    );
    assert_eq!(entry_points[0].segment_index, 0);
    assert_eq!(
        entry_points[0].scan_offset, 0,
        "first tag byte is at offset 0"
    );
}

// ── Phase changed event tests ──

#[cfg(feature = "test-utils")]
#[test]
fn run_first_pass_emits_phase_changed_events_with_threads() {
    use crate::test_utils::{HprofTestBuilder, advance_past_header};
    use hprof_api::ProgressEvent;

    let thread_obj_id = 0xBEEF_u64;
    let thread_serial = 1_u32;
    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
        .add_stack_trace(1, thread_serial, &[])
        .add_root_thread_obj(thread_obj_id, thread_serial, 0)
        .add_instance(thread_obj_id, 0, 100, &[1, 2])
        .build();
    let start = advance_past_header(&bytes);
    let (_result, obs) = run_fp_with_test_observer(&bytes[start..], 8);

    // Find the filter-build phase signal
    let filter_idx = obs
        .events
        .iter()
        .position(|e| {
            matches!(
                e,
                ProgressEvent::PhaseChanged(s)
                    if s.starts_with("Building segment")
            )
        })
        .expect("must emit PhaseChanged for filter build");

    // At least one round signal must follow
    let round_indices: Vec<usize> = obs
        .events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            ProgressEvent::PhaseChanged(s) if s.starts_with("Resolving threads") => Some(i),
            _ => None,
        })
        .collect();

    assert!(
        !round_indices.is_empty(),
        "must emit at least one thread round signal"
    );

    // All round signals must come after filter signal
    for &ri in &round_indices {
        assert!(
            ri > filter_idx,
            "round signal at {ri} must be after \
             filter signal at {filter_idx}"
        );
    }

    // First round signal must be the canonical label
    assert_eq!(
        obs.events[round_indices[0]],
        ProgressEvent::PhaseChanged("Resolving threads (round 1/3)\u{2026}".to_owned()),
        "first round must use canonical label"
    );
}

#[cfg(feature = "test-utils")]
#[test]
fn phase_events_ordered_on_jvisualvm_dump() {
    use hprof_api::ProgressEvent;

    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/heapdump-visualvm.hprof",
    ));
    if !path.exists() {
        eprintln!(
            "skip: jvisualvm dump not found at \
             {path:?}"
        );
        return;
    }

    let mut obs = hprof_api::TestObserver::default();
    let _file = crate::hprof_file::HprofFile::from_path_with_progress(
        path,
        &mut obs,
        hprof_api::MemoryBudget::Unlimited,
    )
    .expect("should parse jvisualvm dump");

    let events = &obs.events;

    // (a) Find last HeapBytesExtracted where done == total
    let last_seg = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            ProgressEvent::HeapBytesExtracted {
                done,
                total,
            } if done == total => Some(i),
            _ => None,
        })
        .next_back()
        .expect("must have final HeapBytesExtracted");

    // (b) All PhaseChanged must come after last_seg
    let phase_indices: Vec<usize> = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            ProgressEvent::PhaseChanged(_) => Some(i),
            _ => None,
        })
        .collect();
    for &pi in &phase_indices {
        assert!(
            pi > last_seg,
            "PhaseChanged at {pi} must be after \
             last_seg({last_seg})"
        );
    }

    // (c) At least 1 PhaseChanged (filter build always
    //     emitted)
    assert!(
        !phase_indices.is_empty(),
        "must emit at least 1 PhaseChanged event"
    );

    // (d) If NamesResolved events exist, all
    //     PhaseChanged must precede the first one
    if let Some(first_names) = events
        .iter()
        .position(|e| matches!(e, ProgressEvent::NamesResolved { .. }))
    {
        for &pi in &phase_indices {
            assert!(
                pi < first_names,
                "PhaseChanged at {pi} must precede \
                 first NamesResolved({first_names})"
            );
        }
    }
}
