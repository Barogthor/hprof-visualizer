# Story 2.2: Structural Record Parsing

Status: done

## Story

As a developer,
I want the system to parse STRING, LOAD_CLASS, START_THREAD, STACK_FRAME, and STACK_TRACE
records into well-defined domain types,
so that the indexer can build precise indexes from these records during the first pass.

## Acceptance Criteria

1. **Given** a STRING record with a known ID and UTF-8 content
   **When** parsed
   **Then** an `HprofString { id: u64, value: String }` is produced with the correct ID and
   content (FR6)

2. **Given** a LOAD_CLASS record
   **When** parsed
   **Then** a `ClassDef { class_serial: u32, class_object_id: u64, stack_trace_serial: u32,
   class_name_string_id: u64 }` is produced with all fields correctly read

3. **Given** a START_THREAD record
   **When** parsed
   **Then** an `HprofThread { thread_serial: u32, object_id: u64, stack_trace_serial: u32,
   name_string_id: u64, group_name_string_id: u64, group_parent_name_string_id: u64 }` is
   produced with all fields correctly read

4. **Given** a STACK_FRAME record
   **When** parsed
   **Then** a `StackFrame { frame_id: u64, method_name_string_id: u64, method_sig_string_id:
   u64, source_file_string_id: u64, class_serial: u32, line_number: i32 }` is produced with
   all fields correctly read

5. **Given** a STACK_TRACE record
   **When** parsed
   **Then** a `StackTrace { stack_trace_serial: u32, thread_serial: u32, frame_ids: Vec<u64>
   }` is produced with the ordered list of frame IDs

6. **Given** any structural record where the payload is shorter than expected
   **When** parsed
   **Then** `HprofError::TruncatedRecord` is returned — never a panic

## Tasks / Subtasks

