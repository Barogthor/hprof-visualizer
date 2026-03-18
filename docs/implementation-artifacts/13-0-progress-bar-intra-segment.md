# Story 13.0: Progress Bar Per-Segment

Status: done

## Story

As a user loading a large heap dump,
I want the progress bar to update smoothly during
heap segment extraction (not just between segments),
so that the UI never appears frozen and I can monitor
real-time loading progress throughout the entire
first-pass scan.

## Background

Known since Epic 8, deferred across 5 epics (8, 10,
11, 12). The progress bar freezes during parallel heap
extraction because `par_iter().collect()` blocks until
an entire batch completes. For a 70 GB dump with ~29
segments, some batches contain segments totaling
hundreds of MB — the bar stalls for tens of seconds.

Root cause: `extract_all()` only calls
`notifier.segment_completed(done, total)` after each
segment result is merged — no progress during
extraction.

The solution is **per-segment via channel**, which is
sufficient post-story-10.2 (segments split to ~64 MB,
completing in ~16-32 ms each). The sprint-status key
`intra-segment` is a historical artifact — update it
to `per-segment` when closing this story.

## Acceptance Criteria

1. **Given** a heap dump with multiple segments
   **When** parallel heap extraction runs **Then**
   the progress bar updates as each segment
   completes (not only after the entire batch
   finishes via `collect()`).

2. **Given** the parallel extraction path (>= 32 MB
   total heap, `num_threads > 1`) **When** workers
   finish segments at different speeds **Then** the
   main thread drains completed segments from a
   `mpsc` channel concurrently inside `rayon::scope`,
   reporting progress without waiting for slower
   workers.

3. **Given** the sequential extraction path **When**
   segments are extracted one by one **Then**
   bytes-extracted progress updates after each
   segment completes.

4. **Given** the CLI progress display **When** heap
   extraction is active **Then** a bytes-based
   progress bar shows `bytes_extracted/total_heap_bytes`
   with throughput and ETA (replacing the current
   segment-count bar).

5. **Given** the `ParseProgressObserver` trait **When**
   a new method is added **Then** it has a default
   no-op implementation so existing callers
   (`NullProgressObserver`, `TestObserver`) don't
   break.

6. **Given** existing tests **When** running the full
   test suite after changes **Then** all tests pass
   with zero regressions.

## Tasks / Subtasks

