//! Typed heap sub-record iterator.
//!
//! [`HeapSubRecordIter`] iterates over the sub-records
//! inside a `HEAP_DUMP` or `HEAP_DUMP_SEGMENT` payload,
//! yielding [`HeapSubRecord`] variants. All sub-tag
//! dispatch and skip logic lives in
//! [`Iterator::next`] — callers never touch raw bytes.

use crate::ClassDumpInfo;
use crate::id::IdSize;
use crate::reader::RecordReader;
use crate::tags::HeapSubTag;

/// A parsed heap sub-record yielded by
/// [`HeapSubRecordIter`].
#[derive(Debug)]
pub enum HeapSubRecord<'a> {
    /// `INSTANCE_DUMP` (sub-tag `0x21`).
    Instance {
        id: u64,
        class_id: u64,
        field_data: &'a [u8],
    },
    /// `OBJECT_ARRAY_DUMP` (sub-tag `0x22`).
    ObjectArray {
        id: u64,
        class_id: u64,
        num_elements: u32,
        elements_data: &'a [u8],
    },
    /// `PRIMITIVE_ARRAY_DUMP` (sub-tag `0x23`).
    PrimArray {
        id: u64,
        element_type: u8,
        num_elements: u32,
        data: &'a [u8],
    },
    /// `CLASS_DUMP` (sub-tag `0x20`).
    ClassDump(ClassDumpInfo),
    /// `GC_ROOT_JAVA_FRAME` (sub-tag `0x03`).
    GcRootJavaFrame {
        object_id: u64,
        thread_serial: u32,
        frame_number: i32,
    },
    /// `GC_ROOT_THREAD_OBJ` (sub-tag `0x08`).
    GcRootThreadObj {
        object_id: u64,
        thread_serial: u32,
        stack_trace_serial: u32,
    },
    /// Other GC root sub-tags.
    GcRootOther { tag: u8, object_id: u64 },
}

/// Iterator over heap sub-records in a segment.
pub struct HeapSubRecordIter<'a> {
    reader: RecordReader<'a>,
    last_tag_pos: u64,
}

impl<'a> HeapSubRecordIter<'a> {
    /// Creates an iterator over the sub-records in
    /// `segment_data`.
    pub fn new(segment_data: &'a [u8], id_size: IdSize) -> Self {
        Self {
            reader: RecordReader::new(segment_data, id_size),
            last_tag_pos: 0,
        }
    }

    /// Returns current byte position within the segment.
    pub fn position(&self) -> u64 {
        self.reader.position()
    }

    /// Byte position of the last yielded sub-record's
    /// tag byte within the segment.
    pub fn tag_position(&self) -> u64 {
        self.last_tag_pos
    }
}

impl<'a> Iterator for HeapSubRecordIter<'a> {
    type Item = HeapSubRecord<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.last_tag_pos = self.reader.position();
        let raw = self.reader.read_u8()?;
        let sub_tag = HeapSubTag::from(raw);
        let id_size = self.reader.id_size();

