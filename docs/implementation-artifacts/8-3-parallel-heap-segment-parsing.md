# Story 8.3: Parallel Heap Segment Parsing

Status: done

## Story

As a user,
I want heap dump segments parsed in parallel across CPU cores,
so that multi-core machines index heap dumps proportionally
faster.

## Acceptance Criteria

### AC1: Parallel parsing above 32 MB threshold

**Given** a heap dump with total heap segment size >= 32 MB
**When** the first pass reaches heap extraction
**Then** heap segments are parsed in parallel using rayon.

### AC2: Sequential parsing below threshold

**Given** a heap dump with total heap segment size < 32 MB
**When** the first pass reaches heap extraction
**Then** heap segments are parsed sequentially (no rayon
overhead).

### AC3: CLASS_DUMP sequential pre-pass

**Given** parallel heap parsing
**When** CLASS_DUMP sub-records (tag `0x20`) are encountered
**Then** they are extracted in a sequential pre-pass before
parallel extraction begins, so `class_dumps` is available as
read-only shared state.

### AC4: Per-worker segment filter IDs

**Given** parallel heap parsing with multiple workers
**When** workers produce object IDs for segment filters
**Then** IDs are collected in per-worker `Vec<u64>`,
concatenated per 64 MiB segment, and built into BinaryFuse8
filters after all workers complete.

### AC5: Per-worker offset data merge

**Given** parallel heap parsing with workers producing offset
data
**When** results are merged
**Then** per-worker `Vec<(u64, u64)>` are concatenated (not
HashMap-merged) and sorted once.

### AC6: Large segment sub-division

**Given** a HEAP_DUMP_SEGMENT larger than 16 MB
**When** it is assigned to a worker
**Then** it may be sub-divided at sub-record boundaries for
finer load balancing.

### AC7: All existing tests pass

**Given** all existing tests (364+ tests)
**When** I run `cargo test`
**Then** all tests pass with identical indexing results to
sequential parsing.

## Tasks / Subtasks

- [x] Task 1: Add rayon dependency to `hprof-parser` (AC: 1)
  - [x] 1.1: Add `rayon = { workspace = true }` to
    `crates/hprof-parser/Cargo.toml` under `[dependencies]`
    (rayon `"1"` is already in workspace deps)

- [x] Task 2: Extract per-segment worker output struct
  (AC: 4, 5)
  - [x] 2.1: Define a struct for per-worker results
    (final implementation omits `class_dumps` field —
    CLASS_DUMP is handled by the sequential pre-pass
    which populates `index.class_dumps` directly):
    ```rust
    struct HeapSegmentResult {
        /// (object_id, records-section offset) pairs
        all_offsets: Vec<(u64, u64)>,
        /// (data_offset, object_id) for segment filter
        filter_ids: Vec<(usize, u64)>,
        /// GC root java frame: (obj_id, thread_serial,
        ///   frame_number)
        raw_frame_roots: Vec<(u64, u32, i32)>,
        /// ROOT_THREAD_OBJ: (obj_id, thread_serial)
        raw_thread_objects: Vec<(u64, u32)>,
        warnings: Vec<String>,
    }
    ```
  - [x] 2.2: This struct is local to `first_pass.rs` — no
    public API change needed

- [x] Task 3: Implement CLASS_DUMP sequential pre-pass
  (AC: 3)
  - [x] 3.1: Create function `extract_class_dumps_only`:
    ```rust
    fn extract_class_dumps_only(
        payload: &[u8],
        id_size: u32,
    ) -> Vec<(u64, ClassDumpInfo)>
    ```
    This scans a heap segment payload, parsing ONLY
    `CLASS_DUMP` (sub-tag `0x20`) sub-records. All other
    sub-tags are skipped by reading their fixed-size
    headers (reuse existing skip logic from
    `extract_heap_object_ids`).
  - [x] 3.2: The pre-pass iterates over ALL
    `heap_record_ranges` sequentially and inserts results
    into `index.class_dumps`. This happens BEFORE the
    parallel phase.
  - [x] 3.3: The existing `extract_heap_object_ids` must
    SKIP `0x20` sub-records in the parallel phase (they
    were already handled). Change the `0x20` match arm to
    skip using `parse_class_dump` cursor advance but do
    NOT insert into `index.class_dumps`.

