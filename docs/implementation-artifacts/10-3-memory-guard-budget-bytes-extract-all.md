# Story 10.3: Memory Guard via budget_bytes in extract_all

Status: done

## Story

As a user,
I want the system to use the configured memory budget as
a batch size limit during parallel heap extraction,
so that RAM usage stays controlled even on very large
dumps with many heap segments processed simultaneously.

**Scope caveat:** This story bounds inter-segment
batching only. It does NOT protect against a single
giant segment (e.g. HotSpot's single `HEAP_DUMP`
record). Story 10.2 (chunked extraction) is the
necessary complement for that case.

## Acceptance Criteria

1. **AC1 – Budget-aware batching:**
   Given `budget_bytes` is set (via CLI `--memory-limit`
   or config),
   When `extract_all` processes segments in parallel,
   Then the total payload size of segments in each parallel
   batch does not exceed `max(budget_bytes, 64 MB)`
   (the 64 MB floor prevents degenerate micro-batching).
   Exception: a single segment whose payload exceeds the
   limit is processed alone in its own batch (see ADR-2).

2. **AC2 – Sequential fallback for oversized batches:**
   Given the total heap data exceeds `budget_bytes`,
   When extraction runs,
   Then segments are grouped into sequential batches where
   each batch's cumulative payload fits within
   `max(budget_bytes, 64 MB)` (same floor as AC1).
   Batches are sequential relative to
   each other but each batch is still processed in
   parallel via `par_iter` — slower than one big batch
   but memory-safe.

3. **AC3 – Auto-budget when not set:**
   Given the user does not pass `--memory-limit`,
   When a dump is loaded,
   Then the system auto-calculates a budget (50% of
   available RAM) and uses it as the batch limit.
   Batching is always active in production — there is
   no "unbounded" mode when going through the Engine.

4. **AC4 – No regression for small dumps:**
   Given a dump whose total heap data fits within
   `budget_bytes`,
   When extraction runs,
   Then all segments are processed in a single parallel
   batch (identical to current behavior, no performance
   regression).

5. **AC5 – Progress reporting unchanged:**
   Given batched extraction with N batches,
   When segments complete,
   Then `segment_completed` reports against
   `total_segments = ranges.len()` (the real segment
   count), not the batch count. Progress is cumulative
   across batches.

6. **AC6 – budget_bytes plumbing end-to-end:**
   Given `EngineConfig.budget_bytes` is set,
   When `Engine::from_file_with_progress` is called,
   Then `budget_bytes` flows through
   `HprofFile::from_path_with_progress` →
   `run_first_pass` → `FirstPassContext` → `extract_all`.

**FRs covered:** FR59
**NFRs verified:** NFR12

## Tasks / Subtasks