        match sub_tag {
            HeapSubTag::InstanceDump => {
                let body = self.reader.parse_instance_dump_body()?;
                Some(HeapSubRecord::Instance {
                    id: body.object_id,
                    class_id: body.class_object_id,
                    field_data: body.field_data,
                })
            }
            HeapSubTag::ObjectArrayDump => {
                let id = self.reader.read_id()?;
                let _serial = self.reader.read_u32()?;
                let num_elements = self.reader.read_u32()?;
                let class_id = self.reader.read_id()?;
                let byte_count = (num_elements as usize).checked_mul(id_size.as_usize())?;
                let elements_data = self.reader.read_bytes(byte_count)?;
                Some(HeapSubRecord::ObjectArray {
                    id,
                    class_id,
                    num_elements,
                    elements_data,
                })
            }
            HeapSubTag::PrimArrayDump => {
                let id = self.reader.read_id()?;
                let _serial = self.reader.read_u32()?;
                let num_elements = self.reader.read_u32()?;
                let element_type = self.reader.read_u8()?;
                let elem_size = primitive_element_size(element_type);
                if elem_size == 0 {
                    return None;
                }
                let byte_count = (num_elements as usize).checked_mul(elem_size)?;
                let data = self.reader.read_bytes(byte_count)?;
                Some(HeapSubRecord::PrimArray {
                    id,
                    element_type,
                    num_elements,
                    data,
                })
            }
            HeapSubTag::ClassDump => {
                let info = self.reader.parse_class_dump()?;
                Some(HeapSubRecord::ClassDump(info))
            }
            HeapSubTag::GcRootJavaFrame => {
                let object_id = self.reader.read_id()?;
                let thread_serial = self.reader.read_u32()?;
                let frame_number = self.reader.read_i32()?;
                Some(HeapSubRecord::GcRootJavaFrame {
                    object_id,
                    thread_serial,
                    frame_number,
                })
            }
            HeapSubTag::GcRootThreadObj => {
                let object_id = self.reader.read_id()?;
                let thread_serial = self.reader.read_u32()?;
                let stack_trace_serial = self.reader.read_u32()?;
                Some(HeapSubRecord::GcRootThreadObj {
                    object_id,
                    thread_serial,
                    stack_trace_serial,
                })
            }
            t if gc_root_has_object_id(t) => {
                let object_id = self.reader.read_id()?;
                let skip = gc_root_remaining_size(t, id_size)?;
                if !self.reader.skip(skip) {
                    return None;
                }
                Some(HeapSubRecord::GcRootOther {
                    tag: raw,
                    object_id,
                })
            }
            _ => None,
        }
    }
}

/// Returns true if this GC root sub-tag starts with
/// an object ID.
fn gc_root_has_object_id(tag: HeapSubTag) -> bool {
    matches!(
        tag,
        HeapSubTag::GcRootJniGlobal
            | HeapSubTag::GcRootJniLocal
            | HeapSubTag::GcRootNativeStack
            | HeapSubTag::GcRootStickyClass
            | HeapSubTag::GcRootMonitorUsed
            | HeapSubTag::GcRootThreadBlock
            | HeapSubTag::GcRootInternedString
    )
}

/// Returns the number of bytes to skip AFTER reading
/// the object_id for a GC root sub-tag.
fn gc_root_remaining_size(tag: HeapSubTag, id_size: IdSize) -> Option<usize> {
    let id = id_size.as_usize();
    match tag {
        HeapSubTag::GcRootJniGlobal | HeapSubTag::GcRootThreadBlock => Some(0),
        HeapSubTag::GcRootJniLocal => Some(id),
        HeapSubTag::GcRootNativeStack | HeapSubTag::GcRootInternedString => Some(8),
        HeapSubTag::GcRootStickyClass | HeapSubTag::GcRootMonitorUsed => Some(4),
        _ => None,
    }
}