- [x] Task 4: Refactor `extract_heap_object_ids` for
  parallel use (AC: 1, 4, 5)
  - [x] 4.1: Create a new function
    `extract_heap_segment_parallel` that takes:
    ```rust
    fn extract_heap_segment_parallel(
        payload: &[u8],
        data_offset: usize,
        id_size: u32,
    ) -> HeapSegmentResult
    ```
    No `&mut` references — all output goes into the
    returned `HeapSegmentResult`. No `SegmentFilterBuilder`
    — collect raw `(data_offset, object_id)` tuples
    instead. No `&mut PreciseIndex` — `class_dumps` are
    already populated from pre-pass (skip `0x20`).
  - [x] 4.2: The function body is adapted from
    `extract_heap_object_ids` with these changes:
    - Remove `builder.add()` calls; push to local
      `filter_ids: Vec<(usize, u64)>` instead
    - Remove `index.class_dumps.insert()` for `0x20`;
      just skip the bytes using `parse_class_dump`
    - Remove progress reporting (no `progress_fn`) — the
      parallel phase is expected to be fast enough that
      intermediate progress is not needed
    - Remove `suppressed_warnings` tracking; collect all
      warnings in the result Vec
  - [x] 4.3: Keep the original `extract_heap_object_ids`
    function intact for the sequential path (threshold
    < 32 MB). Do NOT delete it.

- [x] Task 5: Implement parallel dispatch in
  `run_first_pass` (AC: 1, 2, 3)
  - [x] 5.1: After the main record scan loop, compute
    total heap size:
    ```rust
    let total_heap_bytes: u64 = result
        .heap_record_ranges
        .iter()
        .map(|(_, len)| *len)
        .sum();
    ```
  - [x] 5.2: Branch on threshold:
    ```rust
    const PARALLEL_THRESHOLD: u64 = 32 * 1024 * 1024;
    ```
    - **Below threshold:** Keep current sequential
      behavior — iterate `heap_record_ranges` and call
      `extract_heap_object_ids` per segment (existing
      code path, already works)
    - **Above threshold:** Execute parallel path (Tasks
      5.3-5.6)
  - [x] 5.3: **Restructure the main loop**: Currently,
    heap segments are processed inline during the record
    scan loop (tags `0x0C`/`0x1C`). For the parallel path,
    the main loop must SKIP heap segment payloads (just
    record `heap_record_ranges` and `cursor.set_position`
    past them). Non-heap records (`0x01`, `0x02`, `0x04`,
    `0x05`, `0x06`) continue to be processed inline as
    before.
    **Implementation approach:**
    - Add a boolean `defer_heap_extraction` set before the
      loop based on a preliminary file size heuristic
      (`data.len() >= PARALLEL_THRESHOLD as usize`).
      The actual threshold check on total heap bytes
      happens after the loop and may fall back to
      sequential if the heap is actually small.
    - In the `0x0C`/`0x1C` branch: if
      `defer_heap_extraction`, only push to
      `heap_record_ranges` and skip the payload; else
      call `extract_heap_object_ids` as before.
  - [x] 5.4: **CLASS_DUMP pre-pass** (sequential): After
    the main loop, if parallel path:
    ```rust
    for &(offset, len) in &result.heap_record_ranges {
        let payload = &data[offset as usize
            ..(offset + len) as usize];
        let dumps = extract_class_dumps_only(
            payload, id_size
        );
        for (class_id, info) in dumps {
            result.index.class_dumps.insert(
                class_id, info
            );
        }
    }
    ```
  - [x] 5.5: **Parallel extraction**: Use rayon
    `par_iter()` over `heap_record_ranges`:
    ```rust
    use rayon::prelude::*;

    let segment_results: Vec<HeapSegmentResult> =
        result.heap_record_ranges
            .par_iter()
            .map(|&(offset, len)| {
                let payload = &data[offset as usize
                    ..(offset + len) as usize];
                extract_heap_segment_parallel(
                    payload,
                    offset as usize,
                    id_size,
                )
            })
            .collect();
    ```
  - [x] 5.6: **Merge results** after parallel phase:
    ```rust
    for seg_result in segment_results {
        all_offsets.extend(seg_result.all_offsets);
        for (data_off, obj_id) in seg_result.filter_ids {
            seg_builder.add(data_off, obj_id);
        }
        raw_frame_roots.extend(
            seg_result.raw_frame_roots
        );
        raw_thread_objects.extend(
            seg_result.raw_thread_objects
        );
        result.warnings.extend(seg_result.warnings);
    }
    ```
    **Important**: `seg_builder.add()` calls during merge
    must be in segment order (ascending `data_offset`) for
    the inline finalization logic to work correctly. Since
    `heap_record_ranges` is already in file order AND
    `par_iter().collect()` preserves order, the merge loop
    iterates in order. Within each segment's `filter_ids`,
    the IDs are already in sub-record order.
  - [x] 5.7: After merge, the existing post-loop code
    (`all_offsets.sort_unstable`, thread synthesis, frame
    root correlation, `resolve_thread_transitive_offsets`,
    segment filter build) runs unchanged.

