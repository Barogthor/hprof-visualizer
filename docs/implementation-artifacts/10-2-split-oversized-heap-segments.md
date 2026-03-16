# Story 10.2: Chunked Heap Segment Extraction

Status: done

## Story

As a user,
I want the system to extract heap segments in bounded
memory chunks instead of accumulating full-segment
results,
so that large segments (up to ~4 GB each, the hprof
`u32` limit) do not cause excessive RAM from
pre-allocated vectors when multiple are processed in
parallel.

## Acceptance Criteria

1. **AC1 – Chunked extraction when oversized:**
   Given a heap segment with `payload_length` exceeding
   `max_chunk_bytes` (default:
   `max(budget_bytes / num_threads, 64 MB)`),
   When the system extracts it,
   Then extraction yields partial `HeapSegmentResult`
   chunks every `max_chunk_bytes` of processed data,
   each with bounded pre-allocation.

2. **AC2 – Natural sub-record boundary alignment:**
   Given the extraction loop processes sub-records
   sequentially,
   When a chunk boundary is reached,
   Then the flush occurs between two complete sub-records
   (never mid-record), guaranteed by the loop structure.

3. **AC3 – Identical combined results:**
   Given a segment extracted in N chunks,
   When all chunks are merged,
   Then the combined results are identical to extracting
   the segment as a whole (no missing objects, no
   duplicates, insertion order within `all_offsets` and
   `filter_ids` preserved **within that segment's chunks**
   — the final sort by `object_id` in `run_first_pass`
   produces the same globally-sorted result regardless
   of cross-segment ordering from parallel execution).

4. **AC4 – No-op for small segments:**
   Given a heap dump with all segments under
   `max_chunk_bytes`,
   When extracted,
   Then behavior is unchanged (single chunk = full
   segment, no overhead).

5. **AC5 – Stable API via `HeapSegmentParsingResult`:**
   Given a heap segment extracted with or without chunking,
   When `extract_heap_segment` returns,
   Then the return type is `HeapSegmentParsingResult` in
   both cases. The caller merges via
   `parsing_result.merge_into(ctx)` without knowing
   whether 1 or N chunks were produced internally.

**FRs covered:** FR58
**NFRs verified:** NFR12

## Tasks / Subtasks