- [x] Task 1: Add `budget_bytes` parameter plumbing
      (AC: #6)
  - [x] 1.1 Add `budget_bytes: Option<u64>` parameter to
        `run_first_pass` signature (public API).
  - [x] 1.2 Store `budget_bytes` in `FirstPassContext` as
        a new field `budget_bytes: Option<u64>`.
  - [x] 1.3 Add `budget_bytes: Option<u64>` to
        `HprofFile::from_path_with_progress` only.
        Pass through to `run_first_pass`.
        `HprofFile::from_path` stays unchanged — it
        calls `from_path_with_progress` with `None`
        internally (convenience API, no budget).
  - [x] 1.4 In `Engine::from_file` and
        `Engine::from_file_with_progress`, pass
        `Some(config.effective_budget())` to
        `HprofFile::from_path_with_progress`.
        **CRITICAL:** `Engine::from_file` currently
        calls `HprofFile::from_path` (line 231) which
        passes `None`. Change it to call
        `from_path_with_progress` with a
        `NullProgressObserver` and the budget. Otherwise
        the budget is silently lost.
  - [x] 1.5 Update all other callers of `run_first_pass`
        to pass `None`: test helpers (`run_fp`,
        `run_fp_with_test_observer`), the benchmark
        (`benches/first_pass.rs`).
  - [x] 1.6 Update `open_hprof_file_with_progress` and
        `open_hprof_file` in `hprof-engine/src/lib.rs`
        to pass `None` for `budget_bytes`.

- [x] Task 2: Budget-aware batching in `extract_all`
      (AC: #1, #2, #3, #4, #5)
  - [x] 2.1 In the parallel path of `extract_all`,
        replace the current fixed `batch_size =
        rayon::current_num_threads()` with budget-aware
        grouping via `compute_batch_ranges()`.
        `BATCH_FLOOR = 64 MB` prevents degenerate
        micro-batching.
  - [x] 2.2 A single segment that exceeds
        `max_batch_payload` on its own is processed alone
        in its own batch (never skipped).
  - [x] 2.3 If `budget_bytes` is `None` (Unlimited),
        `max_batch_payload = u64::MAX` so all segments
        go in one batch.
  - [x] 2.4 Preserve cumulative `segments_done` counter
        across batches. Each `segment_completed` call
        reports against the original
        `total_segments = ranges.len()`.
  - [x] 2.5 Sequential path (`< PARALLEL_THRESHOLD`)
        unchanged — already memory-safe.
  - [x] 2.6 `tracing::info!` log at each batch flush
        (gated behind `dev-profiling` feature).

- [x] Task 3: Tests (AC: #1, #2, #3, #4, #5, #6)
  - [x] 3.1a Integration test — progress events
        cumulative: `budget_batching_progress_events_cumulative`
  - [x] 3.1b Integration test — results unchanged:
        `budget_batching_results_identical`
  - [x] 3.2 Regression test: `budget_none_regression`
  - [x] 3.3 Integration test: single oversized segment:
        `budget_single_oversized_segment_processed`
  - [x] 3.4 Regression test: `hprof_file_from_path_no_budget_compiles`
  - [x] 3.5 Edge case: `budget_zero_floor_kicks_in`
  - [x] 3.6 Edge case: `budget_all_segments_exceed_individually`
        + `budget_all_oversized_extraction_succeeds`
  - [x] 3.7 E2E plumbing: `budget_e2e_through_hprof_file`
  - [x] Unit tests for `compute_batch_ranges`: 8 tests
        covering empty, single batch, split, oversized,
        all oversized, exact fit, unlimited, realistic.

## Dev Notes

### Context from Epic 10

This is story 10.3 in the "Large Dump Loading" epic.
Stories 10.1 (progress fidelity) and 10.2 (chunked
segment extraction) are ready-for-dev but NOT yet
implemented.

**Key dependency:** Story 10.2 adds `budget_bytes`
parameter plumbing to `run_first_pass` and
`HprofFile::from_path*`. If 10.2 is implemented first,
Task 1 of this story is already done — reuse its
plumbing. If 10.3 is implemented first, Task 1 must
add the plumbing, and 10.2 will reuse it.

**Recommendation:** Implement 10.2 before 10.3. But if
implementing 10.3 standalone, Task 1 is self-contained.

**If 10.2 is already implemented:** `extract_heap_segment`
returns `HeapSegmentParsingResult` — use `merge_into(ctx)`
instead of `merge_segment_result(ctx, seg)` in the
batching loop. Check the return type at dev time.

### Root Cause

`extract_all` currently batches by thread count:
```rust
let batch_size = rayon::current_num_threads().max(1);
for batch in ranges.chunks(batch_size) {
    let batch_results: Vec<HeapSegmentResult> = batch
        .par_iter()
        .map(|r| { /* extract */ })
        .collect();
    // merge all at once
}
```

This means on a 70 GB dump with 29 segments, 8 threads
= ~4 batches of ~7 segments each. If segments are
large (4 GB each), a single batch processes 7 × 4 GB =
28 GB of payload simultaneously. Each segment's
`extract_heap_segment` pre-allocates vectors proportional
to payload size. Combined pre-allocation can reach
20+ GB — exceeding available RAM.

### Design: Payload-Aware Batching

Replace the fixed `chunks(batch_size)` with dynamic
grouping based on cumulative payload size:

```
Segments: [4GB, 4GB, 4GB, 2GB, 2GB, 1GB, 1GB, ...]
budget_bytes = 8GB

Batch 1: [4GB, 4GB]          → 8GB ≤ budget ✓
Batch 2: [4GB, 2GB, 2GB]     → 8GB ≤ budget ✓
Batch 3: [1GB, 1GB, ...]     → fits ✓
```

Each batch is processed via `par_iter` (unchanged
parallelism model). Batches are sequential relative to
each other. Segment order within and across batches
matches file order (`heap_record_ranges`) — no sorting
or reordering.

**Edge case:** A single 15 GB segment with budget = 8 GB.
The segment exceeds the budget on its own → processed
solo in batch of 1. This is the correct behavior: we
can't avoid allocating for its extraction (that's story
10.2's job — chunking intra-segment allocation).

### Interaction with Story 10.2

- **10.2** bounds **intra-segment** allocation via
  chunked extraction (per-thread working set).
- **10.3** (this) bounds **inter-segment** batch size
  (total payload processed simultaneously).

Both use `budget_bytes` but at different levels. When
both are implemented:
- `extract_all` groups segments into budget-sized batches
- Within each batch, `extract_heap_segment` chunks its
  output to bound per-thread allocation

Combined effect: total in-flight memory is bounded at
both levels.

**Important limitation:** Story 10.3 alone does NOT
protect against a single giant segment (e.g. one 15 GB
`HEAP_DUMP` record). In that case there is only one
segment → one batch → no batching effect. The intra-
segment protection from story 10.2 (chunked extraction)
is the necessary complement for single-segment dumps.

### Key Files to Modify

| File | Purpose |
|------|---------|
| `crates/hprof-parser/src/indexer/first_pass/mod.rs` | Add `budget_bytes` to `FirstPassContext` and `run_first_pass` |
| `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs` | Budget-aware batching in `extract_all` |
| `crates/hprof-parser/src/indexer/first_pass/tests.rs` | New tests |
| `crates/hprof-parser/src/hprof_file.rs` | Add `budget_bytes` param to `from_path_with_progress` only (`from_path` unchanged) |
| `crates/hprof-parser/benches/first_pass.rs` | Update `run_first_pass` call to pass `None` |
| `crates/hprof-engine/src/engine_impl/mod.rs` | Pass `Some(effective_budget())` through `HprofFile` |
| `crates/hprof-engine/src/lib.rs` | Update `open_hprof_file*` to pass `None` |

### Current `extract_all` Implementation

Located at `heap_extraction.rs:257-305`. The parallel
path uses `ranges.chunks(batch_size)` where
`batch_size = rayon::current_num_threads()`. This is a
count-based split, not payload-aware.

The sequential path (< 32 MB total heap) processes one
segment at a time — already memory-safe. No changes
needed there.

### Progress Reporting

`segments_done` counter must be cumulative across
batches. Current code already does this (incremented
per segment, reported against `total_segments`). The
only change is that batch boundaries shift from
count-based to payload-based.

### Existing Patterns from Stories 10.1 and 10.2

- `run_fp_with_test_observer` for progress assertions
- Binary blob construction via `HprofTestBuilder`
- `TestObserver` captures `segment_completed` calls
- Multiple heap segments created by adding multiple
  `HEAP_DUMP_SEGMENT` records in the test builder

### Merge Phase Memory

Batching reduces **peak** memory during extraction
(fewer segments allocated simultaneously). However,
after all batches complete, `ctx.all_offsets` and
`ctx.seg_builder` hold the same total data regardless
of batch count. The final accumulated size is
unchanged — batching does not reduce total memory,
only the peak during extraction. Story 10.4
investigates this post-extraction accumulation.

### Scope Boundaries

This story covers **only** payload-aware batching in
`extract_all`. Out of scope:
- **Chunked intra-segment extraction** (story 10.2)
- **Post-extraction RAM spike** (story 10.4)
- **Skip-index or batch-scan optimizations** (epic 11)

### Architecture Decision Records

**ADR-1: Payload-Aware vs Count-Based Batching**
Decision: Group segments by cumulative payload size
against `budget_bytes`.
Rejected: Keep `chunks(batch_size)` with smaller batch
count — doesn't account for heterogeneous segment sizes
(one 15 GB segment vs twenty 500 MB segments).
Rationale: Payload-aware batching directly controls
memory usage regardless of segment size distribution.

**ADR-2: Solo Batch for Oversized Segments**
Decision: A segment exceeding `budget_bytes` on its own
is processed in a solo batch of 1.
Rejected: Skip the segment — not an option (would lose
data).
Rejected: Split the segment — that's story 10.2's
responsibility (chunked extraction).
Rationale: This story controls inter-segment scheduling.
Intra-segment memory is 10.2's domain.

**ADR-3: 64 MB Floor on `max_batch_payload`**
Decision: `max_batch_payload = budget_bytes.max(64 MB)`.
Rejected: No floor — `budget_bytes = Some(0)` or very
small values cause every segment to be a solo batch
(zero parallelism, hundreds of sequential batches).
Rationale: Consistent with story 10.2's `CHUNK_FLOOR`.
Guards against degenerate inputs without silently
ignoring the user's budget intent.

**ADR-4: Budget Compared to Raw Payload (Not Estimated
Memory)**
Decision: `max_batch_payload` compares cumulative
segment `payload_length` against `budget_bytes`.
Rejected: Compare against estimated memory cost —
more precise but adds complexity and couples batching
logic to allocation heuristics that may change.
Rationale: Raw payload is a reasonable proxy for real
memory cost. The actual cost factor depends on dump
composition:
- `InstanceDump`/`PrimArrayDump`: contribute to both
  `all_offsets` AND `filter_ids` → ~0.8× payload
- `ObjectArrayDump`: contributes to `filter_ids` only
  (no `all_offsets` push) → ~0.4× payload
For mixed dumps the factor is between 0.4× and 0.8×.
Comparing raw payload to budget is therefore
conservative but safe — actual memory cost is always
less than payload. If profiling shows excessive
sequential batching in practice, a future story can
switch to estimated-cost comparison.

### Silent `None` Propagation Risk

`Option<u64>` accepts `None` silently at every plumbing
point. If any link forgets to forward `budget_bytes`,
batching is disabled without error. Test 3.7 mitigates
this by verifying e2e through the Engine.

### Project Structure Notes

- All logic changes in `heap_extraction.rs` (batching)
  and `mod.rs` (parameter plumbing)
- No new modules, no new external dependencies
- Public API change: `run_first_pass` gains a parameter
- `HprofFile::from_path_with_progress` gains a parameter
- No behavioral change without explicit `budget_bytes`

### References

- [Source: docs/report/large-dump-ux-observations-2026-03-14.md#L2]
- [Source: docs/planning-artifacts/epics.md#Story-10.3]
- [Source: crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs:257-305 — extract_all]
- [Source: crates/hprof-parser/src/indexer/first_pass/mod.rs:80-94 — FirstPassContext]
- [Source: crates/hprof-parser/src/indexer/first_pass/mod.rs:186-205 — run_first_pass]
- [Source: crates/hprof-engine/src/lib.rs:56-71 — EngineConfig, effective_budget]
- [Source: crates/hprof-parser/src/hprof_file.rs:81-122 — from_path, from_path_with_progress]
- [Source: crates/hprof-engine/src/engine_impl/mod.rs:229-271 — Engine::from_file, from_file_with_progress]
- [Source: crates/hprof-engine/src/lib.rs:100-128 — open_hprof_file, open_hprof_file_with_progress]
- [Source: docs/implementation-artifacts/10-2-split-oversized-heap-segments.md — story 10.2 (shared plumbing)]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Already complete from story 10.2. `MemoryBudget`
  type (not `Option<u64>`) used throughout the chain.
- Task 2: Extracted `compute_batch_ranges()` for
  payload-aware grouping. Replaced count-based
  `ranges.chunks(batch_size)` with budget-aware batches.
  `BATCH_FLOOR = 64 MB` prevents micro-batching.
  Sequential path unchanged. `tracing::info!` gated
  behind `dev-profiling`.
- Task 3: 17 tests added — 8 unit tests for
  `compute_batch_ranges` algorithm + 9 integration tests
  covering all ACs (progress, correctness, regression,
  oversized, floor, e2e plumbing).

### Change Log

- 2026-03-15: Implemented budget-aware batching in
  `extract_all` (Tasks 2-3).
- 2026-03-15: Code review fixes — parallel path test,
  Engine E2E test, stronger result assertions, test
  rename.

### File List

- `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs` (modified)
- `crates/hprof-parser/src/indexer/first_pass/tests.rs` (modified)
- `crates/hprof-engine/src/engine_impl/tests.rs` (modified)
