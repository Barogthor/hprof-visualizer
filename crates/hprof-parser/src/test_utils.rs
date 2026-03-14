//! Programmatic builder for synthetic hprof byte sequences used in tests.
//!
//! [`HprofTestBuilder`] constructs valid `Vec<u8>` representing hprof files
//! without requiring real binary fixtures. Supports mutation helpers
//! (`truncate_at`, `corrupt_record_at`) for testing error paths.
//!
//! **Only available with the `test-utils` feature.**
//!
//! ## Example
//!
//! ```rust
//! use hprof_parser::HprofTestBuilder;
//!
//! let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
//!     .add_string(1, "main")
//!     .build();
//! assert_eq!(&bytes[..18], b"JAVA PROFILE 1.0.2");
//! ```

use crate::StaticValue;

/// Returns the byte offset of the first record in a builder-produced byte slice.
///
/// Advances past the null-terminated version string, `id_size` (`u32`), and
/// timestamp (`u64`) that constitute the hprof file header.
#[cfg(test)]
pub fn advance_past_header(bytes: &[u8]) -> usize {
    bytes.iter().position(|&b| b == 0).unwrap() + 1 + 4 + 8
}

/// Builder for synthetic hprof binary data.
///
/// Constructs a `Vec<u8>` conforming to the hprof binary format,
/// with optional truncation and corruption mutations for testing error paths.
pub struct HprofTestBuilder {
    version: &'static str,
    id_size: u32,
    records: Vec<Vec<u8>>,
    truncate_at: Option<usize>,
    corrupt_record_at: Option<usize>,
}

impl HprofTestBuilder {
    /// Creates a new builder.
    ///
    /// # Parameters
    /// - `version`: hprof magic string without null terminator (e.g. `"JAVA PROFILE 1.0.2"`);
    ///   `build()` appends the null byte automatically
    /// - `id_size`: byte width of object IDs — must be 4 or 8
    pub fn new(version: &'static str, id_size: u32) -> Self {
        Self::assert_valid_id_size(id_size);
        Self {
            version,
            id_size,
            records: Vec::new(),
            truncate_at: None,
            corrupt_record_at: None,
        }
    }

    /// Appends a `STRING_IN_UTF8` record (tag `0x01`).
    ///
    /// Payload: `id(id_size)` + UTF-8 bytes of `content`.
    pub fn add_string(mut self, id: u64, content: &str) -> Self {
        let mut payload = self.encode_id(id);
        payload.extend_from_slice(content.as_bytes());
        self.records.push(Self::make_record(0x01, &payload));
        self
    }

