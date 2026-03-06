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
        if let Some(idx) = self.corrupt_record_at {
            if let Some(&offset) = record_offsets.get(idx) {
                if offset < bytes.len() {
                    bytes[offset] = 0xFF;
                }
            }
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
        assert!(
            self.id_size == 4 || self.id_size == 8,
            "id_size must be 4 or 8, got {}",
            self.id_size
        );
        let all = id.to_be_bytes();
        all[8 - self.id_size as usize..].to_vec()
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
