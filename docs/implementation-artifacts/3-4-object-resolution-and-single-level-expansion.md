# Story 3.4: Object Resolution & Single-Level Expansion

Status: done

## Story

As a user,
I want to expand a complex object to see its fields, with the system resolving the object
from the hprof file using segment-level indexes,
So that I can inspect individual object state at the point of the heap dump.

## Acceptance Criteria

1. **Given** a complex object with an expand indicator (`Object [expand →]`)
   **When** I press Enter to expand it
   **Then** expansion is initiated as a non-blocking async operation — the UI remains usable
   immediately (FR13)

2. **Given** an expansion is in progress
   **When** the object's children are loading
   **Then** the variable row changes to `Object [▼]` and a child pseudo-node
   `~ Loading...` appears below it; when complete the pseudo-node is replaced by the real
   children (primitives shown inline, complex objects with `Object [expand →]` indicators)
   (FR13, FR16, FR28)

3. **Given** an expansion is in progress and the cursor is on the `~ Loading...` pseudo-node
   **When** I press Escape
   **Then** the async operation is cancelled and the variable reverts to its collapsed
   `Object [expand →]` state

4. **Given** an expansion fails (object not found in file, corrupted instance data)
   **When** the error occurs
   **Then** the `~ Loading...` pseudo-node is replaced by `! Failed to resolve object` and
   navigation continues (NFR6)

5. **Given** an object that needs to be resolved from the hprof file
   **When** the engine resolves it
   **Then** it uses BinaryFuse8 segment filters to identify candidate segments, then performs
   a targeted scan within those segments to find the object (FR22)

## Tasks / Subtasks

---

### Task 1: New domain types — FieldDef, ClassDumpInfo, RawInstance (AC: #5)

**Files:**
- `crates/hprof-parser/src/types.rs`

- [x] **Red**: Write compile test — `FieldDef` has fields `name_string_id: u64` and
  `field_type: u8`
- [x] **Red**: Write compile test — `ClassDumpInfo` has fields `class_object_id: u64`,
  `super_class_id: u64`, `instance_size: u32`, `instance_fields: Vec<FieldDef>`
- [x] **Red**: Write compile test — `RawInstance` has fields `class_object_id: u64` and
  `data: Vec<u8>`
- [x] **Green**: Add to `types.rs`:

  ```rust
  /// One instance field definition from a `CLASS_DUMP` sub-record.
  ///
  /// `field_type` codes: 2=object ref, 4=bool, 5=char, 6=float, 7=double,
  /// 8=byte, 9=short, 10=int, 11=long.
  #[derive(Debug, Clone)]
  pub struct FieldDef {
      pub name_string_id: u64,
      pub field_type: u8,
  }

  /// Parsed instance field layout extracted from a `CLASS_DUMP` sub-record
  /// (heap sub-tag `0x20`).
  #[derive(Debug, Clone)]
  pub struct ClassDumpInfo {
      pub class_object_id: u64,
      pub super_class_id: u64,
      pub instance_size: u32,
      /// Instance fields in declaration order (NOT including inherited fields).
      pub instance_fields: Vec<FieldDef>,
  }

  /// Raw bytes of an `INSTANCE_DUMP` sub-record payload, returned by the
  /// object resolver before field decoding.
  #[derive(Debug, Clone)]
  pub struct RawInstance {
      pub class_object_id: u64,
      /// Field data bytes, ordered as declared in the class hierarchy.
      pub data: Vec<u8>,
  }
  ```

- [x] Re-export from `crates/hprof-parser/src/lib.rs`:
  `pub use types::{ClassDumpInfo, FieldDef, RawInstance};`

---

### Task 2: Parse CLASS_DUMP sub-records during first pass (AC: #5)