- [x] Task 1: Add `budget_bytes` parameter plumbing
      (AC: #1)
  - [x] 1.1 Add `budget_bytes: Option<u64>` parameter to
        `run_first_pass` signature. Default `None` means no
        chunking (backward compatible).
  - [x] 1.2 Store `budget_bytes` in `FirstPassContext`.
  - [x] 1.3 Plumb `budget_bytes` through the full call
        chain. The actual path is:
        `Engine::from_file` → `HprofFile::from_path` →
        `HprofFile::from_path_with_progress` →
        `run_first_pass`.
        Add `budget_bytes: Option<u64>` to
        `HprofFile::from_path_with_progress` and
        `HprofFile::from_path` signatures.
        `EngineConfig.effective_budget()` provides the
        value at the engine level.
        **Verify ALL call sites** — the compiler accepts
        `None` silently, so a forgotten plumbing produces
        no chunking without any error. Call sites:
        `engine_impl/mod.rs` (Engine), `hprof_file.rs`
        (HprofFile constructors),
        `open_hprof_file_with_progress` (pass `None`),
        `open_hprof_file` (pass `None`).
  - [x] 1.4 Update all existing callers of `run_first_pass`
        to pass `None` for `budget_bytes`:
        test helpers (`run_fp`,
        `run_fp_with_test_observer`), **and** the
        benchmark (`benches/first_pass.rs:59`).

- [x] Task 2: Add `HeapSegmentParsingResult` wrapper type
      (AC: #5)
  - [x] 2.1 Add `HeapSegmentParsingResult` and
        `HeapSegmentResult::is_empty()` in
        `heap_extraction.rs`:
        ```rust
        impl HeapSegmentResult {
            fn is_empty(&self) -> bool {
                self.all_offsets.is_empty()
                    && self.filter_ids.is_empty()
                    && self.class_dumps.is_empty()
                    && self.raw_frame_roots.is_empty()
                    && self.raw_thread_objects.is_empty()
                    && self.warnings.is_empty()
            }
        }

        pub(super) struct HeapSegmentParsingResult {
            chunks: Vec<HeapSegmentResult>,
        }

        impl HeapSegmentParsingResult {
            pub(super) fn new(
                chunks: Vec<HeapSegmentResult>,
            ) -> Self {
                Self { chunks }
            }
            pub(super) fn merge_into(
                self,
                ctx: &mut FirstPassContext,
            ) {
                for chunk in self.chunks {
                    merge_segment_result(ctx, chunk);
                }
            }
        }
        ```
  - [x] 2.2 Add `HeapSegmentResult::new_with_capacity(est)`
        constructor. The existing `extract_heap_segment`
        pre-allocates with `payload.len() / 40` — the
        chunked version uses `max_chunk_bytes / 40` instead.
        This is the core RAM fix.
        ```rust
        pub(super) fn new_with_capacity(est: usize) -> Self {
            Self {
                all_offsets: Vec::with_capacity(est),
                filter_ids: Vec::with_capacity(est),
                raw_frame_roots: Vec::new(),
                raw_thread_objects: Vec::new(),
                class_dumps: Vec::new(),
                warnings: Vec::new(),
            }
        }
        ```
        Only `all_offsets` and `filter_ids` get a capacity
        hint (they are the dominant vectors). The others
        use `Vec::new()` — identical to the current struct
        literal.

- [x] Task 3: Replace `extract_heap_segment` with chunked
      extraction (AC: #1, #2, #3, #5)
  - [x] 3.1 Update `extract_heap_segment` signature
        in-place (new parameter, new return type):
        ```rust
        pub(super) fn extract_heap_segment(
            payload: &[u8],
            data_offset: usize,
            id_size: u32,
            max_chunk_bytes: usize,
        ) -> HeapSegmentParsingResult
        ```
        **CRITICAL: The extraction loop exists ONLY in
        this function.** No second copy, no wrapper. The
        old `extract_heap_segment` is replaced in-place
        — not preserved alongside.
  - [x] 3.2 Implementation: replace the body of
        `extract_heap_segment` with the chunked version.
        Initialise bookkeeping variables before the loop,
        then add a checkpoint flush **after the
        `if !ok { break; }` guard** — this ensures only
        successfully parsed sub-records trigger a flush:
        ```rust
        let mut chunks: Vec<HeapSegmentResult> = Vec::new();
        let mut current_result =
            HeapSegmentResult::new_with_capacity(
                max_chunk_bytes / 40,
            );
        let mut next_checkpoint = max_chunk_bytes;

        while let Ok(raw) = cursor.read_u8() {
            let ok = match sub_tag {
                // ... existing sub-record parsing ...
            };

            if !ok {
                break;
            }

            // Checkpoint AFTER ok check: cursor is at a
            // valid sub-record boundary, current sub-record
            // is confirmed good.
            if cursor.position() as usize >= next_checkpoint {
                chunks.push(current_result);
                current_result =
                    HeapSegmentResult::new_with_capacity(
                        max_chunk_bytes / 40,
                    );
                next_checkpoint += max_chunk_bytes;
            }
        }
        // Guard: only push final chunk if non-empty
        if !current_result.is_empty() {
            chunks.push(current_result);
        }
        HeapSegmentParsingResult::new(chunks)
        ```
        The cursor naturally lands on sub-record boundaries
        after each iteration — no boundary finding needed.
        **No-panic invariant:** The loop must never panic —
        only `break` on malformed data, exactly like the
        current code. This ensures rayon threads never
        abort mid-extraction.
  - [x] 3.3 If `max_chunk_bytes >= payload.len()`, the
        function produces a single chunk (no-op, identical
        to current behavior).

- [x] Task 4: Integrate into `extract_all` (AC: #1, #3,
      #4, #5)
  - [x] 4.1 In `extract_all`, read `ctx.budget_bytes`
        and compute `max_chunk_bytes`. Use a saturating
        cast (`usize::try_from(b).unwrap_or(usize::MAX)`)
        to avoid truncation on hypothetical 32-bit targets.
        The 64 MB floor also guards against
        `budget_bytes = Some(0)`:
        ```rust
        const CHUNK_FLOOR: usize = 64 * 1024 * 1024;

        let max_chunk_bytes = ctx.budget_bytes
            .map(|b| {
                let b = usize::try_from(b)
                    .unwrap_or(usize::MAX);
                let per_thread = b
                    / rayon::current_num_threads()
                        .max(1);
                per_thread.max(CHUNK_FLOOR)
            })
            .unwrap_or(usize::MAX);
        // None → usize::MAX → single-chunk no-op
        ```
  - [x] 4.2 Update all `extract_heap_segment` calls in
        `extract_all` to pass `max_chunk_bytes`. Both
        parallel and sequential paths now call the same
        function (no branching on chunked vs non-chunked).
        Each thread returns `HeapSegmentParsingResult`.
  - [x] 4.3 In both parallel and sequential paths, replace
        `merge_segment_result(ctx, seg_result)` with
        `parsing_result.merge_into(ctx)`. The parallel
        loop becomes:
        ```rust
        let batch_results: Vec<HeapSegmentParsingResult> =
            batch.par_iter().map(|r| {
                let start = r.payload_start as usize;
                let end = start + r.payload_length as usize;
                extract_heap_segment(
                    &data[start..end], start,
                    id_size, max_chunk_bytes,
                )
            }).collect();

        for parsing_result in batch_results {
            parsing_result.merge_into(ctx);
            segments_done += 1;
            notifier.segment_completed(
                segments_done, total_segments,
            );
        }
        ```
        The sequential loop follows the same pattern
        (single call per segment, then `merge_into`).
  - [x] 4.4 Progress reporting: preserve the existing
        merge-then-report ordering (`merge_into` first,
        `segment_completed` second — same as current
        code, just with `merge_into` replacing
        `merge_segment_result`). Keep
        `total_segments = ranges.len()` unchanged.
        Chunking is an internal optimization invisible
        to the user.

- [x] Task 5: Tests (AC: #1, #2, #3, #4, #5)
  - [x] 5.1 Unit test: `extract_heap_segment` with
        a payload containing 6 InstanceDump records.
        `max_chunk_bytes` set so each chunk holds ~2
        records. Assert 3 chunks produced, each with 2
        `all_offsets` entries.
  - [x] 5.2 Unit test: same payload, `max_chunk_bytes`
        larger than total payload. Assert 1 chunk produced
        (no-op). Compare results value-by-value with
        a single-chunk extraction.
  - [x] 5.3 Unit test: chunk boundary falls exactly at a
        sub-record end. Assert no off-by-one (the record
        goes into the current chunk, not the next).
  - [x] 5.4 Integration test: construct a payload with
        mixed sub-records (InstanceDump + PrimArrayDump +
        ClassDump + GcRootJavaFrame). Extract as single
        chunk and as multiple chunks. Merge the chunks.
        Assert combined `all_offsets`, `filter_ids`,
        `class_dumps`, `raw_frame_roots` are identical
        **value by value** after sorting.
  - [x] 5.5 Integration test: full plumbing through
        `run_first_pass` with `budget_bytes = Some(512)`
        and a synthetic heap segment containing at least
        10 InstanceDump records (total payload > 512 bytes).
        Assert extraction succeeds and the merged
        `all_offsets` and `filter_ids` are value-for-value
        identical to the result with `budget_bytes = None`.
        This verifies `budget_bytes` flows correctly from
        `run_first_pass` → `FirstPassContext` →
        `extract_all` → `max_chunk_bytes`.
  - [x] 5.6 Regression test: `run_first_pass` with
        `budget_bytes = None`. Assert identical behavior
        to current code (no chunking, single chunk per
        segment).
  - [x] 5.7 Unit test: payload with a truncated sub-record
        at the end (simulates malformed data). Assert
        chunked extraction handles it the same way as
        the non-chunked path (breaks out of loop, returns
        partial results).
  - [x] 5.8 Unit test: verify `data_offset` correctness
        across chunks — object offsets in chunk N must be
        absolute (relative to mmap, not to chunk start).
        Use `InstanceDump` and `PrimArrayDump` sub-records
        (both produce `ObjectOffset` entries in
        `all_offsets`). `ObjectArrayDump` produces only
        `FilterEntry` entries, not `ObjectOffset` — do not
        use it as the sole sub-record type here.
        Construct a payload that spans 2+ chunks, pass a
        non-zero `data_offset`, and assert that every
        `ObjectOffset::offset` and every
        `FilterEntry::data_offset` equals
        `data_offset + cursor_position_at_record_start`.
  - [x] 5.9 Unit test: single-chunk
        `HeapSegmentParsingResult`. `merge_into` produces
        same ctx state as direct `merge_segment_result`
        call. Validates the wrapper is transparent.
  - [x] 5.10 Unit test: empty payload (0 bytes). Assert
        `extract_heap_segment` returns a result with zero
        chunks. `merge_into` is a no-op (no crash, no
        entries added to ctx).

## Dev Notes

### Root Cause

`extract_heap_segment` receives the full segment payload
as `&data[start..end]`. The hprof format uses a `u32`
payload length, so a single segment is capped at ~4 GB.
However, `Vec::with_capacity(payload.len() / 40)`
pre-allocates based on the full payload size. For a 4 GB
segment, this is ~100M entries x 16 bytes = ~1.6 GB just
for `all_offsets`, plus similar for `filter_ids`.

When rayon processes multiple 4 GB segments in parallel
(e.g. 8 threads x 1.6 GB per vector x 2 vectors), the
combined pre-allocation can reach 20+ GB — matching the
observed L2 RAM spike on the 70 GB dump.

### Design: Streaming Chunked Extraction

Instead of pre-computing split points and slicing the
payload, the extraction loop itself **yields partial
results** every `max_chunk_bytes`. This is the simplest
and safest approach:

**Why not pre-computed split points?**
- Boundary finding requires scanning from offset 0 of
  the payload (arbitrary bytes can look like sub-tags).
  This is O(n) per segment — the same cost as extraction.
- The extraction loop already advances sub-record by
  sub-record. Adding a checkpoint flush is ~5 lines.
- Zero risk of corruption — the cursor naturally sits
  on sub-record boundaries between iterations.

**How it works:**
```
Thread 1: segment A (4 GB)
  → extract sub-records sequentially
  → every 500 MB: flush HeapSegmentResult into Vec
  → return Vec<HeapSegmentResult> with ~8 small chunks

Thread 2: segment B (4 GB)
  → same, in parallel

After par_iter collect:
  → merge all chunks sequentially into ctx
```

**RAM profile per thread:**
- Before: `with_capacity(4GB / 40)` = 100M entries =
  ~1.6 GB per vector
- After: `with_capacity(500MB / 40)` = 12.5M entries =
  ~200 MB per vector
- 8 threads: 8 x 200 MB = 1.6 GB total (vs 12.8 GB)

### Parallelism Model

- **Inter-segment:** rayon `par_iter` across segments
  (unchanged)
- **Intra-segment:** sequential (unchanged — one thread
  per segment)
- **Chunk merge:** sequential after `par_iter` collect
  (unchanged pattern, just more items to merge)

No new parallelism model. The only change is that each
thread's return type becomes `HeapSegmentParsingResult`,
which wraps 1 or N `HeapSegmentResult` chunks internally.
The caller uses `merge_into(ctx)` regardless.

### Key Difference from Story 10.3

- **Story 10.2 (this):** Bounds per-segment allocation
  via chunked extraction. Each thread's working set stays
  small.
- **Story 10.3:** Limits total in-flight batch size across
  all segments via sequential batching in `extract_all`.

Both use `budget_bytes` but at different levels. This
story changes intra-segment allocation; 10.3 changes
inter-segment scheduling.

**Note for 10.3 — interface contract:**
Story 10.3 will read `ctx.budget: MemoryBudget` from
`FirstPassContext` — the same field added in Task 1.2 here
(implemented as `MemoryBudget` enum, not `Option<u64>`).
`ctx.budget.bytes()` returns `Option<u64>` for callers
that need the raw byte value.
The field name and type must not be changed in 10.2 without
updating the 10.3 story spec. Story 10.3 must NOT add a
second budget field or re-plumb the parameter; it reads
the one stored by this story.

### `HeapSegmentParsingResult` — Stable Return Type

`extract_heap_segment` returns
`HeapSegmentParsingResult`. This wrapper abstracts
whether 1 or N chunks were produced:

```rust
let result = extract_heap_segment(
    payload, offset, id, max_chunk,
);
// 1 chunk if payload <= max_chunk_bytes, N otherwise
result.merge_into(ctx);
```

There is a single extraction function. When
`max_chunk_bytes >= payload.len()`, the result contains
one chunk (no-op). No branching needed in `extract_all`
— the call and merge path are always the same.

### `run_first_pass` Signature Change

**This is a breaking change to `hprof-parser`'s public API.**
All callers outside this monorepo would need to add `None`
as the last argument. Within the monorepo all call sites
are enumerated below and must be updated in Task 1.4.

Current:
```rust
pub fn run_first_pass(
    data: &[u8],
    id_size: u32,
    base_offset: u64,
    notifier: &mut ProgressNotifier,
) -> IndexResult
```

New:
```rust
pub fn run_first_pass(
    data: &[u8],
    id_size: u32,
    base_offset: u64,
    notifier: &mut ProgressNotifier,
    budget_bytes: Option<u64>,
) -> IndexResult
```

Callers: `HprofFile::from_path_with_progress` (in
`hprof_file.rs`), test helpers (`run_fp`,
`run_fp_with_test_observer`), and the benchmark
(`benches/first_pass.rs`). All non-engine callers pass
`None`.

### `data_offset` Correctness

The `data_offset` parameter to `extract_heap_segment`
is the absolute offset of the
segment payload start in the mmap. It does NOT change
between chunks because all chunks share the same payload
slice. The cursor position within the payload is added
to `data_offset` to compute absolute offsets.

This is already how `extract_heap_segment` works
(`let sub_record_start = data_offset + cursor.position()`).
No change needed — verified by test 5.8.

### Progress Reporting

Two options for chunk-level progress:
1. **Per-chunk:** Each chunk merge fires
   `segment_completed`. More granular but inflates
   segment count.
2. **Per-segment:** Report after all chunks of a segment
   are merged. Simpler, matches current UX.

**Recommendation:** Option 2 (per-segment). The current
progress bar shows segment count. Chunking is an internal
optimization invisible to the user. Keep
`total_segments = ranges.len()` unchanged.

### Key Files to Modify

| File | Purpose |
|------|---------|
| `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs` | Add `HeapSegmentParsingResult`, `HeapSegmentResult::is_empty`, `HeapSegmentResult::new_with_capacity`, replace `extract_heap_segment` with chunked version, update `extract_all` |
| `crates/hprof-parser/src/indexer/first_pass/mod.rs` | Add `budget_bytes` to `FirstPassContext`, update `run_first_pass` signature |
| `crates/hprof-parser/src/indexer/first_pass/tests.rs` | New tests for chunked extraction + integration |
| `crates/hprof-parser/src/hprof_file.rs` | Add `budget_bytes` param to `from_path_with_progress` and `from_path`, pass through to `run_first_pass` |
| `crates/hprof-parser/benches/first_pass.rs` | Update `run_first_pass` call to pass `None` |
| `crates/hprof-engine/src/engine_impl/mod.rs` | Pass `Some(config.effective_budget())` through `HprofFile` constructors |
| `crates/hprof-engine/src/lib.rs` | Update `open_hprof_file_with_progress` and `open_hprof_file` to pass `None` for `budget_bytes` |
| `crates/hprof-cli/src/main.rs` | No change — `budget_bytes` already flows to `EngineConfig` |

### Test Infrastructure

Tests use the existing binary builder pattern and
`run_fp_with_test_observer`. For chunked extraction
tests, construct payloads with known sub-records and
call `extract_heap_segment` directly with a small
`max_chunk_bytes`.

For integration tests comparing chunked vs non-chunked
results, call `extract_heap_segment` with different
`max_chunk_bytes` values on the same payload and assert
identical merged results (value by value, not just
counts).

The `test-utils` feature flag provides `TestObserver` and
`run_fp_with_test_observer`. Gate new tests with
`#[cfg(feature = "test-utils")]` if they use these.

### Existing Patterns from Story 10.1

- Binary blob construction: header + record tag + payload
- `run_fp_with_test_observer` for progress event assertions
- Full-size blob allocation to satisfy
  `payload_end <= data.len()`

### Project Structure Notes

- All changes in `heap_extraction.rs` (main logic) and
  `mod.rs` (parameter plumbing)
- No new modules, no new external dependencies
- No new functions for boundary finding — the extraction
  loop itself handles chunk boundaries naturally
- Public API change: `run_first_pass` gains a parameter
  (public fn — update benchmark and all external callers)
- `HprofFile::from_path` and `from_path_with_progress`
  gain a `budget_bytes` parameter

### Architecture Decision Records

**ADR-1: Streaming Chunked vs Pre-Computed Split Points**
Decision: Streaming chunked (flush between loop iterations).
Rejected alternatives:
- Pre-computed split: O(n) boundary scan from offset 0,
  complex and fragile (false sub-tags if not from 0),
  requires fallback for malformed data.
- First-pass survol: Slows first pass (currently skips
  95% of file), degrades progress bar UX fixed in 10.1.
- Cap on `with_capacity`: Reduces initial peak but not
  final peak (vectors grow regardless).
Rationale: ~5 lines added to existing loop, zero
corruption risk (cursor naturally on boundaries), no
new boundary-finding functions.

**ADR-2: `HeapSegmentParsingResult` Wrapper Type**
Decision: Unified return type wrapping
`Vec<HeapSegmentResult>` with a single `new(chunks)`
constructor and `merge_into(ctx)` method.
Rejected: Returning `Vec<HeapSegmentResult>` directly
forces caller to know chunked vs non-chunked.
Rejected: Separate `single()`/`chunked()` constructors
— semantically identical (both produce a Vec), adds
API surface without value.
Rationale: `merge_into(ctx)` abstracts multiplicity.
Caller never branches on chunk count.

**ADR-3: 64 MB Floor on `max_chunk_bytes`**
Decision: `max(budget / threads, 64 MB)`.
Rejected: No floor — `--memory-limit 256M` with 16
threads = 16 MB chunks = hundreds of flushes per segment.
Rationale: 64 MB bounds RAM effectively while avoiding
micro-chunking overhead. The floor also guards against
degenerate `budget_bytes = Some(0)` (0 / N = 0, floored
to 64 MB).

**ADR-4: Progress Reporting Per-Segment (Not Per-Chunk)**
Decision: `segment_completed` after all chunks of a
segment are merged. `total_segments = ranges.len()`.
Rejected: Per-chunk reporting inflates segment counter.
Rationale: Chunking is an internal optimization
invisible to the user. Progress bar stays consistent
with actual hprof segment count.

### Scope Boundaries

This story covers **only** chunked extraction to bound
per-segment allocation. Out of scope:
- **Batch-level memory guard** (story 10.3)
- **Post-extraction RAM spike** (story 10.4)
- **Progress reporting during extraction** (already works
  via `segment_completed`)

### References

- [Source: docs/report/large-dump-ux-observations-2026-03-14.md#L2]
- [Source: docs/planning-artifacts/epics.md#Story-10.2]
- [Source: crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs — extract_all, extract_heap_segment]
- [Source: crates/hprof-parser/src/indexer/first_pass/mod.rs:80-94 — FirstPassContext]
- [Source: crates/hprof-parser/src/indexer/first_pass/mod.rs:186-205 — run_first_pass]
- [Source: crates/hprof-engine/src/lib.rs:56-71 — EngineConfig, effective_budget]
- [Source: crates/hprof-parser/src/indexer/mod.rs:21-27 — HeapRecordRange]
- [Source: docs/implementation-artifacts/10-1-progress-fidelity-heap-segment-scan.md — previous story patterns]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Added `budget: MemoryBudget` parameter through full call chain: `run_first_pass` → `FirstPassContext` → `HprofFile::from_path_with_progress`. **Design deviation from spec:** Instead of `budget_bytes: Option<u64>`, a `MemoryBudget` enum was introduced in `hprof-api` (`Unlimited` | `Bytes(u64)`) for better type safety and clarity. `HprofFile::from_path` was kept as a zero-arg convenience wrapper (always `Unlimited`, no parameter added). Engine passes `config.memory_budget()` which wraps `effective_budget()` as `Bytes(n)`; all other callers pass `Unlimited`.
- Task 2: Added `HeapSegmentResult::is_empty()`, `HeapSegmentResult::new_with_capacity(est)`, and `HeapSegmentParsingResult` wrapper with `new(chunks)` and `merge_into(ctx)`.
- Task 3: Replaced `extract_heap_segment` body with chunked version. New signature adds `max_chunk_bytes: usize`. Checkpoint flush after each complete sub-record when `cursor.position() >= next_checkpoint`. Returns `HeapSegmentParsingResult`.
- Task 4: Updated `extract_all` to compute `max_chunk_bytes = max(budget/threads, 64MB)`. Both parallel and sequential paths use `HeapSegmentParsingResult::merge_into`. Progress reporting unchanged (per-segment, not per-chunk).
- Task 5: 10 unit/integration tests covering: multi-chunk extraction, no-op single-chunk, exact boundary alignment, mixed sub-record types, full plumbing through `run_first_pass`, regression with `None`, truncated data, `data_offset` correctness across chunks, single-chunk merge equivalence, empty payload.

### File List

- `crates/hprof-api/src/budget.rs` — new `MemoryBudget` enum (`Unlimited` / `Bytes(u64)`)
- `crates/hprof-api/src/lib.rs` — re-exports `MemoryBudget`
- `crates/hprof-parser/src/indexer/first_pass/mod.rs` — `budget: MemoryBudget` in `FirstPassContext`, `run_first_pass` signature
- `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs` — `HeapSegmentResult::is_empty`, `new_with_capacity`, `HeapSegmentParsingResult`, chunked `extract_heap_segment`, updated `extract_all`
- `crates/hprof-parser/src/indexer/first_pass/tests.rs` — 10 new chunked extraction tests, updated test helpers and existing callers
- `crates/hprof-parser/src/hprof_file.rs` — `budget_bytes` param on `from_path_with_progress` and `from_path`
- `crates/hprof-parser/benches/first_pass.rs` — updated `run_first_pass` call to pass `None`
- `crates/hprof-engine/src/engine_impl/mod.rs` — `Engine::from_file` and `from_file_with_progress` pass `Some(effective_budget())`
- `crates/hprof-engine/src/lib.rs` — `open_hprof_file_with_progress` passes `None`
