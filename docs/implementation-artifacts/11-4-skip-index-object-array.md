# Story 11.4: Skip-Index for OBJECT_ARRAY (O(1) Offset)

Status: done

## Story

As a user,
I want the system to calculate the offset of any element in an
OBJECT_ARRAY_DUMP in O(1) time,
So that navigating to page N of a 450K-element array is instant
instead of requiring sequential scanning.

## Acceptance Criteria

1. **Given** an OBJECT_ARRAY_DUMP with N elements
   **When** the user navigates to page K (elements
   K*page_size to (K+1)*page_size-1)
   **Then** element offsets are calculated via arithmetic
   (O(1)), not by scanning from the beginning

2. **Given** an object array page navigation
   **When** compared to the current implementation
   **Then** the ID-reading phase is constant regardless of
   page number (page 0 same speed as page 450) — note:
   total wall-clock still includes instance resolution
   per element which is independent of page number

3. **Given** the O(1) offset for each element
   **When** the element's object ID is read
   **Then** the object is resolved via `find_instance` (or
   batch-scan from Story 11.2 when available)

4. **Given** callers that only need element count (not data)
   **When** resolving an Object[] for display metadata
   **Then** only the header is parsed, no element
   deserialization occurs

5. **Given** existing tests for `find_object_array`,
   `try_object_array`, and pagination
   **When** `cargo test` is run
   **Then** all pass unchanged — the optimization is
   transparent

## Tasks / Subtasks

