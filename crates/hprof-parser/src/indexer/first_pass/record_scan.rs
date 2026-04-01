//! Top-level record scanning loop with enum-based
//! parse-and-insert dispatch.

use std::time::Instant;

use hprof_api::ProgressNotifier;

use super::hprof_primitives::maybe_report_progress;
use super::{RecordScanOutput, push_warning_capped};
use crate::format::IdSize;
use crate::indexer::HeapRecordRange;
use crate::indexer::precise::PreciseIndex;
use crate::reader::RecordReader;
use crate::tags::RecordTag;
use crate::{ClassDef, HprofStringRef, HprofThread, StackFrame, StackTrace};

/// A successfully parsed structural record, ready for
/// insertion into the index.
enum ParsedRecord {
    String(HprofStringRef),
    LoadClass(ClassDef),
    StackFrame(StackFrame),
    StackTrace(StackTrace),
    StartThread(HprofThread),
}

/// Parses a single record payload into a
/// [`ParsedRecord`].
fn try_parse(
    tag: RecordTag,
    reader: &mut RecordReader,
    payload_start: u64,
    header_length: u32,
) -> Option<ParsedRecord> {
    match tag {
        RecordTag::StringInUtf8 => reader
            .parse_string_ref(header_length, payload_start)
            .map(ParsedRecord::String),
        RecordTag::LoadClass => reader.parse_load_class().map(ParsedRecord::LoadClass),
        RecordTag::StackFrame => reader.parse_stack_frame().map(ParsedRecord::StackFrame),
        RecordTag::StackTrace => reader.parse_stack_trace().map(ParsedRecord::StackTrace),
        RecordTag::StartThread => reader.parse_start_thread().map(ParsedRecord::StartThread),
        _ => None,
    }
}

/// Inserts a parsed record into the index.
fn insert_record(index: &mut PreciseIndex, data: &[u8], record: ParsedRecord) {
    match record {
        ParsedRecord::String(s) => {
            index.strings.insert(s.id, s);
        }
        ParsedRecord::LoadClass(c) => {
            let java_name = index
                .strings
                .get(&c.class_name_string_id)
                .map(|sref| sref.resolve(data).replace('/', "."))
                .unwrap_or_default();
            index.class_names_by_id.insert(c.class_object_id, java_name);
            index.classes.insert(c.class_serial, c);
        }
        ParsedRecord::StackFrame(f) => {
            index.stack_frames.insert(f.frame_id, f);
        }
        ParsedRecord::StackTrace(t) => {
            index.stack_traces.insert(t.stack_trace_serial, t);
        }
        ParsedRecord::StartThread(t) => {
            index.threads.insert(t.thread_serial, t);
        }
    }
}

/// Scans all records in `data`, dispatching known tags
/// to parse + insert handlers.
pub(super) fn scan_records(
    data: &[u8],
    id_size: IdSize,
    base_offset: u64,
    notifier: &mut ProgressNotifier,
) -> RecordScanOutput {
    #[cfg(feature = "dev-profiling")]
    let _record_scan_span = tracing::info_span!("record_scan").entered();

    let mut index = PreciseIndex::with_capacity(data.len());
    let mut heap_record_ranges: Vec<HeapRecordRange> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut suppressed: u64 = 0;
    let mut records_attempted: u64 = 0;
    let mut records_indexed: u64 = 0;
    let mut last_progress_bytes: usize = 0;
    let mut last_progress_at = Instant::now();

    let mut reader = RecordReader::new(data, id_size);
    let mut reported_any = false;

    while (reader.position() as usize) < data.len() {
        let Some(header) = reader.parse_record_header() else {
            push_warning_capped(&mut warnings, &mut suppressed, || {
                "EOF mid-header".to_string()
            });
            break;
        };

        let payload_start = reader.position() as usize;
        let tag = RecordTag::from(header.tag);
        let payload_end = match payload_start.checked_add(header.length as usize) {
            Some(end) if end <= data.len() => end,
            Some(end) => {
                push_warning_capped(&mut warnings, &mut suppressed, || {
                    format!(
                        "record {tag} payload end \
                             {end} exceeds file size {}",
                        data.len()
                    )
                });
                break;
            }
            None => {
                push_warning_capped(&mut warnings, &mut suppressed, || {
                    format!(
                        "record {tag} payload \
                             length overflow: {}",
                        header.length
                    )
                });
                break;
            }
        };

        if matches!(tag, RecordTag::HeapDump | RecordTag::HeapDumpSegment) {
            heap_record_ranges.push(HeapRecordRange {
                payload_start: payload_start as u64,
                payload_length: header.length as u64,
            });
            reader.set_position(payload_end as u64);
            let pos = reader.position() as usize;
            reported_any |= maybe_report_progress(
                pos,
                base_offset,
                &mut last_progress_bytes,
                &mut last_progress_at,
                notifier,
            );
            continue;
        }

        if !matches!(
            tag,
            RecordTag::StringInUtf8
                | RecordTag::LoadClass
                | RecordTag::StackFrame
                | RecordTag::StackTrace
                | RecordTag::StartThread
        ) {
            reader.set_position(payload_end as u64);
            let pos = reader.position() as usize;
            reported_any |= maybe_report_progress(
                pos,
                base_offset,
                &mut last_progress_bytes,
                &mut last_progress_at,
                notifier,
            );
            continue;
        }

        records_attempted += 1;

        let payload = &data[payload_start..payload_end];
        let mut payload_reader = RecordReader::new(payload, id_size);

        match try_parse(
            tag,
            &mut payload_reader,
            payload_start as u64,
            header.length,
        ) {
            Some(record) => {
                let consumed = payload_reader.position() as usize == header.length as usize;
                if !consumed {
                    push_warning_capped(&mut warnings, &mut suppressed, || {
                        format!(
                            "record {tag} at offset \
                                 {payload_start}: parsed \
                                 OK but consumed {} of \
                                 {} bytes (extra bytes \
                                 ignored)",
                            payload_reader.position(),
                            header.length,
                        )
                    });
                }
                insert_record(&mut index, data, record);
                records_indexed += 1;
            }
            None => {
                push_warning_capped(&mut warnings, &mut suppressed, || {
                    format!(
                        "record {tag} at offset \
                             {payload_start}: \
                             parse failed (truncated?)"
                    )
                });
            }
        }

        reader.set_position(payload_end as u64);
        let pos = reader.position() as usize;
        reported_any |= maybe_report_progress(
            pos,
            base_offset,
            &mut last_progress_bytes,
            &mut last_progress_at,
            notifier,
        );
    }

    let cursor_position = reader.position();
    if !reported_any || (cursor_position as usize) > last_progress_bytes {
        notifier.bytes_scanned(base_offset + cursor_position);
    }

    RecordScanOutput {
        index,
        heap_record_ranges,
        warnings,
        suppressed_warnings: suppressed,
        records_attempted,
        records_indexed,
    }
}
