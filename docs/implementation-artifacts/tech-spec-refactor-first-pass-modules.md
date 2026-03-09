---
title: 'Refactor first_pass.rs into focused modules'
slug: 'refactor-first-pass-modules'
created: '2026-03-09'
status: 'implementation-complete'
stepsCompleted: [1, 2, 3, 4]
tech_stack: [rust, rayon, byteorder, rustc-hash]
files_to_modify:
  - crates/hprof-parser/src/indexer/first_pass.rs -> first_pass/mod.rs
  - crates/hprof-parser/src/indexer/first_pass/record_scan.rs (new)
  - crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs (new)
  - crates/hprof-parser/src/indexer/first_pass/thread_resolution.rs (new)
  - crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs (new)
  - crates/hprof-parser/src/indexer/first_pass/tests.rs (new)
  - crates/hprof-parser/src/indexer/mod.rs (update module declaration)
code_patterns:
  - FxHashMap for all index collections
  - Cursor<&[u8]> + byteorder for binary parsing
  - pub(crate) for cross-module internal visibility
  - '#[cfg(test)]' for test-only code
  - '#[cfg(feature = "dev-profiling")]' tracing spans
test_patterns:
  - mod tests (37 tests) with hand-crafted binary builders
  - mod builder_tests (24 tests) feature-gated on test-utils
  - make_record / make_*_payload helpers build raw hprof bytes
  - make_*_sub_record helpers build heap segment sub-records
  - All tests call run_first_pass and assert on IndexResult fields
---

# Tech-Spec: Refactor first_pass.rs into focused modules

**Created:** 2026-03-09

## Overview

### Problem Statement

`first_pass.rs` is 2849 lines with a monolithic `run_first_pass`
function (539 L), massive duplication between sequential/parallel
heap extraction paths, a `(Ok, consumed)` pattern copy-pasted 5
times for record parsing, ~200 lines of `#[cfg(test)]`-only code
in the production file, and interleaved phases (record scan, heap
extraction, thread synthesis, transitive resolution) that make
the file hard to understand.

### Solution

Split `first_pass.rs` into thematic sub-modules with clearly
named phases, unify the sequential/parallel heap extraction logic,
factor out the repeated record parsing pattern, and move tests
into a dedicated file.

### Scope

**In Scope:**

- Convert `first_pass.rs` into a `first_pass/` directory module
- Introduce `FirstPassContext` struct to replace 13+ `&mut` params
- Split into sub-modules: `record_scan.rs`, `heap_extraction.rs`,
  `thread_resolution.rs`, `hprof_primitives.rs`
- `mod.rs` becomes a phase orchestrator (~100-150 L) calling
  named phases
- Unify `extract_heap_object_ids` /
  `extract_heap_segment_parallel` via `HeapSegmentResult`
  return type
- Factor out the `(Ok, consumed)` × 5 record parsing pattern
  into a helper
- Move tests (~1315 L) to `first_pass/tests.rs`
- Move `#[cfg(test)]` production code into test module

**Out of Scope:**

- Logic or behavior changes (minor cosmetic differences are
  acceptable: warning text wording, allocation patterns, and
  the fact that heap extraction now always runs post-scan
  instead of inline for small files — the extracted data is
  identical, only the timing within the pipeline changes)
- Performance optimizations
- Public API changes (`run_first_pass` signature, `IndexResult`)

## Context for Development

### Codebase Patterns

- Module docstrings use `//!`
- Max 100 chars per line
- KISS/YAGNI/DRY principles
- TDD — all tests must pass after refactoring

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `crates/hprof-parser/src/indexer/mod.rs` | Parent module, exports `first_pass`, defines `IndexResult` |
| `crates/hprof-parser/src/indexer/first_pass.rs` | Current monolith (2849 L) — target of this refactor |
| `crates/hprof-parser/src/indexer/precise.rs` | `PreciseIndex` struct used by first pass |
| `crates/hprof-parser/src/indexer/segment.rs` | `SegmentFilterBuilder` used by first pass |
| `crates/hprof-parser/benches/first_pass.rs` | Criterion benchmark — imports `run_first_pass`, must still compile |

### Technical Decisions (ADRs)

