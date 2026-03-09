//! Low-level hprof binary parsing primitives and
//! cross-cutting utilities.

use std::io::Cursor;
use std::time::{Duration, Instant};

use byteorder::{BigEndian, ReadBytesExt};

use crate::java_types::{
    PRIM_TYPE_BOOLEAN, PRIM_TYPE_BYTE, PRIM_TYPE_CHAR, PRIM_TYPE_DOUBLE, PRIM_TYPE_FLOAT,
    PRIM_TYPE_INT, PRIM_TYPE_LONG, PRIM_TYPE_OBJECT_REF, PRIM_TYPE_SHORT,
};
use crate::tags::HeapSubTag;
use crate::{ClassDumpInfo, FieldDef, read_id};

/// Minimum bytes between consecutive progress callbacks.
pub(super) const PROGRESS_REPORT_INTERVAL: usize = 4 * 1024 * 1024;

/// Maximum time between consecutive progress callbacks.
pub(super) const PROGRESS_REPORT_MAX_INTERVAL: Duration = Duration::from_secs(1);

/// Maximum number of distinct warning strings kept in
/// [`crate::indexer::IndexResult::warnings`].
pub(super) const MAX_WARNINGS: usize = 100;

/// Heap segments below this total size use the sequential
/// path.
pub(super) const PARALLEL_THRESHOLD: u64 = 32 * 1024 * 1024;

/// Calls `notifier.bytes_scanned` when enough bytes or
/// time have elapsed since the last report.
///
/// `pos` is the relative cursor position within the
/// records section. `base_offset` is the absolute file
/// offset of the records section start. The notifier
/// receives the absolute offset `base_offset + pos`.
pub(super) fn maybe_report_progress(
    pos: usize,
    base_offset: u64,
    last_progress_bytes: &mut usize,
    last_progress_at: &mut Instant,
    notifier: &mut hprof_api::ProgressNotifier,
) -> bool {
    let now = Instant::now();
    let enough_bytes = pos.saturating_sub(*last_progress_bytes) >= PROGRESS_REPORT_INTERVAL;
    let enough_time = now.duration_since(*last_progress_at) >= PROGRESS_REPORT_MAX_INTERVAL;
    if enough_bytes || enough_time {
        notifier.bytes_scanned(base_offset + pos as u64);
        *last_progress_bytes = pos;
        *last_progress_at = now;
        true
    } else {
        false
    }
}

/// Advances the cursor by `n` bytes, returning `false` if
/// out of bounds.
pub(super) fn skip_n(cursor: &mut Cursor<&[u8]>, n: usize) -> bool {
    let pos = cursor.position() as usize;
    let new_pos = pos.saturating_add(n);
    if new_pos > cursor.get_ref().len() {
        return false;
    }
    cursor.set_position(new_pos as u64);
    true
}

/// Returns the byte size of a primitive hprof type code,
/// or 0 for unknown.
pub(super) fn primitive_element_size(type_byte: u8) -> usize {
    match type_byte {
        PRIM_TYPE_BOOLEAN => 1,
        PRIM_TYPE_CHAR => 2,
        PRIM_TYPE_FLOAT => 4,
        PRIM_TYPE_DOUBLE => 8,
        PRIM_TYPE_BYTE => 1,
        PRIM_TYPE_SHORT => 2,
        PRIM_TYPE_INT => 4,
        PRIM_TYPE_LONG => 8,
        _ => 0,
    }
}

/// Returns the skip size for fixed-size GC root sub-tags,
/// or `None` for anything else.
pub(super) fn gc_root_skip_size(sub_tag: HeapSubTag, id_size: u32) -> Option<usize> {
    let id = id_size as usize;
    match sub_tag {
        HeapSubTag::GcRootJniGlobal | HeapSubTag::GcRootThreadBlock => Some(id),
        HeapSubTag::GcRootJniLocal => Some(2 * id),
        HeapSubTag::GcRootJavaFrame | HeapSubTag::GcRootThreadObj => Some(id + 4 + 4),
        HeapSubTag::GcRootNativeStack | HeapSubTag::GcRootInternedString => Some(id + 8),
        HeapSubTag::GcRootStickyClass | HeapSubTag::GcRootMonitorUsed => Some(id + 4),
        _ => None,
    }
}

/// Returns the byte size of a value with the given hprof
/// type code.
pub(crate) fn value_byte_size(type_code: u8, id_size: u32) -> usize {
    match type_code {
        PRIM_TYPE_OBJECT_REF => id_size as usize,
        PRIM_TYPE_BOOLEAN | PRIM_TYPE_BYTE => 1,
        PRIM_TYPE_CHAR | PRIM_TYPE_SHORT => 2,
        PRIM_TYPE_FLOAT | PRIM_TYPE_INT => 4,
        PRIM_TYPE_DOUBLE | PRIM_TYPE_LONG => 8,
        _ => 0,
    }
}

/// Parses a `CLASS_DUMP` sub-record body (after the sub-tag
/// byte), returning `None` on any read failure.
pub(crate) fn parse_class_dump(cursor: &mut Cursor<&[u8]>, id_size: u32) -> Option<ClassDumpInfo> {
    let class_object_id = read_id(cursor, id_size).ok()?;
    let _stack_trace_serial = cursor.read_u32::<BigEndian>().ok()?;
    let super_class_id = read_id(cursor, id_size).ok()?;
    // skip classloader, signers, prot_domain, r1, r2
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