**Files:**
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`

Background: `CLASS_DUMP` (sub-tag `0x20`) is currently skipped via `skip_class_dump`.
We need to parse instance field definitions and store them in `PreciseIndex`.

`CLASS_DUMP` payload layout (after sub-tag byte):
```
class_id          id_size bytes
stack_trace_serial  4 bytes (u32)
super_class_id    id_size bytes
classloader_id    id_size bytes
signers_id        id_size bytes
protection_domain_id  id_size bytes
reserved1         id_size bytes
reserved2         id_size bytes
instance_size     4 bytes (u32)
constant_pool_count  2 bytes (u16)
[constant pool entries — each: index(u16) + type(u8) + value(type-dependent)]
static_fields_count  2 bytes (u16)
[static fields — each: name_string_id(id_size) + type(u8) + value(type-dependent)]
instance_fields_count  2 bytes (u16)
[instance fields — each: name_string_id(id_size) + type(u8)]
```

Value byte sizes by type code:
- 2 (object): `id_size`
- 4 (bool), 8 (byte): 1
- 5 (char), 9 (short): 2
- 6 (float), 10 (int): 4
- 7 (double), 11 (long): 8

- [x] **Red**: Test — `PreciseIndex` has field `class_dumps: HashMap<u64, ClassDumpInfo>`
- [x] **Green**: Add to `PreciseIndex`:
  ```rust
  /// `CLASS_DUMP` sub-records keyed by `class_object_id`.
  pub class_dumps: HashMap<u64, ClassDumpInfo>,
  ```
  Initialize to `HashMap::new()` in `PreciseIndex::new()`.

- [x] **Red**: Test — builder with one `add_class_dump` call → after `run_first_pass`,
  `index.class_dumps` has one entry with correct `class_object_id`, `super_class_id`,
  `instance_size`, and `instance_fields`
- [x] **Red**: Test — `add_class_dump` with two instance fields → `instance_fields.len() == 2`
- [x] **Red**: Test — `add_class_dump` with constant pool entries → still parses correctly
  (constant pool must be skipped properly)
- [x] **Green**: Replace `skip_class_dump` in `first_pass.rs` with a `parse_class_dump`
  function:

  ```rust
  fn parse_class_dump(
      cursor: &mut Cursor<&[u8]>,
      id_size: u32,
  ) -> Option<ClassDumpInfo> {
      let class_object_id = read_id(cursor, id_size).ok()?;
      let _stack_trace_serial = cursor.read_u32::<BigEndian>().ok()?;
      let super_class_id = read_id(cursor, id_size).ok()?;
      // skip classloader_id, signers_id, protection_domain_id, reserved1, reserved2
      if !skip_n(cursor, 5 * id_size as usize) { return None; }
      let instance_size = cursor.read_u32::<BigEndian>().ok()?;

      // Skip constant pool
      let cp_count = cursor.read_u16::<BigEndian>().ok()?;
      for _ in 0..cp_count {
          let _index = cursor.read_u16::<BigEndian>().ok()?;
          let cp_type = cursor.read_u8().ok()?;
          let val_size = value_byte_size(cp_type, id_size);
          if !skip_n(cursor, val_size) { return None; }
      }

      // Skip static fields
      let static_count = cursor.read_u16::<BigEndian>().ok()?;
      for _ in 0..static_count {
          if !skip_n(cursor, id_size as usize) { return None; }  // name_string_id
          let field_type = cursor.read_u8().ok()?;
          let val_size = value_byte_size(field_type, id_size);
          if !skip_n(cursor, val_size) { return None; }
      }

      // Parse instance fields
      let field_count = cursor.read_u16::<BigEndian>().ok()?;
      let mut instance_fields = Vec::with_capacity(field_count as usize);
      for _ in 0..field_count {
          let name_string_id = read_id(cursor, id_size).ok()?;
          let field_type = cursor.read_u8().ok()?;
          instance_fields.push(FieldDef { name_string_id, field_type });
      }

      Some(ClassDumpInfo { class_object_id, super_class_id, instance_size, instance_fields })
  }

  /// Returns the byte size of a value with the given hprof type code.
  fn value_byte_size(type_code: u8, id_size: u32) -> usize {
      match type_code {
          2 => id_size as usize,
          4 | 8 => 1,
          5 | 9 => 2,
          6 | 10 => 4,
          7 | 11 => 8,
          _ => 0,
      }
  }
  ```

  In the heap dump sub-record match, replace:
  ```rust
  0x20 => skip_class_dump(&mut cursor, id_size),
  ```
  with:
  ```rust
  0x20 => {
      match parse_class_dump(&mut cursor, id_size) {
          Some(info) => {
              index.class_dumps.insert(info.class_object_id, info);
              true
          }
          None => {
              add_warning(&mut warnings, "truncated CLASS_DUMP sub-record — skipping");
              false
          }
      }
  }
  ```

  Also add `use crate::{ClassDumpInfo, FieldDef};` import.

---

### Task 3: Store heap_record_ranges and records_start in HprofFile (AC: #5)

**Files:**
- `crates/hprof-parser/src/indexer/mod.rs` (IndexResult)
- `crates/hprof-parser/src/hprof_file.rs`

During the first pass, record the offset and payload length of every
`HEAP_DUMP` (0x0C) and `HEAP_DUMP_SEGMENT` (0x1C) record so the resolver can
do targeted scans without rescanning the entire file from offset 0.

- [x] **Red**: Test — `IndexResult` has field `heap_record_ranges: Vec<(u64, u64)>`
  (each entry is `(payload_start_offset, payload_length)` within the records section)
- [x] **Green**: Add to `IndexResult` in `indexer/mod.rs`:
  ```rust
  pub heap_record_ranges: Vec<(u64, u64)>,
  ```
  Initialize to `Vec::new()`. Populate in `run_first_pass` when the outer record loop
  encounters tag 0x0C or 0x1C:
  ```rust
  // In the outer record loop, after reading the record header:
  if rec.tag == 0x0C || rec.tag == 0x1C {
      let payload_start = cursor.position();
      heap_record_ranges.push((payload_start, rec.length as u64));
  }
  ```
  Pass `heap_record_ranges` into `IndexResult` at the end.

- [x] **Red**: Test — `HprofFile` has field `records_start: usize` and method
  `fn records_bytes(&self) -> &[u8]` returning the mmap slice from `records_start`
- [x] **Green**: Add `records_start: usize` to `HprofFile` and populate it in
  `from_path_with_progress` (store the value of `records_start` computed by `header_end`).
  Add:
  ```rust
  /// Returns the raw bytes of the records section (after the file header).
  pub fn records_bytes(&self) -> &[u8] {
      &self._mmap[self.records_start..]
  }
  ```

- [x] **Green**: Store `heap_record_ranges` from `IndexResult` as a new field
  `pub heap_record_ranges: Vec<(u64, u64)>` in `HprofFile`.

---

### Task 4: Implement `HprofFile::find_instance` (AC: #5)

**Files:** `crates/hprof-parser/src/hprof_file.rs`

`find_instance` locates an `INSTANCE_DUMP` (0x21) sub-record by `object_id` using a
two-phase approach:
1. Filter: query BinaryFuse8 segment filters to identify candidate segment indices
2. Scan: search only the `HEAP_DUMP_SEGMENT` records that overlap each candidate segment

- [x] **Red**: Test — `find_instance` on a file with one instance returns `Some(RawInstance)`
  with correct `class_object_id` and `data`
- [x] **Red**: Test — `find_instance` for an `object_id` not in the file returns `None`
- [x] **Red**: Test — `find_instance` on a file with two instances returns the correct one
- [x] **Red**: Test — `find_instance` on an instance with non-empty field data returns the
  correct `data` bytes

- [x] **Green**: Add to `HprofFile`:

  ```rust
  /// Finds and returns the raw instance dump for `object_id`.
  ///
  /// Uses BinaryFuse8 segment filters to narrow candidate segments, then
  /// performs a targeted scan of overlapping heap record payloads.
  ///
  /// Returns `None` if the object is not found (absent or filter
  /// false-positive).
  pub fn find_instance(&self, object_id: u64) -> Option<RawInstance> {
      use crate::indexer::segment::SEGMENT_SIZE;

      let records = self.records_bytes();
      let id_size = self.header.id_size;

      // Phase 1: collect candidate segment indices from BinaryFuse8 filters.
      let candidate_segs: Vec<usize> = self
          .segment_filters
          .iter()
          .filter(|f| f.contains(object_id))
          .map(|f| f.segment_index)
          .collect();

      if candidate_segs.is_empty() {
          return None;
      }

      // Phase 2: for each heap record that overlaps a candidate segment,
      // scan its sub-records for the target INSTANCE_DUMP.
      for &(payload_start, payload_len) in &self.heap_record_ranges {
          let payload_end = payload_start + payload_len;

          let overlaps = candidate_segs.iter().any(|&seg| {
              let seg_start = seg as u64 * SEGMENT_SIZE as u64;
              let seg_end = seg_start + SEGMENT_SIZE as u64;
              payload_start < seg_end && payload_end > seg_start
          });

          if !overlaps {
              continue;
          }

          let start = payload_start as usize;
          let end = (payload_end as usize).min(records.len());
          if start >= records.len() {
              continue;
          }

          if let Some(raw) = scan_for_instance(&records[start..end], object_id, id_size) {
              return Some(raw);
          }
      }

      None
  }
  ```

  Add private helper:

  ```rust
  fn scan_for_instance(data: &[u8], target_id: u64, id_size: u32) -> Option<RawInstance> {
      use std::io::Cursor;
      use byteorder::ReadBytesExt;
      use crate::read_id;

      let mut cursor = Cursor::new(data);
      loop {
          let sub_tag = match cursor.read_u8() {
              Ok(t) => t,
              Err(_) => return None,
          };
          match sub_tag {
              0x21 => {
                  // INSTANCE_DUMP
                  let obj_id = match read_id(&mut cursor, id_size) {
                      Ok(id) => id,
                      Err(_) => return None,
                  };
                  let _stack_serial = match cursor.read_u32::<BigEndian>() {
                      Ok(v) => v,
                      Err(_) => return None,
                  };
                  let class_object_id = match read_id(&mut cursor, id_size) {
                      Ok(id) => id,
                      Err(_) => return None,
                  };
                  let num_bytes = match cursor.read_u32::<BigEndian>() {
                      Ok(n) => n as usize,
                      Err(_) => return None,
                  };
                  let pos = cursor.position() as usize;
                  if pos + num_bytes > data.len() {
                      return None;
                  }
                  if obj_id == target_id {
                      return Some(RawInstance {
                          class_object_id,
                          data: data[pos..pos + num_bytes].to_vec(),
                      });
                  }
                  // Skip past this instance's data
                  cursor.set_position((pos + num_bytes) as u64);
              }
              // For all other sub-tags: skip using the same skip logic as first_pass.
              // Return None on any read error (truncated payload).
              _ => {
                  if !skip_sub_record(&mut cursor, sub_tag, id_size) {
                      return None;
                  }
              }
          }
      }
  }
  ```

  `skip_sub_record` should implement the same skip logic as `first_pass.rs` for all
  non-0x21 sub-tags. Extract the per-sub-tag skip logic from `first_pass.rs` into a
  shared helper (or duplicate it here — duplication is acceptable since it's in
  `hprof-parser` internal code). The key sub-tags to handle (from the existing first pass):
  - `0x01`: skip id_size bytes (GC_ROOT_UNKNOWN)
  - `0x02`: skip id_size bytes (GC_ROOT_JNI_GLOBAL has extra ref_id — skip 2×id_size)
  - `0x03`: skip id_size + 8 bytes (GC_ROOT_JAVA_FRAME)
  - `0x04`, `0x05`, `0x06`, `0x07`: various gc root types with fixed skips
  - `0x08`: skip id_size + 8 (GC_ROOT_THREAD_OBJECT)
  - `0x09`: skip id_size + 8 (GC_ROOT_MONITOR_USED)
  - `0x20`: skip CLASS_DUMP (use skip_class_dump)
  - `0x22`: skip OBJECT_ARRAY_DUMP
  - `0x23`: skip PRIMITIVE_ARRAY_DUMP
  - Unknown: return false (can't advance safely)

  See `first_pass.rs` for the existing skip implementations for each sub-tag.

---

### Task 5: Add `add_class_dump` to `HprofTestBuilder` (AC: #5)

**File:** `crates/hprof-parser/src/test_utils.rs`

- [x] **Green**: Add method:

  ```rust
  /// Appends a `HEAP_DUMP_SEGMENT` (tag `0x1C`) containing one `CLASS_DUMP`
  /// (sub-tag `0x20`) sub-record.
  ///
  /// `fields`: slice of `(name_string_id, field_type)` pairs.
  pub fn add_class_dump(
      mut self,
      class_object_id: u64,
      super_class_id: u64,
      instance_size: u32,
      fields: &[(u64, u8)],
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
      sub.extend_from_slice(&0u16.to_be_bytes()); // static_fields_count = 0
      sub.extend_from_slice(&(fields.len() as u16).to_be_bytes());
      for &(name_id, field_type) in fields {
          sub.extend_from_slice(&self.encode_id(name_id));
          sub.push(field_type);
      }
      self.records.push(Self::make_record(0x1C, &sub));
      self
  }
  ```

- [x] **Green**: Add test verifying `add_class_dump` produces correct sub-tag and field count

---

### Task 6: Replace `FieldInfo` stub with full types (AC: #1, #2)

**File:** `crates/hprof-engine/src/engine.rs`

- [x] **Red**: Write compile test — `FieldValue` has variants `Null`, `ObjectRef(u64)`,
  `Bool(bool)`, `Char(char)`, `Float(f32)`, `Double(f64)`, `Byte(i8)`, `Short(i16)`,
  `Int(i32)`, `Long(i64)`
- [x] **Red**: Write compile test — `FieldInfo` has fields `name: String` and
  `value: FieldValue`
- [x] **Green**: Replace:
  ```rust
  /// Placeholder for object field display — implemented in Story 3.4.
  #[derive(Debug)]
  pub struct FieldInfo {}
  ```
  with:
  ```rust
  /// Value of one object field, decoded from instance data bytes.
  #[derive(Debug, Clone, PartialEq)]
  pub enum FieldValue {
      /// Object reference with ID 0 (null).
      Null,
      /// Non-null object reference. Class name resolved in Story 3.5.
      ObjectRef(u64),
      Bool(bool),
      /// UTF-16 code unit decoded to Rust `char` (replacement char on invalid).
      Char(char),
      Float(f32),
      Double(f64),
      Byte(i8),
      Short(i16),
      Int(i32),
      Long(i64),
  }

  /// One field of an expanded object instance.
  #[derive(Debug, Clone)]
  pub struct FieldInfo {
      /// Human-readable field name resolved from structural strings.
      pub name: String,
      /// Decoded field value.
      pub value: FieldValue,
  }
  ```

- [x] Update `DummyEngine.expand_object` to return `Option<Vec<FieldInfo>>` → `Some(vec![])`
- [x] Change `expand_object` signature on `NavigationEngine` trait:
  ```rust
  /// Expands an object and returns its decoded fields.
  ///
  /// Returns `None` if the object cannot be resolved (not in file or
  /// BinaryFuse8 false positive).
  fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>>;
  ```

- [x] Update all `expand_object` call sites in engine_impl.rs and tests to match new
  signature.

- [x] Re-export from `crates/hprof-engine/src/lib.rs`:
  ```rust
  pub use engine::{FieldInfo, FieldValue};
  ```

---

### Task 7: Implement field decoding in `resolver.rs` (AC: #5)

**File:** `crates/hprof-engine/src/resolver.rs` (new file)

This module decodes raw `RawInstance` bytes into `Vec<FieldInfo>` using the class
hierarchy from `PreciseIndex`.

- [x] **Red**: Test — `decode_fields` on an instance of a class with one `int` field
  (type 10) returns `[FieldInfo { name: "count", value: FieldValue::Int(42) }]`
- [x] **Red**: Test — `decode_fields` with an inherited field (super class has one field,
  sub class has one field) returns 2 fields in super-first order
- [x] **Red**: Test — `decode_fields` with an object reference field (type 2, non-null
  id) returns `FieldValue::ObjectRef(id)`
- [x] **Red**: Test — `decode_fields` with object reference field where id=0 returns
  `FieldValue::Null`
- [x] **Red**: Test — `decode_fields` with a boolean field (type 4, value 1) returns
  `FieldValue::Bool(true)`
- [x] **Red**: Test — `decode_fields` with a long field (type 11) returns
  `FieldValue::Long(value)`
- [x] **Red**: Test — `decode_fields` with truncated data returns empty vec (graceful)

- [x] **Green**: Create `crates/hprof-engine/src/resolver.rs`:

  ```rust
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
  /// subclass fields last. Unknown field types produce `FieldValue::Null`
  /// and consume 0 bytes (safe fallback).
  ///
  /// Returns an empty `Vec` if the data is truncated or class info is missing.
  pub fn decode_fields(raw: &RawInstance, index: &PreciseIndex, id_size: u32) -> Vec<FieldInfo> {
      // Collect ordered field defs: superclass → subclass.
      let mut ordered_defs: Vec<(u64, u8)> = Vec::new(); // (name_string_id, type)
      collect_fields(raw.class_object_id, index, &mut ordered_defs);

      let mut cursor = Cursor::new(raw.data.as_slice());
      let mut fields = Vec::with_capacity(ordered_defs.len());

      for (name_string_id, field_type) in ordered_defs {
          let name = index
              .strings
              .get(&name_string_id)
              .map(|s| s.value.clone())
              .unwrap_or_else(|| format!("<field:{name_string_id}>"));

          let value = match read_field_value(&mut cursor, field_type, id_size) {
              Some(v) => v,
              None => break, // truncated data — stop decoding
          };
          fields.push(FieldInfo { name, value });
      }

      fields
  }

  /// Recursively collects (name_string_id, field_type) pairs from the full
  /// class hierarchy, superclass first.
  fn collect_fields(
      class_id: u64,
      index: &PreciseIndex,
      out: &mut Vec<(u64, u8)>,
  ) {
      let Some(info) = index.class_dumps.get(&class_id) else { return };
      // Recurse into superclass first (depth-first, super before sub).
      if info.super_class_id != 0 && info.super_class_id != class_id {
          collect_fields(info.super_class_id, index, out);
      }
      for field in &info.instance_fields {
          out.push((field.name_string_id, field.field_type));
      }
  }

  fn read_field_value(cursor: &mut Cursor<&[u8]>, type_code: u8, id_size: u32) -> Option<FieldValue> {
      match type_code {
          2 => {
              let id = read_id(cursor, id_size).ok()?;
              Some(if id == 0 { FieldValue::Null } else { FieldValue::ObjectRef(id) })
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
          _ => None, // unknown type — cannot determine byte width, stop
      }
  }
  ```

- [x] Add `pub(crate) mod resolver;` to `crates/hprof-engine/src/lib.rs`

---

### Task 8: Implement `expand_object` in `engine_impl.rs` (AC: #5)

**File:** `crates/hprof-engine/src/engine_impl.rs`

- [x] **Red**: Test — `expand_object(object_id)` on a file with an instance having one
  `int` field returns `Some([FieldInfo { name: "x", value: FieldValue::Int(7) }])`
- [x] **Red**: Test — `expand_object(object_id)` with a class dump and instance dump in
  the same file returns fields in super-to-sub order (2 fields from 2 classes)
- [x] **Red**: Test — `expand_object` on an unknown object_id returns `None`
- [x] **Red**: Test — `expand_object` on an object with an `ObjectRef` field (non-null)
  returns `FieldValue::ObjectRef(id)` (NOT expanded further — that is Story 3.5)
- [x] **Green**: Replace stub `expand_object`:
  ```rust
  fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>> {
      let raw = self.hfile.find_instance(object_id)?;
      let fields = crate::resolver::decode_fields(&raw, &self.hfile.index, self.hfile.header.id_size);
      Some(fields)
  }
  ```

- [x] Wrap `hfile` in `Arc` so the engine is shareable across threads:
  - Change `Engine` struct: `hfile: Arc<HprofFile>` (was `hfile: HprofFile`)
  - Change `from_file` and `from_file_with_progress` to wrap in `Arc::new(hfile)`
  - Add `use std::sync::Arc;` import
  - `Engine: Send + Sync` will follow automatically since `HprofFile: Send + Sync`
    (verify: `Mmap` is `Send + Sync` for read-only mappings; all index types are too)

---

### Task 9: Async expansion machinery in `App` (AC: #1, #2, #3, #4)

**File:** `crates/hprof-tui/src/app.rs`

The `App` handles async object expansion by spawning a worker thread for each
`expand_object` call and polling the result channel each render frame.

Key design:
- `App<E: NavigationEngine + Send + Sync + 'static>` — add `Send + Sync + 'static` bounds
- `App.engine: Arc<E>` (was `E`) — wrap in Arc to share with worker threads
- `App.pending_expansions: HashMap<u64, Receiver<Option<Vec<FieldInfo>>>>` — per object_id

- [x] **Red**: Test — after calling `start_object_expansion(object_id)` on a stub engine
  (synchronous), `poll_expansions()` immediately returns the result and updates
  `StackState` with the fields
- [x] **Red**: Test — `poll_expansions` on an object_id that returned `None` from engine
  triggers `Failed` state in `StackState`
- [x] **Red**: Test — `cancel_object_expansion(object_id)` removes the pending receiver
  and calls `StackState.cancel_expansion(object_id)`

- [x] **Green**: Update `App` struct:
  ```rust
  use std::sync::{Arc, mpsc::Receiver};

  pub struct App<E: NavigationEngine> {
      engine: Arc<E>,
      // ... existing fields ...
      pending_expansions: HashMap<u64, Receiver<Option<Vec<FieldInfo>>>>,
  }
  ```

- [x] **Green**: Change `App::new` signature to accept `engine: E` (unchanged externally)
  but wrap internally: `engine: Arc::new(engine)`.
  Update `run_tui` / `run_loop` calls to match.

- [x] **Green**: Add to `App`:

  ```rust
  /// Spawns a worker thread to expand `object_id` asynchronously.
  ///
  /// The caller must ensure `E: Send + Sync + 'static` — enforced at the
  /// call site via additional bound on `handle_stack_frames_input`.
  fn start_object_expansion(&mut self, object_id: u64)
  where
      E: Send + Sync + 'static,
  {
      let engine = Arc::clone(&self.engine);
      let (tx, rx) = std::sync::mpsc::channel();
      std::thread::spawn(move || {
          let result = engine.expand_object(object_id);
          let _ = tx.send(result);
      });
      self.pending_expansions.insert(object_id, rx);
      if let Some(s) = &mut self.stack_state {
          s.set_expansion_loading(object_id);
      }
  }

  /// Polls all pending expansion channels (non-blocking). Call once per
  /// render frame before drawing.
  pub fn poll_expansions(&mut self) {
      let completed: Vec<u64> = self
          .pending_expansions
          .iter()
          .filter_map(|(&id, rx)| rx.try_recv().ok().map(|result| (id, result)))
          .map(|(id, result)| {
              if let Some(s) = &mut self.stack_state {
                  match result {
                      Some(fields) => s.set_expansion_done(id, fields),
                      None => s.set_expansion_failed(id, "Failed to resolve object".to_string()),
                  }
              }
              id
          })
          .collect();
      for id in completed {
          self.pending_expansions.remove(&id);
      }
  }
  ```

- [x] **Green**: In `handle_stack_frames_input`, change Enter handling for `OnVar` cursor
  with `VariableValue::ObjectRef(id)` (when not already expanded/loading):
  ```rust
  // When cursor is OnVar and var is ObjectRef and not loading:
  InputEvent::Enter => {
      if let Some(s) = &mut self.stack_state {
          // Existing frame expand logic...
          if let Some(frame_id) = s.selected_frame_id() {
              if matches!(s.cursor(), StackCursor::OnFrame(_)) {
                  // existing toggle_expand logic
              } else if let Some(object_id) = s.selected_object_id() {
                  if s.expansion_state(object_id) == ExpansionPhase::Collapsed {
                      self.start_object_expansion(object_id);
                  } else if s.expansion_state(object_id) == ExpansionPhase::Expanded {
                      s.collapse_object(object_id);
                  }
                  // Loading: Enter is no-op
              }
          }
      }
  }
  ```

- [x] **Green**: In `handle_stack_frames_input`, update Escape to cancel loading if the
  cursor is on a loading pseudo-node:
  ```rust
  InputEvent::Escape => {
      if let Some(s) = &mut self.stack_state {
          if let Some(id) = s.selected_loading_object_id() {
              // Cancel: drop the receiver, revert state
              self.pending_expansions.remove(&id);
              s.cancel_expansion(id);
              return AppAction::Continue;
          }
      }
      // Default: return to ThreadList
      self.stack_state = None;
      self.focus = Focus::ThreadList;
      self.refresh_preview_stack();
  }
  ```

- [x] **Green**: Call `self.poll_expansions()` at the start of `App::render`.

- [x] Update `run_tui` and `run_loop` bounds: `E: NavigationEngine + Send + Sync + 'static`

---

### Task 10: Extend `StackState` for object expansion (AC: #2, #3, #4)

**File:** `crates/hprof-tui/src/views/stack_view.rs`

Extend `StackState` to support per-object expansion: Collapsed, Loading,
Expanded, and Failed states, each represented as inline rows in the flat item list.

**New types:**

```rust
/// Phase of an object expansion driven by `App`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionPhase {
    Collapsed,
    Loading,
    Expanded,
    Failed,
}
```

**Changes to `StackState`:**

```rust
pub struct StackState {
    frames: Vec<FrameInfo>,
    vars: HashMap<u64, Vec<VariableInfo>>,
    expanded: HashSet<u64>,              // frame expansion (existing)
    cursor: StackCursor,
    list_state: ListState,
    // NEW fields:
    object_phases: HashMap<u64, ExpansionPhase>,
    object_fields: HashMap<u64, Vec<FieldInfo>>,   // populated on Done
    object_errors: HashMap<u64, String>,            // populated on Failed
}
```

**Changes to `StackCursor`:**

```rust
pub enum StackCursor {
    NoFrames,
    OnFrame(usize),
    OnVar { frame_idx: usize, var_idx: usize },
    // NEW:
    OnObjectField { frame_idx: usize, var_idx: usize, field_idx: usize },
    OnObjectLoadingNode { frame_idx: usize, var_idx: usize },
}
```

**New methods on `StackState`:**

```rust
/// Returns the current cursor.
pub fn cursor(&self) -> &StackCursor { &self.cursor }

/// Returns the object_id of the selected var if it is an ObjectRef.
pub fn selected_object_id(&self) -> Option<u64> { ... }

/// Returns the object_id if the cursor is on a loading pseudo-node.
pub fn selected_loading_object_id(&self) -> Option<u64> { ... }

/// Returns the expansion phase for `object_id`.
pub fn expansion_state(&self, object_id: u64) -> ExpansionPhase { ... }

/// Marks an object as loading (called by App on expansion start).
pub fn set_expansion_loading(&mut self, object_id: u64) { ... }

/// Marks an object expansion as complete with fields.
pub fn set_expansion_done(&mut self, object_id: u64, fields: Vec<FieldInfo>) { ... }

/// Marks an object expansion as failed.
pub fn set_expansion_failed(&mut self, object_id: u64, error: String) { ... }

/// Cancels a loading expansion — reverts to Collapsed.
pub fn cancel_expansion(&mut self, object_id: u64) { ... }

/// Collapses an expanded object.
pub fn collapse_object(&mut self, object_id: u64) { ... }
```

**Update `flat_items()`:** When a var holds an `ObjectRef(id)`:
- Phase `Loading`: emit `OnVar` then `OnObjectLoadingNode`
- Phase `Expanded`: emit `OnVar` then one `OnObjectField` per field (or `OnObjectLoadingNode`
  if fields are empty — i.e., object has no fields, show `(no fields)`)
- Phase `Failed`: emit `OnVar` then `OnObjectLoadingNode` (used as the error row)
- Phase `Collapsed`: emit `OnVar` only

**Update `build_items()`:**
- `OnVar` with `ObjectRef(id)` and phase `Loading` or `Expanded` or `Failed`: show header
  `[n] Object [▼]` (collapsed arrow replaced by open arrow to show state)
- `OnObjectLoadingNode`: show `    ~ Loading...` (loading) or `    ! Failed: <msg>` (failed)
  or `    (no fields)` (expanded but empty)
- `OnObjectField { field_idx }`: show `    fieldName: <value>` where `<value>` is:
  - Primitives: the value directly (e.g., `42 (int)`, `true`, `3.14 (float)`)
  - `FieldValue::Null`: `null`
  - `FieldValue::ObjectRef(id)`: `Object [expand →]` (further expansion in Story 3.5)

- [x] **Red**: Test — `set_expansion_loading(id)` → `expansion_state(id) == Loading`
- [x] **Red**: Test — `set_expansion_done(id, fields)` → `expansion_state(id) == Expanded`
- [x] **Red**: Test — `set_expansion_failed(id, msg)` → `expansion_state(id) == Failed`
- [x] **Red**: Test — `cancel_expansion(id)` on Loading → `expansion_state(id) == Collapsed`
- [x] **Red**: Test — `flat_items()` on a var with ObjectRef in Loading state includes
  `OnObjectLoadingNode` after `OnVar`
- [x] **Red**: Test — `flat_items()` on an Expanded var with 2 fields includes 2
  `OnObjectField` rows after `OnVar`
- [x] **Red**: Test — `move_down()` from OnVar (Expanded) moves to first `OnObjectField`
- [x] **Red**: Test — `move_down()` past last `OnObjectField` moves to next frame or var
- [x] **Red**: Test — `selected_loading_object_id()` on `OnObjectLoadingNode` cursor returns
  the object_id
- [x] **Green**: Implement all the above

---

### Task 11: Verify all checks pass

- [x] `cargo test -p hprof-parser` — class dump parsing, find_instance, builder
- [x] `cargo test -p hprof-engine` — expand_object, resolver decode_fields
- [x] `cargo test -p hprof-tui` — StackState expansion states, App async polling
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`
- [x] `cargo fmt -- --check`
- [ ] Manual smoke test: `cargo run -- assets/heapdump-visualvm.hprof` — navigate to a
  thread → Enter → select a frame → Enter → select an `Object [expand →]` var → Enter →
  `~ Loading...` appears → fields appear; Escape on loading node cancels expansion

---

## Dev Notes

### hprof INSTANCE_DUMP Format

Sub-tag `0x21` payload:
```
object_id        id_size bytes
stack_trace_serial  4 bytes (u32)  -- ignored by resolver
class_object_id  id_size bytes
num_bytes        4 bytes (u32)     -- byte count of field data following
field data       num_bytes bytes   -- interpreted using class hierarchy
```

Field data bytes are laid out as: superclass fields first, then subclass fields,
each field occupying the number of bytes determined by its type code. There are
NO separators between fields — the decoder must know the field layout from the
CLASS_DUMP chain.

### CLASS_DUMP Super-Class Chain

For a class like `ArrayList extends AbstractList extends AbstractCollection extends Object`:
- `instance_fields` of each class contains ONLY that class's own declared fields
- `super_class_id == 0` means the super is `java.lang.Object` (which has no instance fields
  tracked in typical hprof files) or no super
- Walk the chain: collect super fields first, then sub fields
- Guard against cycles: if `super_class_id == class_object_id`, stop recursion
- Guard against missing class dumps: if `class_dumps.get(super_id)` returns None, stop
  (the object may still be partially decoded from the known fields)

### Async Architecture

`StackState` is intentionally **not** aware of channels or threads — it only tracks
`ExpansionPhase`. The `App` owns all async state (channels, pending receivers) and
calls `StackState` mutation methods to update expansion state when results arrive.

```
App.start_object_expansion(id) →
    thread::spawn(|| engine.expand_object(id)) →
    rx stored in App.pending_expansions[id]
    StackState.set_expansion_loading(id)

App.poll_expansions() [called every render frame] →
    for each pending rx: try_recv() →
        Some(result) → StackState.set_expansion_done/failed(id)
        Err(Empty)   → continue polling next frame
    remove completed from pending_expansions
```

### BinaryFuse8 False Positives

The segment filters have a ~0.4% false positive rate. `find_instance` may scan a segment
that does NOT contain the object (false positive from BinaryFuse8). The scan will simply
not find the object and return `None`. This is correct and non-fatal.

### `SegmentFilter` Visibility

`SegmentFilter` is `pub(crate)` in `hprof-parser`. The `find_instance` method lives in
`hprof_file.rs` (also in `hprof-parser`), so it has access to `SegmentFilter.contains()`.
No visibility change is needed.

### `Arc<HprofFile>` Requirement

`Engine` needs `hfile: Arc<HprofFile>` (not `HprofFile`) so the Arc can be cloned into
worker threads. Verify `HprofFile: Send + Sync`:
- `memmap2::Mmap`: `Send + Sync` (read-only mapping)
- `PreciseIndex` fields: all owned types → `Send + Sync`
- `xorf::BinaryFuse8`: pure data, no references → `Send + Sync`
- `SegmentFilter`: contains `BinaryFuse8` → `Send + Sync`

If the compiler rejects `Engine: Send + Sync`, add explicit `unsafe impl` only after
verifying the above invariants. In practice this should not be needed.

### `VariableValue` unchanged in `engine.rs`

`VariableValue::ObjectRef(u64)` in Story 3.3 stores the raw object_id. In Story 3.4
the `object_id` is used as the key for `start_object_expansion`. No change to
`VariableValue` is needed for this story — the type already carries everything needed.

### Field Value Display Conventions

| FieldValue variant | Display string |
|---|---|
| `Null` | `null` |
| `ObjectRef(id)` | `Object [expand →]` |
| `Bool(v)` | `true` / `false` |
| `Char(c)` | the char literal, e.g. `'A'` |
| `Byte(v)`, `Short(v)`, `Int(v)`, `Long(v)` | decimal integer |
| `Float(v)`, `Double(v)` | decimal float with enough precision |

Prepend field name: `fieldName: <value>`.

### Module Structure — Files Changed

```
crates/hprof-parser/src/
├── types.rs               updated: FieldDef, ClassDumpInfo, RawInstance
├── lib.rs                 updated: re-export FieldDef, ClassDumpInfo, RawInstance
├── hprof_file.rs          updated: records_start field, records_bytes(), find_instance(),
│                                   heap_record_ranges field
├── test_utils.rs          updated: add_class_dump method
└── indexer/
    ├── mod.rs             updated: heap_record_ranges in IndexResult
    ├── precise.rs         updated: class_dumps field
    └── first_pass.rs      updated: parse_class_dump replaces skip_class_dump,
                                    record heap_record_ranges

crates/hprof-engine/src/
├── engine.rs              updated: FieldInfo, FieldValue types; expand_object signature
├── engine_impl.rs         updated: expand_object impl; hfile: Arc<HprofFile>
├── resolver.rs            NEW: decode_fields, collect_fields, read_field_value
└── lib.rs                 updated: pub mod resolver; re-export FieldInfo, FieldValue

crates/hprof-tui/src/
├── app.rs                 updated: pending_expansions, start_object_expansion,
│                                   poll_expansions, Escape cancel, E bounds
└── views/
    └── stack_view.rs      updated: ExpansionPhase, object_phases/fields/errors in
                                    StackState; OnObjectField/OnObjectLoadingNode in
                                    StackCursor; build_items + flat_items extended
```

### Previous Story Intelligence (3.3)

Patterns established in 3.3 to continue:
- `StackState` encapsulates all cursor logic behind methods — no direct field access
- `flat_items()` rebuilds the full flat list on every call (KISS — no caching)
- `build_items()` produces `Vec<ListItem<'static>>` from current state
- Theme constants from `theme.rs` — no inline colors
- `App<E: NavigationEngine>` — engine generic, no concrete type leakage
- Error handling in engine: non-fatal issues → return `None` (not panic)
- 237 tests pass after Story 3.3

### Git Intelligence

Recent commits pattern:
- One focused commit per story
- `fix:` prefix for bug fixes (e.g., `59b1552 fix: thread list empty on real hprof files`)
- 237 tests after Story 3.3 baseline

### References

- [Source: docs/planning-artifacts/epics.md#Story 3.4]
- [Source: docs/planning-artifacts/architecture.md#Data Architecture — Indexing Strategy]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture — Navigation Engine Trait]
- [Source: docs/planning-artifacts/ux-design-specification.md#TreeView — Async loading pattern]
- [Source: crates/hprof-parser/src/indexer/first_pass.rs — existing sub-tag skip logic]
- [Source: crates/hprof-parser/src/indexer/segment.rs — SegmentFilter, SEGMENT_SIZE]
- [Source: crates/hprof-parser/src/hprof_file.rs — HprofFile struct, header_end]
- [Source: crates/hprof-engine/src/engine.rs — FieldInfo stub, NavigationEngine trait]
- [Source: crates/hprof-engine/src/engine_impl.rs — Engine struct, expand_object stub]
- [Source: crates/hprof-tui/src/views/stack_view.rs — StackState, StackCursor patterns]
- [Source: crates/hprof-tui/src/app.rs — App struct, handle_stack_frames_input]
- [Source: docs/implementation-artifacts/3-3-stack-frame-and-local-variable-display.md —
  StackState pattern, App event loop, 237 baseline test count]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- Removed `skip_class_dump` (dead code after replacement with `parse_class_dump`)
- `poll_expansions` uses an iterative collect-then-remove pattern to avoid borrow conflicts
- `handle_stack_frames_input` uses a local `enum Cmd` to separate the read and write phases
  of Enter handling (required to avoid simultaneous &/&mut borrows of self.stack_state)
- All 289 workspace tests pass; clippy clean; fmt clean
- Code review fixes (2026-03-07): `collect_fields` converted to iterative walk with visited
  HashSet to prevent multi-node cycle stack overflow (H1); Enter on Loading var is now a
  no-op instead of silently cancelling (M2); failure message aligned to AC4 wording (M1);
  Task 6 `expand_object` trait signature checkbox corrected to [x] (M3)

### File List

- `crates/hprof-parser/src/types.rs` — added `FieldDef`, `ClassDumpInfo`, `RawInstance`
- `crates/hprof-parser/src/lib.rs` — re-exported `ClassDumpInfo`, `FieldDef`, `RawInstance`
- `crates/hprof-parser/src/indexer/precise.rs` — added `class_dumps: HashMap<u64, ClassDumpInfo>`
- `crates/hprof-parser/src/indexer/mod.rs` — added `heap_record_ranges: Vec<(u64, u64)>`
- `crates/hprof-parser/src/indexer/first_pass.rs` — `parse_class_dump`, `value_byte_size`,
  heap record range collection, removed `skip_class_dump`
- `crates/hprof-parser/src/hprof_file.rs` — `records_start`, `heap_record_ranges`,
  `records_bytes()`, `find_instance()`, `scan_for_instance()`, `skip_sub_record()`
- `crates/hprof-parser/src/test_utils.rs` — `add_class_dump()` builder method
- `crates/hprof-engine/src/engine.rs` — `FieldValue` enum, full `FieldInfo` struct,
  `expand_object` returns `Option<Vec<FieldInfo>>`
- `crates/hprof-engine/src/engine_impl.rs` — real `expand_object`, `hfile: Arc<HprofFile>`
- `crates/hprof-engine/src/resolver.rs` (new) — `decode_fields`, `collect_fields`,
  `read_field_value`
- `crates/hprof-engine/src/lib.rs` — `pub(crate) mod resolver`, re-exported `FieldValue`
- `crates/hprof-engine/Cargo.toml` — added `byteorder = "1"`
- `crates/hprof-tui/src/views/stack_view.rs` — `ExpansionPhase`, `OnObjectField`,
  `OnObjectLoadingNode` cursors, object phase/field/error maps in `StackState`
- `crates/hprof-tui/src/app.rs` — `pending_expansions`, `start_object_expansion`,
  `poll_expansions`, Escape cancel, `engine: Arc<E>`, `Send + Sync + 'static` bounds
