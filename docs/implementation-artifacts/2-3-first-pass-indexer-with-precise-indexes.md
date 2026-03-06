# Story 2.3: First Pass Indexer with Precise Indexes

Status: done

## Story

As a developer,
I want the system to perform a single sequential mmap pass over the entire file, building
precise HashMap indexes for threads, stack frames, stack traces, class definitions, and
structural strings,
so that all structural metadata is available for instant O(1) lookup after indexing.

## Acceptance Criteria

1. **Given** a valid hprof file opened via mmap
   **When** the first pass indexer runs
   **Then** it reads every record sequentially from start to end, calling the record parser
   for each (FR4)

2. **Given** the first pass completes
   **When** I query the precise index
   **Then** all threads, stack frames, stack traces, class definitions, and structural
   strings are retrievable by their IDs in O(1) via HashMap (FR5, FR6)

3. **Given** a file with 4-byte IDs and a file with 8-byte IDs (separate test files)
   **When** each is indexed
   **Then** the indexer handles both correctly using the ID size from the header

## Tasks / Subtasks

- [x] Create `crates/hprof-parser/src/indexer/precise.rs` (AC: #2)
  - [x] **Red**: Write test — `PreciseIndex::new()` creates empty index (all HashMaps empty)
  - [x] **Red**: Write test — insert `HprofString` with id=5, retrieve by id=5 → correct
  - [x] **Red**: Write test — insert `ClassDef` with class_serial=1, retrieve by serial=1 → correct
  - [x] **Red**: Write test — insert `HprofThread` with thread_serial=2, retrieve by 2 → correct
  - [x] **Red**: Write test — insert `StackFrame` with frame_id=10, retrieve by 10 → correct
  - [x] **Red**: Write test — insert `StackTrace` with stack_trace_serial=3, retrieve by 3 → correct
  - [x] **Green**: Define `pub struct PreciseIndex` with five `pub` HashMap fields
  - [x] **Green**: Implement `PreciseIndex::new() -> Self`
  - [x] Add `//!` module docstring

- [x] Create `crates/hprof-parser/src/indexer/mod.rs`
  - [x] Declare `pub(crate) mod precise;` and `pub(crate) mod first_pass;`
  - [x] Add `//!` module docstring

- [x] Create `crates/hprof-parser/src/indexer/first_pass.rs` (AC: #1, #2, #3)
  - [x] **Red**: Write test — empty data slice → `Ok(PreciseIndex)` with all maps empty (AC #1)
  - [x] **Red**: Write test — single STRING record (id_size=8) → indexed in `strings`
  - [x] **Red**: Write test — single LOAD_CLASS (id_size=8) → indexed in `classes`
  - [x] **Red**: Write test — single START_THREAD (id_size=8) → indexed in `threads`
  - [x] **Red**: Write test — single STACK_FRAME (id_size=8) → indexed in `stack_frames`
  - [x] **Red**: Write test — single STACK_TRACE (id_size=8) → indexed in `stack_traces`
  - [x] **Red**: Write test — unknown record tag (0xFF) → skipped, index empty, no error
  - [x] **Red**: Write test — 3 STRING records → all 3 indexed
  - [x] **Red**: Write test — id_size=4 STRING + LOAD_CLASS → both indexed correctly (AC #3)
  - [x] **Green**: Implement `pub(crate) fn run_first_pass(data: &[u8], id_size: u32)
        -> Result<PreciseIndex, HprofError>`
  - [x] **Refactor**: No `unwrap()`/`expect()` in production code
  - [x] Add `//!` module docstring
  - [x] Builder-based integration tests (`#[cfg(all(test, feature = "test-utils"))]`):
    - [x] Build file with 2 threads, 1 class, 2 strings, 1 stack_trace, 1 stack_frame
          → call `run_first_pass` on records slice → verify all 7 entries indexed

- [x] Create `crates/hprof-parser/src/hprof_file.rs` (AC: #1, #2, #3)
  - [x] **Red**: Write test — `HprofFile::from_path` on non-existent path → `MmapFailed`
  - [x] **Red**: Write test — `HprofFile::from_path` on valid temp file → header fields correct
  - [x] **Red**: Write test (test-utils) — valid temp file with 1 STRING record →
        `from_path` succeeds, `index.strings` contains the string
  - [x] **Green**: Define `pub struct HprofFile` with private `mmap: Mmap`, `pub header:
        HprofHeader`, `pub index: PreciseIndex`
  - [x] **Green**: Implement `pub fn from_path(path: &Path) -> Result<Self, HprofError>`
  - [x] **Refactor**: No `unwrap()`/`expect()`; add `//!` module docstring

- [x] Update `crates/hprof-parser/src/lib.rs`
  - [x] Add `pub(crate) mod indexer;`
  - [x] Add `pub(crate) mod hprof_file;`
  - [x] Add `pub use hprof_file::HprofFile;`
  - [x] Add `pub use indexer::precise::PreciseIndex;`
  - [x] Update `//!` crate docstring to mention `HprofFile` and `PreciseIndex`

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
    ├── lib.rs           ← add mod indexer, mod hprof_file, pub use re-exports
    ├── hprof_file.rs    ← NEW: HprofFile struct + from_path()
    └── indexer/
        ├── mod.rs       ← NEW: indexer module root
        ├── precise.rs   ← NEW: PreciseIndex struct
        └── first_pass.rs ← NEW: run_first_pass()
```

No new crate dependencies — `memmap2`, `byteorder`, `thiserror` are already in
`[dependencies]`. `tempfile` is already in `[dev-dependencies]`.

[Source: docs/planning-artifacts/architecture.md#Project Structure]

### PreciseIndex Definition

```rust
// crates/hprof-parser/src/indexer/precise.rs
use std::collections::HashMap;
use crate::{ClassDef, HprofString, HprofThread, StackFrame, StackTrace};

pub struct PreciseIndex {
    pub strings: HashMap<u64, HprofString>,        // keyed by string ID
    pub classes: HashMap<u32, ClassDef>,           // keyed by class_serial
    pub threads: HashMap<u32, HprofThread>,        // keyed by thread_serial
    pub stack_frames: HashMap<u64, StackFrame>,    // keyed by frame_id
    pub stack_traces: HashMap<u32, StackTrace>,    // keyed by stack_trace_serial
}

impl PreciseIndex {
    pub fn new() -> Self {
        Self {
            strings: HashMap::new(),
            classes: HashMap::new(),
            threads: HashMap::new(),
            stack_frames: HashMap::new(),
            stack_traces: HashMap::new(),
        }
    }
}
```

[Source: docs/planning-artifacts/architecture.md#Data Architecture]

### run_first_pass Implementation Pattern

```rust
// crates/hprof-parser/src/indexer/first_pass.rs
use std::io::Cursor;
use crate::{
    HprofError,
    parse_record_header, skip_record,
    parse_string_record, parse_load_class, parse_start_thread,
    parse_stack_frame, parse_stack_trace,
};
use crate::indexer::precise::PreciseIndex;

pub(crate) fn run_first_pass(
    data: &[u8],
    id_size: u32,
) -> Result<PreciseIndex, HprofError> {
    let mut cursor = Cursor::new(data);
    let mut index = PreciseIndex::new();

    while (cursor.position() as usize) < data.len() {
        let header = parse_record_header(&mut cursor)?;
        match header.tag {
            0x01 => {
                let s = parse_string_record(&mut cursor, id_size, header.length)?;
                index.strings.insert(s.id, s);
            }
            0x02 => {
                let c = parse_load_class(&mut cursor, id_size)?;
                index.classes.insert(c.class_serial, c);
            }
            0x04 => {
                let f = parse_stack_frame(&mut cursor, id_size)?;
                index.stack_frames.insert(f.frame_id, f);
            }
            0x05 => {
                let t = parse_stack_trace(&mut cursor, id_size)?;
                index.stack_traces.insert(t.stack_trace_serial, t);
            }
            0x06 => {
                let t = parse_start_thread(&mut cursor, id_size)?;
                index.threads.insert(t.thread_serial, t);
            }
            _ => {
                skip_record(&mut cursor, &header)?;
            }
        }
    }

    Ok(index)
}
```

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]

### HprofFile Implementation Pattern

```rust
// crates/hprof-parser/src/hprof_file.rs
use std::path::Path;
use memmap2::Mmap;
use crate::{HprofError, HprofHeader, open_readonly, parse_header};
use crate::indexer::{first_pass::run_first_pass, precise::PreciseIndex};

pub struct HprofFile {
    _mmap: Mmap,                // private — kept alive; records slice borrows from it
    pub header: HprofHeader,
    pub index: PreciseIndex,
}

impl HprofFile {
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header_end(&mmap)?;
        let index = run_first_pass(&mmap[records_start..], header.id_size)?;
        Ok(Self { _mmap: mmap, header, index })
    }
}

/// Returns the byte offset of the first record in the mmap.
fn header_end(data: &[u8]) -> Result<usize, HprofError> {
    let null_pos = data
        .iter()
        .position(|&b| b == 0)
        .ok_or(HprofError::TruncatedRecord)?;
    Ok(null_pos + 1 + 4 + 8) // null-term + id_size(u32) + timestamp(u64)
}
```

**Critical:** `_mmap` must be prefixed with `_` to signal intentional non-use, but
the field MUST remain in the struct — it keeps the underlying file mapping alive.
Do NOT name it `mmap` (clippy dead_code warning), do NOT drop it.

[Source: docs/planning-artifacts/architecture.md#Mmap Lifetime Rule]

### Scope Boundaries — What NOT to Build

| Concern | Story |
|---------|-------|
| BinaryFuse8 / segment filters | 2.5 |
| Progress bar / ETA reporting | 2.6 |
| Truncated file tolerance | 2.4 |
| Object ID → mmap offset resolution | Story 3.4+ |

For 2.3, `run_first_pass` propagates `HprofError` on truncated records. Story 2.4
will wrap this with tolerance (collecting warnings and continuing).

Do NOT create `indexer/segment.rs` in this story.

[Source: docs/planning-artifacts/epics.md#Story 2.4, 2.5, 2.6]

### Test Pattern

Use the dual-gate pattern established in Stories 2.1 and 2.2:

```rust
// Manual byte tests — no builder (fast, no feature gate needed)
#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{BigEndian, WriteBytesExt};
    use std::io::{Cursor, Write};

    fn make_string_record(id: u64, content: &str, id_size: u32) -> Vec<u8> {
        let mut rec = Vec::new();
        rec.write_u8(0x01).unwrap();                      // tag
        rec.write_u32::<BigEndian>(0).unwrap();           // time_offset
        let payload_len = (id_size + content.len() as u32) as u32;
        rec.write_u32::<BigEndian>(payload_len).unwrap(); // length
        if id_size == 8 {
            rec.write_u64::<BigEndian>(id).unwrap();
        } else {
            rec.write_u32::<BigEndian>(id as u32).unwrap();
        }
        rec.extend_from_slice(content.as_bytes());
        rec
    }

    #[test]
    fn empty_data_returns_empty_index() {
        let index = run_first_pass(&[], 8).unwrap();
        assert!(index.strings.is_empty());
        assert!(index.classes.is_empty());
        assert!(index.threads.is_empty());
        assert!(index.stack_frames.is_empty());
        assert!(index.stack_traces.is_empty());
    }
}

// Builder-based integration tests
#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::{HprofTestBuilder, advance_past_header};

    #[test]
    fn full_index_round_trip() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .add_string(2, "java.lang.Thread")
            .add_class(10, 200, 0, 2)
            .add_thread(1, 300, 0, 1, 0, 0)
            .add_stack_frame(50, 1, 2, 1, 10, 42)
            .add_stack_trace(100, 1, &[50])
            .build();
        let start = advance_past_header(&bytes);
        let index = run_first_pass(&bytes[start..], 8).unwrap();
        assert_eq!(index.strings.len(), 2);
        assert_eq!(index.classes.len(), 1);
        assert_eq!(index.threads.len(), 1);
        assert_eq!(index.stack_frames.len(), 1);
        assert_eq!(index.stack_traces.len(), 1);
        assert_eq!(index.strings[&1].value, "main");
        assert_eq!(index.classes[&10].class_object_id, 200);
        assert_eq!(index.threads[&1].object_id, 300);
        assert_eq!(index.stack_frames[&50].line_number, 42);
        assert_eq!(index.stack_traces[&100].frame_ids, vec![50u64]);
    }
}
```

`advance_past_header` is already in `test_utils.rs` behind `#[cfg(test)]` — import it
as `use crate::test_utils::advance_past_header;` in builder_tests.

[Source: docs/implementation-artifacts/2-2-structural-record-parsing.md#Test Pattern]

### lib.rs Updates

```rust
pub(crate) mod indexer;
pub use indexer::precise::PreciseIndex;

pub(crate) mod hprof_file;
pub use hprof_file::HprofFile;
```

Update `//!` crate docstring to mention `HprofFile` (single entry point for engine
construction) and `PreciseIndex` (O(1) HashMap indexes for structural records).

[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### Key Rules to Follow

- All imports from crate root (`use crate::HprofString`, `use crate::parse_record_header`),
  never from internal submodule paths.
- `indexer` module declared `pub(crate)` in `lib.rs`; functions inside `first_pass.rs`
  declared `pub(crate)`. Only `PreciseIndex` and `HprofFile` are `pub`.
- `_mmap` field: prefix `_` prevents dead-code warning, but do NOT remove the field.
- No `unwrap()` / `expect()` in production code — use `?` or explicit `match`.
- All modules need a `//!` docstring.
- All public structs derive at minimum `Debug`.

[Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### Previous Story Intelligence (2.2)

From Story 2.2 completion and review:

- `parse_string_record` requires `payload_length: u32` as a third argument (needed to
  compute `content_len = payload_length - id_size`). The record header's `length` field
  is exactly that payload length — pass `header.length` directly.
- `parse_stack_trace` uses `Vec::new()` (not `Vec::with_capacity`) to prevent DoS on
  malformed `num_frames` values. Do not change this.
- Builder `advance_past_header` is now in `test_utils.rs` under `#[cfg(test)]` — not a
  local helper, import from `crate::test_utils::advance_past_header`.
- Current test count: 81 (51 without test-utils, 81 with). New tests will add to this.
- All types re-exported from crate root: `use crate::HprofString` works from any module.

[Source: docs/implementation-artifacts/2-2-structural-record-parsing.md]

### Git Intelligence

Recent commits show consistent patterns:
- `Story 2.2`: `strings.rs` + `types.rs` flat under `src/` with `pub(crate)` in lib.rs
- `Story 2.1`: `id.rs` + `record.rs` flat under `src/` — same pattern
- Story 2.3 is the first to use a subdirectory (`indexer/`). Follow Rust idiom:
  `indexer/mod.rs` as module root, submodules declared inside `mod.rs`.

### Project Structure Notes

- `indexer/` goes under `crates/hprof-parser/src/` — not at workspace root.
- `hprof_file.rs` goes directly under `crates/hprof-parser/src/` (flat, like `header.rs`).
- Story 2.5 will add `indexer/segment.rs` to the same `indexer/` directory. Do not
  pre-create it; do not add stubs.
- `hprof-engine` crate does not exist yet and will NOT be created in this story.
  Story 3.1 creates it.

[Source: docs/planning-artifacts/architecture.md#Project Structure]
[Source: docs/planning-artifacts/epics.md#Story 3.1]

### References

- [Source: docs/planning-artifacts/architecture.md#Data Architecture]
- [Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]
- [Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
- [Source: docs/planning-artifacts/architecture.md#Mmap Lifetime Rule]
- [Source: docs/planning-artifacts/epics.md#Story 2.3]
- [Source: docs/implementation-artifacts/2-2-structural-record-parsing.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Created `indexer/precise.rs`: `PreciseIndex` struct with 5 `HashMap` fields + `Default` impl.
  6 unit tests covering insert/retrieve for all record types.
- Created `indexer/mod.rs`: module root declaring `precise` and `first_pass`.
- Created `indexer/first_pass.rs`: `run_first_pass` parses all 5 structural record types and
  skips unknown tags. 9 manual-byte unit tests + 1 builder integration test (full round trip
  with 2 strings, 1 class, 1 thread, 1 frame, 1 trace).
- Created `hprof_file.rs`: `HprofFile::from_path` mmap + header parse + first pass in one call.
  3 tests (non-existent path, header fields, string indexed via test-utils).
- Updated `lib.rs`: added `indexer` and `hprof_file` modules + re-exports for `HprofFile` and
  `PreciseIndex`. Updated crate docstring.
- Total tests: 70 (no feature) → 102 (test-utils). Clippy clean, fmt clean.

### File List

- `crates/hprof-parser/src/indexer/mod.rs` (new)
- `crates/hprof-parser/src/indexer/precise.rs` (new)
- `crates/hprof-parser/src/indexer/first_pass.rs` (new)
- `crates/hprof-parser/src/hprof_file.rs` (new)
- `crates/hprof-parser/src/lib.rs` (modified)
- `.gitignore` (modified)
- `docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md` (modified)
- `docs/implementation-artifacts/sprint-status.yaml` (modified)

## Senior Developer Review (AI)

**Review Date:** 2026-03-07
**Outcome:** Changes Requested (all fixed in same session)
**Action Items:** 3 total — 0 remaining

### Action Items

- [x] [High] `run_first_pass` now constrains known-record parsing to the declared
  `header.length` payload window, preventing cross-record over-read on malformed length
  metadata. [indexer/first_pass.rs]
- [x] [High] known-record branches now require exact payload consumption
  (`consumed == header.length`), returning `CorruptedData` on mismatch so cursor
  alignment invariants are preserved. [indexer/first_pass.rs]
- [x] [Medium] added malformed-length regression coverage:
  `known_record_with_too_short_declared_length_returns_truncated`,
  `known_record_with_extra_payload_returns_corrupted_data`, and
  `string_record_declared_length_smaller_than_id_size_returns_truncated`.
  [indexer/first_pass.rs]
