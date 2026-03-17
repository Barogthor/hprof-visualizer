# Story 11.3: Parallel Eager Item Resolution

Status: done

## Story

As a user,
I want the first 100 eager items of a collection to be resolved
in parallel using a bounded thread pool,
So that opening a large collection on a 70 GB dump is faster
when items span multiple segment filters (speedup ≈ min(K,
num_cpus) over the 11.2 sequential baseline, where K is the
number of distinct segment filters; typical K = 1-3).

**Depends on:** Story 11.2 (batch-scan by segment filter)

## Acceptance Criteria

1. **Given** parallel resolution of 100 items on a multi-segment dump
   **When** items span K ≥ 2 segment filters
   **Then** all items are resolved correctly with no duplicates,
   no panics, and no data races — identical output to the
   sequential 11.2 baseline
   **Note (manual only):** wall-clock speedup (target ≥ 20% for
   K ≥ 2, overhead ≤ 5% for K = 1) cannot be verified on CI;
   validate with `HPROF_BENCH_FILE` on a large multi-segment dump.

2. **Given** existing tests using `find_instance`, `get_page`,
   and `batch_find_instances` on small dumps
   **When** `cargo test` is run
   **Then** all pass unchanged — parallelization is transparent

## Tasks / Subtasks

- [x] Task 1: Parallelize per-segment-filter scans in
      `batch_find_instances` (AC: #1)
  - [x] 1.0 **Pre-check 11.2 invariants:** All 5 invariants
        verified: (a) per-segment HashSet targets, (b) side-effect-free,
        (c) returns all results, (d) no-panic scanner, (e) OffsetCache
        memory accounting correct.
  - [x] 1.1 Replaced sequential `for` loop with `par_iter()` +
        local collect + sequential merge (first-found wins).
        Added `use rayon::prelude::*` at module scope.
        Exposed `batch_find_instances_inner` as `pub(crate)` for
        test access.
  - [x] 1.2 Added `tracing::debug_span!("batch_find_instances_parallel")`
        gated by `#[cfg(feature = "dev-profiling")]`.

- [x] Task 2: Test parallel completeness (AC: #1, #2)
  - [x] 2.1 `parallel_batch_multi_filter_returns_all_items`:
        10 IDs with segment_size=1024, all found with correct
        class_object_id and data.
  - [x] 2.2 `parallel_batch_single_filter_returns_all_items`:
        3 IDs in single segment filter (K=1), all correct.
  - [x] 2.3 `parallel_batch_empty_slice_returns_empty`:
        empty input returns empty result, no panic.

- [x] Task 3: Regression & clippy (AC: #2)
  - [x] 3.1 `cargo test` — 949 tests pass, 0 failures
  - [x] 3.2 `cargo clippy --all-targets -- -D warnings` — clean
  - [x] 3.3 `cargo fmt -- --check` — clean
  - [x] 3.4 Manual test deferred (CI-only change; K=1 dump
        cannot validate parallel speedup)

## Dev Notes

### Core Change Summary

Replace the sequential `for` loop with `par_iter()` inside
`batch_find_instances` (added by Story 11.2), plus add
tracing instrumentation. The 11.2 design explicitly prepared
for this parallelization:

1. **Per-segment-filter target HashSets** (not a shrinking global
   `remaining`) — each segment filter scan is independent
2. **Post-scan dedup** (first-found wins after all scans) — no
   cross-filter coordination needed during scanning
3. **Side-effect-free method** — `batch_find_instances` does NOT
   read or write `OffsetCache`; the caller handles cache updates
4. **Immutable mmap** — `&[u8]` is `Send + Sync`, safe to share
5. **No-panic scanner** — `scan_segment_for_instances` handles
   corrupted data by logging and skipping, never panicking.
   Rayon propagates panics from `par_iter` to the caller via
   `collect()` — if the scanner violated this, it would crash
   the pagination thread.

### Rayon Already Available

rayon is already in the workspace dependency tree and used by:
- `crates/hprof-engine/src/engine_impl/mod.rs` —
  `build_thread_cache` (chunked `par_iter`)
- `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs`
  — parallel heap segment extraction

No new dependency to add. Add `use rayon::prelude::*;` at
**module scope** (top of `hprof_file.rs`), consistent with
how `heap_extraction.rs` imports rayon. Do not use a
function-level import.

### Performance Model

**Sequential (11.2 baseline):**
- 100 items across K segment filters → K sequential scans
- Each scan = O(filter_range_size), covering the byte ranges
  matched by the BinaryFuse8 filter
- Total = K × scan_time

**Parallel (11.3):**
- K segment filter scans dispatched to rayon pool
- Total ≈ max(scan_time per filter) + merge overhead
- Speedup ≈ min(K, num_cpus) for K > 1

**Typical scenario:** 100 items from same allocation region →
K = 1-3 segment filters → 1-3× speedup. The main win is when
items span multiple segment filters (common on large dumps
with split segments from Epic 10.2).

**Small dump (41 MB):** Single segment filter, K = 1 → par_iter
dispatches to one thread → negligible overhead (~50ns).

**Storage caveat:** On slow I/O (HDD, NFS), concurrent mmap
page faults may limit the parallel speedup.

**CI limitation:** The primary AC (K ≥ 2 speedup) cannot be
verified on CI — it requires a large multi-segment dump. The
unit tests verify correctness (completeness + data integrity),
not performance. Manual benchmarking with `HPROF_BENCH_FILE`
is needed to validate the speedup claim.

### What NOT To Do

- Do NOT create a dedicated `rayon::ThreadPool` — YAGNI
- Do NOT add a parallelism threshold — rayon handles it
- Do NOT modify `find_instance` — single-ID path stays as-is
- Do NOT add opportunistic array offset caching — YAGNI
  (Story 11.4 uses O(1) arithmetic for OBJECT_ARRAY)

### Key Code Locations

| File | Purpose |
|------|---------|
| `crates/hprof-parser/src/hprof_file.rs` | `batch_find_instances()` — THE loop to parallelize (added by 11.2) |
| `crates/hprof-parser/src/hprof_file.rs` | `scan_segment_for_instances()` — per-segment-filter scanner (added by 11.2) |
| `crates/hprof-engine/src/pagination/mod.rs` | `paginate_id_slice()` — caller of batch (refactored by 11.2) |
| `crates/hprof-engine/src/engine_impl/mod.rs` | `read_instance()` — single-ID path (unchanged) |
| `crates/hprof-parser/src/indexer/precise.rs` | `OffsetCache` — thread-safe offset wrapper (added by 11.2) |

### Architecture Constraints

- **Crate boundary:** The `par_iter` change is in the parser
  crate (`hprof_file.rs`). rayon is already a dependency of
  `hprof-parser`.
- **Thread safety:** `HprofFile` is behind `Arc` in the engine
  layer. `batch_find_instances` takes `&self` — immutable
  reference, safe to share. `OffsetCache` uses `RwLock` for
  post-batch insertion (caller's responsibility, not inside
  the parallel loop).
- **Memory:** No additional memory allocation beyond what 11.2
  already allocates. rayon's work-stealing uses the existing
  global thread pool stack space.

### Project Structure Notes

- Changes in `crates/hprof-parser/src/hprof_file.rs`
  (par_iter swap + tracing instrumentation)
- No TUI changes needed
- No new modules or files needed
- No new dependencies needed

### References

- [Source: docs/planning-artifacts/epics.md#Epic 11, Story 11.3]
- [Source: docs/implementation-artifacts/11-2-batch-scan-by-segment.md]
- [Source: docs/planning-artifacts/architecture.md#NFR2, NFR3]
- [Source: crates/hprof-engine/src/engine_impl/mod.rs:286-325]

## Dev Agent Record

### Agent Model Used
Claude Opus 4.6

### Debug Log References
None

### Completion Notes List
- Replaced sequential segment-filter loop with `par_iter()` in
  `batch_find_instances_inner` — each segment filter scans
  independently into local FxHashMaps, then merges sequentially
  with first-found-wins semantics.
- Added `use rayon::prelude::*` at module scope in `hprof_file.rs`.
- Exposed `batch_find_instances_inner` as `pub(crate)` for test
  access with custom `segment_size`.
- Added `tracing::debug_span!` gated by `dev-profiling` feature.
- 3 new tests: multi-filter (10 IDs, segment_size=1024),
  single-filter (K=1), empty batch.
- All 949 tests pass, clippy clean, fmt clean.

### Manual Testing Results (2026-03-17)
- Large dump: collection list opening fast, deep jump ~1s or less
- Go-to deep index also fast
- AC #1 validated: parallel resolution works correctly on
  multi-segment dump with no duplicates, no panics

### Change Log
- 2026-03-17: Story 11.3 implemented — parallel batch_find_instances
- 2026-03-17: Manual testing passed on large dump
- 2026-03-17: Code review fixes — removed bogus `cfg_attr(test,allow(dead_code))`,
  renamed misleading test to `parallel_batch_correctness_small_segment_size`,
  `continue` → `break` after truncated slice, Phase 3 non-determinism comment,
  `seg_count` tracing event, M3 TODO made actionable

### File List
- `crates/hprof-parser/src/hprof_file.rs` (modified: par_iter + tests)