- [x] Create `crates/hprof-parser/src/strings.rs` (AC: #1, #6)
  - [x] **Red**: Write test — parse STRING with id_size=8, id=5, content="main" → HprofString
        { id: 5, value: "main" }
  - [x] **Red**: Write test — parse STRING with id_size=4
  - [x] **Red**: Write test — payload shorter than id_size → TruncatedRecord
  - [x] **Red**: Write test — valid id but zero-length UTF-8 content → HprofString with
        empty value
  - [x] **Green**: Define `pub struct HprofString { pub id: u64, pub value: String }`
  - [x] **Green**: Implement `pub fn parse_string_record(cursor: &mut Cursor<&[u8]>,
        id_size: u32, payload_length: u32) -> Result<HprofString, HprofError>`
  - [x] **Refactor**: No `unwrap()`/`expect()` in production code
  - [x] Add `//!` module docstring

- [x] Create `crates/hprof-parser/src/types.rs` (AC: #2, #3, #4, #5, #6)
  - [x] **Red**: Write test — parse LOAD_CLASS with id_size=8 → correct ClassDef fields
  - [x] **Red**: Write test — parse LOAD_CLASS with id_size=4
  - [x] **Red**: Write test — truncated LOAD_CLASS → TruncatedRecord
  - [x] **Green**: Define `pub struct ClassDef { pub class_serial: u32, pub class_object_id:
        u64, pub stack_trace_serial: u32, pub class_name_string_id: u64 }`
  - [x] **Green**: Implement `pub fn parse_load_class(cursor: &mut Cursor<&[u8]>, id_size:
        u32) -> Result<ClassDef, HprofError>`
  - [x] **Red**: Write test — parse START_THREAD with id_size=8 → correct HprofThread fields
  - [x] **Red**: Write test — parse START_THREAD with id_size=4
  - [x] **Red**: Write test — truncated START_THREAD → TruncatedRecord
  - [x] **Green**: Define `pub struct HprofThread { pub thread_serial: u32, pub object_id:
        u64, pub stack_trace_serial: u32, pub name_string_id: u64, pub group_name_string_id:
        u64, pub group_parent_name_string_id: u64 }`
  - [x] **Green**: Implement `pub fn parse_start_thread(cursor: &mut Cursor<&[u8]>, id_size:
        u32) -> Result<HprofThread, HprofError>`
  - [x] **Red**: Write test — parse STACK_FRAME with id_size=8 → correct StackFrame fields
        (line_number positive, zero, negative)
  - [x] **Red**: Write test — parse STACK_FRAME with id_size=4
  - [x] **Red**: Write test — truncated STACK_FRAME → TruncatedRecord
  - [x] **Green**: Define `pub struct StackFrame { pub frame_id: u64, pub
        method_name_string_id: u64, pub method_sig_string_id: u64, pub source_file_string_id:
        u64, pub class_serial: u32, pub line_number: i32 }`
  - [x] **Green**: Implement `pub fn parse_stack_frame(cursor: &mut Cursor<&[u8]>, id_size:
        u32) -> Result<StackFrame, HprofError>`
  - [x] **Red**: Write test — parse STACK_TRACE with id_size=8, 3 frame IDs → correct
        StackTrace
  - [x] **Red**: Write test — parse STACK_TRACE with id_size=4
  - [x] **Red**: Write test — parse STACK_TRACE with 0 frame IDs → empty Vec
  - [x] **Red**: Write test — truncated STACK_TRACE (num_frames claims 5 but only 2 IDs
        present) → TruncatedRecord
  - [x] **Green**: Define `pub struct StackTrace { pub stack_trace_serial: u32, pub
        thread_serial: u32, pub frame_ids: Vec<u64> }`
  - [x] **Green**: Implement `pub fn parse_stack_trace(cursor: &mut Cursor<&[u8]>, id_size:
        u32) -> Result<StackTrace, HprofError>`
  - [x] **Refactor**: No `unwrap()`/`expect()` in production code
  - [x] Add `//!` module docstring

- [x] Update `crates/hprof-parser/src/lib.rs` (AC: all)
  - [x] Add `pub(crate) mod strings;` and `pub use strings::{HprofString, parse_string_record};`
  - [x] Add `pub(crate) mod types;` and `pub use types::{ClassDef, HprofThread, StackFrame,
        StackTrace, parse_load_class, parse_start_thread, parse_stack_frame, parse_stack_trace};`
  - [x] Update `//!` crate docstring to mention new modules

- [x] Add builder-based integration tests (feature-gated)
  - [x] `#[cfg(all(test, feature = "test-utils"))]` in `strings.rs`: build a file with
        `add_string`, parse past the file header, call `parse_string_record`
  - [x] `#[cfg(all(test, feature = "test-utils"))]` in `types.rs`: build a file with
        `add_class`, `add_thread`, `add_stack_frame`, `add_stack_trace`, parse each record in
        sequence using the builder bytes

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-parser`
  - [x] `cargo test -p hprof-parser --features test-utils`
  - [x] `cargo clippy -p hprof-parser -- -D warnings`
  - [x] `cargo fmt -- --check`

## Dev Notes

### Files to Create or Modify

```
crates/hprof-parser/
└── src/
    ├── lib.rs       ← add pub(crate) mod strings/types + pub use re-exports
    ├── strings.rs   ← NEW: HprofString, parse_string_record()
    └── types.rs     ← NEW: ClassDef, HprofThread, StackFrame, StackTrace + parsers
```

No new crate dependencies — `byteorder` is already in `[workspace.dependencies]` and
`[dependencies]` of `hprof-parser`.

[Source: docs/planning-artifacts/architecture.md#Project Structure]

### hprof Record Payloads for This Story

All parsers receive a `cursor` positioned at the start of the record payload (the 9-byte
record header has already been consumed by `parse_record_header`).

**STRING (tag 0x01)**

```
id:      id_size bytes BE  — string ID
content: remaining bytes   — raw UTF-8 (no length prefix; use payload_length - id_size)
```

The `parse_string_record` function requires `payload_length` to compute how many bytes
belong to the UTF-8 content: `content_len = payload_length - id_size`.

**LOAD_CLASS (tag 0x02)**

```
class_serial:          u32 BE
class_object_id:       id_size bytes BE
stack_trace_serial:    u32 BE
class_name_string_id:  id_size bytes BE
```

**START_THREAD (tag 0x06)**

```
thread_serial:              u32 BE
object_id:                  id_size bytes BE
stack_trace_serial:         u32 BE
name_string_id:             id_size bytes BE
group_name_string_id:       id_size bytes BE
group_parent_name_string_id: id_size bytes BE
```

**STACK_FRAME (tag 0x04)**

```
frame_id:              id_size bytes BE
method_name_string_id: id_size bytes BE
method_sig_string_id:  id_size bytes BE
source_file_string_id: id_size bytes BE
class_serial:          u32 BE
line_number:           i32 BE  (negative = unknown, 0 = compiled, >0 = source line)
```

**STACK_TRACE (tag 0x05)**

```
stack_trace_serial: u32 BE
thread_serial:      u32 BE
num_frames:         u32 BE
frame_ids:          [id_size bytes BE; num_frames]
```

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]

### Implementation Reference

**parse_string_record**

```rust
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Cursor, Read};
use crate::{HprofError, read_id};

pub fn parse_string_record(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
    payload_length: u32,
) -> Result<HprofString, HprofError> {
    let id = read_id(cursor, id_size)?;
    let content_len = (payload_length as usize).saturating_sub(id_size as usize);
    let mut content_bytes = vec![0u8; content_len];
    cursor
        .read_exact(&mut content_bytes)
        .map_err(|_| HprofError::TruncatedRecord)?;
    let value = String::from_utf8(content_bytes)
        .map_err(|e| HprofError::CorruptedData(format!("invalid UTF-8 in string: {e}")))?;
    Ok(HprofString { id, value })
}
```

**parse_load_class**

```rust
pub fn parse_load_class(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
) -> Result<ClassDef, HprofError> {
    let class_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let class_object_id = read_id(cursor, id_size)?;
    let stack_trace_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let class_name_string_id = read_id(cursor, id_size)?;
    Ok(ClassDef {
        class_serial,
        class_object_id,
        stack_trace_serial,
        class_name_string_id,
    })
}
```

**parse_stack_trace**

```rust
pub fn parse_stack_trace(
    cursor: &mut Cursor<&[u8]>,
    id_size: u32,
) -> Result<StackTrace, HprofError> {
    let stack_trace_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let thread_serial = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let num_frames = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let mut frame_ids = Vec::with_capacity(num_frames as usize);
    for _ in 0..num_frames {
        frame_ids.push(read_id(cursor, id_size)?);
    }
    Ok(StackTrace {
        stack_trace_serial,
        thread_serial,
        frame_ids,
    })
}
```

Follow the same pattern for `parse_start_thread` and `parse_stack_frame`.

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
[Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]

### Test Pattern

Use the same dual-gate pattern as Story 2.1:

```rust
// Manual byte tests — no builder
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use crate::id::read_id;

    #[test]
    fn parse_load_class_id_size_8() {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_be_bytes());       // class_serial
        data.extend_from_slice(&100u64.to_be_bytes());     // class_object_id (8 bytes)
        data.extend_from_slice(&0u32.to_be_bytes());       // stack_trace_serial
        data.extend_from_slice(&200u64.to_be_bytes());     // class_name_string_id (8 bytes)
        let mut cursor = Cursor::new(data.as_slice());
        let def = parse_load_class(&mut cursor, 8).unwrap();
        assert_eq!(def.class_serial, 1);
        assert_eq!(def.class_object_id, 100);
        assert_eq!(def.stack_trace_serial, 0);
        assert_eq!(def.class_name_string_id, 200);
    }
}

