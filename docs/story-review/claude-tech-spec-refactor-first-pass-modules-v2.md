# Tech Spec Review (Second Pass): Refactor first_pass.rs into focused modules

**Reviewed:** 2026-03-09
**Spec:** `docs/implementation-artifacts/tech-spec-refactor-first-pass-modules.md`
**First review:** `docs/code-review/claude-tech-spec-refactor-first-pass-modules.md`
**Source verified against:** `crates/hprof-parser/src/indexer/first_pass.rs` (2849 lines)

---

## Part A: Verification of Previous 15 Findings

### Fixed (8 of 15)

- **Finding 1 (test counts):** FIXED. Spec now correctly states
  37 + 24 = 61 tests. Verified by counting `#[test]` annotations
  in source: 37 in `mod tests` (lines 1535-2258), 24 in
  `mod builder_tests` (lines 2259-2849). AC-1 and Testing Strategy
  also reference 61.

- **Finding 2 (task ordering / compilation breakage):** FIXED.
  Tasks reordered: `FirstPassContext` is now Task 2 (before any
  module extraction), eliminating throwaway intermediate signatures.

- **Finding 3 (push_warning/push_suppressed_summary unassigned):**
  FIXED. Task 2 explicitly moves them to `FirstPassContext` methods
  and deletes the free functions.

- **Finding 5 (benchmark file unmentioned):** FIXED. Benchmark
  file listed in "Files to Reference" table and Task 1 includes
  `cargo bench --no-run` verification.

- **Finding 6 (builder_tests feature gate):** FIXED. Task 7
  explicitly states simplifying from
  `#[cfg(all(test, feature = "test-utils"))]` to
  `#[cfg(feature = "test-utils")]` since parent `tests.rs` is
  already behind `#[cfg(test)]`.

- **Finding 9 (HashSet dependency):** FIXED. ADR-2 now mentions
  `extract_obj_refs` uses `std::collections::HashSet` and says
  "include that import."

- **Finding 10 (FirstPassContext introduced too late):** FIXED.
  `FirstPassContext` is now Task 2, before any module extraction.

- **Finding 11 (AC-8 self-contradiction):** FIXED. AC-8 now
  reads "the only match is `#[cfg(test)] mod tests;` in `mod.rs`."

### Partially Fixed (3 of 15)

