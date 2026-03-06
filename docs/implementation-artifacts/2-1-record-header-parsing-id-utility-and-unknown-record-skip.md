# Story 2.1: Record Header Parsing, ID Utility & Unknown Record Skip

Status: done

## Story

As a developer,
I want the system to parse hprof record headers (tag + timestamp + length), provide a
dynamic `read_id()` utility for 4/8-byte IDs, and skip unknown record types gracefully,
so that the parser infrastructure is in place for all subsequent record-level parsing.

## Acceptance Criteria

1. **Given** a byte slice containing an hprof record header (tag + timestamp + length)
   **When** the parser reads the record header
   **Then** it correctly identifies the record type tag, extracts the payload length,
   and advances the cursor past the 9-byte header

2. **Given** all ID reads in the parser
   **When** any ID field is read
   **Then** it goes through `read_id(cursor, id_size) -> u64` — never hardcoded to 4
   or 8 bytes

3. **Given** a record with an unknown tag byte
   **When** the parser encounters it
   **Then** it skips the entire record payload using the length field and continues
   without error (FR7)

4. **Given** a record header where the length field exceeds remaining bytes
   **When** the parser attempts to skip the record
   **Then** it returns `HprofError::TruncatedRecord` instead of panicking

## Tasks / Subtasks

- [x] Create `crates/hprof-parser/src/id.rs` (AC: #2)
  - [x] **Red**: Write test — `read_id` with `id_size=4` reads 4 big-endian bytes into `u64`
  - [x] **Red**: Write test — `read_id` with `id_size=8` reads 8 big-endian bytes into `u64`
  - [x] **Red**: Write test — insufficient bytes returns `HprofError::TruncatedRecord`
  - [x] **Red**: Write test — unsupported `id_size` (e.g. 2) returns `HprofError::CorruptedData`
  - [x] **Green**: Implement `pub fn read_id(cursor: &mut Cursor<&[u8]>, id_size: u32) -> Result<u64, HprofError>`
  - [x] **Refactor**: Ensure no `unwrap()` / `expect()` in production code

- [x] Create `crates/hprof-parser/src/record.rs` (AC: #1, #3, #4)
  - [x] **Red**: Write test — parses valid 9-byte record header → correct `tag` and `length`
  - [x] **Red**: Write test — truncated header (< 9 bytes) returns `HprofError::TruncatedRecord`
  - [x] **Green**: Define `RecordHeader { pub tag: u8, pub length: u32 }` with `Debug, Clone, Copy`
  - [x] **Green**: Implement `pub fn parse_record_header(cursor: &mut Cursor<&[u8]>) -> Result<RecordHeader, HprofError>`
  - [x] **Red**: Write test — `skip_record` on known-length payload advances cursor correctly
  - [x] **Red**: Write test — `skip_record` where `length > remaining` returns `TruncatedRecord`
  - [x] **Red**: Write test — unknown tag byte (e.g. `0xFF`) + valid length → `skip_record` succeeds (AC: #3)
  - [x] **Green**: Implement `pub fn skip_record(cursor: &mut Cursor<&[u8]>, header: &RecordHeader) -> Result<(), HprofError>`
  - [x] Add `//!` module docstring
  - [x] Add `#[cfg(all(test, feature = "test-utils"))]` builder-based integration test
        (builder bytes → cursor → parse_record_header → skip_record)

- [x] Update `crates/hprof-parser/src/lib.rs` (AC: #1, #2, #3, #4)
  - [x] Add `pub(crate) mod id;` and `pub use id::read_id;`
  - [x] Add `pub(crate) mod record;` and `pub use record::{RecordHeader, parse_record_header, skip_record};`

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
    ├── lib.rs      ← add pub(crate) mod id, record + pub use re-exports
    ├── id.rs       ← NEW: read_id() utility
    └── record.rs   ← NEW: RecordHeader, parse_record_header(), skip_record()
```

No new crate dependencies required — `byteorder` is already in `[workspace.dependencies]`
and in `[dependencies]` of `hprof-parser`.

[Source: docs/planning-artifacts/architecture.md#Project Structure]

### hprof Record Header Format

Every record in an hprof file starts with a 9-byte header:

```
tag:         u8         (1 byte)  — identifies the record type
time_offset: u32 BE    (4 bytes) — milliseconds since dump start (ignored, not stored)
length:      u32 BE    (4 bytes) — byte length of payload that follows the header
```

Total header: 9 bytes. Payload follows immediately.

Known tag values (others must be skipped gracefully per FR7):

| Tag  | Record type        |
|------|--------------------|
| 0x01 | STRING_IN_UTF8     |
| 0x02 | LOAD_CLASS         |
| 0x04 | STACK_FRAME        |
| 0x05 | STACK_TRACE        |
| 0x06 | START_THREAD       |
| 0x1C | HEAP_DUMP_SEGMENT  |

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]

### `read_id` Implementation

```rust
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;
use crate::HprofError;

pub fn read_id(cursor: &mut Cursor<&[u8]>, id_size: u32) -> Result<u64, HprofError> {
    match id_size {
        4 => cursor
            .read_u32::<BigEndian>()
            .map(|v| v as u64)
            .map_err(|_| HprofError::TruncatedRecord),
        8 => cursor
            .read_u64::<BigEndian>()
            .map_err(|_| HprofError::TruncatedRecord),
        _ => Err(HprofError::CorruptedData(format!(
            "invalid id_size: {id_size}"
        ))),
    }
}
```

Key rules:
- Never hardcode 4 or 8 — always route through this utility.
- Map `io::Error` from `ReadBytesExt` to `HprofError::TruncatedRecord` (EOF = truncated).
- Return `CorruptedData` for any `id_size` that is not 4 or 8.

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
[Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]

### `parse_record_header` Implementation

```rust
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;
use crate::HprofError;

pub fn parse_record_header(cursor: &mut Cursor<&[u8]>) -> Result<RecordHeader, HprofError> {
    let tag = cursor
        .read_u8()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let _time_offset = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let length = cursor
        .read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    Ok(RecordHeader { tag, length })
}
```

`time_offset` is read and discarded — the hprof format requires it to be consumed to
advance the cursor, but it carries no information needed for parsing or indexing.

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
[Source: docs/planning-artifacts/architecture.md#Mmap Lifetime Rule]

### `skip_record` Implementation

```rust
pub fn skip_record(
    cursor: &mut Cursor<&[u8]>,
    header: &RecordHeader,
) -> Result<(), HprofError> {
    let pos = cursor.position() as usize;
    let remaining = cursor.get_ref().len().saturating_sub(pos);
    if remaining < header.length as usize {
        return Err(HprofError::TruncatedRecord);
    }
    cursor.set_position(pos as u64 + header.length as u64);
    Ok(())
}
```

`skip_record` is used for both unknown records and any record the caller chooses not to
parse. The caller is responsible for deciding whether to call it after
`parse_record_header` based on the `tag`.

[Source: docs/planning-artifacts/architecture.md#Error Handling]

### TDD Cycle for This Story

Follow Red → Green → Refactor strictly:

1. **`id.rs`**: Write all tests first (4-byte, 8-byte, truncated, invalid id_size).
   Implement `read_id`. All tests must pass.

2. **`record.rs`**: Write test for truncated header → implement `parse_record_header` to
   pass. Write test for skip valid payload → implement `skip_record`. Write test for
   length exceeds remaining → add boundary check in `skip_record`. Write builder-based
   integration test (feature-gated).

3. **`lib.rs`**: Add re-exports, run full test suite to confirm no regressions.

### Test Pattern: `#[cfg(test)]` vs `#[cfg(all(test, feature = "test-utils"))]`

Use plain `#[cfg(test)]` for all tests that craft bytes manually. Only use the double
gate for tests that require `HprofTestBuilder`:

```rust
// Plain manual byte tests — no builder needed
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_valid_record_header() {
        // tag=0x01, time_offset=0, length=12
        let mut data = vec![0x01u8];
        data.extend_from_slice(&0u32.to_be_bytes());  // time_offset
        data.extend_from_slice(&12u32.to_be_bytes()); // length
        let mut cursor = Cursor::new(data.as_slice());
        let header = parse_record_header(&mut cursor).unwrap();
        assert_eq!(header.tag, 0x01);
        assert_eq!(header.length, 12);
        assert_eq!(cursor.position(), 9); // cursor advanced 9 bytes
    }
}

// Builder-based tests — require HprofTestBuilder
#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;
    use crate::header::parse_header;
    use std::io::Cursor;

    #[test]
    fn round_trip_string_record_header_and_skip() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();
        let header = parse_header(&bytes).unwrap();
        // Advance past the file header
        let hdr_end = bytes.iter().position(|&b| b == 0).unwrap() + 1 + 4 + 8;
        let mut cursor = Cursor::new(&bytes[hdr_end..]);
        let rec = parse_record_header(&mut cursor).unwrap();
        assert_eq!(rec.tag, 0x01);
        // Payload = id(8) + "main"(4) = 12 bytes
        assert_eq!(rec.length, 8 + 4);
        skip_record(&mut cursor, &rec).unwrap();
        // Cursor is at end — no more records
        assert_eq!(
            cursor.position(),
            rec.length as u64
        );
    }
}
```

[Source: docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md#Test Pattern]
[Source: docs/planning-artifacts/architecture.md#Testing Patterns]

### Error Propagation Rules

- `unwrap()` and `expect()` forbidden in non-test code.
- `.map_err(|_| HprofError::TruncatedRecord)` for EOF from `ReadBytesExt` — the
  `io::Error` detail is not needed.
- `.map_err(|_| HprofError::TruncatedRecord)` for skip boundary checks.
- `CorruptedData` for invalid `id_size` (caller-provided value outside 4/8).

[Source: docs/planning-artifacts/architecture.md#Error Propagation Patterns]

### Previous Story Intelligence (1.3)

From story 1.3 completion notes:

- `byteorder = "1"` is in `[workspace.dependencies]` and `[dependencies]` of
  `hprof-parser` — no new dependency needed.
- `Cursor` is transient — created within the function, never stored. Parse into owned
  types before returning.
- Modules are declared `pub(crate)` in `lib.rs` and types are re-exported with `pub use`.
  Follow exactly: `pub(crate) mod id;` + `pub use id::read_id;`.
- `HprofError::UnknownRecordType { tag: u8 }` already exists in `error.rs` — it is a
  non-fatal variant. Story 2.1 does NOT return this error on unknown tags; it silently
  skips them via `skip_record`. `UnknownRecordType` will be used by the indexer in Story
  2.3 when it wants to surface warnings.
- Feature-gated builder tests: `#[cfg(all(test, feature = "test-utils"))]`. Import as
  `use crate::test_utils::HprofTestBuilder`.
- `id_size` validation (4 or 8) was added to `parse_header` in story 1.3 code review —
  by the time `read_id` is called, `id_size` is already validated, but `read_id` must
  still handle bad values defensively.

[Source: docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md]
[Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md]

### Git Intelligence

Recent commits (for context on patterns used):
- `Story 1.2/1.3: close code review follow-ups` — adds engine helper
  `open_hprof_header`, CLI FR1 flow, cross-platform path test
- `Story 1.3: Hprof header parsing and mmap file access` — establishes `header.rs`
  and `mmap.rs` patterns followed exactly in this story
- `Story 1.2: HprofError enum and HprofTestBuilder` — establishes `error.rs` and
  `test_utils.rs` patterns

### Project Structure Notes

- `id.rs` and `record.rs` go directly under `crates/hprof-parser/src/` — flat module
  design consistent with `error.rs`, `header.rs`, `mmap.rs`, `test_utils.rs`.
- No sub-directories needed for this story; the `indexer/` sub-directory is for Stories
  2.3–2.5.
- `RecordHeader`, `parse_record_header`, `skip_record`, and `read_id` are re-exported
  from the crate root so future engine/indexer code imports from `hprof_parser::*`.

[Source: docs/planning-artifacts/architecture.md#Project Structure]
[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### References

- [Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Error Handling]
- [Source: docs/planning-artifacts/architecture.md#Error Propagation Patterns]
- [Source: docs/planning-artifacts/architecture.md#Project Structure — hprof-parser files]
- [Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]
- [Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
- [Source: docs/planning-artifacts/epics.md#Story 2.1]
- [Source: docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md]
- [Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- Builder test `round_trip_string_record_header_and_skip`: spec asserted
  `cursor.position() == rec.length` (12) but correct value is 21 (9-byte header +
  12-byte payload). Fixed assertion to `cursor.position() == cursor.get_ref().len()`.

### Completion Notes List

- `id.rs`: `read_id` implemented per spec. 4 unit tests cover 4-byte, 8-byte, truncated,
  and unsupported id_size paths. No `unwrap`/`expect` in production code.
- `record.rs`: `RecordHeader`, `parse_record_header`, `skip_record` implemented per spec.
  9 unit tests + 2 builder-gated integration tests. Unknown tag 0xFF skipped without
  error (AC #3). Truncated record (length > remaining) returns `TruncatedRecord` (AC #4).
- `lib.rs`: `pub(crate) mod id/record` + `pub use` re-exports added.
- All 54 tests pass (with test-utils feature). Clippy and fmt clean.

### File List

- `crates/hprof-parser/src/id.rs` — new
- `crates/hprof-parser/src/record.rs` — new
- `crates/hprof-parser/src/lib.rs` — updated
- `docs/implementation-artifacts/sprint-status.yaml` — updated (in-progress → review)
- `docs/implementation-artifacts/2-1-record-header-parsing-id-utility-and-unknown-record-skip.md` — updated