**ADR-1: `FirstPassContext` struct vs individual params →
Context struct.**
Carries all mutable state (`IndexResult`,
`SegmentFilterBuilder`, `all_offsets`, `raw_frame_roots`,
`raw_thread_objects`, `suppressed_warnings`, progress tracking).
Eliminates the 13-param signatures. Trade-off: slight
indirection vs massive clarity gain. Warning helpers
(`push_warning`, `push_suppressed_summary`) become methods on
`FirstPassContext` (`ctx.push_warning(msg)`) to avoid passing
`&mut warnings` + `&mut suppressed` separately. Introduced
early (Task 2) so all extracted modules receive
`&mut FirstPassContext` from the start — no throwaway
intermediate signatures.

**ADR-2: 4 sub-modules (scan, heap, thread, primitives) →
Sweet spot.**
Option B (2 modules) leaves `post_processing` too big (~400 L).
Option C (5+) is over-engineering. 4 modules gives 1
responsibility per file without being too granular.
`hprof_primitives.rs` contains low-level binary parsing
functions (`skip_n`, `primitive_element_size`,
`gc_root_skip_size`, `value_byte_size`, `parse_class_dump`)
plus cross-cutting utilities (`maybe_report_progress`,
`PROGRESS_REPORT_INTERVAL`, `PROGRESS_REPORT_MAX_INTERVAL`,
`MAX_WARNINGS`, `PARALLEL_THRESHOLD`). Functions used only by
`thread_resolution` (`read_raw_instance_at`,
`extract_obj_refs`, `lookup_offset`) stay in
`thread_resolution.rs`. Note: `extract_obj_refs` uses
`std::collections::HashSet` — include that import.

**ADR-3: Unified heap extraction via `HeapSegmentResult` →
Single return type.**
Sequential path calls `extract_heap_segment` then merges via
`merge_segment_result()`. Parallel path collects via `par_iter`
then merges. Trade-off: temporary Vec allocation in sequential
path is negligible vs disk I/O. Eliminates ~190 L of
duplication. `HeapSegmentResult` lives in `heap_extraction.rs`
with `pub(super)` visibility.

**ADR-4: Tests in `first_pass/tests.rs` (same module) →
Best access.**
Tests use `super::*` plus explicit sub-module imports for
internal access. Moving to `tests/` at crate level would force
`pub(crate)` on too many internals. A 1315 L test file is
acceptable — it's tests, not business logic. Both `mod tests`
(37 hand-crafted) and `mod builder_tests` (24, feature-gated
`#[cfg(all(test, feature = "test-utils"))]`) move here. When
nested under `#[cfg(test)] mod tests;` in `mod.rs`, the
`builder_tests` gate simplifies to
`#[cfg(feature = "test-utils")]` since the parent is already
test-gated.

**ADR-5: Record parse helper with closures (not macros) →
Type-safe.**
A function `parse_and_insert()` taking parse + insert closures.
Macros would be debug-hostile and IDE-unfriendly. The pattern is
stable and won't vary. Note: the 5 record types have slightly
different parse signatures — `StringInUtf8` passes
`body_start` and `header.length` while others just pass
`id_size`. The helper must accept a generic
`parse_fn: FnOnce(&mut Cursor<&[u8]>) -> Result<T, E>` that
each call site wraps with the appropriate captured args.

**Borrow checker constraint for `LoadClass`**: the insert
closure needs to read `index.strings` to resolve class names,
but also write to `index.class_names_by_id` and
`index.classes`. This is a partial borrow conflict if using
`&mut ctx`. Solution: the `parse_and_insert` helper should NOT
take `&mut FirstPassContext` — instead it takes separate
`&mut Vec<String>` (warnings) + `&mut u64` (suppressed) for
warning reporting. The insert closure captures the specific
`&mut` fields it needs. Alternatively, the `LoadClass` arm can
do the string lookup before calling the helper, passing the
resolved name as a captured value. The dev agent should choose
whichever approach satisfies the borrow checker.

Concrete signature:
```rust
fn parse_and_insert<T, E: std::fmt::Display>(
    tag: RecordTag,
    payload_start: usize,
    header_length: u32,
    payload_cursor: &mut Cursor<&[u8]>,
    warnings: &mut Vec<String>,
    suppressed: &mut u64,
    parse_fn: impl FnOnce(
        &mut Cursor<&[u8]>,
    ) -> Result<T, E>,
    insert_fn: impl FnOnce(T),
) -> bool
```