- **Finding 4 (#[cfg(test)] line count inflated):** PARTIALLY
  FIXED. The overview now says "~200 lines of `#[cfg(test)]`-only
  code" which is close to the actual ~203 lines. Acceptable.

- **Finding 7 (parse_and_insert signature under-specified):**
  PARTIALLY FIXED. ADR-5 now describes using closures that capture
  call-site-specific args, and Task 6 specifies the generic
  `parse_fn: FnOnce` approach. But see NEW Finding 2 below for
  a remaining borrow checker problem the spec does not address.

- **Finding 8 (StringInUtf8 special handling):** PARTIALLY FIXED.
  ADR-5 now acknowledges different parse signatures and says
  "each call site wraps with the appropriate captured args."
  Task 6 repeats this. The idea is sound, but the spec still
  does not show a concrete signature for the helper. See
  NEW Finding 3 for why this matters.

### Not Fixed (4 of 15)

- **Finding 12 (PARALLEL_THRESHOLD visibility change unjustified):**
  NOT FIXED. Task 3 moves it to `hprof_primitives.rs` with
  `pub(super)` but never states which sub-modules need it.
  `heap_extraction.rs` needs it (for the parallel threshold check
  in `extract_all`) and `mod.rs` needs it (for
  `defer_heap_extraction` heuristic in `scan_records` after
  Task 6). Neither is mentioned.

- **Finding 13 (run_first_pass line count):** NOT FIXED. The spec
  still says "539 L" but the function spans lines 128-663 = 536
  lines (or 539 counting some way). The overview says "a monolithic
  `run_first_pass` function (539 L)." Counting inclusively
  128 to 663 is 536 lines. Nitpick, but still wrong.

- **Finding 14 (no rollback plan):** NOT FIXED. Still no
  recommendation for intermediate tags/branches. "cargo test
  must pass after each task" is the only guard.

- **Finding 15 (behavior change denied but present):** NOT FIXED.
  The scope section still says "No logic or behavior changes" with
  a parenthetical about cosmetic differences. But Task 4 makes
  a real structural change: the inline `!defer_heap_extraction`
  path that did per-sub-record progress reporting is deleted,
  and ALL extraction is deferred to `extract_all` after the scan
  loop. See NEW Finding 1 for details.

---

## Part B: New Findings

1. **[High] Task 4 changes execution semantics, not just structure.**
   Currently, when `defer_heap_extraction` is false (files < 32 MB),
   heap objects are extracted INLINE during the scan loop — the
   `extract_heap_object_ids` call at line 213 runs inside the
   `while cursor < data.len()` loop, and `maybe_report_progress`
   is called per sub-record (line 1374). Task 4 says to "replace
   ALL heap extraction logic (both the inline
   `!defer_heap_extraction` path inside the scan loop AND the
   post-loop deferred extraction) with
   `heap_extraction::extract_all`." This means small files that
   previously got inline extraction now get deferred extraction.
   The `defer_heap_extraction` heuristic and the inline path are
   both deleted. This changes: (a) progress reporting granularity
   for small files (per-sub-record becomes per-segment), (b)
   memory lifetime of `HeapSegmentResult` temporaries, and (c)
   the order of warnings (previously interleaved with record scan
   warnings, now batched after). The scope section claims "no
   behavior changes" while Task 4 implements one.

2. **[High] `LoadClass` insert closure creates an unresolvable
   borrow conflict with `parse_and_insert`.** The `LoadClass`
   match arm (lines 308-346) reads from
   `ctx.result.index.strings` (immutable borrow) and writes to
   `ctx.result.index.class_names_by_id` and
   `ctx.result.index.classes` (mutable borrows) — all through
   the same `ctx` reference. The proposed `parse_and_insert`
   helper takes `ctx: &mut FirstPassContext`. The `insert_fn`
   closure would need to capture `&ctx.result.index.strings`
   immutably AND `&mut ctx.result.index.class_names_by_id`
   mutably, but `ctx` is already mutably borrowed by
   `parse_and_insert`. Rust's borrow checker will reject this.
   The spec provides no solution (e.g., splitting the struct,
   using indices, doing the string lookup inside the closure
   body after the parse). Task 6 note says "The `LoadClass`
   insert closure captures `&ctx.result.index.strings`" —
   confirming the spec authors know about this access pattern
   but did not reason through the borrowing implications.

3. **[High] `parse_and_insert` helper signature is still not
   concrete.** Task 6 describes the helper's behavior in prose
   but never gives a function signature. For a spec that is
   supposed to be "ready-for-dev," this is a significant gap.
   The developer must invent the signature, discover the borrow
   checker issue from Finding 2, and design a workaround — all
   unguided by the spec. At minimum the spec should show the
   actual Rust signature with lifetime annotations and generic
   bounds.

4. **[High] `ctx.cursor_position()` in the final `run_first_pass`
   skeleton does not exist.** Task 6 shows:
   ```rust
   progress_fn(ctx.cursor_position());
   ```
   But `FirstPassContext` (Task 2) has no `cursor` field and no
   `cursor_position()` method. The main cursor is a local in
   `run_first_pass` that moves into `scan_records` at Task 6.
   After `scan_records` returns, the cursor is gone. The spec
   never defines how the final cursor position is communicated
   back to `run_first_pass`. Options: (a) add a `bytes_scanned`
   field to `FirstPassContext`, (b) return the position from
   `scan_records`, (c) use `data.len()`. None is specified.

5. **[Medium] Task 4 `extract_all` needs `&mut progress_fn` but
   Task 2's `FirstPassContext` does not store it.** The final
   orchestrator in Task 6 passes `&mut progress_fn` to both
   `scan_records` and `extract_all`. But `extract_all` also
   needs `last_progress_bytes` and `last_progress_at` from
   `FirstPassContext` to call `maybe_report_progress`. If
   `extract_all` takes `&mut FirstPassContext`, the progress
   state travels correctly. But after `scan_records` finishes,
   `last_progress_bytes` and `last_progress_at` reflect the
   scan loop's final position. `extract_all` then continues
   from there. This works only if `extract_all` receives the
   same `&mut FirstPassContext` — which it does per the spec.
   However, the parallel path inside `extract_all` does NOT use
   `ctx.last_progress_bytes` / `ctx.last_progress_at` — it
   computes progress from `batch.last()` offsets. The sequential
   fallback in `extract_all` (from the current
   `extract_heap_object_ids`) uses per-sub-record progress via
   `maybe_report_progress`. The spec says to delete
   `extract_heap_object_ids` and use `extract_heap_segment`
   (the parallel worker) for BOTH paths. The unified sequential
   path would then lose per-sub-record progress reporting,
   making it report progress only per-segment. For a 32 MB
   file with 1-2 heap segments, this means zero intermediate
   progress updates during extraction. This contradicts the
   module docstring's promise of progress every
   `PROGRESS_REPORT_INTERVAL` bytes.

6. **[Medium] `gc_root_skip_size` handles `GcRootJavaFrame` and
   `GcRootThreadObj` in the skip table, but these are also
   handled as explicit match arms in `extract_heap_segment_parallel`
   (lines 967-1023) and `extract_heap_object_ids` (lines
   1208-1286).** The skip function returns `Some(id + 8)` for
   both, but they are never reached because the explicit arms
   match first. When Task 4 renames `extract_heap_segment_parallel`
   to `extract_heap_segment` and it becomes the sole extraction
   function, these dead arms in `gc_root_skip_size` become
   permanently unreachable for heap extraction. They are only
   reachable from test-only code (`skip_heap_object`,
   `subdivide_segment`, `extract_class_dumps_only`). The spec
   does not note this and does not consider whether these arms
   should be removed or documented as test-only.

7. **[Medium] `GcRootJniGlobal` skip size is `Some(id)` (one ID)
   but the hprof spec defines it as `object_id + jni_global_ref_id`
   (two IDs).** Line 696: `HeapSubTag::GcRootJniGlobal |
   HeapSubTag::GcRootThreadBlock => Some(id)`. For
   `GcRootJniGlobal`, the correct skip size is `2 * id` (object ID
   + JNI global ref ID). `GcRootThreadBlock` is `id + 4` (object
   ID + thread serial). Having them share `Some(id)` is wrong for
   both. This is a pre-existing bug in the source code, NOT
   introduced by the spec, but a refactoring spec that claims
   "no behavior changes" should at minimum flag known bugs that
   will be preserved. Alternatively, if the spec is wrong about
   "no behavior changes" being the goal, it should state that
   these bugs are intentionally carried forward.

8. **[Medium] `FirstPassContext.finish()` does too much or too
   little.** Task 2 says `finish(mut self) -> IndexResult`
   handles: sort `all_offsets`, build segment filters, append
   suppressed summary. But `all_offsets` sorting currently
   happens at line 565 BEFORE thread resolution (lines 576-648).
   Thread resolution DEPENDS on sorted `all_offsets` (it calls
   `lookup_offset` which does binary search). If `finish()` sorts
   `all_offsets` AND is called AFTER `thread_resolution::resolve_all`,
   then `resolve_all` would operate on unsorted `all_offsets`.
   The spec's Task 5 `resolve_all` says it calls
   `resolve_thread_transitive_offsets` which calls `lookup_offset`
   on `all_offsets` — requiring them to be sorted. Either sorting
   must happen before `resolve_all` (not in `finish()`), or
   `resolve_all` must sort first then call `finish()`. The spec
   does not address this ordering dependency at all.

9. **[Medium] Test imports after refactoring are under-specified.**
   Task 7 says tests use `use super::*;` plus explicit sub-module
   imports. But `super::*` from `tests.rs` points to `mod.rs`.
   Functions like `extract_heap_segment` (in `heap_extraction.rs`),
   `skip_n`, `gc_root_skip_size`, `parse_class_dump` (in
   `hprof_primitives.rs`) are `pub(super)` — visible to `mod.rs`
   but NOT re-exported by `mod.rs` (only `parse_class_dump` and
   `value_byte_size` get `pub(crate)` re-exports). So
   `use super::*;` will NOT bring `skip_n`,
   `gc_root_skip_size`, `extract_heap_segment`, etc. into scope.
   Tests that call these functions directly will fail to compile.
   The spec mentions
   `use super::hprof_primitives::{skip_n, gc_root_skip_size, parse_class_dump};`
   as an example, but does not list ALL needed imports. For a
   refactoring spec that promises "all 61 tests pass unchanged
   after each task," this is a gap that will cause compilation
   failures at Task 7.

10. **[Medium] Task 2 creates a ~2900-line `mod.rs` with a new
    struct and methods bolted on.** The spec acknowledges this
    implicitly ("The function body still contains all logic
    inline at this point"). But Task 2's actual diff is: add
    ~60 lines for `FirstPassContext` struct + methods, rewrite
    every local variable reference in the 536-line
    `run_first_pass` to use `ctx.field`, and delete the 2 free
    functions. The resulting `mod.rs` is arguably HARDER to work
    with than the original because now every line says `ctx.`
    and the struct definition is separated from its usage by
    hundreds of lines. The spec does not acknowledge this
    temporary regression in readability or suggest any
    mitigation (e.g., collapsing the refactor into fewer tasks
    to minimize time spent in this degraded state).

11. **[Low] `extract_heap_segment_parallel` is used by tests
    directly (e.g., `parallel_path_produces_correct_results` at
    line 2781).** When Task 4 renames it to `extract_heap_segment`,
    all tests referencing the old name break. Task 4 says nothing
    about updating test call sites. Task 7 moves tests later,
    but the rename happens at Task 4 and "cargo test must pass"
    after Task 4. The test at line 2781 still calls
    `extract_heap_segment_parallel` and will fail.

12. **[Low] ADR-3 says "`HeapSegmentResult` lives in
    `heap_extraction.rs` with `pub(super)` visibility" but
    `extract_heap_segment_parallel` (the parallel worker) also
    constructs it.** After Task 4, when `extract_heap_segment`
    lives in `heap_extraction.rs`, this is fine — both the
    struct and its constructor are in the same file. But the
    spec does not mention that `HeapSegmentResult` is currently
    private (no visibility modifier, line 57) and needs
    `pub(super)` added. This is trivially correct but is the
    kind of detail a "ready-for-dev" spec should not leave
    implicit.

---

## Summary

Of the original 15 findings: 8 fixed, 3 partially fixed,
4 not fixed. The most impactful unfixed item is Finding 15
(denied behavior change), which is now compounded by NEW
Finding 1 (Task 4 fundamentally changes small-file execution
path).

The new findings cluster around two themes:

1. **Borrow checker blindness** (Findings 2-3): The
   `parse_and_insert` helper and `LoadClass` closure create a
   conflict the spec does not address. This will block Task 6
   implementation.

2. **Ordering dependencies not modeled** (Findings 4, 8): The
   `cursor_position` phantom method and the `all_offsets` sort
   timing relative to `resolve_all` will cause compilation or
   logic errors that the developer must discover independently.

The spec is in better shape than v1 but is not truly
"ready-for-dev" — a developer following it literally will hit
compilation failures at Tasks 4, 6, and 7.
