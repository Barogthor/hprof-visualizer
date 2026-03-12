//! Field decoding from raw `INSTANCE_DUMP` bytes using class hierarchy.
//!
//! [`decode_fields`] converts [`RawInstance`] bytes into [`FieldInfo`] values
//! by walking the super-class chain in [`PreciseIndex`] and interpreting each
//! field's bytes according to its declared type.

use std::io::Cursor;

use byteorder::{BigEndian, ReadBytesExt};
use hprof_parser::{PreciseIndex, RawInstance, read_id};

use crate::engine::{FieldInfo, FieldValue};

/// Decodes instance field bytes into a list of [`FieldInfo`] values.
///
/// Field order follows the hprof convention: superclass fields first,
/// subclass fields last. Unknown field types produce a break in decoding
/// (safe fallback — cannot determine byte width).
///
/// `records_bytes` is the records section data slice used to
/// resolve field names from [`HprofStringRef`] offsets.
///
/// Returns an empty `Vec` if the data is truncated or class info
/// is missing.
pub fn decode_fields(
    raw: &RawInstance,
    index: &PreciseIndex,
    id_size: u32,
    records_bytes: &[u8],
) -> Vec<FieldInfo> {
    let mut ordered_defs: Vec<(u64, u8)> = Vec::new();
    collect_fields(raw.class_object_id, index, &mut ordered_defs);

    let mut cursor = Cursor::new(raw.data.as_slice());
    let mut fields = Vec::with_capacity(ordered_defs.len());

    for (name_string_id, field_type) in ordered_defs {
        let name = index
            .strings
            .get(&name_string_id)
            .map(|sref| sref.resolve(records_bytes))
            .unwrap_or_else(|| format!("<field:{name_string_id}>"));

        let value = match read_field_value(&mut cursor, field_type, id_size) {
            Some(v) => v,
            None => break,
        };
        fields.push(FieldInfo { name, value });
    }

    fields
}

/// Collects `(name_string_id, field_type)` pairs from the full class
/// hierarchy, superclass first.
///
/// Uses an iterative walk with a visited set to guard against multi-node
/// cycles in corrupted heap metadata (e.g. A → B → A).
fn collect_fields(class_id: u64, index: &PreciseIndex, out: &mut Vec<(u64, u8)>) {
    use std::collections::HashSet;

    // Phase 1: walk up the super-class chain, recording the order.
    let mut chain: Vec<u64> = Vec::new();
    let mut visited: HashSet<u64> = HashSet::new();
    let mut current = class_id;
    loop {
        if !visited.insert(current) {
            break; // cycle detected — stop
        }
        let Some(info) = index.class_dumps.get(&current) else {
            break;
        };
        chain.push(current);
        let super_id = info.super_class_id;
        if super_id == 0 {
            break;
        }
        current = super_id;
    }

    // Phase 2: emit fields in leaf-first order (same as walk order).
    // HotSpot writes INSTANCE_DUMP data from the leaf class up
    // to the root, so the byte layout is subclass fields first,
    // then superclass fields.
    for &cid in chain.iter() {
        if let Some(info) = index.class_dumps.get(&cid) {
            for field in &info.instance_fields {
                out.push((field.name_string_id, field.field_type));
            }
        }
    }
}

