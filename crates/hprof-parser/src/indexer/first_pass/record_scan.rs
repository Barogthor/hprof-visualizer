//! Top-level record scanning loop with enum-based
//! parse-and-insert dispatch.

use std::io::Cursor;

use hprof_api::ProgressNotifier;

use super::FirstPassContext;
use super::hprof_primitives::maybe_report_progress;
use crate::tags::RecordTag;
use crate::indexer::HeapRecordRange;
use crate::{
    ClassDef, HprofError, HprofStringRef, HprofThread, StackFrame, StackTrace, parse_load_class,
    parse_record_header, parse_stack_frame, parse_stack_trace, parse_start_thread,
    parse_string_ref,
};

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
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
    payload_start: u64,
    header_length: u32,
) -> Result<ParsedRecord, HprofError> {
    match tag {
        RecordTag::StringInUtf8 => parse_string_ref(cursor, id_size, header_length, payload_start)
            .map(ParsedRecord::String),
        RecordTag::LoadClass => parse_load_class(cursor, id_size).map(ParsedRecord::LoadClass),
        RecordTag::StackFrame => parse_stack_frame(cursor, id_size).map(ParsedRecord::StackFrame),
        RecordTag::StackTrace => parse_stack_trace(cursor, id_size).map(ParsedRecord::StackTrace),
        RecordTag::StartThread => {
            parse_start_thread(cursor, id_size).map(ParsedRecord::StartThread)
        }
        _ => Err(HprofError::CorruptedData(format!(
            "try_parse called with non-structural tag {tag}"
        ))),
    }
}

impl FirstPassContext<'_> {
    /// Inserts a parsed record into the index.
    fn insert_record(&mut self, record: ParsedRecord) {
        match record {
            ParsedRecord::String(s) => {
                self.result.index.strings.insert(s.id, s);
            }
            ParsedRecord::LoadClass(c) => {
                let java_name = self
                    .result
                    .index
                    .strings
                    .get(&c.class_name_string_id)
                    .map(|sref| sref.resolve(self.data).replace('/', "."))
                    .unwrap_or_default();
                self.result
                    .index
                    .class_names_by_id
                    .insert(c.class_object_id, java_name);
                self.result.index.classes.insert(c.class_serial, c);
            }
            ParsedRecord::StackFrame(f) => {
                self.result.index.stack_frames.insert(f.frame_id, f);
            }
            ParsedRecord::StackTrace(t) => {
                self.result
                    .index
                    .stack_traces
                    .insert(t.stack_trace_serial, t);
            }
            ParsedRecord::StartThread(t) => {
                self.result.index.threads.insert(t.thread_serial, t);
            }
        }
    }
}

/// Scans all records in `data`, dispatching known tags to
/// parse + insert handlers.
pub(super) fn scan_records(ctx: &mut FirstPassContext, notifier: &mut ProgressNotifier) {
    #[cfg(feature = "dev-profiling")]
    let _record_scan_span = tracing::info_span!("record_scan").entered();

    let data = ctx.data;
    let mut cursor = Cursor::new(data);
    let mut reported_any = false;

    while (cursor.position() as usize) < data.len() {
        let header = match parse_record_header(&mut cursor) {
            Ok(h) => h,
            Err(e) => {
                ctx.push_warning(format!("EOF mid-header: {e}"));
                break;
            }
        };

        let payload_start = cursor.position() as usize;
        let tag = RecordTag::from(header.tag);
        let payload_end = match payload_start.checked_add(header.length as usize) {
            Some(end) if end <= data.len() => end,
            Some(end) => {
                ctx.push_warning(format!(
                    "record {tag} payload end {end} \
                         exceeds file size {}",
                    data.len()
                ));
                break;
            }
            None => {
                ctx.push_warning(format!(
                    "record {tag} payload length \
                         overflow: {}",
                    header.length
                ));
                break;
            }
        };

        if matches!(tag, RecordTag::HeapDump | RecordTag::HeapDumpSegment) {
            ctx.result.heap_record_ranges.push(HeapRecordRange {
                payload_start: payload_start as u64,
                payload_length: header.length as u64,
            });
            cursor.set_position(payload_end as u64);
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
            cursor.set_position(payload_end as u64);
            let pos = cursor.position() as usize;
            reported_any |= maybe_report_progress(
                pos,
                ctx.base_offset,
                &mut ctx.last_progress_bytes,
                &mut ctx.last_progress_at,
                notifier,
            );
            continue;
        }

        ctx.result.records_attempted += 1;

        let payload = &data[payload_start..payload_end];
        let mut payload_cursor = Cursor::new(payload);

        match try_parse(
            tag,
            &mut payload_cursor,
            ctx.id_size,
            payload_start as u64,
            header.length,
        ) {
            Ok(record) => {
                let consumed = payload_cursor.position() as usize == header.length as usize;
                if !consumed {
                    ctx.push_warning(format!(
                        "record {tag} at offset \
                         {payload_start}: parsed OK but \
                         consumed {} of {} bytes \
                         (extra bytes ignored)",
                        payload_cursor.position(),
                        header.length,
                    ));
                }
                ctx.insert_record(record);
                ctx.result.records_indexed += 1;
            }
            Err(e) => {
                ctx.push_warning(format!(
                    "record {tag} at offset \
                     {payload_start}: {e}"
                ));
            }
        }

        cursor.set_position(payload_end as u64);
        let pos = cursor.position() as usize;
        reported_any |= maybe_report_progress(
            pos,
            ctx.base_offset,
            &mut ctx.last_progress_bytes,
            &mut ctx.last_progress_at,
            notifier,
        );
    }

    ctx.cursor_position = cursor.position();
    if !reported_any || (ctx.cursor_position as usize) > ctx.last_progress_bytes {
        notifier.bytes_scanned(ctx.base_offset + ctx.cursor_position);
    }
}