- [x] Task 1: Extend `ParseProgressObserver` trait (AC: #5)
  - [x] 1.1 Add `on_heap_bytes_extracted(&mut self, done: u64, total: u64)` with default no-op. Uses `u64` (not `usize` like other trait methods) because byte counts can exceed 4 GB on 32-bit targets. Existing methods use `usize` for counts (segments, names) which are always small.
  - [x] 1.2 Add corresponding `heap_bytes_extracted()` to `ProgressNotifier` — include `debug_assert!(done <= total)`
  - [x] 1.3 Add `HeapBytesExtracted { done, total }` variant to `ProgressEvent` (test-utils)
  - [x] 1.4 Implement in `TestObserver`

- [x] Task 2: `rayon::scope` + channel parallel extraction (AC: #1, #2)
  - [x] 2.1 Replace `par_iter().map().collect()` with `rayon::in_place_scope` that spawns one task per segment and a drain loop on the main thread (in_place_scope avoids Send requirement on ProgressNotifier)
  - [x] 2.2 Each spawned task sends `(payload_length, HeapSegmentParsingResult)` via `std::sync::mpsc::channel`
  - [x] 2.3 Main thread drains `rx.iter()` **concurrently** inside the scope, calling `merge_into(ctx)` and `notifier.heap_bytes_extracted(cumulative, total)` as segments arrive
  - [x] 2.4 Drop `tx` before the drain loop so `rx.iter()` terminates when all workers finish — **add explicit comment** (deadlock if forgotten)
  - [x] 2.5 Compute `total_heap_bytes` upfront (already exists at line 375)
  - [x] 2.6 Use `let _ = tx.send(...)` instead of `unwrap()` in workers (prevents double-panic if main thread panics during drain)
  - [x] 2.7 Guard: extend existing condition `if total_heap_bytes >= PARALLEL_THRESHOLD` to `if total_heap_bytes >= PARALLEL_THRESHOLD && rayon::current_num_threads() > 1` — falls to sequential path when single-threaded.

- [x] Task 3: Sequential path progress (AC: #3)
  - [x] 3.1 In `extract_all()` sequential branch, report cumulative bytes after each segment via `notifier.heap_bytes_extracted()`

- [x] Task 4: Update CLI progress bar (AC: #4)
  - [x] 4.1 Add `extraction_bar: Option<ProgressBar>` to `CliProgressObserver`
  - [x] 4.2 Implement `on_heap_bytes_extracted` — lazy-init a bytes-based bar (template: `[{elapsed_precise}] [{bar:40.green/blue}] {bytes}/{total_bytes} extracted ({bytes_per_sec}, ETA {eta})`)
  - [x] 4.3 On first call, finish `scan_bar` if not already finished (guard with `if !self.scan_bar.is_finished()`)
  - [x] 4.4 Finish extraction_bar when `done == total`
  - [x] 4.5 Remove `segment_bar` field. Keep `on_segment_completed` impl as no-op with TODO(cleanup) comment. Removed `on_phase_changed` segment_bar position check.

- [x] Task 5: Tests (AC: #6)
  - [x] 5.1 Unit test: `TestObserver` captures `HeapBytesExtracted` events (updated `test_observer_collects_all_event_types`)
  - [x] 5.2 Unit test: `NullProgressObserver` compiles with new method (default impl, covered by existing test)
  - [x] 5.3 Integration test: first-pass on multi-segment synthetic dump emits `HeapBytesExtracted` events with monotonically increasing `done` and constant `total` (updated `sequential_path_reports_all_segments`, `parallel_path_reports_all_segments`)
  - [x] 5.4 Integration test: `done == total` on final event (asserted in all progress tests)
  - [x] 5.5 Unit test: `CliProgressObserver::on_heap_bytes_extracted` with `done == total` — `on_heap_bytes_extracted_finishes_bar_at_done_eq_total`
  - [x] 5.6 Integration test: `extract_all_terminates_no_deadlock` — thread::spawn + join
  - [x] 5.7 Integration test: `single_threaded_fallback_emits_bytes_events` — rayon pool num_threads(1)
  - [x] 5.9 Integration test: `multi_batch_bytes_monotonicity` — asserts strictly increasing done across batches
  - [x] 5.8 Run `cargo clippy --all-targets -- -D warnings` — clean

## Dev Notes

### Current Code (What to Change)

**Parallel path** (`heap_extraction.rs:405-443`):
```
for batch in batches {
    let results: Vec<_> = batch
        .par_iter()
        .map(|r| extract_heap_segment(...))
        .collect();                           // ← BLOCKS
    for result in results {
        result.merge_into(ctx);
        notifier.segment_completed(...);     // ← ONLY HERE
    }
}
```

### Solution: `rayon::scope` + Channel

Replace with concurrent drain — main thread merges
and reports while workers are still active.

**Borrow separation invariant:** Workers capture only
`data: &[u8]` (already extracted at line 383 via
`let data = ctx.data`), `id_size: u32` (Copy), and
`max_chunk_bytes: usize` (Copy) — all `Send`. The
drain loop exclusively owns `&mut ctx` and
`&mut notifier`. Do NOT move `notifier` calls or
`merge_into` into workers — `ProgressNotifier` wraps
`&mut dyn` which is not `Send`.

**Batching preserved:** The outer `for batch in batches`
loop (story 10.3 memory guard) is kept. Each batch
gets its own `rayon::scope`. This bounds in-flight
results to one batch at a time.

```rust
// Outer loop preserved (story 10.3 memory guard)
// bytes_done is cumulative across ALL batches so the
// progress bar never regresses at a batch boundary.
let mut bytes_done: u64 = 0;
for (batch_idx, batch_range) in batches.iter().enumerate() {
    let batch = &ranges[batch_range.clone()];
    let (tx, rx) = std::sync::mpsc::channel();

    rayon::scope(|s| {
        for r in batch {
            let tx = tx.clone();
            s.spawn(move |_| {
                let start = r.payload_start as usize;
                let end = start + r.payload_length as usize;
                let result = extract_heap_segment(
                    &data[start..end], start,
                    id_size, max_chunk_bytes,
                );
                let _ = tx.send(
                    (r.payload_length, result),
                );
            });
        }
        // CRITICAL: drop original tx so rx.iter()
        // terminates when all worker clones drop.
        drop(tx);

        // Drain concurrently — runs while workers
        // are still active.
        // Order-independent: SegmentFilterBuilder,
        // class_dumps, entry_points all handle
        // non-deterministic arrival order (each
        // HeapSegmentResult merges atomically).
        for (payload_len, result) in rx {
            result.merge_into(ctx);
            bytes_done += payload_len as u64;
            notifier.heap_bytes_extracted(
                bytes_done, total_heap_bytes,
            );
        }
    });
}
```

### ADR: Parallel Extraction Progress Mechanism

**Status:** Accepted (2026-03-18)

| Option | Verdict | Reason |
|--------|---------|--------|
| A: `rayon::scope` + `mpsc` | **Accepted** | Concurrent drain = real-time progress. No extra threads, no atomics, no signature changes. Lower peak memory within each batch (results freed on merge instead of accumulated by `collect()`). Slight throughput trade-off (~5-10% fewer parallel workers), offset by merge overlap and elimination of multi-second freeze. |
| B: `AtomicU64` + tick thread | Rejected | Over-engineering post-story-10.2 (~64 MB segments). Adds thread lifecycle, atomics, signature change for imperceptible gain. |
| C: `for_each_with(tx)` | Rejected | Blocks main thread — drain is post-completion only. Same freeze as `collect()`. |
| D: Smaller batches | Rejected | Reduces parallelism without solving intra-batch freeze. |

**Reversibility:** If future dumps have non-splittable
giant segments, option B can layer on top of A.

### Failure Modes

| Risk | Severity | Mitigation |
|------|----------|------------|
| `tx` not dropped before drain → deadlock | **Blocker** | Explicit `drop(tx)` + comment + non-hang test (Task 5.6) |
| `bytes_done` reset per batch → progress regression | **Blocker** | Declare `bytes_done` outside outer `for batch` loop (see snippet) + multi-batch monotonicity test (Task 5.9) |
| `extract_all` called inside existing `rayon::scope` → deadlock | **Blocker** | Guard in Task 2.7; verify call chain is not wrapped in parallel context |
| `num_threads == 1` → drain blocks with no worker | **Medium** | Guard in parallel condition (Task 2.7) |
| `tx.send().unwrap()` double-panic | Low | Use `let _ = tx.send(...)` (Task 2.6) |
| `done > total` overflow | Minor | `debug_assert!(done <= total)` (Task 1.2) |

### Anti-Patterns to Avoid

- Do NOT use `for_each_with(tx)` — blocks main thread, same freeze as `collect()` (ADR option C)
- Do NOT forget `drop(tx)` before drain loop — deadlock
- Do NOT use `rayon::scope` when `current_num_threads() == 1` — deadlock
- Do NOT introduce generic `F: FnMut` on `extract_heap_segment` — deliberately removed in prior work (tech-spec-progress-observer-trait.md)
- `SegmentFilterBuilder` ordering: safe because each `HeapSegmentResult` merges atomically (all filter_ids from one segment in one call). Out-of-order segment arrival causes segment index changes but each segment's IDs stay contiguous. May produce duplicate partial filters if two ranges share a 64 MB segment boundary — `find_instance` iterates all filters, so lookups still correct. This was validated in story 10.2; no additional test needed here, but do not change the merge logic without re-validating.

### Files to Modify

| File | Change |
|------|--------|
| `crates/hprof-api/src/progress.rs` | Add `on_heap_bytes_extracted` to trait, `ProgressNotifier`, `ProgressEvent`, `TestObserver` |
| `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs` | Replace `par_iter().map().collect()` with `rayon::scope` + `mpsc` channel drain; add bytes reporting in sequential path |
| `crates/hprof-cli/src/progress.rs` | Add `extraction_bar`, implement `on_heap_bytes_extracted`, remove `segment_bar` |

### Project Structure Notes

- `hprof-api` owns `ParseProgressObserver` trait
- `hprof-parser` owns `extract_all` / `extract_heap_segment`
- `hprof-cli` owns `CliProgressObserver`
- Dependency: `hprof-cli → hprof-engine → hprof-parser → hprof-api`
- Parser must NOT depend on indicatif

### ADR Extraction

The ADR table above should be copied to
`docs/adr/adr-parallel-extraction-progress.md`
when this story is merged, so the decision is
discoverable independently of sprint artifacts.

### References

- [Source: docs/implementation-artifacts/sprint-status.yaml] — ACTION items about progress freeze
- [Source: docs/implementation-artifacts/epic-11-retro-2026-03-18.md] — Decision to create story 13.0
- [Source: docs/implementation-artifacts/epic-10-retro-2026-03-16.md] — 70 GB dump confirmed freeze
- [Source: docs/implementation-artifacts/tech-spec-progress-observer-trait.md] — Previous progress work
- [Source: crates/hprof-api/src/progress.rs] — Current trait definition
- [Source: crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs] — extract_all, chunk checkpoints
- [Source: crates/hprof-cli/src/progress.rs] — CLI bar implementation

## Dev Agent Record

### Agent Model Used
Claude Opus 4.6 (1M context)

### Debug Log References
- Used `rayon::in_place_scope` instead of `rayon::scope` — the latter requires `Send` on the closure, but `ProgressNotifier` wraps `&mut dyn` (non-Send). `in_place_scope` runs the closure on the calling thread, avoiding the Send requirement while still spawning workers on rayon threads.

### Completion Notes List
- Task 1: Added `on_heap_bytes_extracted(u64, u64)` to trait with default no-op, `heap_bytes_extracted()` to ProgressNotifier with debug_assert, `HeapBytesExtracted` variant to ProgressEvent, implemented in TestObserver
- Task 2: Replaced `par_iter().map().collect()` with `rayon::in_place_scope` + `mpsc::channel` concurrent drain. `bytes_done` cumulative across batches. Explicit `drop(tx)` with deadlock warning comment. Guard: `current_num_threads() > 1` for parallel path. Post-fix: sort segment filters and entry points after build (concurrent drain delivers segments out of order; `batch_lookup_by_filter` requires sorted inputs).
- Task 3: Sequential path now reports cumulative `heap_bytes_extracted` after each segment
- Task 4: Replaced `segment_bar` with `extraction_bar` (bytes-based). `on_segment_completed` is now a no-op with TODO(cleanup). Removed `on_phase_changed` segment_bar logic.
- Task 5: All 9 test items passing. Updated 6 existing tests from SegmentCompleted to HeapBytesExtracted. Added 3 new integration tests (deadlock, single-thread fallback, multi-batch monotonicity).
- Removed unused `rayon::prelude::*` import

### File List
- crates/hprof-api/src/progress.rs
- crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs
- crates/hprof-parser/src/indexer/first_pass/tests.rs
- crates/hprof-parser/src/indexer/segment.rs
- crates/hprof-parser/src/indexer/first_pass/mod.rs
- crates/hprof-cli/src/progress.rs
- docs/implementation-artifacts/13-0-progress-bar-intra-segment.md
- docs/implementation-artifacts/sprint-status.yaml
- docs/planning-artifacts/epics.md
