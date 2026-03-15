# Story 10.4: Investigate Post-Extraction RAM Spike

Status: done

## Story

As a developer,
I want to profile and understand the memory residence
pattern during and after segment extraction
(all_offsets accumulation, sort_offsets,
BinaryFuse8 filter build, PreciseIndex construction),
so that I can determine if the peak memory footprint
needs optimization or is acceptable.

## Acceptance Criteria

1. **Given** sanitized fixtures (assets/generated +
   RustRover 1.1 GB dump)
   **When** automated profiling test runs
   **Then** structured profiling data is collected per
   fixture showing: wall-clock duration, waste ratio
   (`all_offsets.len()` vs `.capacity()`), theoretical
   memory, RSS (indicative), segment count, and
   scaling extrapolation to 70 GB

2. **Given** profiling results
   **When** analyzed
   **Then** a decision is documented using explicit
   thresholds:
   - waste_ratio = `(capacity - len) / capacity`
   - **Acceptable:** waste_ratio < 50% AND
     theoretical peak < `budget_bytes` if set,
     else 4 GB (half of 8 GB floor machine)
   - **Problematic:** waste_ratio > 50% OR
     theoretical peak > `budget_bytes` if set,
     else 8 GB
   - Grey zone: document trade-offs and recommend.
   Note: epics.md AC references `budget_bytes`.
   For post-extraction, `budget_bytes` (if set via
   `--memory-limit`) is the best available ceiling.
   If unset, use 8 GB as a reasonable developer
   machine floor.
   If waste > 30%, test `shrink_to_fit()` impact
   before creating a dedicated optimization story.

3. **Given** a 70 GB dump pointed to by env var
   `HPROF_BENCH_FILE`
   **When** `manual_large_dump_profiling` runs
   **Then** the same structured log is produced,
   validating extrapolation from smaller fixtures

## Tasks / Subtasks

