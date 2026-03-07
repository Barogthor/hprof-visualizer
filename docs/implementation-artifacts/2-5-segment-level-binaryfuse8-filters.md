# Story 2.5: Segment-Level BinaryFuse8 Filters

Status: done

## Story

As a developer,
I want the indexer to construct BinaryFuse8 filters per fixed-size segment (64 MB) during the
first pass, containing all object IDs found in each segment,
so that object resolution can quickly identify which segment contains a given object without
scanning the full file.

## Acceptance Criteria

1. **Given** the first pass is running over a file
   **When** heap dump segment records (0x0C or 0x1C) are encountered
   **Then** object IDs within each 64 MB file segment are collected and a BinaryFuse8 filter is
   built per segment using the `xorf` crate (FR5)

2. **Given** a completed index with segment filters
   **When** I query for an object ID that exists in segment 3
   **Then** the BinaryFuse8 filter for segment 3 returns `contains = true` and most other
   segments return `false` (with ~0.4% false positive rate)

3. **Given** a file smaller than 64 MB
   **When** indexed
   **Then** a single segment filter is created covering the entire file (segment index 0)

4. **Given** a truncated file where the last segment is incomplete
   **When** the indexer finishes
   **Then** a filter is still built for the partial last segment with whatever object IDs were
   successfully parsed

## Tasks / Subtasks