/// Returns the byte size of a primitive element type.
fn primitive_element_size(type_byte: u8) -> usize {
    use crate::java_types::*;
    match type_byte {
        PRIM_TYPE_BOOLEAN | PRIM_TYPE_BYTE => 1,
        PRIM_TYPE_CHAR | PRIM_TYPE_SHORT => 2,
        PRIM_TYPE_FLOAT | PRIM_TYPE_INT => 4,
        PRIM_TYPE_DOUBLE | PRIM_TYPE_LONG => 8,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_instance(id: u64, class_id: u64, field_data: &[u8], id_size: IdSize) -> Vec<u8> {
        let sz = id_size.as_usize();
        let mut buf = vec![0x21];
        buf.extend_from_slice(&id.to_be_bytes()[8 - sz..]);
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&class_id.to_be_bytes()[8 - sz..]);
        buf.extend_from_slice(&(field_data.len() as u32).to_be_bytes());
        buf.extend_from_slice(field_data);
        buf
    }

    fn make_prim_array(
        id: u64,
        elem_type: u8,
        elements: &[u8],
        num_elements: u32,
        id_size: IdSize,
    ) -> Vec<u8> {
        let sz = id_size.as_usize();
        let mut buf = vec![0x23];
        buf.extend_from_slice(&id.to_be_bytes()[8 - sz..]);
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&num_elements.to_be_bytes());
        buf.push(elem_type);
        buf.extend_from_slice(elements);
        buf
    }

    #[test]
    fn iter_instance_dump() {
        let id_size = IdSize::Eight;
        let data = make_instance(42, 100, &[1, 2, 3], id_size);
        let records: Vec<_> = HeapSubRecordIter::new(&data, id_size).collect();
        assert_eq!(records.len(), 1);
        match &records[0] {
            HeapSubRecord::Instance {
                id,
                class_id,
                field_data,
            } => {
                assert_eq!(*id, 42);
                assert_eq!(*class_id, 100);
                assert_eq!(*field_data, &[1, 2, 3]);
            }
            other => {
                panic!("expected Instance, got {other:?}")
            }
        }
    }

    #[test]
    fn iter_prim_array() {
        let id_size = IdSize::Eight;
        let data = make_prim_array(99, 10, &[0, 0, 0, 1, 0, 0, 0, 2], 2, id_size);
        let records: Vec<_> = HeapSubRecordIter::new(&data, id_size).collect();
        assert_eq!(records.len(), 1);
        match &records[0] {
            HeapSubRecord::PrimArray {
                id,
                element_type,
                num_elements,
                ..
            } => {
                assert_eq!(*id, 99);
                assert_eq!(*element_type, 10);
                assert_eq!(*num_elements, 2);
            }
            other => {
                panic!("expected PrimArray, got {other:?}")
            }
        }
    }

    #[test]
    fn iter_multiple_records() {
        let id_size = IdSize::Eight;
        let mut data = make_instance(1, 100, &[0xAA], id_size);
        data.extend(make_instance(2, 200, &[0xBB], id_size));
        let records: Vec<_> = HeapSubRecordIter::new(&data, id_size).collect();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn iter_truncated_stops() {
        let id_size = IdSize::Eight;
        let data = [0x21, 0x00];
        let records: Vec<_> = HeapSubRecordIter::new(&data, id_size).collect();
        assert!(records.is_empty());
    }

    #[test]
    fn iter_empty_yields_nothing() {
        let records: Vec<_> = HeapSubRecordIter::new(&[], IdSize::Eight).collect();
        assert!(records.is_empty());
    }

    #[test]
    fn iter_gc_root_java_frame() {
        let id_size = IdSize::Eight;
        let mut data = vec![0x03];
        data.extend_from_slice(&42u64.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&5i32.to_be_bytes());
        let records: Vec<_> = HeapSubRecordIter::new(&data, id_size).collect();
        assert_eq!(records.len(), 1);
        match &records[0] {
            HeapSubRecord::GcRootJavaFrame {
                object_id,
                thread_serial,
                frame_number,
            } => {
                assert_eq!(*object_id, 42);
                assert_eq!(*thread_serial, 1);
                assert_eq!(*frame_number, 5);
            }
            other => panic!(
                "expected GcRootJavaFrame, \
                 got {other:?}"
            ),
        }
    }

    #[test]
    fn tag_position_tracks_sub_record_offsets() {
        let id_size = IdSize::Eight;
        let inst1 = make_instance(1, 100, &[], id_size);
        let inst2_start = inst1.len();
        let mut data = inst1;
        data.extend(make_instance(2, 200, &[], id_size));

        let mut iter = HeapSubRecordIter::new(&data, id_size);
        iter.next().unwrap();
        assert_eq!(iter.tag_position(), 0);
        iter.next().unwrap();
        assert_eq!(iter.tag_position(), inst2_start as u64,);
    }
}