- [x] Task 6: Sub-divide large segments (AC: 6)
  - [x] 6.1: Before `par_iter()`, expand
    `heap_record_ranges` by splitting any segment larger
    than 16 MB into chunks at sub-record boundaries:
    ```rust
    const SUB_DIVIDE_THRESHOLD: u64 =
        16 * 1024 * 1024;

    fn subdivide_segment(
        data: &[u8],
        offset: u64,
        len: u64,
        id_size: u32,
        threshold: u64,
    ) -> Vec<(u64, u64)>
    ```
    This function scans sub-record tags to find split
    points near the threshold boundary. Each chunk starts
    at a sub-record boundary.
  - [x] 6.2: The subdivision scan reads sub-tag + skips
    the sub-record body using the same size logic as
    `extract_heap_object_ids`. It does NOT parse field
    data — just advances the cursor to find split points.
  - [x] 6.3: Replace `heap_record_ranges` with the
    expanded list before passing to `par_iter()`. The
    expanded ranges are used ONLY for parallel dispatch;
    the original `heap_record_ranges` is preserved in
    `IndexResult` unchanged.

- [x] Task 7: Add tracing spans for parallel phase
  (AC: 1)
  - [x] 7.1: Add `#[cfg(feature = "dev-profiling")]`
    tracing spans:
    - `"class_dump_prepass"` around the sequential
      CLASS_DUMP extraction
    - `"parallel_heap_extraction"` around the
      `par_iter()` call
    - `"parallel_merge"` around the merge loop
  - [x] 7.2: Preserve ALL existing tracing spans. The
    `"heap_extraction"` span in the sequential path
    remains as-is.

- [x] Task 8: Tests (AC: 7)
  - [x] 8.1: All 364+ existing tests must pass — parallel
    path must produce identical results to sequential
  - [x] 8.2: Add unit test: file with total heap < 32 MB
    uses sequential path (test via the existing test
    fixtures which are 41 MB file size but heap segments
    may be smaller — verify behavior)
  - [x] 8.3: Add unit test: verify `subdivide_segment`
    produces valid sub-record boundaries (construct a
    synthetic payload with known sub-records)
  - [x] 8.4: Add unit test: `extract_class_dumps_only`
    returns correct CLASS_DUMP entries from a synthetic
    heap segment
  - [x] 8.5: Add unit test:
    `extract_heap_segment_parallel` skips CLASS_DUMP
    (0x20) and collects INSTANCE_DUMP (0x21),
    OBJECT_ARRAY_DUMP (0x22), PRIMITIVE_ARRAY_DUMP (0x23)
    correctly
  - [x] 8.6: Run `cargo clippy` — no warnings
  - [x] 8.7: Run `cargo fmt -- --check` — clean