### Other Technical Notes

- **Phase orchestration in `mod.rs`**: `run_first_pass` calls
  `record_scan::scan_records()` →
  `heap_extraction::extract_all()` →
  `thread_resolution::resolve_all()` → `ctx.finish()`.
- **`#[cfg(test)]` functions** (`skip_heap_object`,
  `subdivide_segment`, `prepass_and_subdivide_segment`,
  `extract_class_dumps_only`, `SUB_DIVIDE_THRESHOLD`) move
  into `tests.rs`.
- **Visibility**: `parse_class_dump` and `value_byte_size`
  remain `pub(crate)` via re-export from `first_pass/mod.rs`.
- **`extract_heap_segment_parallel`** stays in prod code (not
  `#[cfg(test)]`) since it's the core parallel worker, tested
  directly.
- **`precise.rs` and `segment.rs`** are simple and stay as-is
  — not in scope.
- **Tracing spans** (`#[cfg(feature = "dev-profiling")]`): each
  phase keeps its own spans. `record_scan_span` in
  `record_scan.rs`, `parallel_heap_extraction` in
  `heap_extraction.rs`, `thread_cache_build` in
  `thread_resolution.rs`, `segment_filter_build` in `mod.rs`.
- **Benchmark** (`benches/first_pass.rs`): imports
  `hprof_parser::indexer::first_pass::run_first_pass`. Since
  the public API is unchanged, the benchmark compiles without
  modification. Verify with `cargo bench --no-run` after
  Task 1.

## Implementation Plan

### Tasks

- [x] Task 1: Convert `first_pass.rs` into a directory module
  - File: `first_pass.rs` →
    `first_pass/mod.rs`
  - Action: Create `first_pass/` directory, move
    `first_pass.rs` content into `first_pass/mod.rs`.
  - Action: No change needed in `indexer/mod.rs` —
    `pub mod first_pass` works for both file and directory
    modules.
  - Action: Verify `cargo bench --no-run` still compiles.
  - Notes: Pure file move. `cargo test` must pass with zero
    logic changes.