// Builder-based integration tests
#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;
    use crate::header::parse_header;
    use crate::record::{parse_record_header, skip_record};
    use std::io::Cursor;

    fn advance_past_header(bytes: &[u8]) -> usize {
        bytes.iter().position(|&b| b == 0).unwrap() + 1 + 4 + 8
    }

    #[test]
    fn round_trip_load_class() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_class(1, 100, 0, 200)
            .build();
        let hdr_end = advance_past_header(&bytes);
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x02);
        let def = parse_load_class(&mut cursor, 8).unwrap();
        assert_eq!(def.class_serial, 1);
        assert_eq!(def.class_object_id, 100);
        assert_eq!(def.class_name_string_id, 200);
    }
}
```

[Source: docs/implementation-artifacts/2-1-record-header-parsing-id-utility-and-unknown-record-skip.md#Test Pattern]
[Source: docs/planning-artifacts/architecture.md#Testing Patterns]

### Key Rules to Follow

- All ID reads go through `read_id(cursor, id_size)` — never hardcode 4 or 8 bytes.
- Map `io::Error` from `ReadBytesExt` reads to `HprofError::TruncatedRecord` via
  `.map_err(|_| HprofError::TruncatedRecord)`.
- Map `String::from_utf8` failure to `HprofError::CorruptedData`.
- No `unwrap()`/`expect()` in production code — use `?` or explicit `match`.
- Modules declared `pub(crate)` in `lib.rs`, types re-exported with `pub use`.
- Every module file needs a `//!` module docstring.
- All structs derive at minimum `Debug, Clone`.

