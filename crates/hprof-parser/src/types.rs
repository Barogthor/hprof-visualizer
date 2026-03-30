//! Structural hprof record types: `LOAD_CLASS`,
//! `START_THREAD`, `STACK_FRAME`, and `STACK_TRACE`.
//!
//! Parsing is handled by
//! [`RecordReader`](crate::reader::RecordReader).

use hprof_api::MemorySize;

/// A parsed `LOAD_CLASS` record (tag `0x02`).
///
/// ## Fields
/// - `class_serial`: `u32` -- class serial number
/// - `class_object_id`: `u64` -- object ID of the class
/// - `stack_trace_serial`: `u32` -- serial of associated
///   stack trace
/// - `class_name_string_id`: `u64` -- string ID of the
///   class name
#[derive(Debug, Clone)]
pub struct ClassDef {
    pub class_serial: u32,
    pub class_object_id: u64,
    pub stack_trace_serial: u32,
    pub class_name_string_id: u64,
}

/// A parsed `START_THREAD` record (tag `0x06`).
///
/// ## Fields
/// - `thread_serial`: `u32`
/// - `object_id`: `u64` -- object ID of the thread
/// - `stack_trace_serial`: `u32`
/// - `name_string_id`: `u64` -- string ID of thread name
/// - `group_name_string_id`: `u64` -- string ID of thread
///   group name
/// - `group_parent_name_string_id`: `u64` -- string ID of
///   parent group name
#[derive(Debug, Clone)]
pub struct HprofThread {
    pub thread_serial: u32,
    pub object_id: u64,
    pub stack_trace_serial: u32,
    pub name_string_id: u64,
    pub group_name_string_id: u64,
    pub group_parent_name_string_id: u64,
}

/// A parsed `STACK_FRAME` record (tag `0x04`).
///
/// ## Fields
/// - `frame_id`: `u64`
/// - `method_name_string_id`: `u64`
/// - `method_sig_string_id`: `u64`
/// - `source_file_string_id`: `u64`
/// - `class_serial`: `u32`
/// - `line_number`: `i32` -- negative = unknown,
///   0 = compiled, >0 = source line
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub frame_id: u64,
    pub method_name_string_id: u64,
    pub method_sig_string_id: u64,
    pub source_file_string_id: u64,
    pub class_serial: u32,
    pub line_number: i32,
}

/// A parsed `STACK_TRACE` record (tag `0x05`).
///
/// ## Fields
/// - `stack_trace_serial`: `u32`
/// - `thread_serial`: `u32`
/// - `frame_ids`: ordered list of `u64` frame IDs
#[derive(Debug, Clone)]
pub struct StackTrace {
    pub stack_trace_serial: u32,
    pub thread_serial: u32,
    pub frame_ids: Vec<u64>,
}

/// One instance field definition from a `CLASS_DUMP`
/// sub-record.
///
/// `field_type` codes: 2=object ref, 4=bool, 5=char,
/// 6=float, 7=double, 8=byte, 9=short, 10=int, 11=long.
#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name_string_id: u64,
    pub field_type: u8,
}

/// Decoded value of a static field from a `CLASS_DUMP`
/// sub-record.
#[derive(Debug, Clone, PartialEq)]
pub enum StaticValue {
    ObjectRef(u64),
    Bool(bool),
    Char(char),
    Float(f32),
    Double(f64),
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
}

/// One decoded static field from a `CLASS_DUMP`
/// sub-record.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticFieldDef {
    pub name_string_id: u64,
    pub value: StaticValue,
}

/// Parsed instance field layout extracted from a
/// `CLASS_DUMP` sub-record (heap sub-tag `0x20`).
#[derive(Debug, Clone)]
pub struct ClassDumpInfo {
    pub class_object_id: u64,
    pub super_class_id: u64,
    pub instance_size: u32,
    /// Static fields declared on this class.
    pub static_fields: Vec<StaticFieldDef>,
    /// Instance fields in declaration order
    /// (NOT including inherited fields).
    pub instance_fields: Vec<FieldDef>,
}

/// Raw bytes of an `INSTANCE_DUMP` sub-record payload,
/// returned by the object resolver before field decoding.
#[derive(Debug, Clone)]
pub struct RawInstance {
    pub class_object_id: u64,
    /// Field data bytes, ordered as declared in the
    /// class hierarchy.
    pub data: Vec<u8>,
}