    /// Appends a `LOAD_CLASS` record (tag `0x02`).
    ///
    /// Payload: `class_serial(u32)` + `class_object_id(id_size)` +
    /// `stack_trace_serial(u32)` + `class_name_string_id(id_size)`.
    pub fn add_class(
        mut self,
        class_serial: u32,
        object_id: u64,
        stack_trace_serial: u32,
        class_name_string_id: u64,
    ) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&class_serial.to_be_bytes());
        payload.extend_from_slice(&self.encode_id(object_id));
        payload.extend_from_slice(&stack_trace_serial.to_be_bytes());
        payload.extend_from_slice(&self.encode_id(class_name_string_id));
        self.records.push(Self::make_record(0x02, &payload));
        self
    }

    /// Appends a `STACK_FRAME` record (tag `0x04`).
    ///
    /// Payload: `frame_id(id_size)` + `method_name_id(id_size)` +
    /// `method_sig_id(id_size)` + `source_file_id(id_size)` +
    /// `class_serial(u32)` + `line_number(i32)`.
    pub fn add_stack_frame(
        mut self,
        frame_id: u64,
        method_name_id: u64,
        method_sig_id: u64,
        source_file_id: u64,
        class_serial: u32,
        line_number: i32,
    ) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&self.encode_id(frame_id));
        payload.extend_from_slice(&self.encode_id(method_name_id));
        payload.extend_from_slice(&self.encode_id(method_sig_id));
        payload.extend_from_slice(&self.encode_id(source_file_id));
        payload.extend_from_slice(&class_serial.to_be_bytes());
        payload.extend_from_slice(&line_number.to_be_bytes());
        self.records.push(Self::make_record(0x04, &payload));
        self
    }

    /// Appends a `STACK_TRACE` record (tag `0x05`).
    ///
    /// Payload: `stack_trace_serial(u32)` + `thread_serial(u32)` +
    /// `num_frames(u32)` + `frame_ids([id_size; num_frames])`.
    pub fn add_stack_trace(
        mut self,
        stack_trace_serial: u32,
        thread_serial: u32,
        frame_ids: &[u64],
    ) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&stack_trace_serial.to_be_bytes());
        payload.extend_from_slice(&thread_serial.to_be_bytes());
        payload.extend_from_slice(&(frame_ids.len() as u32).to_be_bytes());
        for &fid in frame_ids {
            payload.extend_from_slice(&self.encode_id(fid));
        }
        self.records.push(Self::make_record(0x05, &payload));
        self
    }

    /// Appends a `START_THREAD` record (tag `0x06`).
    ///
    /// Payload: `thread_serial(u32)` + `object_id(id_size)` +
    /// `stack_trace_serial(u32)` + `name_string_id(id_size)` +
    /// `group_name_string_id(id_size)` + `group_parent_name_string_id(id_size)`.
    pub fn add_thread(
        mut self,
        thread_serial: u32,
        object_id: u64,
        stack_trace_serial: u32,
        name_string_id: u64,
        group_name_string_id: u64,
        group_parent_name_string_id: u64,
    ) -> Self {
        let mut payload = Vec::new();
        payload.extend_from_slice(&thread_serial.to_be_bytes());
        payload.extend_from_slice(&self.encode_id(object_id));
        payload.extend_from_slice(&stack_trace_serial.to_be_bytes());
        payload.extend_from_slice(&self.encode_id(name_string_id));
        payload.extend_from_slice(&self.encode_id(group_name_string_id));
        payload.extend_from_slice(&self.encode_id(group_parent_name_string_id));
        self.records.push(Self::make_record(0x06, &payload));
        self
    }

    /// Appends a `HEAP_DUMP_SEGMENT` record (tag `0x1C`) wrapping an
    /// `INSTANCE_DUMP` sub-record (sub-tag `0x21`).
    ///
    /// Sub-record payload: `object_id(id_size)` + `stack_trace_serial(u32)` +
    /// `class_object_id(id_size)` + `num_bytes(u32)` + `instance_data`.
    pub fn add_instance(
        mut self,
        object_id: u64,
        stack_trace_serial: u32,
        class_object_id: u64,
        instance_data: &[u8],
    ) -> Self {
        let mut sub = Vec::new();
        sub.push(0x21_u8); // INSTANCE_DUMP sub-tag
        sub.extend_from_slice(&self.encode_id(object_id));
        sub.extend_from_slice(&stack_trace_serial.to_be_bytes());
        sub.extend_from_slice(&self.encode_id(class_object_id));
        sub.extend_from_slice(&(instance_data.len() as u32).to_be_bytes());
        sub.extend_from_slice(instance_data);
        self.records.push(Self::make_record(0x1C, &sub));
        self
    }

    /// Appends a `HEAP_DUMP_SEGMENT` record (tag `0x1C`) wrapping an
    /// `OBJECT_ARRAY_DUMP` sub-record (sub-tag `0x22`).
    ///
    /// Sub-record payload: `array_id(id_size)` + `stack_trace_serial(u32)` +
    /// `num_elements(u32)` + `element_class_id(id_size)` +
    /// `elements([id_size; num_elements])`.
    pub fn add_object_array(
        mut self,
        array_id: u64,
        stack_trace_serial: u32,
        element_class_id: u64,
        elements: &[u64],
    ) -> Self {
        let mut sub = Vec::new();
        sub.push(0x22_u8); // OBJECT_ARRAY_DUMP sub-tag
        sub.extend_from_slice(&self.encode_id(array_id));
        sub.extend_from_slice(&stack_trace_serial.to_be_bytes());
        sub.extend_from_slice(&(elements.len() as u32).to_be_bytes());
        sub.extend_from_slice(&self.encode_id(element_class_id));
        for &elem in elements {
            sub.extend_from_slice(&self.encode_id(elem));
        }
        self.records.push(Self::make_record(0x1C, &sub));
        self
    }

    /// Appends a `HEAP_DUMP_SEGMENT` record (tag `0x1C`) wrapping a
    /// `PRIMITIVE_ARRAY_DUMP` sub-record (sub-tag `0x23`).
    ///
    /// Sub-record payload: `array_id(id_size)` + `stack_trace_serial(u32)` +
    /// `num_elements(u32)` + `element_type(u8)` + `byte_data`.
    pub fn add_prim_array(
        mut self,
        array_id: u64,
        stack_trace_serial: u32,
        num_elements: u32,
        element_type: u8,
        byte_data: &[u8],
    ) -> Self {
        let mut sub = Vec::new();
        sub.push(0x23_u8); // PRIMITIVE_ARRAY_DUMP sub-tag
        sub.extend_from_slice(&self.encode_id(array_id));
        sub.extend_from_slice(&stack_trace_serial.to_be_bytes());
        sub.extend_from_slice(&num_elements.to_be_bytes());
        sub.push(element_type);
        sub.extend_from_slice(byte_data);
        self.records.push(Self::make_record(0x1C, &sub));
        self
    }

    /// Appends a `HEAP_DUMP_SEGMENT` (tag `0x1C`) containing one `CLASS_DUMP`
    /// (sub-tag `0x20`) sub-record.
    ///
    /// `fields`: slice of `(name_string_id, field_type)` pairs.
    pub fn add_class_dump(
        self,
        class_object_id: u64,
        super_class_id: u64,
        instance_size: u32,
        fields: &[(u64, u8)],
    ) -> Self {
        self.add_class_dump_with_static_fields(
            class_object_id,
            super_class_id,
            instance_size,
            fields,
            &[],
        )
    }

    /// Appends a `HEAP_DUMP_SEGMENT` (tag `0x1C`) containing one `CLASS_DUMP`
    /// (sub-tag `0x20`) sub-record with static field values.
    ///
    /// - `instance_fields`: slice of `(name_string_id, field_type)` pairs.
    /// - `static_fields`: slice of `(name_string_id, value)` pairs.
    pub fn add_class_dump_with_static_fields(
        mut self,
        class_object_id: u64,
        super_class_id: u64,
        instance_size: u32,
        instance_fields: &[(u64, u8)],
        static_fields: &[(u64, StaticValue)],
    ) -> Self {
        let mut sub = vec![0x20u8]; // CLASS_DUMP sub-tag
        sub.extend_from_slice(&self.encode_id(class_object_id));
        sub.extend_from_slice(&0u32.to_be_bytes()); // stack_trace_serial
        sub.extend_from_slice(&self.encode_id(super_class_id));
        // classloader_id, signers_id, protection_domain_id, reserved1, reserved2 = 0
        for _ in 0..5 {
            sub.extend_from_slice(&self.encode_id(0));
        }
        sub.extend_from_slice(&instance_size.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes()); // constant_pool_count = 0
        sub.extend_from_slice(&(static_fields.len() as u16).to_be_bytes());
        for (name_id, value) in static_fields {
            sub.extend_from_slice(&self.encode_id(*name_id));
            sub.push(Self::static_value_type(value));
            Self::encode_static_value(&mut sub, value, self.id_size);
        }
        sub.extend_from_slice(&(instance_fields.len() as u16).to_be_bytes());
        for &(name_id, field_type) in instance_fields {
            sub.extend_from_slice(&self.encode_id(name_id));
            sub.push(field_type);
        }
        self.records.push(Self::make_record(0x1C, &sub));
        self
    }

    /// Appends a `HEAP_DUMP_SEGMENT` (tag `0x1C`) containing one
    /// `GC_ROOT_JAVA_FRAME` (sub-tag `0x03`) sub-record.
    ///
    /// Payload: `object_id(id_size)` + `thread_serial(u32)` + `frame_number(i32)`
    pub fn add_java_frame_root(
        mut self,
        object_id: u64,
        thread_serial: u32,
        frame_number: i32,
    ) -> Self {
        let mut sub = vec![0x03u8]; // GC_ROOT_JAVA_FRAME sub-tag
        sub.extend_from_slice(&self.encode_id(object_id));
        sub.extend_from_slice(&thread_serial.to_be_bytes());
        sub.extend_from_slice(&frame_number.to_be_bytes());
        self.records.push(Self::make_record(0x1C, &sub));
        self
    }

    /// Appends a `HEAP_DUMP_SEGMENT` (tag `0x1C`) containing one
    /// `ROOT_THREAD_OBJ` (sub-tag `0x08`) sub-record.
    ///
    /// Payload: `object_id(id_size)` + `thread_serial(u32)` +
    /// `stack_trace_serial(u32)`
    pub fn add_root_thread_obj(
        mut self,
        object_id: u64,
        thread_serial: u32,
        stack_trace_serial: u32,
    ) -> Self {
        let mut sub = vec![0x08u8]; // ROOT_THREAD_OBJ sub-tag
        sub.extend_from_slice(&self.encode_id(object_id));
        sub.extend_from_slice(&thread_serial.to_be_bytes());
        sub.extend_from_slice(&stack_trace_serial.to_be_bytes());
        self.records.push(Self::make_record(0x1C, &sub));
        self
    }

    /// Sets a truncation point. `build()` truncates the final bytes to `offset`.
    /// If `offset` exceeds the total length, this is a no-op.
    pub fn truncate_at(mut self, offset: usize) -> Self {
        self.truncate_at = Some(offset);
        self
    }

    /// Marks the record at `record_index` for corruption. `build()` overwrites
    /// the tag byte of that record with `0xFF`.
    pub fn corrupt_record_at(mut self, record_index: usize) -> Self {
        self.corrupt_record_at = Some(record_index);
        self
    }

    /// Serializes the builder into a `Vec<u8>` hprof byte sequence.
    ///
    /// Applies truncation and corruption mutations if configured.
    pub fn build(self) -> Vec<u8> {
        Self::assert_valid_id_size(self.id_size);

        let mut bytes = Vec::new();

        // Header: null-terminated version string
        bytes.extend_from_slice(self.version.as_bytes());
        bytes.push(0x00);

        // Header: id_size as u32 big-endian
        bytes.extend_from_slice(&self.id_size.to_be_bytes());

        // Header: dump timestamp (8 bytes, big-endian, use 0)
        bytes.extend_from_slice(&0u64.to_be_bytes());

        // Records — track start offsets for corrupt_record_at
        let mut record_offsets: Vec<usize> = Vec::new();
        for record in &self.records {
            record_offsets.push(bytes.len());
            bytes.extend_from_slice(record);
        }

        // Apply corruption mutation
        if let Some(idx) = self.corrupt_record_at
            && let Some(&offset) = record_offsets.get(idx)
            && offset < bytes.len()
        {
            bytes[offset] = 0xFF;
        }

        // Apply truncation mutation
        if let Some(offset) = self.truncate_at {
            bytes.truncate(offset);
        }

        bytes
    }

    /// Encodes `id` as big-endian bytes of length `id_size`.
    ///
    /// # Panics
    /// Panics if `id_size` is not 4 or 8 — the only valid hprof ID sizes.
    fn encode_id(&self, id: u64) -> Vec<u8> {
        Self::assert_valid_id_size(self.id_size);
        let all = id.to_be_bytes();
        all[8 - self.id_size as usize..].to_vec()
    }

    fn assert_valid_id_size(id_size: u32) {
        assert!(
            id_size == 4 || id_size == 8,
            "id_size must be 4 or 8, got {id_size}"
        );
    }

    fn static_value_type(value: &StaticValue) -> u8 {
        match value {
            StaticValue::ObjectRef(_) => 2,
            StaticValue::Bool(_) => 4,
            StaticValue::Char(_) => 5,
            StaticValue::Float(_) => 6,
            StaticValue::Double(_) => 7,
            StaticValue::Byte(_) => 8,
            StaticValue::Short(_) => 9,
            StaticValue::Int(_) => 10,
            StaticValue::Long(_) => 11,
        }
    }

    fn encode_static_value(out: &mut Vec<u8>, value: &StaticValue, id_size: u32) {
        match value {
            StaticValue::ObjectRef(id) => {
                let all = id.to_be_bytes();
                out.extend_from_slice(&all[8 - id_size as usize..]);
            }
            StaticValue::Bool(v) => out.push(u8::from(*v)),
            StaticValue::Char(v) => {
                let code = *v as u32;
                let unit = if code <= u16::MAX as u32 {
                    code as u16
                } else {
                    0xFFFD
                };
                out.extend_from_slice(&unit.to_be_bytes());
            }
            StaticValue::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
            StaticValue::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
            StaticValue::Byte(v) => out.push(*v as u8),
            StaticValue::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
            StaticValue::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
            StaticValue::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        }
    }

    /// Builds a record: `tag(u8)` + `time_offset(u32=0)` + `length(u32)` + `payload`.
    fn make_record(tag: u8, payload: &[u8]) -> Vec<u8> {
        let mut rec = Vec::new();
        rec.push(tag);
        rec.extend_from_slice(&0u32.to_be_bytes()); // time_offset = 0
        rec.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        rec.extend_from_slice(payload);
        rec
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;

    fn header_size(version: &str) -> usize {
        version.len() + 1 + 4 + 8 // null-term + id_size(u32) + timestamp(u64)
    }

    #[test]
    fn header_magic_at_offset_0() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8).build();
        assert_eq!(&bytes[..18], b"JAVA PROFILE 1.0.2");
        assert_eq!(bytes[18], 0x00, "null terminator after version");
    }

    #[test]
    fn header_id_size_correct_offset() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8).build();
        // id_size field starts right after the null-terminated version string
        let id_size_offset = version.len() + 1;
        let id_size = u32::from_be_bytes(
            bytes[id_size_offset..id_size_offset + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(id_size, 8);
    }

    #[test]
    fn header_id_size_4() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 4).build();
        let id_size_offset = version.len() + 1;
        let id_size = u32::from_be_bytes(
            bytes[id_size_offset..id_size_offset + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(id_size, 4);
    }

    #[test]
    fn string_record_tag_at_correct_offset() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_string(1, "main")
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x01, "STRING_IN_UTF8 tag");
    }

    #[test]
    fn string_record_payload_correct() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_string(1, "main")
            .build();
        let hdr = header_size(version);
        // tag(1) + time_offset(4) + length(4) = 9 bytes record header
        let rec_hdr = 9;
        // payload = id(8) + utf8 bytes of "main"(4)
        let payload_start = hdr + rec_hdr;
        let id = u64::from_be_bytes(bytes[payload_start..payload_start + 8].try_into().unwrap());
        assert_eq!(id, 1);
        assert_eq!(&bytes[payload_start + 8..payload_start + 12], b"main");
    }

    #[test]
    fn truncate_at_produces_correct_length() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_string(1, "hello")
            .truncate_at(10)
            .build();
        assert_eq!(bytes.len(), 10);
    }

    #[test]
    fn truncate_at_beyond_length_is_noop() {
        let version = "JAVA PROFILE 1.0.2";
        let full = HprofTestBuilder::new(version, 8)
            .add_string(1, "hi")
            .build();
        let truncated = HprofTestBuilder::new(version, 8)
            .add_string(1, "hi")
            .truncate_at(full.len() + 100)
            .build();
        assert_eq!(full.len(), truncated.len());
    }

    #[test]
    fn corrupt_record_at_overwrites_tag_with_ff() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_string(1, "foo")
            .corrupt_record_at(0)
            .build();
        let hdr = header_size(version);
        assert_eq!(
            bytes[hdr], 0xFF,
            "first record tag must be 0xFF after corruption"
        );
    }

    #[test]
    fn corrupt_second_record() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_string(1, "foo")
            .add_string(2, "bar")
            .corrupt_record_at(1)
            .build();
        let hdr = header_size(version);
        // First record: tag(1) + time_offset(4) + length(4) + id(8) + "foo"(3) = 20 bytes
        let first_record_size = 1 + 4 + 4 + 8 + 3;
        assert_eq!(bytes[hdr], 0x01, "first record tag unchanged");
        assert_eq!(
            bytes[hdr + first_record_size],
            0xFF,
            "second record tag must be 0xFF"
        );
    }

    #[test]
    fn add_class_record_tag() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_class(1, 100, 0, 200)
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x02, "LOAD_CLASS tag");
    }

    #[test]
    fn add_stack_frame_record_tag() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_stack_frame(1, 2, 3, 4, 5, 10)
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x04, "STACK_FRAME tag");
    }

    #[test]
    fn add_stack_trace_record_tag() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_stack_trace(1, 1, &[10, 20])
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x05, "STACK_TRACE tag");
    }

    #[test]
    fn add_thread_record_tag() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_thread(1, 100, 0, 1, 2, 3)
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x06, "START_THREAD tag");
    }

    #[test]
    fn add_object_array_produces_heap_dump_segment_with_0x22_subtag() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_object_array(42, 0, 100, &[])
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x1C, "HEAP_DUMP_SEGMENT tag");
        assert_eq!(bytes[hdr + 9], 0x22, "OBJECT_ARRAY_DUMP sub-tag");
    }

    #[test]
    fn add_prim_array_produces_heap_dump_segment_with_0x23_subtag() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_prim_array(99, 0, 0, 8, &[])
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x1C, "HEAP_DUMP_SEGMENT tag");
        assert_eq!(bytes[hdr + 9], 0x23, "PRIMITIVE_ARRAY_DUMP sub-tag");
    }

    #[test]
    fn add_class_dump_produces_heap_dump_segment_with_0x20_subtag_and_correct_field_count() {
        let version = "JAVA PROFILE 1.0.2";
        // two instance fields
        let bytes = HprofTestBuilder::new(version, 8)
            .add_class_dump(100, 0, 16, &[(10, 10u8), (11, 8u8)])
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x1C, "HEAP_DUMP_SEGMENT tag");
        assert_eq!(bytes[hdr + 9], 0x20, "CLASS_DUMP sub-tag");
    }

    #[test]
    fn add_instance_wraps_in_heap_dump_segment() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_instance(1, 0, 100, &[0xDE, 0xAD])
            .build();
        let hdr = header_size(version);
        assert_eq!(bytes[hdr], 0x1C, "HEAP_DUMP_SEGMENT tag");
        // sub-record starts after: tag(1) + time_offset(4) + length(4) = 9
        assert_eq!(bytes[hdr + 9], 0x21, "INSTANCE_DUMP sub-tag");
    }

    #[test]
    fn string_record_payload_id_size_4() {
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 4)
            .add_string(42, "hi")
            .build();
        let hdr = header_size(version);
        // record header: tag(1) + time_offset(4) + length(4) = 9
        let payload_start = hdr + 9;
        // id encoded on 4 bytes
        let id = u32::from_be_bytes(bytes[payload_start..payload_start + 4].try_into().unwrap());
        assert_eq!(id, 42);
        assert_eq!(&bytes[payload_start + 4..payload_start + 6], b"hi");
    }

    #[test]
    #[should_panic(expected = "id_size must be 4 or 8")]
    fn encode_id_panics_on_invalid_id_size() {
        HprofTestBuilder::new("JAVA PROFILE 1.0.2", 3)
            .add_string(1, "x")
            .build();
    }

    #[test]
    #[should_panic(expected = "id_size must be 4 or 8")]
    fn new_panics_on_invalid_id_size_without_records() {
        HprofTestBuilder::new("JAVA PROFILE 1.0.2", 3).build();
    }

    #[test]
    fn ac4_header_and_string_record() {
        // AC #4: specific byte layout check
        let version = "JAVA PROFILE 1.0.2";
        let bytes = HprofTestBuilder::new(version, 8)
            .add_string(1, "main")
            .build();

        // Magic at offset 0
        assert_eq!(&bytes[..18], b"JAVA PROFILE 1.0.2");

        // id_size at offset 19 (after 18 chars + null)
        let id_size_offset = 19;
        let id_size = u32::from_be_bytes(
            bytes[id_size_offset..id_size_offset + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(id_size, 8);

        // STRING record tag at offset 31 (header = 19 + 4 + 8 = 31)
        assert_eq!(bytes[31], 0x01, "STRING_IN_UTF8 tag at offset 31");
    }
}
