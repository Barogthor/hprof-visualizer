# Story 8.2: Lazy String References

Status: done

## Story

As a user,
I want the indexing phase to skip eagerly loading all 130K+ string
values,
so that the first pass is faster and uses less memory.

## Acceptance Criteria

### AC1: STRING records store only offset+length

**Given** the first pass encounters a STRING record (tag `0x01`)
**When** it is indexed
**Then** only `HprofStringRef { id, offset, len }` is stored — no
string content is allocated.

### AC2: On-demand string resolution via mmap

**Given** a component needs a string value (class name, method name,
field name)
**When** it calls `resolve_string(sref)` on the `HprofFile`
**Then** the string is resolved on-demand from the mmap data with
`from_utf8_lossy`.

### AC3: class_names_by_id stays eager

**Given** the first pass builds `class_names_by_id`
**When** it encounters `LOAD_CLASS` records
**Then** class names are resolved eagerly and stored as owned `String`
in `class_names_by_id` (not lazy).

### AC4: All existing tests pass

**Given** all existing tests (363+ tests)
**When** I run `cargo test`
**Then** all tests pass with identical string values to
pre-optimization behavior.

## Tasks / Subtasks

- [x] Task 1: Replace `HprofString` with `HprofStringRef` (AC: 1)
  - [x] 1.1: In `strings.rs`, rename `HprofString` to
    `HprofStringRef` and change fields:
    ```rust
    #[derive(Debug, Clone, Copy)]
    pub struct HprofStringRef {
        pub id: u64,
        pub offset: u64,
        pub len: u32,
    }
    ```
  - [x] 1.2: Replace `parse_string_record` with
    `parse_string_ref` that stores offset+len instead of
    allocating content:
    ```rust
    pub fn parse_string_ref(
        cursor: &mut Cursor<&[u8]>,
        id_size: u32,
        payload_length: u32,
        record_body_start: u64,
    ) -> Result<HprofStringRef, HprofError>
    ```
    `offset` = `record_body_start + id_size as u64` (points
    to content bytes after the ID).
    `len` = `payload_length - id_size`.
  - [x] 1.3: Update `strings.rs` unit tests to test
    `HprofStringRef` fields (offset, len) instead of `.value`

- [x] Task 2: Add `resolve_string` to `HprofFile` (AC: 2)
  - [x] 2.1: Add method to `HprofFile`:
    ```rust
    pub fn resolve_string(
        &self,
        sref: &HprofStringRef,
    ) -> String {
        let start = sref.offset as usize;
        let end = start + sref.len as usize;
        let bytes = &self._mmap[start..end];
        String::from_utf8_lossy(bytes).into_owned()
    }
    ```
    Note: `offset` is an **absolute file offset** into the
    mmap, NOT relative to `records_start`.
  - [x] 2.2: Add a unit test that constructs an `HprofFile`
    via `from_path` on a test fixture and verifies
    `resolve_string` returns the expected value for a known
    string ID

- [x] Task 3: Update `PreciseIndex` type (AC: 1)
  - [x] 3.1: In `precise.rs`, change field type:
    `pub strings: FxHashMap<u64, HprofStringRef>`
  - [x] 3.2: Update `with_capacity` — no change needed since
    the capacity formula is the same (keys are unchanged)
  - [x] 3.3: Fix all compilation errors from the type change

- [x] Task 4: Update first_pass STRING parsing (AC: 1, 3)
  - [x] 4.1: In `first_pass.rs`, at the `0x01` tag handler
    (current line ~214), call `parse_string_ref` instead of
    `parse_string_record`. Pass the absolute file offset of
    the record body start:
    ```rust
    let abs_offset = records_start_abs + cursor.position();
    ```
    Where `records_start_abs` is passed into `run_first_pass`
    or computed from context.
  - [x] 4.2: **CRITICAL** — The `record_body_start` offset
    passed to `parse_string_ref` must be the **absolute mmap
    offset** (not relative to records section), because
    `resolve_string` slices from `self._mmap[start..end]`.
    Compute as: `records_start + cursor.position()` where
    `records_start` is the byte offset of the records section
    in the file (i.e., `header.records_start`).
  - [x] 4.3: Pass `records_start` as a new parameter to
    `run_first_pass` (or compute from context). Currently
    `run_first_pass` receives `data: &[u8]` which is already
    sliced to `&mmap[records_start..]`. So the absolute
    offset = `records_start_abs + cursor.position()` where
    `records_start_abs` is a new `usize` param.