[Source: docs/planning-artifacts/architecture.md#Error Propagation Patterns]
[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### Previous Story Intelligence (2.1)

From Story 2.1 completion notes and debug log:

- `read_id` is re-exported from the crate root as `pub use id::read_id` — import as
  `use crate::read_id` (or `use crate::id::read_id` from within the crate).
- `parse_record_header` and `skip_record` are also re-exported at crate root — builder
  integration tests import from `crate::record::parse_record_header`.
- Builder test `advance_past_header` helper: `bytes.iter().position(|&b| b == 0).unwrap()
  + 1 + 4 + 8` advances past the null-terminated version string + id_size(u32) +
  timestamp(u64). Do NOT assert `cursor.position() == rec.length` after `skip_record`;
  assert against the actual byte slice length (this was the bug fixed in 2.1's debug log).
- 54 tests currently pass (with test-utils feature). New tests add to this baseline.
- `HprofTestBuilder::add_thread` requires 6 arguments including
  `group_parent_name_string_id` — make sure `HprofThread` has this field.

[Source: docs/implementation-artifacts/2-1-record-header-parsing-id-utility-and-unknown-record-skip.md]

### Git Intelligence

Recent commits:
- `Story 2.1: record header parsing, ID utility and unknown record skip` — establishes
  `id.rs` and `record.rs` patterns; imports from `crate::` root work correctly.
- `Story 1.2/1.3: close code review follow-ups` — established `open_hprof_header` engine
  helper; cross-platform path test pattern.
- `Story 1.3: Hprof header parsing and mmap file access` — `header.rs` and `mmap.rs` as
  structural reference for module layout.

### Project Structure Notes

- `strings.rs` and `types.rs` go directly under `crates/hprof-parser/src/` — flat module
  design consistent with `error.rs`, `header.rs`, `id.rs`, `record.rs`.
- The `indexer/` sub-directory is for Story 2.3+; do not create it now.
- All public types and functions are re-exported from `lib.rs` so downstream crates
  (`hprof-engine`) import via `hprof_parser::HprofThread` not
  `hprof_parser::types::HprofThread`.

[Source: docs/planning-artifacts/architecture.md#Project Structure]
[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### References

- [Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Error Handling]
- [Source: docs/planning-artifacts/architecture.md#Error Propagation Patterns]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]
- [Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
- [Source: docs/planning-artifacts/epics.md#Story 2.2]
- [Source: docs/implementation-artifacts/2-1-record-header-parsing-id-utility-and-unknown-record-skip.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Implemented `strings.rs`: `HprofString` struct + `parse_string_record` (id_size 4 & 8, empty
  content, truncated payload, UTF-8 error path).
- Implemented `types.rs`: `ClassDef`, `HprofThread`, `StackFrame`, `StackTrace` + all four
  parsers. `parse_stack_frame` covers positive/zero/negative `line_number`. `parse_stack_trace`
  handles 0 frames and truncation mid-frame-list.
- Updated `lib.rs` crate docstring and re-exports for all new public types/functions.
- 81 tests pass (51 without test-utils, 81 with). Baseline was 54; +27 new tests.
- No `unwrap()`/`expect()` in production code. All errors mapped via `map_err`.

### File List

- `crates/hprof-parser/src/strings.rs` (new)
- `crates/hprof-parser/src/types.rs` (new)
- `crates/hprof-parser/src/lib.rs` (modified)
- `docs/implementation-artifacts/2-2-structural-record-parsing.md` (modified)
- `docs/implementation-artifacts/sprint-status.yaml` (modified)

## Senior Developer Review (AI)

**Review Date:** 2026-03-06
**Outcome:** Changes Requested (all fixed in same session)
**Action Items:** 5 total — 0 remaining

### Action Items

- [x] [Med] `parse_stack_trace`: `Vec::with_capacity(num_frames as usize)` — DoS/crash on
  malformed file with large `num_frames`, violates NFR6. Fixed: replaced with `Vec::new()`.
  [types.rs:180]
- [x] [Low] `parse_string_record`: `CorruptedData` (invalid UTF-8) path untested. Fixed: added
  `parse_string_invalid_utf8_returns_corrupted_data` test. [strings.rs]
- [x] [Low] `parse_stack_frame_line_number_variants`: two scenarios in one test. Fixed: split
  into `parse_stack_frame_line_number_zero` and `parse_stack_frame_line_number_negative`.
  [types.rs]
- [x] [Low] `parse_stack_frame_id_size_4`: missing assertions for `method_name_string_id`,
  `method_sig_string_id`, `source_file_string_id`. Fixed: added all three assertions. [types.rs]
- [x] [Low] `advance_past_header` duplicated in `strings.rs` and `types.rs` builder_tests.
  Fixed: moved to `test_utils.rs` with `#[cfg(test)]`, imported from there. [test_utils.rs]

### Follow-up Fixes (AI) — 2026-03-07

- [x] [High] `parse_string_record` now rejects impossible payload contracts where
  `payload_length < id_size` and returns `TruncatedRecord` before any ID read. Added
  `parse_string_payload_shorter_than_id_size_returns_truncated`. [strings.rs]
- [x] [Medium] `parse_stack_trace` now validates that `num_frames * id_size` fits in
  remaining payload bytes before reading frame IDs; otherwise returns `TruncatedRecord`.
  [types.rs]
