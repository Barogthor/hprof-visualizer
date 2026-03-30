//! Top-level record scanning loop with enum-based
//! parse-and-insert dispatch.

use hprof_api::ProgressNotifier;

use super::FirstPassContext;
use super::hprof_primitives::maybe_report_progress;
use crate::indexer::HeapRecordRange;
use crate::reader::RecordReader;
use crate::tags::RecordTag;
use crate::{
    ClassDef, HprofStringRef, HprofThread,
    StackFrame, StackTrace,
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
    reader: &mut RecordReader,
    payload_start: u64,
    header_length: u32,
) -> Option<ParsedRecord> {
    match tag {
        RecordTag::StringInUtf8 => reader
            .parse_string_ref(
                header_length,
                payload_start,
            )
            .map(ParsedRecord::String),
        RecordTag::LoadClass => reader
            .parse_load_class()
            .map(ParsedRecord::LoadClass),
        RecordTag::StackFrame => reader
            .parse_stack_frame()
            .map(ParsedRecord::StackFrame),
        RecordTag::StackTrace => reader
            .parse_stack_trace()
            .map(ParsedRecord::StackTrace),
        RecordTag::StartThread => reader
            .parse_start_thread()
            .map(ParsedRecord::StartThread),
        _ => None,
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
                    .map(|sref| {
                        sref.resolve(self.data)
                            .replace('/', ".")
                    })
                    .unwrap_or_default();
                self.result
                    .index
                    .class_names_by_id
                    .insert(c.class_object_id, java_name);
                self.result
                    .index
                    .classes
                    .insert(c.class_serial, c);
            }
            ParsedRecord::StackFrame(f) => {
                self.result
                    .index
                    .stack_frames
                    .insert(f.frame_id, f);
            }
            ParsedRecord::StackTrace(t) => {
                self.result
                    .index
                    .stack_traces
                    .insert(t.stack_trace_serial, t);
            }
            ParsedRecord::StartThread(t) => {
                self.result
                    .index
                    .threads
                    .insert(t.thread_serial, t);
            }
        }
    }
}

/// Scans all records in `data`, dispatching known tags to
/// parse + insert handlers.
pub(super) fn scan_records(
    ctx: &mut FirstPassContext,
    notifier: &mut ProgressNotifier,
) {
    #[cfg(feature = "dev-profiling")]
    let _record_scan_span =
        tracing::info_span!("record_scan").entered();

    let data = ctx.data;
    let mut reader = RecordReader::new(data, ctx.id_size);
    let mut reported_any = false;

    while (reader.position() as usize) < data.len() {
        let Some(header) =
            reader.parse_record_header()
        else {
            ctx.push_warning(|| {
                "EOF mid-header".to_string()
            });
            break;
        };

        let payload_start = reader.position() as usize;
        let tag = RecordTag::from(header.tag);
        let payload_end = match payload_start
            .checked_add(header.length as usize)
        {
            Some(end) if end <= data.len() => end,
            Some(end) => {
                ctx.push_warning(|| {
                    format!(
                        "record {tag} payload end \
                         {end} exceeds file size {}",
                        data.len()
                    )
                });
                break;
            }
            None => {
                ctx.push_warning(|| {
                    format!(
                        "record {tag} payload length \
                         overflow: {}",
                        header.length
                    )
                });
                break;
            }
        };

        if matches!(
            tag,
            RecordTag::HeapDump
                | RecordTag::HeapDumpSegment
        ) {
            ctx.result
                .heap_record_ranges
                .push(HeapRecordRange {
                    payload_start: payload_start as u64,
                    payload_length: header.length as u64,
                });
            reader.set_position(payload_end as u64);
            let pos = reader.position() as usize;
            reported_any |= maybe_report_progress(
                pos,
                ctx.base_offset,
                &mut ctx.last_progress_bytes,
                &mut ctx.last_progress_at,
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
                ctx.base_offset,
                &mut ctx.last_progress_bytes,
                &mut ctx.last_progress_at,
                notifier,
            );
            continue;
        }

        ctx.result.records_attempted += 1;

        let payload = &data[payload_start..payload_end];
        let mut payload_reader =
            RecordReader::new(payload, ctx.id_size);

        match try_parse(
            tag,
            &mut payload_reader,
            payload_start as u64,
            header.length,
        ) {
            Some(record) => {
                let consumed =
                    payload_reader.position() as usize
                        == header.length as usize;
                if !consumed {
                    ctx.push_warning(|| {
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
                ctx.insert_record(record);
                ctx.result.records_indexed += 1;
            }
            None => {
                ctx.push_warning(|| {
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
            ctx.base_offset,
            &mut ctx.last_progress_bytes,
            &mut ctx.last_progress_at,
            notifier,
        );
    }

    ctx.cursor_position = reader.position();
    if !reported_any
        || (ctx.cursor_position as usize)
            > ctx.last_progress_bytes
    {
        notifier.bytes_scanned(
            ctx.base_offset + ctx.cursor_position,
        );
    }
}