## Dev Notes

### Two-Pass Architecture (Critical Design Decision)

The Red Team analysis identified that `class_dumps` is
shared mutable state during heap extraction — CLASS_DUMP
records (0x20) can appear in ANY heap segment, and
INSTANCE_DUMP records (0x21) in later segments reference
them via `class_object_id` for field layout. Pure parallel
parsing would require either:
- A `DashMap` (lock contention, rejected)
- Pre-collecting all CLASS_DUMPs first (chosen)

**Sub-pass 1 (sequential):** Scan all heap segments
extracting ONLY CLASS_DUMP (0x20). This is fast because
CLASS_DUMPs are a tiny fraction of heap data (~7K records
vs millions of instances). The scan skips all other
sub-records by reading their fixed-size headers.

**Sub-pass 2 (parallel):** `par_iter()` over segments.
Each worker has `class_dumps` as read-only shared state
(it's in `PreciseIndex` which is `&` not `&mut`). Workers
collect all other sub-record data into local Vecs.

[Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Red Team]

### Merge Strategy: Vec Concat + Single Sort

Per party-mode Pre-mortem scenario 6: HashMap merge would
create contention. Instead:
- Each worker produces a local `Vec<(u64, u64)>` for
  offsets
- After `par_iter()`, extend the main `all_offsets` Vec
  from each worker's Vec (O(1) amortized per extend)
- Single `sort_unstable_by_key` at the end (already
  exists in current code at line 432)

This avoids all synchronization during the hot loop.

### Segment Filter Coherence

Current `SegmentFilterBuilder` uses inline finalization:
when `add()` detects a new segment index, it finalizes the
previous segment immediately. This works because the
sequential path processes sub-records in file order.

For the parallel merge: worker results are merged in
segment order (guaranteed by `par_iter().collect()`
preserving order). Each worker's `filter_ids` contains
`(data_offset, object_id)` tuples. The merge loop feeds
these to `seg_builder.add()` in ascending offset order,
so the inline finalization continues to work correctly.

### Threshold: 32 MB Minimum

Pre-mortem scenario 7: rayon thread pool initialization
and work-stealing overhead is measurable on small dumps.
The 41 MB test fixture (`heapdump-visualvm.hprof`) has
~38 MB of heap segments — it will cross the threshold.
The 1.4 GB RustRover dump will massively benefit.

Threshold is based on **total heap segment bytes** (sum
of all `heap_record_ranges` lengths), NOT total file size.

### Sub-dividing Segments > 16 MB

Large HEAP_DUMP_SEGMENT records (common in big dumps) can
create load imbalance — one worker gets a 200 MB segment
while others finish quickly. Sub-dividing at sub-record
boundaries allows rayon's work-stealing to distribute the
load more evenly.

The subdivision scan is lightweight: it reads sub-tag bytes
and skips sub-record bodies without parsing field data.
Split points are chosen at the first sub-record boundary
after each 16 MB chunk.

### Sequential Path Preservation

The original `extract_heap_object_ids` function and the
inline processing in the main loop are kept intact for the
sequential path. This ensures:
- Zero regression risk for small dumps
- The sequential path remains the reference implementation
- Easy A/B comparison via threshold override

### Progress Reporting in Parallel Phase

The parallel phase does NOT report per-sub-record progress.
Rationale: on the 1.4 GB dump, the heap extraction phase
currently takes ~15-20s sequentially. With 8 cores, it
should complete in ~4-5s — fast enough that intermediate
progress is not needed. The progress bar will show the
main loop's progress (non-heap records) and then jump
when the parallel phase completes.

If needed later, a `AtomicUsize` counter shared across
workers can provide coarse-grained progress.

### `data` Slice Context

`run_first_pass` receives `data: &[u8]` which is
`&mmap[records_start..]`. All offsets in
`heap_record_ranges` are relative to this slice. The
parallel workers receive sub-slices of `data` — their
`data_offset` parameter tells them the absolute offset
within `data` for correct segment filter assignment and
`all_offsets` recording.

### Performance Expectations

| Metric | Before (8.2) | Expected (8.3) | Rationale |
|--------|-------------|----------------|-----------|
| Heap extraction (41 MB) | ~30 ms | ~15-20 ms | 2x on 4+ cores (small file overhead) |
| Heap extraction (1.4 GB) | ~15-20s | ~4-5s | 3-4x on 8 cores |
| First pass total (1.4 GB) | ~1.5s | ~0.8-1s | Heap extraction is dominant phase |
| Peak RSS | ~430 MB | ~440 MB | +10 MB for per-worker Vecs (temporary) |

### Architecture Compliance

- **New dependency:** `rayon` added to `hprof-parser`
  (already in workspace deps, already used by
  `hprof-engine`)
- **Crate boundary:** All changes in `hprof-parser`.
  No engine or TUI changes needed.
- **Dependency direction:**
  `hprof-cli -> hprof-engine -> hprof-parser` (unchanged)
- **No `println!`** — only tracing macros, gated behind
  `dev-profiling` feature flag
- **Error handling:** Workers collect warnings in local
  Vecs. No panics — all sub-record parsing uses tolerant
  `break` on error (same as sequential path).
- **Thread safety:** No shared mutable state during
  parallel phase. `class_dumps` is populated before
  `par_iter()` and accessed read-only. Workers return
  owned data structures.

### Key Code Locations

| File | What Changes |
|------|-------------|
| `crates/hprof-parser/Cargo.toml` | Add `rayon` dependency |
| `crates/hprof-parser/src/indexer/first_pass.rs` | `HeapSegmentResult` struct, `extract_class_dumps_only`, `extract_heap_segment_parallel`, `subdivide_segment`, parallel dispatch in `run_first_pass` |
| `crates/hprof-parser/src/indexer/segment.rs` | No changes — `SegmentFilterBuilder` works as-is |
| `crates/hprof-parser/src/indexer/mod.rs` | No changes |

### Previous Story Intelligence (Story 8.2)

**Learnings from 8.2:**
- `HprofStringRef` with relative offsets works correctly.
  Parallel workers receive `data` sub-slices but offsets
  stored in `all_offsets` are relative to the full `data`
  slice (records section) — the `data_offset` parameter
  ensures this.
- `class_names_by_id` is built from LOAD_CLASS (tag 0x02)
  records which are NOT in heap segments — no parallel
  concern.
- `extract_obj_refs` (used by
  `resolve_thread_transitive_offsets` post-loop) reads
  from `class_dumps` — this is why CLASS_DUMP must be
  fully populated before the thread cache build phase.
- The 5s freeze action item (thread cache fallback to
  linear scan) is a separate concern from this story.
  Do NOT attempt to fix it here.
- 364 tests at end of Story 8.2 (baseline).

**Files modified in 8.2 that overlap with 8.3:**
- `crates/hprof-parser/src/indexer/first_pass.rs`
  (primary target for both stories)

### Git Intelligence

Recent commits follow pattern:
- `feat: Story X.Y — <description>` for implementation
- `fix: Story X.Y code review fixes (<reviewers>)`
- `docs: Story X.Y story file + code reviews`

### Anti-Patterns to Avoid

- Do NOT use `DashMap` or any concurrent HashMap — the
  whole point is lock-free per-worker Vecs merged after
  completion.
- Do NOT modify `SegmentFilterBuilder` internals — feed
  it merged IDs in order after parallel phase.
- Do NOT add progress reporting to parallel workers unless
  proven necessary by benchmarks — it adds complexity for
  a phase that should complete in seconds.
- Do NOT change `resolve_thread_transitive_offsets` or
  thread cache build — those run after merge and are
  unaffected.
- Do NOT change the `PreciseIndex` struct or `IndexResult`
  struct — the parallel phase is an internal optimization
  of `run_first_pass`.
- Do NOT remove or modify existing tracing spans — only
  ADD new ones for the parallel phase.
- Do NOT change `extract_heap_object_ids` — it's the
  sequential path reference implementation.
- Do NOT attempt to parallelize non-heap record parsing
  (STRING, LOAD_CLASS, etc.) — they are fast and have
  sequential dependencies.
- Do NOT change segment filter SEGMENT_SIZE (64 MiB) —
  that is a separate tuning decision.
- Do NOT address the 5s thread cache freeze (action item
  from 8.2) — that is a separate story/bug fix.

### Testing Strategy

1. **All 364+ existing tests** must pass — parallel path
   must produce bit-identical results to sequential.
2. **New unit tests** for `extract_class_dumps_only`,
   `extract_heap_segment_parallel`, `subdivide_segment`.
3. **Benchmark validation** (manual, if `HPROF_BENCH_FILE`
   set): Run `cargo bench --bench first_pass` and compare
   to 8.2 baseline. Expect 2-4x improvement on multi-core.
4. **Manual test** with
   `assets/heapdump-visualvm.hprof` (41 MB): verify
   behavior at threshold boundary.

### Project Structure Notes

- No new files or modules needed.
- All new functions are private to `first_pass.rs`.
- `HeapSegmentResult` is a private struct in
  `first_pass.rs`.
- `rayon` import added to `first_pass.rs` only.

### References

- [Source: docs/planning-artifacts/epics.md#Story 8.3]
- [Source: docs/planning-artifacts/architecture.md#Indexing Strategy]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Story 8.3]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Red Team]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Pre-mortem]
- [Source: docs/implementation-artifacts/8-2-lazy-string-references.md]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- Task 1: Added `rayon = { workspace = true }` to hprof-parser Cargo.toml
- Task 2: Defined `HeapSegmentResult` struct — private to first_pass.rs, no `class_dumps` field (pre-pass populates index directly)
- Task 3: Implemented `extract_class_dumps_only` scanning only 0x20 sub-records, skipping all others via header-size skip logic
- Task 4: Implemented `extract_heap_segment_parallel` — no mutable shared state, collects all_offsets/filter_ids/frame_roots/thread_objects into owned Vecs, skips 0x20 via `parse_class_dump` cursor advance
- Task 5: Added `defer_heap_extraction` heuristic in main loop (file size >= 32MB), post-loop parallel path with CLASS_DUMP pre-pass → subdivide → par_iter → merge. Fallback to sequential if total heap < 32MB despite large file
- Task 6: Implemented `subdivide_segment` splitting at sub-record boundaries past 16MB threshold
- Task 7: Added tracing spans `class_dump_prepass`, `parallel_heap_extraction`, `parallel_merge` gated behind `dev-profiling`
- Task 8: 369 tests pass (5 new: extract_class_dumps_only, extract_heap_segment_parallel_skips_class_dump, subdivide_segment_no_split, subdivide_segment_splits, small_file_uses_sequential_path). Clippy clean, fmt clean.

### Change Log

- 2026-03-08: Story 8.3 implementation — parallel heap segment parsing via rayon

### File List

- `Cargo.lock` (modified — rayon dep resolution)
- `crates/hprof-parser/Cargo.toml` (modified — added rayon dep)
- `crates/hprof-parser/src/indexer/first_pass.rs` (modified — HeapSegmentResult, extract_class_dumps_only, extract_heap_segment_parallel, subdivide_segment, parallel dispatch, tracing spans, 6 new tests)
- `docs/implementation-artifacts/sprint-status.yaml` (modified — story status)