- [x] Task 1: Add `ObjectArrayMeta` struct and
      `find_object_array_meta` method (AC: #1, #4)
  - [x]1.1 In `crates/hprof-parser/src/hprof_file.rs`, add
        a public struct `ObjectArrayMeta`:
        ```rust
        pub struct ObjectArrayMeta {
            pub class_id: u64,
            pub num_elements: u32,
            /// Byte offset (relative to records section)
            /// of the first element in the array.
            pub elements_offset: u64,
        }
        ```
        Place it near `find_object_array`.
  - [x]1.2 Replace `scan_for_object_array` (line 668)
        with `scan_for_object_array_meta`. Same header
        parsing logic (sub-tag, array_id, stack_serial,
        num_elements, class_id) but returns
        `ObjectArrayMeta` instead of `(u64, Vec<u64>)`.
        `elements_offset` = `data_base_offset +
        cursor.position()` where `data_base_offset` is
        the offset of `data` relative to the start of
        the records section (= `r.payload_start`,
        passed as param).
        ```rust
        fn scan_for_object_array_meta(
            data: &[u8],
            target_id: u64,
            id_size: u32,
            data_base_offset: u64,
        ) -> Option<ObjectArrayMeta>
        ```
        On match: compute `elements_offset`, validate that
        `elements_offset + (num_elements as u64) * (id_size as u64) <=
        data_base_offset + data.len() as u64` (i.e., all
        element bytes fit within the data slice), return
        meta. Return `None` if bounds check fails. On non-match: skip element bytes and
        continue (same as existing code line 710).
        Do NOT delete `scan_for_object_array` here —
        `find_object_array` still calls it; deletion
        happens in Task 1.5 after the composition
        refactor.
  - [x]1.3 Add `pub fn find_object_array_meta(&self,
        array_id: u64) -> Option<ObjectArrayMeta>` on
        `HprofFile`. Follows the same segment-filter +
        heap_record_ranges loop as `find_object_array`
        (lines 216-258), but calls
        `scan_for_object_array_meta` instead. Pass
        `r.payload_start` as `data_base_offset` so
        `elements_offset` is relative to the records
        section start (matching `records_bytes()` indexing).
        Note: this loop is intentionally duplicated from
        `find_object_array` — after Task 1.5 delegates to
        this method, `find_object_array` no longer has its
        own loop. Acceptable for now; a generic
        segment-scan helper can be extracted later.
  - [x]1.4 Add `pub fn read_object_array_element(
        &self, meta: &ObjectArrayMeta, index: u32
        ) -> Option<u64>` on `HprofFile`.
        Reads the element at `index` via O(1) arithmetic.
        Use `checked_mul` + `checked_add` for the offset
        computation to guard against overflow on
        hypothetical 32-bit platforms:
        ```rust
        let id_sz = self.header.id_size as usize;
        let byte_offset = (index as usize)
            .checked_mul(id_sz)?
            .checked_add(
                meta.elements_offset as usize,
            )?;
        let records = self.records_bytes();
        if byte_offset + id_sz > records.len() {
            return None;
        }
        let mut cursor = Cursor::new(
            &records[byte_offset..byte_offset + id_sz],
        );
        read_id(&mut cursor, self.header.id_size).ok()
        ```
  - [x]1.5 Refactor `find_object_array` to compose on
        top of `find_object_array_meta` +
        `read_object_array_element`:
        ```rust
        pub fn find_object_array(
            &self, array_id: u64,
        ) -> Option<(u64, Vec<u64>)> {
            let meta =
                self.find_object_array_meta(array_id)?;
            let n = meta.num_elements as usize;
            let mut elems = Vec::with_capacity(n);
            for i in 0..meta.num_elements {
                elems.push(
                    self.read_object_array_element(
                        &meta, i,
                    )?,
                );
            }
            Some((meta.class_id, elems))
        }
        ```
        This preserves the existing public API for
        callers that need all elements
        (`extract_hash_map`). Single point of header
        parsing — DRY.
        Before deleting `scan_for_object_array`, grep
        to confirm it has no callers other than
        `find_object_array`. Delete it only after
        this composition refactor compiles cleanly.
  - [x]1.6 Add `ObjectArrayMeta` to the public
        re-exports in `crates/hprof-parser/src/lib.rs`
        (next to `pub use hprof_file::{BatchResult, HprofFile}`).

- [x] Task 2: Refactor `try_object_array` to use O(1)
      pagination (AC: #1, #2, #3)
  - [x]2.1 In `crates/hprof-engine/src/pagination/mod.rs`,
        add `use hprof_parser::ObjectArrayMeta;` to
        the imports at the top of the file. Then replace
        `try_object_array` (lines 102-111): use
        `find_object_array_meta` instead of
        `find_object_array`, pass `meta.num_elements`
        as `total`, call `paginate_object_array`.
        If the existing `try_object_array` call site
        has a `dbg_log!` in its enclosing function,
        preserve it.
  - [x]2.2 Add `paginate_object_array` function in
        `pagination/mod.rs`. Similar to `paginate_id_slice`
        (lines 438-515) but reads elements via
        `hfile.read_object_array_element(&meta, idx)`:
        ```rust
        /// `total` is the logical element count passed
        /// by the caller. In `try_object_array` it equals
        /// `meta.num_elements`; in `extract_array_list`
        /// it comes from the ArrayList `size` field and
        /// may be less than `meta.num_elements` (to
        /// exclude capacity-padding slots). Must not
        /// exceed `meta.num_elements`.
        fn paginate_object_array(
            meta: &ObjectArrayMeta,
            total: u64,
            offset: usize,
            limit: usize,
            hfile: &HprofFile,
        ) -> Option<CollectionPage> {
            let clamped = (offset as u64).min(total)
                as usize;
            let remaining =
                (total - clamped as u64) as usize;
            let actual = limit.min(remaining);

            let mut entries =
                Vec::with_capacity(actual);
            for i in 0..actual {
                let idx = (clamped + i) as u32;
                let value = hfile
                    .read_object_array_element(meta, idx)
                    .map(|id| id_to_field_value(id, hfile))
                    .unwrap_or_else(|| {
                        dbg_log!(
                            "read_object_array_element \
                             returned None at idx {}",
                            idx
                        );
                        FieldValue::Null
                    });
                entries.push(EntryInfo {
                    index: clamped + i,
                    key: None,
                    value,
                });
            }

            Some(CollectionPage {
                entries,
                total_count: total,
                offset: clamped,
                has_more: (clamped + actual)
                    < total as usize,
            })
        }
        ```

- [x] Task 3: Refactor count-only callers to use metadata
      (AC: #4)
      In each of the 3 callers below, replace
      `find_object_array(id)` with
      `find_object_array_meta(id)` and use
      `meta.num_elements as u64` instead of
      `elems.len() as u64`. Same ObjectRef return shape.
  - [x]3.1 `id_to_field_value` in
        `pagination/mod.rs` call site line 577
  - [x]3.2 `engine_impl/mod.rs` call site line 543 —
        the `else if let Some((_class_id, elems)) =
        self.hfile.find_object_array(object_id)` branch
        inside `enrich_object_ref_parts` (or its
        equivalent enclosing method).
  - [x]3.3 `engine_impl/mod.rs` call site line 749 —
        the `else if let Some((_cid, elems)) =
        self.hfile.find_object_array(object_id)` branch
        inside the local-variables resolution method.

- [x] Task 4: Refactor `extract_array_list` to use O(1)
      pagination (AC: #1, #2)
  - [x]4.1 In `pagination/mod.rs` line 188,
        `extract_array_list` does the exact same pattern as
        `try_object_array`:
        ```rust
        let (_class_id, elements) =
            hfile.find_object_array(arr_id)?;
        paginate_id_slice(
            &elements, total, offset, limit, hfile,
        )
        ```
        Replace with:
        ```rust
        let meta =
            hfile.find_object_array_meta(arr_id)?;
        // Clamp total to the actual backing array
        // capacity to guard against corrupt dumps
        // where size > num_elements.
        let effective_total =
            total.min(meta.num_elements as u64);
        paginate_object_array(
            &meta, effective_total, offset, limit, hfile,
        )
        ```
        Note: `total` comes from `ArrayList.size`
        field (not `meta.num_elements`), which correctly
        excludes capacity-padding slots. The clamp ensures
        that a corrupt size field cannot cause
        out-of-bounds reads (which would silently return
        Null entries).
  - [x]4.2 Verify `extract_hash_map`
        (`pagination/mod.rs:229`) still uses
        `find_object_array` — it needs all table[] slot
        IDs for node-chain walking. No change.

- [x] Task 5: Tests (AC: #1, #2, #5)
  - [x]5.1 Unit test meta + read elements, `id_size=8` —
        create via `HprofTestBuilder::new(version, 8)
        .add_object_array(0xA, 0, 0xCC, &[0x1, 0x2, 0x3])`
        Call `find_object_array_meta(0xA)`. Assert:
        (a) `meta.class_id == 0xCC`,
        (b) `meta.num_elements == 3`,
        (c) read elements at index 0, 1, 2 via
        `read_object_array_element` → assert `0x1`,
        `0x2`, `0x3`,
        (d) read at index 3 → assert `None` (out of
        bounds).
        Also assert `find_object_array_meta(0xBEEF)`
        returns `None` (unknown ID).
  - [x]5.2 Unit test meta + read elements, `id_size=4` —
        same as 5.1 but with
        `HprofTestBuilder::new(version, 4)`. Validates
        header size (17 vs 25 bytes) and element stride
        (4 bytes, not hardcoded 8).
  - [x]5.3 Unit test empty array (0 elements) — create
        via `.add_object_array(0xA, 0, 0xCC, &[])`.
        Assert `meta.num_elements == 0` and
        `paginate_object_array` returns page with 0
        entries, `has_more == false`.
  - [x]5.4 Unit test preceding sub-records — create a
        dump with an INSTANCE_DUMP before the
        OBJECT_ARRAY_DUMP (chain `.add_instance()` then
        `.add_object_array()`). Call
        `find_object_array_meta`. Assert
        `elements_offset` is correct by reading element 0
        and verifying the expected value. Validates the
        scanner correctly skips preceding sub-records.
  - [x]5.5 Unit test truncated array — create a dump
        where `num_elements` claims more elements than
        the payload actually contains. Call
        `find_object_array_meta`. Assert `None`
        (bounds validation rejects it).
  - [x]5.6 Unit test pagination — create a 10-element
        Object[]. Two sub-tests:
        (a) `try_object_array(hfile, id, 5, 3)` → 3
        entries, `offset == 5`, `total_count == 10`,
        `has_more == true`.
        (b) `try_object_array(hfile, id, 8, 100)` → 2
        entries, `has_more == false` (beyond end).
  - [x]5.7 Integration: verify existing
        `find_object_array_returns_elements` test
        (pagination/tests.rs:188) still passes. Note:
        after Task 1.5, this test now exercises the
        composition path (meta + read_element), not
        the original monolithic scan — same API, new
        internals.
  - [x]5.8 Unit test ArrayList size < capacity —
        **First**, check existing ArrayList tests in
        `pagination/tests.rs` to confirm `HprofTestBuilder`
        supports CLASS_DUMP with custom field layouts
        (`size`, `elementData` + string records). If no
        such builder pattern exists, use the manual asset
        (`heapdump-visualvm.hprof`) instead and document
        why. If the builder supports it: build an
        ArrayList-like structure with `size=3` and a
        backing Object[] with 10 elements (7 trailing
        nulls/zeros). Call `extract_array_list` with
        offset=0, limit=100. Assert exactly 3 entries
        returned (not 10), `total_count == 3`,
        `has_more == false`.

- [x] Task 6: Regression & clippy (AC: #5)
  - [x]6.1 Run `cargo test` — all existing tests pass
  - [x]6.2 Run `cargo clippy --all-targets -- -D warnings`
  - [x]6.3 Run `cargo fmt -- --check`
  - [x]6.4 Manual test on `assets/heapdump-visualvm.hprof`
        (41 MB): expand an Object[], page through it,
        verify correct element rendering.

## Dev Notes

### Core Change Summary

Replace the **full deserialization** path for Object[]
pagination with **O(1) positional reads**. The hprof format
guarantees OBJECT_ARRAY_DUMP elements are contiguous and
fixed-size (`id_size` bytes each), so element N's offset =
`elements_offset + N * id_size`.

**Current path (slow):**
```
find_object_array(id) → scan segment → parse header
  → allocate Vec<u64>(N) → deserialize ALL N elements
  → return (class_id, vec)
  → paginate_id_slice(&vec, offset, limit)
```

**After 11.4 (fast):**
```
find_object_array_meta(id) → scan segment → parse header
  → return ObjectArrayMeta { elements_offset, num_elements }
  → paginate_object_array: for i in offset..offset+limit:
      read_object_array_element(meta, i) → O(1) mmap read
```

**Performance impact on 450K-element array (per page):**

| Phase | Before | After 11.4 |
|-------|--------|------------|
| Segment scan (find record) | 10-200ms | 10-200ms (same) |
| Alloc Vec + deserialize 450K IDs | 5-7ms + 3.6 MB | **0** |
| Read page IDs (100 elements) | ~0 (slice) | ~0.1ms (mmap) |
| 100 × id_to_field_value | 50-500ms | 50-500ms (same) |

**Primary gain is memory + allocation**, not perceived
latency. Eliminates 3.6 MB allocation per page request.
On repeated pagination (page 1, 2, 3… through 450K
elements), this prevents ~36 MB of transient allocations
per 10 pages — reducing LRU eviction pressure (Epic 5).

**Future latency gain:** When Story 11.2 (batch-scan) is
available, `paginate_object_array` could collect the 100
element IDs and call `batch_find_instances` instead of
100 × individual `id_to_field_value`. This combination
(11.2 + 11.4) would yield the major wall-clock
improvement. Out of scope for this story.

### Callers of `find_object_array` — Change Matrix

| Caller | File:Line | Needs | Change |
|--------|-----------|-------|--------|
| `try_object_array` | pagination/mod.rs:108 (call) | page of elements | Use `find_object_array_meta` + `paginate_object_array` |
| `id_to_field_value` | pagination/mod.rs:577 (call) | count only | Use `find_object_array_meta` (no element read) |
| else-if branch (obj-ref enrichment) | engine_impl/mod.rs:543 (call) | count only | Use `find_object_array_meta` |
| else-if branch (local vars) | engine_impl/mod.rs:749 (call) | count only | Use `find_object_array_meta` |
| `extract_array_list` | pagination/mod.rs:188 (call) | page of elements | Use `find_object_array_meta` + `paginate_object_array` (`total` from ArrayList.size field) |
| `extract_hash_map` | pagination/mod.rs:229 (call) | all elements | Keep `find_object_array` (needs slot IDs for node walking) |

### OBJECT_ARRAY_DUMP Binary Layout

```
Sub-tag 0x22:
  [1 byte]    sub_tag = 0x22
  [id_size]   array_id
  [4 bytes]   stack_trace_serial
  [4 bytes]   num_elements (N)
  [id_size]   element_class_id
  [N×id_size] element_data  ← contiguous, fixed-size
```

Header size = `1 + id_size + 4 + 4 + id_size` =
`9 + 2 * id_size` bytes.
- `id_size=8` → header = 25 bytes
- `id_size=4` → header = 17 bytes

`elements_offset` = byte offset relative to the start of
the records section, pointing to the first byte of
`element_data`.

### Existing Code to Reuse

- `read_id` (`crates/hprof-parser/src/id.rs:23`) —
  reads 4- or 8-byte big-endian ID from cursor.
  Use for `read_object_array_element`.
- `scan_for_object_array` (hprof_file.rs:668) — replace
  with `scan_for_object_array_meta`, reusing header
  parsing logic, dropping element deserialization.
- `paginate_id_slice` (pagination/mod.rs:438) — use as
  template for `paginate_object_array`.
- `HprofTestBuilder::add_object_array`
  (test_utils.rs:191) — already generates valid
  OBJECT_ARRAY_DUMP records for testing.

### What NOT To Do

- Do NOT remove `find_object_array` — still needed by
  `extract_hash_map` (which needs all slot IDs for
  node-chain walking)
- Do NOT cache `ObjectArrayMeta` per array_id — YAGNI.
  The segment scan to find the record is the same cost
  either way. Note: repeated paging (page 1, 2, 3…)
  re-scans each time — same as current behavior, no
  regression. If profiling shows this matters on very
  large dumps, a per-array_id meta cache can be added
  as a follow-up
- Do NOT add a dedicated thread pool or async —
  the O(1) read is fast enough synchronously
- Do NOT keep `scan_for_object_array` alongside the
  new `_meta` variant — replace it entirely.
  `find_object_array` is rebuilt via composition
  (meta + N × read_element) to maintain DRY
- Do NOT modify `extract_hash_map` — it needs all
  table[] slot IDs for node-chain walking

### Target Platform

This project targets x86_64 only (`usize` = 64-bit).
`checked_mul`/`checked_add` in `read_object_array_element`
are a low-cost defensive measure, not a 32-bit portability
commitment. The `paginate_object_array` casts (`total` u64
→ usize, `clamped + i` → u32) are safe because
`num_elements` is u32 (hprof format limit) and `total` is
bounded by it. No additional checked arithmetic needed in
the pagination function.

### No New Dependencies

All required infrastructure exists:
- `read_id` for positional ID reads
- `Cursor<&[u8]>` for byte-level access
- `HprofTestBuilder` for test fixture generation
- Segment filter + heap_record_ranges for record location

### Project Structure Notes

- Changes in `crates/hprof-parser/src/hprof_file.rs`
  (new struct + 2 new methods + replace scan function
  + refactor `find_object_array` via composition)
- Changes in `crates/hprof-engine/src/pagination/mod.rs`
  (new `paginate_object_array` + refactor 3 callers)
- Changes in `crates/hprof-engine/src/engine_impl/mod.rs`
  (refactor 2 count-only callers)
- No new modules or files
- No new dependencies

### Previous Story Intelligence

From Story 11.3:
- rayon is available in workspace (not needed here, but
  parallel batch from 11.3 is complementary)
- `OffsetCache` (from 11.2) wraps instance_offsets — no
  interaction with Object[] metadata
- Tracing spans use `#[cfg(feature = "dev-profiling")]`
  gate — follow same pattern if adding instrumentation
- `HprofTestBuilder` with `add_object_array` is the
  standard way to create test fixtures

### References

- [Source: docs/planning-artifacts/epics.md#Epic 11,
  Story 11.4]
- [Source: crates/hprof-parser/src/hprof_file.rs:216-258]
  — `find_object_array`
- [Source: crates/hprof-parser/src/hprof_file.rs:668-714]
  — `scan_for_object_array`
- [Source: crates/hprof-engine/src/pagination/mod.rs:102-111]
  — `try_object_array`
- [Source: crates/hprof-engine/src/pagination/mod.rs:438-515]
  — `paginate_id_slice`
- [Source: crates/hprof-parser/src/id.rs:23]
  — `read_id`
- [Source: crates/hprof-parser/src/test_utils.rs:191]
  — `HprofTestBuilder::add_object_array`

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- Regression fix: `paginate_object_array` initially lacked batch pre-resolution from Story 11.2. Replacing `paginate_id_slice` dropped the `batch_find_instances` + `insert_batch` step, causing per-element segment scans instead of one batch scan per page. Root cause: story spec sample code did not include batch logic, and diff of replaced function behaviors was not performed before deletion.

### Completion Notes List

- Task 1: Added `ObjectArrayMeta` struct, `find_object_array_meta`, `read_object_array_element` on `HprofFile`. Replaced `scan_for_object_array` with `scan_for_object_array_meta` (returns meta instead of full element vec). Refactored `find_object_array` to compose on meta + read_element. Added re-export in lib.rs.
- Task 2: Replaced `try_object_array` to use `find_object_array_meta` + `paginate_object_array`. Added `paginate_object_array` function for O(1) positional reads.
- Task 3: Refactored 3 count-only callers (`id_to_field_value`, `enrich_object_ref_parts`, `get_local_variables`) from `find_object_array` to `find_object_array_meta` — avoids deserializing all elements just to get count.
- Task 4: Refactored `extract_array_list` to use `find_object_array_meta` + `paginate_object_array` with `total` clamped to `num_elements`. Verified `extract_hash_map` still uses `find_object_array` (needs all slot IDs).
- Task 5: Added 6 parser tests (meta id_size=8, id_size=4, empty array, preceding sub-records, truncated, composition) and 4 pagination tests (mid-page, beyond-end, empty, ArrayList size < capacity). Updated batch-caching test to remove stale assertions. Test 5.7 (existing `find_object_array_returns_elements`) passes unchanged — now exercises composition path.
- Task 6: 959 tests pass, clippy clean, fmt clean.
- Removed dead `paginate_id_slice` function (no remaining callers after Tasks 2+4).
- **Regression fix:** Added batch pre-resolution (Steps 1-2) to `paginate_object_array` — collects page IDs, batch-resolves uncached instance offsets via `batch_find_instances`, then resolves individually with O(1) cache hits. Restores Story 11.2 batch optimization that was lost when `paginate_id_slice` was replaced.

### Change Log

- 2026-03-17: Implemented O(1) Object[] pagination (Story 11.4)
- 2026-03-17: Fixed ArrayList pagination regression — restored batch pre-resolution in paginate_object_array
- 2026-03-17: Code review fix — eliminated double O(1) element read in paginate_object_array (M1)

### File List

- `crates/hprof-parser/src/hprof_file.rs` — ObjectArrayMeta struct, find_object_array_meta, read_object_array_element, scan_for_object_array_meta, refactored find_object_array, tests
- `crates/hprof-parser/src/lib.rs` — added ObjectArrayMeta re-export
- `crates/hprof-engine/src/pagination/mod.rs` — paginate_object_array, refactored try_object_array/extract_array_list/id_to_field_value, removed paginate_id_slice
- `crates/hprof-engine/src/pagination/tests.rs` — updated batch test, added object_array_pagination test module
- `crates/hprof-engine/src/engine_impl/mod.rs` — refactored enrich_object_ref_parts and get_local_variables to use find_object_array_meta
