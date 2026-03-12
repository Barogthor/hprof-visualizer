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
use crate::{ClassDumpInfo, FieldDef, StaticFieldDef, StaticValue, read_id};

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
fn read_static_value(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
    type_code: u8,
) -> Option<StaticValue> {
    match type_code {
        PRIM_TYPE_OBJECT_REF => Some(StaticValue::ObjectRef(read_id(cursor, id_size).ok()?)),
        PRIM_TYPE_BOOLEAN => Some(StaticValue::Bool(cursor.read_u8().ok()? != 0)),
        PRIM_TYPE_CHAR => {
            let code = cursor.read_u16::<BigEndian>().ok()?;
            Some(StaticValue::Char(
                char::from_u32(code as u32).unwrap_or(char::REPLACEMENT_CHARACTER),
            ))
        }
        PRIM_TYPE_FLOAT => Some(StaticValue::Float(cursor.read_f32::<BigEndian>().ok()?)),
        PRIM_TYPE_DOUBLE => Some(StaticValue::Double(cursor.read_f64::<BigEndian>().ok()?)),
        PRIM_TYPE_BYTE => Some(StaticValue::Byte(cursor.read_i8().ok()?)),
        PRIM_TYPE_SHORT => Some(StaticValue::Short(cursor.read_i16::<BigEndian>().ok()?)),
        PRIM_TYPE_INT => Some(StaticValue::Int(cursor.read_i32::<BigEndian>().ok()?)),
        PRIM_TYPE_LONG => Some(StaticValue::Long(cursor.read_i64::<BigEndian>().ok()?)),
        _ => None,
    }
}

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

    // Parse static fields
    let static_count = cursor.read_u16::<BigEndian>().ok()?;
    #[cfg(feature = "dev-profiling")]
    if static_count > 0 {
        tracing::debug!(
            "parse_class_dump class=0x{:X}: declared_static_fields={}",
            class_object_id,
            static_count
        );
    }
    let mut static_fields = Vec::with_capacity(static_count as usize);
    let mut static_parse_ok = true;
    for _ in 0..static_count {
        let name_string_id = match read_id(cursor, id_size).ok() {
            Some(id) => id,
            None => {
                static_parse_ok = false;
                break;
            }
        };
        let field_type = match cursor.read_u8().ok() {
            Some(t) => t,
            None => {
                static_parse_ok = false;
                break;
            }
        };
        match read_static_value(cursor, id_size, field_type) {
            Some(v) => static_fields.push(StaticFieldDef {
                name_string_id,
                value: v,
            }),
            None => {
                // Unknown or unreadable type: cursor is at an unknown position
                // so we cannot safely parse the remaining static or instance
                // fields. Preserve class identity with empty field lists.
                #[cfg(feature = "dev-profiling")]
                tracing::debug!(
                    "parse_class_dump class=0x{:X}: unknown static field type=0x{:02X} \
                     at idx={}, dropping remaining fields",
                    class_object_id,
                    field_type,
                    static_fields.len(),
                );
                static_parse_ok = false;
                break;
            }
        }
    }

    if !static_parse_ok {
        return Some(ClassDumpInfo {
            class_object_id,
            super_class_id,
            instance_size,
            static_fields: vec![],
            instance_fields: vec![],
        });
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

    #[cfg(feature = "dev-profiling")]
    if !static_fields.is_empty() {
        tracing::debug!(
            "parse_class_dump class=0x{:X}: parsed_static_fields={} instance_fields={}",
            class_object_id,
            static_fields.len(),
            instance_fields.len()
        );
    }

    Some(ClassDumpInfo {
        class_object_id,
        super_class_id,
        instance_size,
        static_fields,
        instance_fields,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_id(buf: &mut Vec<u8>, id: u64, id_size: u32) {
        if id_size == 8 {
            buf.extend_from_slice(&id.to_be_bytes());
        } else {
            buf.extend_from_slice(&(id as u32).to_be_bytes());
        }
    }

    fn make_minimal_class_dump_body(id_size: u32) -> Vec<u8> {
        let mut body = Vec::new();
        push_id(&mut body, 100, id_size); // class_object_id
        body.extend_from_slice(&0u32.to_be_bytes()); // stack_trace_serial
        push_id(&mut body, 50, id_size); // super_class_id
        for _ in 0..5 {
            push_id(&mut body, 0, id_size); // classloader/signers/protection_domain/r1/r2
        }
        body.extend_from_slice(&16u32.to_be_bytes()); // instance_size
        body.extend_from_slice(&0u16.to_be_bytes()); // constant_pool_count
        body
    }

    #[test]
    fn parse_class_dump_with_static_fields_returns_correct_count_and_values() {
        let id_size = 8;
        let mut body = make_minimal_class_dump_body(id_size);

        body.extend_from_slice(&2u16.to_be_bytes()); // static_fields_count

        // static int field: count = 42
        push_id(&mut body, 10, id_size);
        body.push(PRIM_TYPE_INT);
        body.extend_from_slice(&42i32.to_be_bytes());

        // static object ref field: owner = 0xDEAD
        push_id(&mut body, 11, id_size);
        body.push(PRIM_TYPE_OBJECT_REF);
        push_id(&mut body, 0xDEAD, id_size);

        body.extend_from_slice(&1u16.to_be_bytes()); // instance_fields_count
        push_id(&mut body, 20, id_size);
        body.push(PRIM_TYPE_INT);

        let mut cursor = Cursor::new(body.as_slice());
        let parsed = parse_class_dump(&mut cursor, id_size).expect("class dump should parse");

        assert_eq!(parsed.static_fields.len(), 2);
        assert_eq!(parsed.static_fields[0].name_string_id, 10);
        assert_eq!(parsed.static_fields[0].value, StaticValue::Int(42));
        assert_eq!(parsed.static_fields[1].name_string_id, 11);
        assert_eq!(
            parsed.static_fields[1].value,
            StaticValue::ObjectRef(0xDEAD)
        );
    }

    #[test]
    fn parse_class_dump_no_static_fields_returns_empty_vec() {
        let id_size = 8;
        let mut body = make_minimal_class_dump_body(id_size);
        body.extend_from_slice(&0u16.to_be_bytes()); // static_fields_count
        body.extend_from_slice(&1u16.to_be_bytes()); // instance_fields_count
        push_id(&mut body, 20, id_size);
        body.push(PRIM_TYPE_INT);

        let mut cursor = Cursor::new(body.as_slice());
        let parsed = parse_class_dump(&mut cursor, id_size).expect("class dump should parse");
        assert!(parsed.static_fields.is_empty());
    }

    #[test]
    fn parse_class_dump_unknown_static_field_type_returns_partial_info() {
        // When a static field has an unknown type, the parser cannot advance
        // the cursor past the value (unknown byte size).  Rather than dropping
        // the whole class dump (losing class identity), it returns a partial
        // ClassDumpInfo with empty field lists so the class is still indexed.
        let id_size = 8;
        let mut body = make_minimal_class_dump_body(id_size);
        body.extend_from_slice(&1u16.to_be_bytes()); // static_fields_count
        push_id(&mut body, 10, id_size);
        body.push(0x03); // unknown field type
        body.extend_from_slice(&0u16.to_be_bytes()); // instance_fields_count

        let mut cursor = Cursor::new(body.as_slice());
        let result = parse_class_dump(&mut cursor, id_size);
        let info = result.expect("partial ClassDumpInfo must be returned, not None");
        assert_eq!(info.class_object_id, 100);
        assert_eq!(info.super_class_id, 50);
        assert!(
            info.static_fields.is_empty(),
            "static_fields must be empty on unknown type"
        );
        assert!(
            info.instance_fields.is_empty(),
            "instance_fields must be empty (cursor position unknown after bad static type)"
        );
    }
}