- [x] Add `xorf` to workspace and crate dependencies (AC: #1)
  - [x] **Red**: Confirm project builds with `xorf` in `Cargo.toml` and a `use xorf::BinaryFuse8`
        import compiles
  - [x] Add `xorf = "0.11"` (or latest stable — check crates.io) to
        `[workspace.dependencies]` in root `Cargo.toml`
  - [x] Add `xorf = { workspace = true }` to `[dependencies]` in
        `crates/hprof-parser/Cargo.toml`

- [x] Create `crates/hprof-parser/src/indexer/segment.rs` (AC: #1, #2, #3, #4)
  - [x] **Red**: Write test — `SegmentFilterBuilder::new().build()` returns empty `Vec`
  - [x] **Red**: Write test — `add(0, 42)` + `build()` → one filter,
        `filter.segment_index == 0`, `filter.contains(42) == true`
  - [x] **Red**: Write test — `add(SEGMENT_SIZE, 99)` + `build()` →
        `filter.segment_index == 1`, `filter.contains(99) == true`
  - [x] **Red**: Write test — two objects at segment-3 offsets + one at segment-0 offset
        → two filters, segment-3 filter contains both segment-3 IDs
  - [x] **Red**: Write test — segment-0 filter does not contain a segment-3 ID
        (guaranteed false negative; do not test probabilistic false positives)
  - [x] **Red**: Write test — duplicate IDs in same segment → dedup, one filter contains it
  - [x] **Green**: Define `pub(crate) const SEGMENT_SIZE: usize = 64 * 1024 * 1024`
  - [x] **Green**: Define `pub(crate) struct SegmentFilter { pub segment_index: usize, filter:
        xorf::BinaryFuse8 }` with `Debug` (derive or manual)
  - [x] **Green**: Implement `pub(crate) fn contains(&self, id: u64) -> bool` on
        `SegmentFilter` (delegates to `xorf::Filter::contains`)
  - [x] **Green**: Define `pub(crate) struct SegmentFilterBuilder { buckets:
        HashMap<usize, Vec<u64>> }`
  - [x] **Green**: Implement `pub(crate) fn new() -> Self` on `SegmentFilterBuilder`
  - [x] **Green**: Implement `pub(crate) fn add(&mut self, data_offset: usize, id: u64)` —
        computes `segment_index = data_offset / SEGMENT_SIZE`, appends to bucket
  - [x] **Green**: Implement `pub(crate) fn build(self) -> Vec<SegmentFilter>` — for each
        bucket: sort + dedup keys, call `BinaryFuse8::try_from(keys.as_slice())`, skip
        silently on construction failure, collect into `Vec<SegmentFilter>`
  - [x] Add `//!` module docstring

- [x] Update `crates/hprof-parser/src/indexer/mod.rs` (AC: #1, #3)
  - [x] **Red**: Write test — `IndexResult` has a `segment_filters: Vec<SegmentFilter>` field
        (compile check)
  - [x] **Green**: Add `pub(crate) mod segment;` declaration
  - [x] **Green**: Add `pub segment_filters: Vec<SegmentFilter>` field to `IndexResult`
  - [x] **Green**: Update `IndexResult` construction in its tests (`make_result` helper) to
        include `segment_filters: Vec::new()`
  - [x] Update `//!` module docstring to mention `segment`

- [x] Update `crates/hprof-parser/src/indexer/first_pass.rs` (AC: #1, #3, #4)
  - [x] **Red**: Write test — single `HEAP_DUMP_SEGMENT` (0x1C) with one `INSTANCE_DUMP`
        (0x21) sub-record → `result.segment_filters.len() == 1` and
        `result.segment_filters[0].contains(object_id) == true`
  - [x] **Red**: Write test — no heap dump records → `result.segment_filters` is empty
  - [x] **Red**: Write test — two `add_instance` calls with different object IDs in the same
        (small) file → one segment filter (segment 0) containing both IDs
  - [x] **Red**: Write test (builder): truncated `HEAP_DUMP_SEGMENT` mid-sub-record →
        partial filter still built with successfully parsed IDs
  - [x] **Red**: Write test — `HEAP_DUMP_SEGMENT` with `OBJECT_ARRAY_DUMP` (0x22) sub-record
        → filter contains the array ID
  - [x] **Red**: Write test — `HEAP_DUMP_SEGMENT` with `PRIMITIVE_ARRAY_DUMP` (0x23) →
        filter contains the array ID
  - [x] **Green**: Add `use crate::indexer::segment::{SegmentFilterBuilder, SEGMENT_SIZE};`
        (SEGMENT_SIZE used implicitly via `SegmentFilterBuilder`)
  - [x] **Green**: In `run_first_pass`, instantiate `SegmentFilterBuilder` before the loop
  - [x] **Green**: In the tag-skip branch, also detect `0x0C` and `0x1C` and call
        `extract_heap_object_ids` instead of skipping
  - [x] **Green**: Implement `fn extract_heap_object_ids(payload: &[u8], data_offset: usize,
        id_size: u32, builder: &mut SegmentFilterBuilder)` — see Dev Notes for full spec
  - [x] **Green**: After the main loop: `result.segment_filters = builder.build()`
  - [x] **Green**: Add `OBJECT_ARRAY_DUMP` (0x22) and `PRIMITIVE_ARRAY_DUMP` (0x23) support
        to `extract_heap_object_ids` (see Dev Notes)
  - [x] **Refactor**: `extract_heap_object_ids` must not use `unwrap()`; all read errors
        silently break the sub-record loop (tolerant, same philosophy as the outer loop)

- [x] Update `crates/hprof-parser/src/test_utils.rs` (AC: #1) — feature-gated
  - [x] **Red**: Write test — `add_object_array` produces a `HEAP_DUMP_SEGMENT` (0x1C) with
        0x22 sub-tag
  - [x] **Red**: Write test — `add_prim_array` produces a `HEAP_DUMP_SEGMENT` (0x1C) with
        0x23 sub-tag
  - [x] **Green**: Add `pub fn add_object_array(mut self, array_id: u64,
        stack_trace_serial: u32, element_class_id: u64, elements: &[u64]) -> Self` — creates
        `HEAP_DUMP_SEGMENT` record wrapping an `OBJECT_ARRAY_DUMP` sub-record
  - [x] **Green**: Add `pub fn add_prim_array(mut self, array_id: u64,
        stack_trace_serial: u32, num_elements: u32, element_type: u8, byte_data: &[u8]) -> Self` —
        creates `HEAP_DUMP_SEGMENT` record wrapping a `PRIMITIVE_ARRAY_DUMP` sub-record

- [x] Update `crates/hprof-parser/src/hprof_file.rs` (AC: #1, #3)
  - [x] **Red**: Write test — `from_path` with a file containing an `add_instance` call →
        `hfile.segment_filters.len() == 1`
  - [x] **Green**: Add `pub segment_filters: Vec<SegmentFilter>` to `HprofFile`
  - [x] **Green**: Update `from_path` to propagate `result.segment_filters` into `HprofFile`
  - [x] **Green**: Update `HprofFile` docstring to document the new field
  - [x] **Green**: Update `lib.rs` to re-export `SegmentFilter` from `indexer::segment`
        if needed for downstream crates (keep `pub(crate)` for now — engine is not built yet)

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-parser`
  - [x] `cargo test -p hprof-parser --features test-utils`
  - [x] `cargo clippy -p hprof-parser -- -D warnings`
  - [x] `cargo fmt -- --check`

## Dev Notes

### Dependency: `xorf` crate

Add to root `Cargo.toml`:
```toml
[workspace.dependencies]
xorf = "0.11"   # check crates.io for latest stable
```

Add to `crates/hprof-parser/Cargo.toml`:
```toml
[dependencies]
xorf = { workspace = true }
```

BinaryFuse8 API (xorf 0.11):
```rust
use xorf::{BinaryFuse8, Filter};

// Construction — keys must be unique (sort + dedup first)
let filter = BinaryFuse8::try_from(keys.as_slice())?;  // keys: &[u64]

// Query — returns true if key was in the construction set
// (~0.4% false positive rate for keys NOT in the set)
let present: bool = filter.contains(&key);  // key: u64, via Filter trait
```

`BinaryFuse8::try_from` returns `Result<Self, xorf::error::Error>`. It can fail on empty
input or if the internal construction graph fails. Always handle the error by skipping that
segment (do not `unwrap()`).

`xorf::BinaryFuse8` does not implement `Debug` in all versions — if derive fails, implement
it manually:
```rust
impl std::fmt::Debug for SegmentFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SegmentFilter {{ segment_index: {} }}", self.segment_index)
    }
}
```

### `segment.rs` — Full Design

```rust
// crates/hprof-parser/src/indexer/segment.rs
use std::collections::HashMap;
use xorf::{BinaryFuse8, Filter};

pub(crate) const SEGMENT_SIZE: usize = 64 * 1024 * 1024; // 64 MB

pub(crate) struct SegmentFilter {
    pub segment_index: usize,
    filter: BinaryFuse8,
}

impl SegmentFilter {
    pub(crate) fn contains(&self, id: u64) -> bool {
        self.filter.contains(&id)
    }
}

pub(crate) struct SegmentFilterBuilder {
    buckets: HashMap<usize, Vec<u64>>,
}

impl SegmentFilterBuilder {
    pub(crate) fn new() -> Self {
        Self { buckets: HashMap::new() }
    }

    pub(crate) fn add(&mut self, data_offset: usize, id: u64) {
        let seg = data_offset / SEGMENT_SIZE;
        self.buckets.entry(seg).or_default().push(id);
    }

    pub(crate) fn build(self) -> Vec<SegmentFilter> {
        let mut filters = Vec::new();
        for (segment_index, mut ids) in self.buckets {
            ids.sort_unstable();
            ids.dedup();
            if let Ok(filter) = BinaryFuse8::try_from(ids.as_slice()) {
                filters.push(SegmentFilter { segment_index, filter });
            }
        }
        filters
    }
}
```

### `extract_heap_object_ids` — Full Design

This function is called from `run_first_pass` when a `HEAP_DUMP` (0x0C) or
`HEAP_DUMP_SEGMENT` (0x1C) record is encountered. `payload` is the bounded slice
(`&data[payload_start..payload_end]`), `data_offset` is `payload_start` (offset within
the `data` slice passed to `run_first_pass`).

```rust
fn extract_heap_object_ids(
    payload: &[u8],
    data_offset: usize,
    id_size: u32,
    builder: &mut SegmentFilterBuilder,
) {
    use byteorder::ReadBytesExt;
    use std::io::Cursor;

    let mut cursor = Cursor::new(payload);

    loop {
        let sub_tag = match cursor.read_u8() {
            Ok(t) => t,
            Err(_) => break,
        };

        let sub_record_start = data_offset + cursor.position() as usize;

        let ok = match sub_tag {
            // GC root sub-records — fixed sizes, skip (IDs are roots, not heap definitions)
            0x01 => skip_n(&mut cursor, id_size as usize),           // GC_ROOT_UNKNOWN
            0x02 => skip_n(&mut cursor, 2 * id_size as usize),       // GC_ROOT_JNI_GLOBAL
            0x03 => skip_n(&mut cursor, id_size as usize + 8),       // GC_ROOT_JNI_LOCAL
            0x04 => skip_n(&mut cursor, id_size as usize + 8),       // GC_ROOT_JAVA_FRAME
            0x05 => skip_n(&mut cursor, id_size as usize + 4),       // GC_ROOT_NATIVE_STACK
            0x06 => skip_n(&mut cursor, id_size as usize),           // GC_ROOT_STICKY_CLASS
            0x07 => skip_n(&mut cursor, id_size as usize + 4),       // GC_ROOT_THREAD_BLOCK
            0x08 => skip_n(&mut cursor, id_size as usize),           // GC_ROOT_MONITOR_USED
            0x09 => skip_n(&mut cursor, id_size as usize + 8),       // GC_ROOT_THREAD_OBJ

            // CLASS_DUMP — variable size, must be parsed to skip correctly
            0x20 => skip_class_dump(&mut cursor, id_size),

            // INSTANCE_DUMP — extract object_id, skip rest
            0x21 => {
                let Ok(obj_id) = read_id(&mut cursor, id_size) else { break };
                builder.add(sub_record_start, obj_id);
                // skip: stack_trace_serial(4) + class_id(id_size) + num_bytes(4) + data
                let Ok(_) = cursor.read_u32::<BigEndian>() else { break };
                let Ok(_) = read_id(&mut cursor, id_size) else { break };
                let Ok(num_bytes) = cursor.read_u32::<BigEndian>() else { break };
                skip_n(&mut cursor, num_bytes as usize)
            }

            // OBJECT_ARRAY_DUMP — extract array_id, skip rest
            0x22 => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else { break };
                builder.add(sub_record_start, arr_id);
                // skip: stack_trace_serial(4) + num_elements(4) + element_class_id(id_size)
                //       + elements(num_elements * id_size)
                let Ok(_) = cursor.read_u32::<BigEndian>() else { break };
                let Ok(num_elements) = cursor.read_u32::<BigEndian>() else { break };
                let Ok(_) = read_id(&mut cursor, id_size) else { break };
                skip_n(&mut cursor, num_elements as usize * id_size as usize)
            }

            // PRIMITIVE_ARRAY_DUMP — extract array_id, skip rest
            0x23 => {
                let Ok(arr_id) = read_id(&mut cursor, id_size) else { break };
                builder.add(sub_record_start, arr_id);
                // skip: stack_trace_serial(4) + num_elements(4) + element_type(1)
                //       + data(num_elements * element_size(element_type))
                let Ok(_) = cursor.read_u32::<BigEndian>() else { break };
                let Ok(num_elements) = cursor.read_u32::<BigEndian>() else { break };
                let Ok(elem_type) = cursor.read_u8() else { break };
                let elem_size = primitive_element_size(elem_type);
                if elem_size == 0 { break; } // unknown type
                skip_n(&mut cursor, num_elements as usize * elem_size)
            }

            // Unknown sub-tag: cannot determine size, stop gracefully
            _ => break,
        };

        if !ok {
            break;
        }
    }
}
```

`skip_n` helper:
```rust
fn skip_n(cursor: &mut Cursor<&[u8]>, n: usize) -> bool {
    let pos = cursor.position() as usize;
    let new_pos = pos.saturating_add(n);
    if new_pos > cursor.get_ref().len() {
        return false;
    }
    cursor.set_position(new_pos as u64);
    true
}
```

`skip_class_dump` helper (complex but necessary to maintain correct cursor position):
```rust
fn skip_class_dump(cursor: &mut Cursor<&[u8]>, id_size: u32) -> bool {
    use byteorder::ReadBytesExt;

    // class_id(id_size) + stack_trace_serial(4) + super_class_id(id_size) +
    // classloader_id(id_size) + signers_id(id_size) + protection_domain_id(id_size) +
    // reserved1(id_size) + reserved2(id_size) + instance_size(4)
    // = 7 * id_size + 8 bytes
    if !skip_n(cursor, 7 * id_size as usize + 8) {
        return false;
    }

    // constant pool: count(u16) + [index(u16) + type(u8) + value(variable)]
    let Ok(cp_count) = cursor.read_u16::<BigEndian>() else { return false };
    for _ in 0..cp_count {
        if cursor.read_u16::<BigEndian>().is_err() { return false; } // cp index
        let Ok(elem_type) = cursor.read_u8() else { return false };
        let size = if elem_type == 2 {
            id_size as usize
        } else {
            let s = primitive_element_size(elem_type);
            if s == 0 { return false; }
            s
        };
        if !skip_n(cursor, size) { return false; }
    }

    // static fields: count(u16) + [name_string_id(id_size) + type(u8) + value(variable)]
    let Ok(sf_count) = cursor.read_u16::<BigEndian>() else { return false };
    for _ in 0..sf_count {
        if read_id(cursor, id_size).is_err() { return false; }
        let Ok(field_type) = cursor.read_u8() else { return false };
        let size = if field_type == 2 {
            id_size as usize
        } else {
            let s = primitive_element_size(field_type);
            if s == 0 { return false; }
            s
        };
        if !skip_n(cursor, size) { return false; }
    }

    // instance fields: count(u16) + [name_string_id(id_size) + type(u8)]
    let Ok(if_count) = cursor.read_u16::<BigEndian>() else { return false };
    skip_n(cursor, if_count as usize * (id_size as usize + 1))
}
```

`primitive_element_size` helper:
```rust
fn primitive_element_size(type_byte: u8) -> usize {
    match type_byte {
        4 => 1,  // boolean
        5 => 2,  // char
        6 => 4,  // float
        7 => 8,  // double
        8 => 1,  // byte
        9 => 2,  // short
        10 => 4, // int
        11 => 8, // long
        _ => 0,  // unknown
    }
}
```

### Updated `run_first_pass` Integration Point

In `run_first_pass`, the existing tag-skip branch currently reads:
```rust
if !matches!(header.tag, 0x01 | 0x02 | 0x04 | 0x05 | 0x06) {
    cursor.set_position(payload_end as u64);
    continue;
}
```

Replace with:
```rust
if !matches!(header.tag, 0x01 | 0x02 | 0x04 | 0x05 | 0x06 | 0x0C | 0x1C) {
    cursor.set_position(payload_end as u64);
    continue;
}

if matches!(header.tag, 0x0C | 0x1C) {
    extract_heap_object_ids(
        &data[payload_start..payload_end],
        payload_start,
        id_size,
        &mut seg_builder,
    );
    cursor.set_position(payload_end as u64);
    continue;
}
```

After the loop:
```rust
result.segment_filters = seg_builder.build();
```

### `test_utils.rs` — New Builder Methods

`add_object_array` sub-record layout:
- sub-tag: 0x22 (1 byte)
- array_id (id_size bytes)
- stack_trace_serial (u32)
- num_elements (u32)
- element_class_id (id_size bytes)
- elements: [id_size bytes each]

`add_prim_array` sub-record layout:
- sub-tag: 0x23 (1 byte)
- array_id (id_size bytes)
- stack_trace_serial (u32)
- num_elements derived from byte_data length and implied element size (pass raw bytes directly)
- element_type (u8)
- byte_data (raw bytes)

For `add_prim_array`, `num_elements` = `byte_data.len() / element_size` is unreliable in a
builder (element size depends on type). Simplest approach: the caller provides raw
`byte_data` and a separate `num_elements: u32` parameter:

```rust
pub fn add_prim_array(
    mut self,
    array_id: u64,
    stack_trace_serial: u32,
    num_elements: u32,
    element_type: u8,
    byte_data: &[u8],
) -> Self
```

Both methods wrap in a `HEAP_DUMP_SEGMENT` (0x1C) record (same pattern as `add_instance`).

### Updated `HprofFile`

```rust
pub struct HprofFile {
    _mmap: Mmap,
    pub header: HprofHeader,
    pub index: PreciseIndex,
    pub index_warnings: Vec<String>,
    pub records_attempted: u64,
    pub records_indexed: u64,
    /// Probabilistic per-segment filters for object ID resolution.
    ///
    /// Each [`SegmentFilter`] covers a 64 MB slice of the records section
    /// and allows fast candidate-segment lookup before a targeted scan.
    pub segment_filters: Vec<SegmentFilter>,
}
```

### `first_pass.rs` — Imports Update

Add these imports:
```rust
use crate::indexer::segment::SegmentFilterBuilder;
use crate::read_id;
use byteorder::{BigEndian, ReadBytesExt};
```

(`read_id` is already re-exported from `crate` root, so `use crate::read_id` works.)

### Scope Boundaries — What NOT to Build

| Concern | Story |
|---------|-------|
| Progress bar / ETA reporting | 2.6 |
| NavigationEngine trait | 3.1 |
| Object resolution via filters | 3.4+ |
| Surface filter stats in TUI | future |
| Public re-export of `SegmentFilter` | deferred until engine needs it |

### Key Rules to Follow

- All imports from crate root (`use crate::…`), never from internal submodule paths.
- `extract_heap_object_ids` is infallible — never return `Err`, break on read failure.
- No `unwrap()` / `expect()` in production code.
- BinaryFuse8 keys must be deduped before calling `try_from`.
- `SegmentFilterBuilder::add` uses data-relative offsets (offset within `data` in
  `run_first_pass`), not absolute file offsets. The resolver in Story 3.4+ must use the
  same convention.
- The `_mmap` field prefix must be preserved.
- All modules need a `//!` docstring.

### Previous Story Intelligence (2.4)

From Story 2.4 completion notes:
- The payload-window approach (`&data[payload_start..payload_end]`) is the established
  pattern for safe sub-slice parsing. Use the same pattern for heap dump sub-records.
- `run_first_pass` is infallible — this must remain true after the changes.
- `IndexResult` construction in tests uses a `make_result` helper — update it to include
  `segment_filters: Vec::new()`.
- The "preferred parse pattern" (parse into local var, check consumption) applies to
  the structural records (0x01-0x06) but is not required for `extract_heap_object_ids`
  since we don't insert into a typed index — we just extract IDs.
- 84 tests pass (no feature) / 117 (test-utils) after Story 2.4. Expect ~15–25 new tests
  for Story 2.5.

### Git Intelligence

- Established pattern: new sub-module → new file (`segment.rs`), declared in `mod.rs`.
- All helper functions (`skip_n`, `skip_class_dump`, `extract_heap_object_ids`,
  `primitive_element_size`) live in `first_pass.rs` since they are only used there.
  Do not extract to a separate file.
- Pattern for new struct with `pub(crate)` visibility: keep in the module file where it
  is defined, only split if it grows large.
- `segment.rs` is standalone (no cross-module helpers needed from other parser modules
  except `xorf` which is an external crate).

### Project Structure Notes

- `segment.rs` lives at `crates/hprof-parser/src/indexer/segment.rs`.
- `extract_heap_object_ids`, `skip_n`, `skip_class_dump`, `primitive_element_size` live
  in `crates/hprof-parser/src/indexer/first_pass.rs` (not a new file — YAGNI).
- `SegmentFilter` stays `pub(crate)` — `hprof-engine` does not exist yet (Story 3.1).
  When engine is built, `lib.rs` will re-export `SegmentFilter` with `pub use`.
- No changes to `crates/hprof-engine/`, `crates/hprof-tui/`, or `crates/hprof-cli/`.

### References

- [Source: docs/planning-artifacts/epics.md#Story 2.5]
- [Source: docs/planning-artifacts/architecture.md#Data Architecture]
- [Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
- [Source: docs/implementation-artifacts/2-4-tolerant-indexing.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- Fixed `loop { match read_u8() }` → `while let Ok(sub_tag) = cursor.read_u8()` to satisfy
  `clippy::while_let_loop`.
- Added `#[allow(dead_code)]` to `SegmentFilter::contains` (engine Story 3.4+ will use it).
- Added `#[allow(private_interfaces)]` to `HprofFile::segment_filters` (temporary until
  `SegmentFilter` is re-exported from `lib.rs` in Story 3.1).
- `add_prim_array` signature differs slightly from story draft: caller provides `num_elements`
  explicitly, consistent with Dev Notes clarification.

### Completion Notes List

- Created `segment.rs` with `SEGMENT_SIZE`, `SegmentFilter`, `SegmentFilterBuilder`; 6 unit
  tests (all passing).
- Updated `mod.rs`: added `segment` module, `segment_filters` field to `IndexResult`, updated
  `make_result` helper.
- Updated `first_pass.rs`: added `SegmentFilterBuilder` integration, `extract_heap_object_ids`
  with full sub-record parsing (GC roots, CLASS_DUMP, INSTANCE_DUMP, OBJECT_ARRAY_DUMP,
  PRIMITIVE_ARRAY_DUMP), helper functions `skip_n`, `skip_class_dump`, `primitive_element_size`.
  5 new builder tests.
- Updated `test_utils.rs`: added `add_object_array` and `add_prim_array` builder methods with
  2 new tests.
- Updated `hprof_file.rs`: added `segment_filters` field to `HprofFile` struct and `from_path`
  propagation. 1 new builder test.
- Final counts: 90 tests (no feature), 131 tests (test-utils). Clippy clean. Fmt clean.

### File List

- `Cargo.toml` — added `xorf = "0.11"` to `[workspace.dependencies]`
- `Cargo.lock` — updated with xorf 0.11.0 and transitive deps
- `crates/hprof-parser/Cargo.toml` — added `xorf = { workspace = true }` to `[dependencies]`
- `crates/hprof-parser/src/indexer/segment.rs` — new file
- `crates/hprof-parser/src/indexer/mod.rs` — added `segment` module + `segment_filters` field
- `crates/hprof-parser/src/indexer/first_pass.rs` — heap dump extraction + segment builder
- `crates/hprof-parser/src/test_utils.rs` — `add_object_array`, `add_prim_array`
- `crates/hprof-parser/src/hprof_file.rs` — `segment_filters` field on `HprofFile`
- `docs/implementation-artifacts/sprint-status.yaml` — status updated to `review`