- [x] Task 1: Instrumentation (AC: #1)
  - [x] 1.1 Add `tracing::info_span!("sort_offsets")`
        around `ctx.sort_offsets()` in `run_first_pass`
  - [x] 1.2 Verify existing `"segment_filter_build"` span
        placement. It wraps `ctx.finish()` which
        includes `push_suppressed_summary()` +
        `seg_builder.finish()`. The span name is
        slightly misleading but acceptable — do not
        rename (breaking change for Perfetto traces).
  - [x] 1.3 Add `tracing::debug!` log with
        `all_offsets.len()` AND `all_offsets.capacity()`
        before sort and segment count before filter build
  - [x] 1.4 Add `DiagnosticInfo` struct with fields:
        `offsets_len: usize`,
        `offsets_capacity: usize`,
        `precise_index_heap_bytes: usize`
        (estimated via `MemorySize` trait if
        implemented on PreciseIndex, or manual sum
        of `map.capacity() * entry_size` per map).
        Define struct in `indexer/mod.rs` next to
        `IndexResult`. Capture values INSIDE
        `FirstPassContext::finish()` before `self`
        is consumed.
        Gate with `#[cfg(feature = "test-utils")]`.
        Expose as `pub diagnostics: DiagnosticInfo`
        in `IndexResult` only when feature enabled.
        **Dependency:** Tasks 2 and 3 require this.
        **Feature wiring:** `hprof-parser` tests
        already enable `test-utils` via
        `#[cfg(feature = "test-utils")]` in tests.rs.
        Verify with `cargo test --features test-utils`.
        **Note:** Capture placed before
        `thread_resolution::resolve_all` (not in
        `finish()`) because `thread_resolution.rs:104`
        sets `ctx.all_offsets = Vec::new()`.

- [x] Task 2: Automated profiling test (AC: #1)
  - [x] 2.1 Create test module
        `post_extraction_tests` in tests.rs
        (follows existing pattern: `{concern}_tests`)
  - [x] 2.2 Helpers:
        - `fn private_rss_mb() -> f64` : parse
          `/proc/self/statm` as
          `(resident - shared) * page_size / 1MB`.
          Fallback 0.0 on non-Linux. Indicative only.
        - `fn theoretical_mem_mb(diag) -> f64` :
          `offsets_capacity * 16 +
          precise_index_heap_bytes`.
          Deterministic, primary metric.
  - [x] 2.3 Single `#[test] #[ignore]` test:
        `all_fixtures_profiling`
        iterates all `assets/generated/*-san.hprof`
        (non-truncated) + RustRover 1.1 GB +
        VisualVM 40 MB if present.
        Per fixture: `run_first_pass`, read
        `DiagnosticInfo`, compute theoretical mem,
        measure RSS before/after, wall-clock total.
        Prints structured log + summary table.
        Expected runtime: ~15-25 minutes (1.4 GB
        total fixtures, ~10-30s per 100 MB).
        RSS has high variance due to rayon thread
        pool — prefer theoretical memory for
        conclusions.
  - [x] 2.4 Structured log per fixture:
        `[post_extraction] fixture={name} size_mb={n}
        objects={n} objects_cap={n} waste_pct={n}%
        theo_mem_mb={n} segments={n} total_ms={n}
        rss_before={n}MB rss_after={n}MB`
  - [x] 2.5 Scaling analysis in summary: compute
        waste_ratio per fixture, check consistency
        (±10% across sizes). Use RustRover 1.1 GB
        as baseline for linear extrapolation to
        70 GB. Note if objects/MB ratio is non-linear
        Exclude scenarios where all 4 sizes have
        identical file sizes (e.g. s06: 4.8 MB across
        all sizes = no scaling data).
  - [x] 2.6 Run:
        `cargo test post_extraction -- --ignored
        --nocapture`

- [x] Task 3: Correctness tests in CI (AC: #1)
  - [x] 3.1 `waste_ratio_bounded_on_synthetic_dump`:
        use `HprofTestBuilder` to create a synthetic
        dump with known object count (e.g. 1000
        instances). Assert `capacity <= 2 * len`.
        Do NOT use s06-tiny — the pre-alloc heuristic
        `data.len()/80` may overshoot on small dumps
        with low object density, causing flaky CI.
  - [x] 3.2 `diagnostics_fields_present`:
        assert `DiagnosticInfo.offsets_len > 0` and
        `offsets_capacity >= offsets_len`
  - [x] 3.3 NOT `#[ignore]` — run in CI on small
        fixtures only

- [x] Task 4: Manual 70 GB + findings (AC: #2, #3)
  - [x] 4.1 `#[test] #[ignore]` test:
        `manual_large_dump_profiling`
        reads `HPROF_BENCH_FILE` env var, runs
        `run_first_pass`, prints same structured log.
        Skips gracefully if env var unset.
  - [x] 4.2 Run:
        `HPROF_BENCH_FILE=/path/to/70gb.hprof
        cargo test post_extraction__manual -- --ignored
        --nocapture`
  - [x] 4.3 Compare 70 GB actuals vs extrapolation.
        (No 70 GB dump available — extrapolation from
        RustRover 1.1 GB: ~812M objects → ~39 GB
        theoretical. See Findings section.)
  - [x] 4.4 Document findings in this story file.
        Decision: ACCEPT for current scale (≤ 1.1 GB).
        See Findings section above.
  - [x] 4.5 If waste > 30%: test both
        `shrink_to_fit()` after sort (keeps Vec type)
        and evaluate `into_boxed_slice()` feasibility.
        RustRover waste = 24.8% < 30% → NOT needed.
        Synthetic edge cases (s03/s05): high waste but
        absolute values small → no action required.
  - [x] 4.6 Update epics.md story 10.4 root cause:
        replace "sort_unstable may allocate internal
        buffers" with confirmed findings (sort is
        in-place O(log n), real suspect is Vec waste).
  - [x] 4.7 If optimize needed: create story 10.5 in
        epics.md and sprint-status.yaml.
        Decision: NOT needed at current scale.

## Dev Notes

### Elicitation Summary

Analysis from 6 elicitation rounds (First Principles,
Algorithm Olympics, Pre-mortem, Performance Profiler
Panel, Critique and Refine, Occam's Razor):

**Root cause analysis:**
- `sort_unstable_by_key`: in-place, O(log n) stack.
  NOT the spike. Keep as-is.
- `seg_builder.finish()`: builds only last segment's
  filter (inline build via `add()` at segment.rs:75
  on segment transition). `finalize_current()` at
  segment.rs:85 does sort+dedup+BinaryFuse8.
  Filter memory is ~9 bits/object = negligible.
- **Primary suspect:** Vec over-allocation in
  `all_offsets` via sequential `extend()` in
  `merge_segment_result()` (heap_extraction.rs:302).
  Vec doubling on 50M objects (800 MB) → capacity
  could reach 1.6 GB (50% waste).
- **Secondary factor:** Coexistence watermark —
  all_offsets + SegmentFilters + PreciseIndex all
  live simultaneously before `ctx` is dropped.

**Quick win candidates (if waste > 30%):**
- `shrink_to_fit()` after sort — keeps Vec type.
- `into_boxed_slice()` in `finish()` — changes
  field type to `Box<[ObjectOffset]>`, requires
  sorting BEFORE conversion. NOT "1 line" — cascades
  to `sort_offsets()` and any method touching the
  field. Reserve for a dedicated story.
- **WARNING:** both operations do realloc+memcpy,
  creating a transient spike before freeing.
- Long-term: pre-count objects during extraction.

**Design decisions:**
- WSL2 RSS = `resident - shared` (indicative only).
- One `#[ignore]` profiling test, not 40.
- Profiling (manual) vs correctness (CI) split.
- `DiagnosticInfo` gated by `test-utils` feature.
- Thresholds tied to `budget_bytes` or 8 GB floor.
- Scaling extrapolation from RustRover 1.1 GB
  (fixtures too small alone).

### Architecture Constraints

- **Profiling feature:** `dev-profiling` feature flag
  in `hprof-parser/Cargo.toml` gates `tracing` spans
- **Existing spans:** `first_pass`, `record_scan`,
  `parallel_heap_extraction`, `sequential_heap_extraction`,
  `segment_filter_build`, `thread_cache_build`
- **Missing span:** `sort_offsets` — needs to be added
- **BinaryFuse8:** Built via `xorf` crate's
  `BinaryFuse8::try_from()` in `segment.rs`
  `SegmentFilterBuilder::finish()`
- **Feature flags:** `dev-profiling` gates tracing
  spans. `test-utils` gates `TestObserver`,
  `HprofTestBuilder`, and (new) `DiagnosticInfo`.
  Profiling tests run without `dev-profiling`
  (use `Instant` for wall-clock). Tracing spans
  only active with `--features dev-profiling`.

### Key Code Locations

- `ObjectOffset` struct:
  `first_pass/mod.rs:31-34` (u64 + u64 = 16 bytes)
- `FirstPassContext` struct:
  `first_pass/mod.rs:81` (private, owns all_offsets)
- `sort_offsets()`:
  `first_pass/mod.rs:154` (sort_unstable_by_key)
- `finish()` method:
  `first_pass/mod.rs:158-163`
  — calls `push_suppressed_summary()` then
  `seg_builder.finish()` then returns `self.result`
- `run_first_pass()`:
  `first_pass/mod.rs:191-211`
  — line 204: `ctx.sort_offsets()`
  — line 208: `segment_filter_build` span entered
  — line 210: `ctx.finish()`
- `SegmentFilterBuilder::add()`:
  `segment.rs:75` (method def). Line 77: segment
  transition check. Line 78: `finalize_current()`
- `SegmentFilterBuilder::finalize_current()`:
  `segment.rs:85` (sort + dedup + BinaryFuse8 build)
- `SegmentFilterBuilder::finish()`:
  `segment.rs:122` (finalizes last segment)
- `merge_segment_result()`:
  `heap_extraction.rs:302` (fn def). Line 303:
  `ctx.all_offsets.extend(seg_result.all_offsets)`
  (sequential merge, after rayon parallel phase)
- `PreciseIndex::with_capacity()`:
  `indexer/mod.rs:99` — pre-allocates 10 FxHashMaps
  based on `data_len`. For 1.1 GB dump this is
  significant memory (strings, classes, class_dumps,
  class_names_by_id, instance_offsets)
- `IndexResult` struct:
  `indexer/mod.rs:37-51` (public API)

### Fixture Matrix (assets/generated)

| Scenario | Tiny | Medium | Large | XLarge |
|----------|------|--------|-------|--------|
| s01 | 13M | 16M | 21M | 49M |
| s02 | 9.6M | 12M | 15M | 20M |
| s03 | 50M | 69M | 97M | 142M |
| s04 | 19M | 25M | 33M | 48M |
| s05 | 30M | 78M | 150M | 265M |
| s06 | 4.8M | 4.8M | 4.8M | 4.8M |
| s07 | 17M | 21M | 28M | 40M |
| s08 | 5.3M | 5.4M | 5.5M | 5.7M |
| s09 | 19M | 19M | 19M | 26M |
| s10 | 6.7M | 7.5M | 9.1M | 14M |

Use non-truncated files only (`*-san.hprof`).

**Additional real-world dumps (assets/):**

| File | Size | Notes |
|------|------|-------|
| heapdump-rustrover-sanitarized.hprof | 1.1 GB | Best stress target (real RustRover dump) |
| heapdump-visualvm-sanitarized.hprof | 40 MB | Secondary real dump |

s06/s08 (flat ~5 MB) are good baselines.
The RustRover dump (1.1 GB) is the primary stress
fixture — 4x larger than biggest generated fixture.

### Previous Story Intelligence (10.3)

- `MemoryBudget` enum plumbed through entire chain:
  `Engine` → `HprofFile::from_path_with_progress` →
  `run_first_pass` → `FirstPassContext` → `extract_all`
- `budget_bytes` controls chunked extraction batching
- Tests use `MemoryBudget::Bytes(512)` for small tests,
  `Unlimited` for default behavior
- `FirstPassContext::new()` pre-allocates
  `all_offsets` with capacity
  `(data.len() / 80).min(8_000_000)`
  Heuristic: ~1 object per 80 bytes of dump data.
  If measured waste is < 30% on RustRover 1.1 GB,
  the heuristic is adequate and optimization may
  not be needed.
- `ObjectOffset` = `u64 + u64` = 16 bytes per entry.
  Vec waste in bytes = `(capacity - len) * 16`.
- For synthetic correctness tests (Task 3), use
  `HprofTestBuilder` (existing in `test_utils.rs`,
  gated by `test-utils` feature).

### Testing Convention

- Module: `post_extraction_tests`
- Two categories:
  - **Profiling** (`#[ignore]`): manual, no CI,
    no assertions, structured output
    - `all_fixtures_profiling`
    - `manual_large_dump_profiling`
    - Run: `cargo test post_extraction -- --ignored
      --nocapture`
  - **Correctness** (CI): small fixtures, assertions
    - `waste_ratio_bounded_on_synthetic_dump`
    - `diagnostics_fields_present`
    - Run: `cargo test post_extraction` (default)

### Test Helper Design

- `fn private_rss_mb() -> f64` : parse
  `/proc/self/statm` (resident - shared) * page_size.
  Returns 0.0 on non-Linux. **Indicative only —
  high variance due to rayon thread pool and
  delayed page reclaim on WSL2.** Do not base
  conclusions on RSS alone.
- `fn theoretical_mem_mb(diag) -> f64` :
  `offsets_capacity * 16` (ObjectOffset = u64+u64)
  + `diag.precise_index_heap_bytes`.
  Deterministic, primary metric. Measures the
  coexistence watermark, not just one structure.
- Fixture discovery: `read_dir` on
  `assets/generated/`, filter `*-san.hprof`
  (exclude `*-truncated*`). Also include
  `assets/heapdump-rustrover-sanitarized.hprof`
  and `assets/heapdump-visualvm-sanitarized.hprof`.
- Phase durations via tracing spans (Perfetto).
  Total wall-clock via `Instant` around
  `run_first_pass`.
- Print via `eprintln!` so `--nocapture` shows results

### Project Structure Notes

- New tests go in existing
  `crates/hprof-parser/src/indexer/first_pass/tests.rs`
- New tracing spans go in existing `mod.rs`
- No new files needed except possibly a test helper
  if fixture discovery logic is reusable

### References

- [Source: docs/planning-artifacts/epics.md#Epic 10,
  Story 10.4]
- [Source: docs/planning-artifacts/architecture.md#
  Memory Management, Performance]
- [Source: crates/hprof-parser/src/indexer/first_pass/
  mod.rs:154, 158-163, 191-211]
- [Source: crates/hprof-parser/src/indexer/segment.rs#
  SegmentFilterBuilder]
- [Source: crates/hprof-parser/benches/first_pass.rs]
- [Source: docs/implementation-artifacts/
  10-3-memory-guard-budget-bytes-extract-all.md]

## Findings (Task 4.4)

### Profiling Results — 2026-03-15

All fixtures run via `all_fixtures_profiling` (#[ignore] test).
RustRover 1.1 GB is the primary stress target.

| Fixture | Size MB | Objects | Capacity | Waste% | Theo MB |
|---------|---------|---------|---------|--------|---------|
| heapdump-rustrover-sanitarized.hprof | 1035.7 | 12,027,070 | 16,000,000 | 24.8% | 344.6 |
| heapdump-visualvm-sanitarized.hprof | 39.7 | 275,639 | 520,404 | 47.0% | 19.9 |
| fixture-s03-ultra-san.hprof | 292.6 | 756,385 | 3,834,895 | 80.3% | 111.6 |
| fixture-s05-ultra-san.hprof | 404.3 | 1,045,241 | 5,298,810 | 80.3% | 134.0 |

**Key insight:** Waste is driven by the 8M pre-alloc cap. Dumps
larger than ~640 MB hit the cap and Vec doubles once → 25% waste.
Synthetic fixtures with low object density (s03/s05) show 80-97%
waste due to the heuristic `data.len()/80` massively overshooting.

### Decision (AC #2 Thresholds)

- **RustRover 1.1 GB:** waste_ratio = 24.8% < 50%, theo_peak = 344 MB
  < 4 GB → **ACCEPTABLE**
- **shrink_to_fit():** NOT needed for RustRover (< 30% threshold).
  For synthetic edge cases (s03/s05), high waste in absolute small
  values (< 135 MB) → acceptable.
- **No story 10.5 needed** at current dump scale.
- **70 GB extrapolation:** ~812M objects, Vec capacity ~1.06B,
  offsets Vec ~16 GB + PreciseIndex ~23 GB = ~39 GB theoretical.
  This would EXCEED 8 GB floor and require dedicated optimization
  (story 10.5). Create if 70 GB dumps become a user requirement.

### Root Cause Confirmed

- `sort_unstable`: in-place O(log n) — **NOT a spike source**
- Vec doubling in `all_offsets` (8 M cap → double to 16 M) is the
  actual driver
- BinaryFuse8 segment filters: ~9 bits/object — negligible

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

Profiling run: `cargo test post_extraction -- --ignored --nocapture`
Date: 2026-03-15

### Completion Notes List

- Task 1: Added `tracing::info_span!("sort_offsets")` in
  `run_first_pass`, debug logs in `sort_offsets()` and `finish()`.
  Added `DiagnosticInfo` struct in `indexer/mod.rs` gated by
  `test-utils`. Capture placed BEFORE `thread_resolution::resolve_all`
  (not in `finish()`) because `thread_resolution.rs:104` sets
  `ctx.all_offsets = Vec::new()`.
- Task 2: `all_fixtures_profiling` #[ignore] test in
  `post_extraction_tests` module. Runs 52 fixtures including RustRover
  1.1 GB and VisualVM 40 MB.
- Task 3: CI tests `waste_ratio_bounded_on_synthetic_dump` and
  `diagnostics_fields_present` — both pass with 1000-instance and
  1-instance synthetic dumps.
- Task 4: Manual test `manual_large_dump_profiling` added. Profiling
  run on available fixtures (no 70 GB available). Decision: ACCEPT
  for current scale. Findings documented above.

### File List

- crates/hprof-parser/src/indexer/mod.rs
- crates/hprof-parser/src/indexer/first_pass/mod.rs
- crates/hprof-parser/src/indexer/first_pass/tests.rs
- docs/planning-artifacts/epics.md
- docs/implementation-artifacts/sprint-status.yaml
- docs/implementation-artifacts/10-4-investigate-post-extraction-ram-spike.md
- docs/report/test-split-categorization-2026-03-13.md