fn read_field_value(cursor: &mut Cursor<&[u8]>, type_code: u8, id_size: u32) -> Option<FieldValue> {
    match type_code {
        2 => {
            let id = read_id(cursor, id_size).ok()?;
            Some(if id == 0 {
                FieldValue::Null
            } else {
                FieldValue::ObjectRef {
                    id,
                    class_name: String::new(),
                    entry_count: None,
                    inline_value: None,
                }
            })
        }
        4 => Some(FieldValue::Bool(cursor.read_u8().ok()? != 0)),
        5 => {
            let code = cursor.read_u16::<BigEndian>().ok()?;
            let ch = char::from_u32(code as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
            Some(FieldValue::Char(ch))
        }
        6 => Some(FieldValue::Float(cursor.read_f32::<BigEndian>().ok()?)),
        7 => Some(FieldValue::Double(cursor.read_f64::<BigEndian>().ok()?)),
        8 => Some(FieldValue::Byte(cursor.read_i8().ok()?)),
        9 => Some(FieldValue::Short(cursor.read_i16::<BigEndian>().ok()?)),
        10 => Some(FieldValue::Int(cursor.read_i32::<BigEndian>().ok()?)),
        11 => Some(FieldValue::Long(cursor.read_i64::<BigEndian>().ok()?)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use hprof_parser::{ClassDumpInfo, FieldDef, HprofStringRef, PreciseIndex};

    use super::*;

    /// Builds a `PreciseIndex` and a records-bytes buffer where
    /// each string is laid out at a known offset.
    fn make_index_with_class(
        class_id: u64,
        super_id: u64,
        fields: &[(u64, u8)],
        strings: &[(u64, &str)],
    ) -> (PreciseIndex, Vec<u8>) {
        let mut index = PreciseIndex::new();
        index.class_dumps.insert(
            class_id,
            ClassDumpInfo {
                class_object_id: class_id,
                super_class_id: super_id,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: fields
                    .iter()
                    .map(|&(name_id, ft)| FieldDef {
                        name_string_id: name_id,
                        field_type: ft,
                    })
                    .collect(),
            },
        );
        let mut buf = Vec::new();
        for &(id, val) in strings {
            let offset = buf.len() as u64;
            buf.extend_from_slice(val.as_bytes());
            index.strings.insert(
                id,
                HprofStringRef {
                    id,
                    offset,
                    len: val.len() as u32,
                },
            );
        }
        (index, buf)
    }

    #[test]
    fn decode_fields_int_field_returns_field_info() {
        let (index, buf) = make_index_with_class(100, 0, &[(1, 10)], &[(1, "count")]);
        let raw = RawInstance {
            class_object_id: 100,
            data: 42i32.to_be_bytes().to_vec(),
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "count");
        assert_eq!(fields[0].value, FieldValue::Int(42));
    }

    #[test]
    fn decode_fields_inherited_field_leaf_first_order() {
        let mut index = PreciseIndex::new();
        index.class_dumps.insert(
            50,
            ClassDumpInfo {
                class_object_id: 50,
                super_class_id: 0,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![FieldDef {
                    name_string_id: 1,
                    field_type: 10,
                }],
            },
        );
        index.class_dumps.insert(
            100,
            ClassDumpInfo {
                class_object_id: 100,
                super_class_id: 50,
                instance_size: 0,
                static_fields: vec![],
                instance_fields: vec![FieldDef {
                    name_string_id: 2,
                    field_type: 10,
                }],
            },
        );
        // Build records buffer with "x" at offset 0, "y" at offset 1
        let buf = b"xy".to_vec();
        index.strings.insert(
            1,
            HprofStringRef {
                id: 1,
                offset: 0,
                len: 1,
            },
        );
        index.strings.insert(
            2,
            HprofStringRef {
                id: 2,
                offset: 1,
                len: 1,
            },
        );

        let mut data = Vec::new();
        data.extend_from_slice(&3i32.to_be_bytes()); // y (sub)
        data.extend_from_slice(&7i32.to_be_bytes()); // x (super)
        let raw = RawInstance {
            class_object_id: 100,
            data,
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "y");
        assert_eq!(fields[0].value, FieldValue::Int(3));
        assert_eq!(fields[1].name, "x");
        assert_eq!(fields[1].value, FieldValue::Int(7));
    }

    #[test]
    fn decode_fields_object_ref_non_null_returns_object_ref() {
        let (index, buf) = make_index_with_class(100, 0, &[(1, 2)], &[(1, "ref")]);
        let id: u64 = 0xDEAD;
        let raw = RawInstance {
            class_object_id: 100,
            data: id.to_be_bytes().to_vec(),
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert_eq!(
            fields[0].value,
            FieldValue::ObjectRef {
                id: 0xDEAD,
                class_name: String::new(),
                entry_count: None,
                inline_value: None,
            }
        );
    }

    #[test]
    fn decode_fields_object_ref_zero_returns_null() {
        let (index, buf) = make_index_with_class(100, 0, &[(1, 2)], &[(1, "ref")]);
        let raw = RawInstance {
            class_object_id: 100,
            data: 0u64.to_be_bytes().to_vec(),
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert_eq!(fields[0].value, FieldValue::Null);
    }

    #[test]
    fn decode_fields_bool_true_returns_bool_true() {
        let (index, buf) = make_index_with_class(100, 0, &[(1, 4)], &[(1, "flag")]);
        let raw = RawInstance {
            class_object_id: 100,
            data: vec![1u8],
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert_eq!(fields[0].value, FieldValue::Bool(true));
    }

    #[test]
    fn decode_fields_long_field_returns_long_value() {
        let (index, buf) = make_index_with_class(100, 0, &[(1, 11)], &[(1, "timestamp")]);
        let val: i64 = i64::MAX;
        let raw = RawInstance {
            class_object_id: 100,
            data: val.to_be_bytes().to_vec(),
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert_eq!(fields[0].value, FieldValue::Long(i64::MAX));
    }

    #[test]
    fn decode_fields_truncated_data_returns_empty() {
        let (index, buf) = make_index_with_class(100, 0, &[(1, 10)], &[(1, "x")]);
        let raw = RawInstance {
            class_object_id: 100,
            data: vec![0u8, 1],
        };
        let fields = decode_fields(&raw, &index, 8, &buf);
        assert!(fields.is_empty());
    }
}