impl MemorySize for ClassDef {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl MemorySize for HprofThread {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl MemorySize for StackFrame {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl MemorySize for StackTrace {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.frame_ids.capacity() * std::mem::size_of::<u64>()
    }
}

impl MemorySize for FieldDef {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl MemorySize for StaticValue {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl MemorySize for StaticFieldDef {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

impl MemorySize for ClassDumpInfo {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.static_fields.capacity() * std::mem::size_of::<StaticFieldDef>()
            + self.instance_fields.capacity() * std::mem::size_of::<FieldDef>()
    }
}

impl MemorySize for RawInstance {
    fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.data.capacity()
    }
}

#[cfg(test)]
mod memory_size_tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn class_def_returns_static_size() {
        let c = ClassDef {
            class_serial: 1,
            class_object_id: 100,
            stack_trace_serial: 0,
            class_name_string_id: 200,
        };
        assert_eq!(c.memory_size(), size_of::<ClassDef>());
    }

    #[test]
    fn hprof_thread_returns_static_size() {
        let t = HprofThread {
            thread_serial: 1,
            object_id: 100,
            stack_trace_serial: 0,
            name_string_id: 10,
            group_name_string_id: 20,
            group_parent_name_string_id: 30,
        };
        assert_eq!(t.memory_size(), size_of::<HprofThread>());
    }

    #[test]
    fn stack_frame_returns_static_size() {
        let f = StackFrame {
            frame_id: 1,
            method_name_string_id: 2,
            method_sig_string_id: 3,
            source_file_string_id: 4,
            class_serial: 5,
            line_number: 42,
        };
        assert_eq!(f.memory_size(), size_of::<StackFrame>());
    }

    #[test]
    fn stack_trace_includes_vec_capacity() {
        let mut frame_ids = Vec::with_capacity(10);
        frame_ids.push(1);
        frame_ids.push(2);
        let st = StackTrace {
            stack_trace_serial: 1,
            thread_serial: 1,
            frame_ids,
        };
        let expected = size_of::<StackTrace>() + 10 * size_of::<u64>();
        assert_eq!(st.memory_size(), expected);
    }

    #[test]
    fn field_def_returns_static_size() {
        let f = FieldDef {
            name_string_id: 1,
            field_type: 10,
        };
        assert_eq!(f.memory_size(), size_of::<FieldDef>());
    }

    #[test]
    fn class_dump_info_includes_fields_capacity() {
        let mut fields = Vec::with_capacity(5);
        fields.push(FieldDef {
            name_string_id: 1,
            field_type: 10,
        });
        let mut static_fields = Vec::with_capacity(3);
        static_fields.push(StaticFieldDef {
            name_string_id: 2,
            value: StaticValue::Int(7),
        });
        let c = ClassDumpInfo {
            class_object_id: 100,
            super_class_id: 50,
            instance_size: 16,
            static_fields,
            instance_fields: fields,
        };
        let expected = size_of::<ClassDumpInfo>()
            + 3 * size_of::<StaticFieldDef>()
            + 5 * size_of::<FieldDef>();
        assert_eq!(c.memory_size(), expected);
    }

    #[test]
    fn raw_instance_includes_data_capacity() {
        let mut data = Vec::with_capacity(100);
        data.extend_from_slice(&[1, 2, 3]);
        let r = RawInstance {
            class_object_id: 200,
            data,
        };
        let expected = size_of::<RawInstance>() + 100;
        assert_eq!(r.memory_size(), expected);
    }

    #[test]
    fn empty_stack_trace_returns_static_size() {
        let st = StackTrace {
            stack_trace_serial: 1,
            thread_serial: 1,
            frame_ids: Vec::new(),
        };
        assert_eq!(st.memory_size(), size_of::<StackTrace>());
    }
}

#[cfg(test)]
mod new_type_compile_tests {
    use super::*;

    #[test]
    fn field_def_has_required_fields() {
        let f = FieldDef {
            name_string_id: 1,
            field_type: 10,
        };
        assert_eq!(f.name_string_id, 1);
        assert_eq!(f.field_type, 10);
    }

    #[test]
    fn class_dump_info_has_required_fields() {
        let c = ClassDumpInfo {
            class_object_id: 100,
            super_class_id: 50,
            instance_size: 16,
            static_fields: vec![StaticFieldDef {
                name_string_id: 2,
                value: StaticValue::Int(9),
            }],
            instance_fields: vec![FieldDef {
                name_string_id: 1,
                field_type: 10,
            }],
        };
        assert_eq!(c.class_object_id, 100);
        assert_eq!(c.super_class_id, 50);
        assert_eq!(c.instance_size, 16);
        assert_eq!(c.static_fields.len(), 1);
        assert_eq!(c.instance_fields.len(), 1);
    }

    #[test]
    fn raw_instance_has_required_fields() {
        let r = RawInstance {
            class_object_id: 200,
            data: vec![1, 2, 3],
        };
        assert_eq!(r.class_object_id, 200);
        assert_eq!(r.data, vec![1u8, 2, 3]);
    }
}