- [x] Task 5: Update LOAD_CLASS to resolve eagerly (AC: 3)
  - [x] 5.1: At `first_pass.rs` line ~261 (tag `0x02`
    handler), replace:
    ```rust
    .map(|s| s.value.replace('/', "."))
    ```
    with inline resolution from mmap data:
    ```rust
    .map(|sref| {
        let start = sref.offset as usize;
        let end = start + sref.len as usize;
        let raw = String::from_utf8_lossy(
            &data_with_abs_offset[start..end]
        );
        raw.replace('/', ".")
    })
    ```
    **IMPORTANT:** Since `run_first_pass` receives
    `data = &mmap[records_start..]` but `sref.offset` is
    absolute, you need access to the full mmap slice OR
    adjust the offset to be relative. **Recommended approach:**
    store offsets as **relative to records section start**
    and have `resolve_string` add `self.records_start` when
    slicing. This avoids passing the full mmap into
    `run_first_pass`.
  - [x] 5.2: Same fix at line ~286 (partial-parse branch of
    tag `0x02`)

- [x] Task 6: Update engine call sites (AC: 2, 4)
  - [x] 6.1: `resolver.rs:32` — change:
    ```rust
    .map(|s| s.value.clone())
    ```
    to:
    ```rust
    .map(|sref| hfile.resolve_string(sref))
    ```
    This requires passing `&HprofFile` (or a resolver trait)
    into `decode_fields`. Check current signature.
  - [x] 6.2: `engine_impl.rs:92` — `collection_entry_count`:
    ```rust
    .map(|s| s.value.as_str())
    ```
    → resolve string, then use `.as_str()` on the result
  - [x] 6.3: `engine_impl.rs:249` — `build_thread_cache`:
    ```rust
    .map(|s| s.value.clone())
    ```
    → `hfile.resolve_string(sref)`
  - [x] 6.4: `engine_impl.rs:417` — `resolve_name`:
    ```rust
    .map(|s| s.value.clone())
    ```
    → `self.hfile.resolve_string(sref)`

- [x] Task 7: Update `first_pass.rs` internal call site
  (AC: 2, 4)
  - [x] 7.1: Line ~896 in `extract_obj_refs`:
    ```rust
    .map(|s| s.value.as_str())
    ```
    → resolve from `data` slice using `sref.offset` and
    `sref.len` (same inline pattern as Task 5)

- [x] Task 8: Fix test assertions (AC: 4)
  - [x] 8.1: Update `strings.rs` tests — assertions on
    `HprofStringRef` fields (offset, len) instead of `.value`
  - [x] 8.2: Update `hprof_file.rs` tests — any test
    accessing `hfile.index.strings[&id].value` must go through
    `hfile.resolve_string(...)` instead
  - [x] 8.3: Update `first_pass.rs` tests — any assertion on
    `.strings.get(&id).unwrap().value` must resolve via mmap
    or inline
  - [x] 8.4: Run `cargo test` — all 363+ tests must pass
  - [x] 8.5: Run `cargo clippy` — no warnings
  - [x] 8.6: Run `cargo fmt -- --check` — clean

## Dev Notes

### Offset Strategy Decision: Relative vs Absolute

**Recommended: offsets relative to records section start.**

Rationale:
- `run_first_pass` receives `data: &[u8]` already sliced to
  `&mmap[records_start..]`. Cursor positions are relative to
  this slice.
- Storing **relative offsets** (relative to records_start)
  means `sref.offset = cursor.position() + id_size` inside
  `run_first_pass` — no extra parameter needed.
- `HprofFile::resolve_string` adds `self.records_start`:
  ```rust
  let start = self.records_start + sref.offset as usize;
  let end = start + sref.len as usize;
  ```
- For inline resolution inside `run_first_pass`, use
  `data[sref.offset as usize .. (sref.offset + sref.len) as usize]`
  directly since `data` is already the records slice.

This is simpler than passing `records_start` into
`run_first_pass` or storing absolute offsets.

### class_names_by_id: Why Stay Eager

Per party-mode analysis: `class_names_by_id` is the natural
cache for class/method names displayed in the TUI. It's
populated once during first pass from ~7K LOAD_CLASS records.
Making it lazy would add latency to every UI interaction for
negligible savings. No LRU cache needed — this was rejected as
over-engineering by Red Team analysis.

[Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Red Team]

### HprofStringRef: Why Not a Trait

A simple struct with `Copy` is the right abstraction. No trait
indirection needed — all consumers access the same mmap.
`Copy` enables zero-cost passing without reference lifetimes.

### Blast Radius Analysis

**Parser crate (hprof-parser):**
- `strings.rs` — struct rename + parse function rewrite
- `precise.rs` — type change on `strings` field
- `first_pass.rs` — 3 call sites (lines ~214, ~261/286, ~896)
- `hprof_file.rs` — add `resolve_string` method

**Engine crate (hprof-engine):**
- `resolver.rs:32` — 1 call site
- `engine_impl.rs` — 3 call sites (lines ~92, ~249, ~417)

**Test files:** ~15 assertions across strings.rs, first_pass.rs,
hprof_file.rs tests.

### Performance Expectations

| Metric | Before (8.1) | Expected (8.2) | Rationale |
|--------|-------------|----------------|-----------|
| First pass time (41 MB) | 42.6 ms | ~36-38 ms | Skip 135K string allocs |
| First pass time (1.4 GB) | 1.74 s | ~1.5 s | -10-15% |
| Peak RSS (1.4 GB dump) | ~430 MB | ~423 MB | -7 MB (135K strings) |

The gains are moderate because string allocation is not the
dominant bottleneck (that was `all_offsets`, fixed in 8.1).
The real value is reduced allocator pressure and GC-free
string content.

### Architecture Compliance

- **Crate boundary:** Primary changes in `hprof-parser`.
  Engine crate adapts call sites but does NOT depend on
  `rustc-hash` (unchanged from 8.1).
- **Dependency direction:**
  `hprof-cli → hprof-engine → hprof-parser` (unchanged).
- **No `println!`** — only tracing macros, gated behind
  `dev-profiling` feature flag.
- **Error propagation:** `resolve_string` is infallible
  (returns `String` via `from_utf8_lossy`). The mmap slice is
  guaranteed valid because offsets come from the same file.
- **`_mmap` field:** Currently prefixed with `_` because it
  was only kept alive for lifetime. After this story,
  `resolve_string` actively reads from it. **Rename `_mmap`
  to `mmap`** and remove the underscore prefix. Update the
  doc comment accordingly.

### Key Code Locations

| File | What Changes |
|------|-------------|
| `crates/hprof-parser/src/strings.rs` | `HprofString` → `HprofStringRef`, `parse_string_record` → `parse_string_ref` |
| `crates/hprof-parser/src/indexer/precise.rs` | `strings: FxHashMap<u64, HprofStringRef>` |
| `crates/hprof-parser/src/indexer/first_pass.rs` | Update tag 0x01 handler, inline resolve for 0x02 and `extract_obj_refs` |
| `crates/hprof-parser/src/hprof_file.rs` | Add `resolve_string`, rename `_mmap` → `mmap` |
| `crates/hprof-engine/src/resolver.rs` | Pass `&HprofFile` or use `resolve_string` |
| `crates/hprof-engine/src/engine_impl.rs` | 3 call sites adapted |

### Previous Story Intelligence (Story 8.1)

**Learnings from 8.1:**
- FxHashMap API is identical to HashMap for `.get()`,
  `.values()`, `.iter()` — engine needed zero code changes
  for the type swap. Same applies here: changing `HprofString`
  to `HprofStringRef` only breaks `.value` accesses.
- Pre-allocation caps were added in code review to prevent
  multi-GB reservations. Keep caps unchanged for `strings`
  map (500K max).
- `lookup_offset` helper is a private fn in `first_pass.rs` —
  similar pattern for any inline resolve helper.
- All `#[cfg(feature = "dev-profiling")]` tracing spans must
  be preserved — do NOT disturb them.
- 363 tests at end of Story 8.1 (baseline).

**Files modified in 8.1 that overlap with 8.2:**
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/strings.rs` (indirectly — type
  used but file not modified)

### Anti-Patterns to Avoid

- Do NOT add an LRU cache for resolved strings — rejected as
  over-engineering. OS page cache handles mmap access.
- Do NOT make `class_names_by_id` lazy — it's the UI cache.
- Do NOT change `HprofStringRef` to hold a `&str` or borrow
  from the mmap — owned resolution avoids lifetime complexity.
- Do NOT modify segment filter logic — that is Story 8.3.
- Do NOT disturb existing `#[cfg(feature = "dev-profiling")]`
  tracing spans.