- [x] Task 2: Introduce `FirstPassContext` in `mod.rs`
  - File: `first_pass/mod.rs` (update)
  - Action: Create `struct FirstPassContext<'a>` with fields:
    - `data: &'a [u8]`
    - `id_size: u32`
    - `result: IndexResult`
    - `seg_builder: SegmentFilterBuilder`
    - `all_offsets: Vec<(u64, u64)>`
    - `raw_frame_roots: Vec<(u64, u32, i32)>`
    - `raw_thread_objects: Vec<(u64, u32)>`
    - `suppressed_warnings: u64`
    - `last_progress_bytes: usize`
    - `last_progress_at: Instant`
    - `defer_heap_extraction: bool` — **Note:** After Task 4,
      all extraction is post-scan via `extract_all`, so this
      flag is always `true` and becomes dead code. The dev
      agent may remove it as natural cleanup.
    - `cursor_position: u64` (tracks the main cursor
      position for final progress report)
  - Action: Add methods:
    - `fn new(data: &'a [u8], id_size: u32) -> Self`
    - `fn push_warning(&mut self, msg: String)`
    - `fn push_suppressed_summary(&mut self)`
    - `fn sort_offsets(&mut self)` (sort `all_offsets` by
      object ID — must be called before `resolve_all`
      which needs sorted offsets for binary search)
    - `fn finish(mut self) -> IndexResult` (build segment
      filters, append suppressed summary, return result
      — does NOT sort `all_offsets`, that's done earlier)
  - Action: Refactor `run_first_pass` to use
    `FirstPassContext` internally — replace all local
    variables with `ctx.field` access. The function body
    still contains all logic inline at this point; sub-module
    extraction happens in subsequent tasks.
  - Action: Move `push_warning` and `push_suppressed_summary`
    free functions into `FirstPassContext` methods. Delete
    the old free functions.
  - Notes: This is done early so that all subsequent
    extractions receive `&mut FirstPassContext` directly,
    avoiding throwaway intermediate signatures.
    `cargo test` must pass.

- [x] Task 3: Extract `hprof_primitives.rs`
  - File: `first_pass/hprof_primitives.rs` (new)
  - File: `first_pass/mod.rs` (update)
  - Action: Move these functions into `hprof_primitives.rs`:
    - `skip_n`
    - `primitive_element_size`
    - `gc_root_skip_size`
    - `value_byte_size` (keep `pub(crate)` via re-export)
    - `parse_class_dump` (keep `pub(crate)` via re-export)
    - `maybe_report_progress`
  - Action: Move these constants:
    - `PROGRESS_REPORT_INTERVAL`
    - `PROGRESS_REPORT_MAX_INTERVAL`
    - `MAX_WARNINGS`
    - `PARALLEL_THRESHOLD`
  - Action: Add `pub(super)` visibility on all items. Add
    `pub(crate)` on `parse_class_dump` and `value_byte_size`.
  - Action: In `mod.rs`, add `mod hprof_primitives;` and
    re-export
    `pub(crate) use hprof_primitives::{parse_class_dump, value_byte_size};`.
  - Action: Add module docstring:
    `//! Low-level hprof binary parsing primitives and cross-cutting utilities.`
  - Notes: Update all `use` paths in `mod.rs` to reference
    `hprof_primitives::*`. `cargo test` must pass.

- [x] Task 4: Extract `heap_extraction.rs` and unify
  sequential/parallel
  - File: `first_pass/heap_extraction.rs` (new)
  - File: `first_pass/mod.rs` (update)
  - Action: Move `HeapSegmentResult` struct into
    `heap_extraction.rs` with `pub(super)` visibility.
  - Action: Rename `extract_heap_segment_parallel` →
    `extract_heap_segment` (it's now the only extraction
    function). Also update any test call sites that
    reference the old name (tests are still inline in
    `mod.rs` at this point).
  - Action: Delete `extract_heap_object_ids` (the 13-param
    sequential version).
  - Action: Add `pub(super) fn merge_segment_result(ctx, segment_result)`
    that merges a `HeapSegmentResult` into
    `FirstPassContext` fields (all_offsets, seg_builder,
    raw_frame_roots, raw_thread_objects, class_dumps,
    warnings via `ctx.push_warning`).
  - Action: Add `pub(super) fn extract_all(ctx, progress_fn)`
    that orchestrates:
    - If parallel: `par_iter` over `heap_record_ranges` →
      `extract_heap_segment` → `merge_segment_result`
      per batch
    - If sequential fallback: loop over
      `heap_record_ranges` → `extract_heap_segment` →
      `merge_segment_result`
  - Action: In `mod.rs`, replace ALL heap extraction logic
    (both the inline `!defer_heap_extraction` path inside
    the scan loop AND the post-loop deferred extraction)
    with `heap_extraction::extract_all(&mut ctx, &mut progress_fn)`.
    The inline path during scan now just records ranges in
    `heap_record_ranges`; all extraction is deferred to
    `extract_all` after the scan loop.
  - Action: Add module docstring:
    `//! Heap segment object extraction — sequential and parallel paths.`
  - Notes: `extract_heap_segment` uses
    `hprof_primitives::{skip_n, primitive_element_size, gc_root_skip_size, parse_class_dump}`.
    Import via `use super::hprof_primitives::*`.
    `cargo test` must pass.

- [x] Task 5: Extract `thread_resolution.rs`
  - File: `first_pass/thread_resolution.rs` (new)
  - File: `first_pass/mod.rs` (update)
  - Action: Move these functions into
    `thread_resolution.rs`:
    - `resolve_thread_transitive_offsets`
    - `read_raw_instance_at`
    - `extract_obj_refs` (needs `std::collections::HashSet`
      import)
    - `lookup_offset`
  - Action: Add `pub(super) fn resolve_all(ctx: &mut FirstPassContext)`
    that encapsulates the post-loop logic:
    - Synthesise threads from `STACK_TRACE` records
    - Populate `thread_object_ids` from `raw_thread_objects`
    - Correlate `GC_ROOT_JAVA_FRAME` roots with stack traces
      → `java_frame_roots`
    - Cross-reference `thread_object_ids` with `all_offsets`
      → `instance_offsets` (uses `lookup_offset` which
      requires `all_offsets` to be sorted — ensured by
      `ctx.sort_offsets()` called before `resolve_all`)
    - Call `resolve_thread_transitive_offsets`
    - Drop `all_offsets` (via `ctx.all_offsets = Vec::new()`)
  - Action: In `mod.rs`, add `mod thread_resolution;` and
    replace inline post-loop logic with
    `thread_resolution::resolve_all(&mut ctx)`.
  - Action: Move tracing span `thread_cache_build` into
    `resolve_all`.
  - Action: Add module docstring:
    `//! Thread synthesis, GC root correlation, and transitive offset resolution.`
  - Notes: `resolve_all` uses
    `hprof_primitives::value_byte_size` (via
    `extract_obj_refs`). `cargo test` must pass.

- [x] Task 6: Extract `record_scan.rs` and factor the parse
  pattern
  - File: `first_pass/record_scan.rs` (new)
  - File: `first_pass/mod.rs` (update)
  - Action: Create a helper function per ADR-5 signature:
    ```rust
    fn parse_and_insert<T, E: std::fmt::Display>(
        tag: RecordTag,
        payload_start: usize,
        header_length: u32,
        payload_cursor: &mut Cursor<&[u8]>,
        warnings: &mut Vec<String>,
        suppressed: &mut u64,
        parse_fn: impl FnOnce(
            &mut Cursor<&[u8]>,
        ) -> Result<T, E>,
        insert_fn: impl FnOnce(T),
    ) -> bool
    ```
    Takes `&mut Vec<String>` + `&mut u64` for warnings
    instead of `&mut FirstPassContext` to avoid borrow
    conflicts (see ADR-5 re: `LoadClass` arm).
    Call sites pass `&mut ctx.result.warnings` and
    `&mut ctx.suppressed_warnings`, leaving other ctx
    fields available for the insert closure to capture.
    - On `(Ok(val), consumed)`: call `insert_fn(val)`,
      push warning if `!consumed`, return `true`
    - On `(Err(e), _)`: push warning, return `false`
  - Action: Move the `while cursor < data.len()` record
    scanning loop into
    `pub(super) fn scan_records(ctx: &mut FirstPassContext, progress_fn)`.
  - Action: Replace the 5 copy-pasted match arms with calls
    to the helper using appropriate closures.
  - Action: The heap segment handling stays in
    `scan_records` — it just records ranges in
    `ctx.result.heap_record_ranges`.
  - Action: In `mod.rs`, add `mod record_scan;` and replace
    the scanning loop with
    `record_scan::scan_records(&mut ctx, &mut progress_fn)`.
  - Action: Move tracing spans `first_pass` and
    `record_scan` into this module.
  - Action: Add module docstring:
    `//! Top-level record scanning loop with factored parse-and-insert dispatch.`
  - Action: Simplify `run_first_pass` in `mod.rs` to:
    ```rust
    pub fn run_first_pass(...) -> IndexResult {
        let mut ctx = FirstPassContext::new(data, id_size);
        record_scan::scan_records(
            &mut ctx, &mut progress_fn,
        );
        heap_extraction::extract_all(
            &mut ctx, &mut progress_fn,
        );
        ctx.sort_offsets();
        thread_resolution::resolve_all(&mut ctx);
        progress_fn(ctx.cursor_position);
        ctx.finish()
    }
    ```
  - Notes: `scan_records` needs access to `ctx.result.index`
    for inserts and `ctx.data` for string resolution.
    `scan_records` must set `ctx.cursor_position` to the
    final cursor position before returning.
    The `LoadClass` arm requires reading
    `ctx.result.index.strings` while also writing to
    `ctx.result.index.class_names_by_id` and
    `ctx.result.index.classes`. Since `parse_and_insert`
    takes separate `&mut` refs for warnings (not
    `&mut ctx`), the insert closure can borrow disjoint
    fields of `ctx.result.index`. Alternatively, resolve
    the class name before calling `parse_and_insert` and
    pass it as a captured `String`. The dev agent should
    choose whichever approach is cleanest.
    `cargo test` must pass.

- [x] Task 7: Move tests to `tests.rs`
  - File: `first_pass/tests.rs` (new)
  - File: `first_pass/mod.rs` (update)
  - Action: Move `mod tests` (37 tests + all helpers) from
    `mod.rs` into `tests.rs`.
  - Action: Move `mod builder_tests` (24 tests) into
    `tests.rs`. Simplify its gate from
    `#[cfg(all(test, feature = "test-utils"))]` to
    `#[cfg(feature = "test-utils")]` since the parent
    `tests.rs` is already behind `#[cfg(test)]`.
  - Action: Move all `#[cfg(test)]` production functions
    into `tests.rs`:
    - `skip_heap_object`
    - `extract_class_dumps_only`
    - `PrepassResult` struct
    - `prepass_and_subdivide_segment`
    - `subdivide_segment`
    - `SUB_DIVIDE_THRESHOLD` constant
  - Action: In `mod.rs`, add `#[cfg(test)] mod tests;`.
  - Action: In `tests.rs`, add `use super::*;` for `mod.rs`
    items, plus explicit sub-module imports for functions
    tested directly:
    `use super::heap_extraction::extract_heap_segment;`,
    `use super::hprof_primitives::{skip_n, gc_root_skip_size, parse_class_dump};`,
    etc.
  - Action: Add module docstring:
    `//! Tests for the first-pass indexing pipeline.`
  - Notes: Tests access internal functions via `super::*`
    and explicit sub-module imports. `cargo test` must pass.
    `cargo clippy` must pass (no dead code warnings from
    moved `#[cfg(test)]` items).

### Acceptance Criteria

- [x] AC-1: Given the refactored codebase, when running
  `cargo test`, then all 61 existing tests pass with
  identical results.
- [x] AC-2: Given the refactored codebase, when running
  `cargo clippy`, then no new warnings are introduced.
- [x] AC-3: Given the refactored codebase, when running
  `cargo fmt -- --check`, then formatting is clean.
- [x] AC-4: Given `first_pass/mod.rs`, when reading
  `run_first_pass`, then the function body is ≤30 lines
  and reads as a sequence of named phase calls.
- [x] AC-5: Given `first_pass/mod.rs`, when inspecting the
  public API, then `run_first_pass` is the only `pub`
  function and its signature is unchanged.
- [x] AC-6: Given `heap_extraction.rs`, when searching for
  `extract_heap_object_ids`, then it does not exist — only
  `extract_heap_segment` (unified) exists.
- [x] AC-7: Given `record_scan.rs`, when inspecting the 5
  record type match arms, then each arm is ≤10 lines
  using the parse helper.
- [x] AC-8: Given the production code (excluding `tests.rs`),
  when searching for `#[cfg(test)]`, then the only match
  is `#[cfg(test)] mod tests;` in `mod.rs`.
- [x] AC-9: Given `hprof_primitives.rs`, when checking
  exports, then `parse_class_dump` and `value_byte_size`
  are `pub(crate)` via re-export in `mod.rs`.
- [x] AC-10: Given the `first_pass/` directory, when listing
  files, then exactly 6 files exist: `mod.rs`,
  `hprof_primitives.rs`, `record_scan.rs`,
  `heap_extraction.rs`, `thread_resolution.rs`, `tests.rs`.
- [x] AC-11: Given `benches/first_pass.rs`, when running
  `cargo bench --no-run`, then the benchmark compiles
  without modification.

## Additional Context

### Dependencies

None — pure refactoring, no new crates.
`thread_resolution.rs` uses `std::collections::HashSet`
(already in std).

### Testing Strategy

- All 61 existing tests (37 `mod tests` + 24
  `mod builder_tests`) must pass unchanged after each task.
- Run `cargo test` after every task — never proceed with
  failing tests.
- Run `cargo clippy` after Task 7 to verify no dead code
  warnings from moved `#[cfg(test)]` items.
- Run `cargo bench --no-run` after Task 1 to verify
  benchmark still compiles.
- No new tests needed — this is a structural refactoring
  with no behavior change.
- Manual verification: run
  `cargo run -- assets/heapdump-visualvm.hprof` to confirm
  the binary still works end-to-end.

### Notes

- `run_first_pass` is the only public function; its
  signature must not change.
- `parse_class_dump` and `value_byte_size` are `pub(crate)`
  — visibility must be preserved via re-export.
- Each task must leave the codebase in a compilable,
  test-passing state. No "big bang" multi-task commits.
- Minor cosmetic differences in warning text from the
  unification (e.g., `push_warning` via method vs free fn)
  are acceptable as long as the warning semantics are
  preserved.
- **Future improvement**: `PreciseIndex` has 10 `pub` fields
  (open data bag). Consider restricting to `pub(crate)` +
  accessors in a separate refactoring that also touches
  `hprof-engine`.