- Do NOT change the `class_dumps`, `threads`, `stack_frames`,
  or `stack_traces` maps — only `strings` changes.
- Do NOT store absolute mmap offsets if using relative-to-
  records-start approach. Pick ONE strategy and be consistent.
- Do NOT break the `parse_string_record` public API without
  checking if benchmarks or external code depend on it. The
  benchmark in `benches/first_pass.rs` calls `run_first_pass`
  which calls the parser internally — it should work without
  changes.

### Testing Strategy

1. **All 363+ existing tests** must pass — `HprofStringRef` +
   `resolve_string` must produce identical string values.
2. **Updated unit tests** in `strings.rs` — verify
   `HprofStringRef` stores correct offset and len.
3. **Integration test** — `HprofFile::from_path` on test
   fixture, then `resolve_string` for known string IDs.
4. **Benchmark validation** (manual, if `HPROF_BENCH_FILE`
   set): Run `cargo bench --bench first_pass` and compare to
   8.1 baseline.

### Project Structure Notes

- No new files or modules needed.
- `HprofStringRef` replaces `HprofString` in the same file.
- The `parse_string_ref` function replaces
  `parse_string_record` in the same file.
- `resolve_string` is added to the existing `impl HprofFile`
  block.

### References

- [Source: docs/planning-artifacts/epics.md#Story 8.2]
- [Source: docs/planning-artifacts/architecture.md#strings.rs]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Story 8.2]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Red Team]
- [Source: docs/implementation-artifacts/8-1-fxhashmap-pre-allocation-and-all-offsets-optimization.md]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None

### Completion Notes List

- Replaced `HprofString { id, value }` with `HprofStringRef { id, offset, len }` (Copy type)
- `parse_string_record` → `parse_string_ref` — no longer allocates string content during first pass
- `HprofFile::resolve_string(&self, sref)` — on-demand string resolution via mmap + `from_utf8_lossy`
- Offsets stored relative to records section start (per Dev Notes recommendation)
- `_mmap` renamed to `mmap` (actively used now)
- `class_names_by_id` stays eager — inline resolution from `data` slice in first_pass.rs
- `decode_fields` and `collection_entry_count` now accept `records_bytes: &[u8]` parameter
- `extract_obj_refs` now accepts `records_data: &[u8]` parameter for field name resolution
- Engine call sites updated: `resolve_name`, `build_thread_cache`, `expand_object`, `resolve_string`
- All 364 tests pass, 0 clippy warnings, formatting clean

### File List

- `crates/hprof-parser/src/strings.rs` — `HprofString` → `HprofStringRef`, `parse_string_record` → `parse_string_ref`, updated tests
- `crates/hprof-parser/src/lib.rs` — updated re-exports
- `crates/hprof-parser/src/indexer/precise.rs` — `strings: FxHashMap<u64, HprofStringRef>`, updated tests
- `crates/hprof-parser/src/hprof_file.rs` — `_mmap` → `mmap`, added `resolve_string`, updated tests
- `crates/hprof-parser/src/indexer/first_pass.rs` — tag 0x01 uses `parse_string_ref`, tag 0x02 inline resolve, `extract_obj_refs` takes `records_data`, updated tests
- `crates/hprof-engine/src/resolver.rs` — `decode_fields` accepts `records_bytes`, updated tests
- `crates/hprof-engine/src/engine_impl.rs` — `resolve_name`, `build_thread_cache`, `collection_entry_count` use `resolve_string`/inline, updated tests
- `docs/implementation-artifacts/sprint-status.yaml` — status `in-progress` → `review`
- `docs/implementation-artifacts/8-2-lazy-string-references.md` — story file updates

### Action Items

- [ ] Investigate 5s freeze between progress bar 100% and TUI
  on RustRover dump (1.4 GB). Root cause: `build_thread_cache`
  → `resolve_thread_from_heap` falls back to `find_instance`
  (linear segment scan) when transitive object IDs are missing
  from `instance_offsets`. Add logging to count fallbacks and
  ensure `resolve_thread_transitive_offsets` covers all
  reachable objects (holder, name, value, char[]/byte[]).

### Change Log

- 2026-03-08: Story 8.2 implemented — lazy string references replacing eager string allocation
- 2026-03-08: Code review fixes (Claude + Codex) — centralized `HprofStringRef::resolve()`, bounds check, clippy strict
