# Review: Tech-Spec Progress Observer Trait (v2)

**Reviewed:** 2026-03-09
**Spec:** `docs/implementation-artifacts/tech-spec-progress-observer-trait.md`

---

## Findings

### 1. Scan bar stalls during extraction, then jumps to 100% -- UX regression

**Severity: High**

**Spec ref:** Task 3 line 499 ("Remove `maybe_report_progress` calls from `extract_all` entirely"), Task 2 line 436 (final `bytes_scanned` call after extraction).
**Source ref:** `mod.rs:148-151` -- `extract_all` is called between `scan_records` and the final `progress_fn(ctx.cursor_position)`.

Currently, `extract_all` calls `maybe_report_progress` on every segment boundary (both parallel and sequential paths), keeping the byte progress bar alive during the longest phase of indexing. The spec removes ALL byte-level progress from `extract_all` and replaces it with segment-count progress. But the final `notifier.bytes_scanned(base_offset + cursor_position)` fires AFTER `extract_all` completes (line 151 of `mod.rs`).

Result: during heap extraction (often the majority of wall-clock time for large dumps), the scan bar sits frozen at whatever the last throttled position was from `scan_records`. Then it jumps to 100% after all segments are done. Users will think the tool is hung.

The spec also fails to finish the scan bar before the segment bar appears, violating **AC-6** ("scan bar finishes before segment bar appears"). There is no code in `CliProgressObserver` to detect the phase transition -- `on_bytes_scanned` just calls `set_position` and `on_segment_completed` lazily creates a new bar. Both will be visible simultaneously.

**Fix needed:** Either (a) emit a final `bytes_scanned(total)` at the END of `scan_records` and before `extract_all`, finishing the scan bar at the phase boundary, or (b) have `on_segment_completed`'s first call finish-and-clear the scan bar.

---

### 2. `Engine::from_file` example won't compile -- mutable reference to inline temporary

**Severity: High**

**Spec ref:** Task 4, lines 611-621.

The spec shows:
```rust
let mut notifier = ProgressNotifier::new(
    &mut NullProgressObserver,
);
```

`NullProgressObserver` is a temporary. `ProgressNotifier::new` takes `&'a mut dyn ParseProgressObserver`, where `'a` must outlive the `notifier` binding. But the temporary `NullProgressObserver` is dropped at the end of the statement (it is not subject to temporary lifetime extension because the reference passes through a function call, not a direct `let` binding).

This will produce a compile error: "temporary value dropped while borrowed."

**Fix:** Must write `let mut null = NullProgressObserver;` as a separate binding before creating `ProgressNotifier`.

---

### 3. `on_names_resolved` contract says "done from 1 to total" but actual values jump by chunk_size

**Severity: Medium**

**Spec ref:** Trait docstring lines 211-215 ("done ranges from 1 to total").
**Source ref:** `engine_impl.rs:242-269` -- `build_thread_cache` uses `chunk_size = (total / 10).clamp(1, 50)` and reports `cache.len()` after each chunk.

For 33 threads with chunk_size=4, the observer receives `done` values of 4, 8, 12, ..., 32, 33. NOT 1, 2, 3, ..., 33. The docstring's "done ranges from 1 to total" implies per-item granularity that does not exist.

This also means the `on_segment_completed` pattern ("done increases by 1 each call, from 1 to total") and the `on_names_resolved` pattern have fundamentally different calling conventions, despite identical signatures. One increments by 1, the other by chunk_size. The spec treats them as interchangeable in the trait design but they are not.

**Impact:** Tests asserting sequential 1..N values for names_resolved will fail. Implementors may build UI assumptions around per-item updates.

---

### 4. `maybe_report_progress` spec is ambiguous about relative vs absolute position

**Severity: Medium**

**Spec ref:** Task 2, lines 408-421.
**Source ref:** `hprof_primitives.rs:32-46`.

The spec proposes `maybe_report_progress` takes `pos: usize` and `base_offset: u64` and calls `notifier.bytes_scanned(base_offset + pos as u64)`. But the throttling logic compares `pos.saturating_sub(*last_progress_bytes)` -- this uses the relative `pos`, not the absolute position.

The spec never shows the updated function body. An implementor could reasonably add `base_offset` to `pos` before the throttle check, which would break the interval calculation (first call would pass a huge delta, subsequent calls would have correct deltas only if `last_progress_bytes` is also in absolute terms). The correct approach is: throttle on relative `pos`, report absolute `base_offset + pos`. This is implicit but never stated.

---

### 5. `on_bytes_scanned` fires after all `on_segment_completed` calls -- confusing event ordering

**Severity: Medium**

**Spec ref:** Task 2 line 436, Task 3 lines 471-493.
**Source ref:** `mod.rs:148-151`.

The execution order in `run_first_pass` is:
1. `scan_records` -- emits multiple `on_bytes_scanned`
2. `extract_all` -- emits `on_segment_completed(1..N, N)`
3. Final `notifier.bytes_scanned(base_offset + cursor_position)` -- emits one more bytes_scanned

So the event stream is: `[bytes, bytes, ..., segment, segment, ..., bytes]`. A bytes event arrives AFTER all segments are done. The `CliProgressObserver` would update a scan bar that has been superseded by (and is visually below) the segment bar. If the segment bar called `finish_and_clear()` on `done == total` (which the spec does, line 675), the scan bar then gets a belated update.

**Fix:** Move the final `bytes_scanned` call to the end of `scan_records`, before `extract_all`.

---

### 6. No mechanism to print "Loaded in X (Y GB/s)" summary line

**Severity: Medium**

**Spec ref:** Task 5, lines 704-724 (the `finish()` method).
**Source ref:** `main.rs:56-58` -- currently uses `reporter.elapsed_summary()` to get elapsed time and total bytes.

The spec's `CliProgressObserver.finish()` method (lines 706-724) prints the summary line directly. But the current code prints it from `main.rs` using `reporter.elapsed_summary()`. The spec replaces this with `eprintln!("Loaded in ...")` inside `finish()`, which means the summary goes to stderr. The current code uses `println!` (stdout, `main.rs:58`). This is a silent behavior change: the summary line moves from stdout to stderr.

**Severity adjusted to Low** -- not a bug, but a behavioral change that should be intentional.

---

### 7. AC-8 hardcodes segment count as "currently 29"

**Severity: Low**

**Spec ref:** AC-8, line 799.

The acceptance criterion asserts `on_segment_completed` is called "exactly N times (where N = `heap_record_ranges.len()`, currently 29 for the test asset)." The parenthetical "currently 29" is a comment, not a hardcoded assertion, but it invites the test author to write `assert_eq!(segment_events.len(), 29)`. The test should derive the expected count from the parsed `HprofFile`'s `heap_record_ranges.len()`, never from a literal.

---

### 8. `CliProgressObserver` in `hprof-tui` conflates TUI and CLI concerns

**Severity: Low**

**Spec ref:** Task 5, file list ("crates/hprof-tui/src/progress.rs").
**Source ref:** `hprof-tui/src/progress.rs` -- currently owns `ProgressReporter`.

`hprof-tui` is the ratatui TUI crate. Placing an `indicatif`-based CLI progress observer there means the TUI crate depends on `indicatif` for a non-TUI use case. The `CliProgressObserver` only exists for the CLI binary (`main.rs`). It would be more natural in `hprof-cli` (the binary crate). The current placement exists because `ProgressReporter` was already there, but this refactor is the opportunity to fix the layering.

---

### 9. `ProgressNotifier` adds zero value over `&mut dyn ParseProgressObserver`

**Severity: Low**

**Spec ref:** Task 1, lines 240-283; Technical Decisions lines 133-136.

The spec justifies `ProgressNotifier` as avoiding "re-importing the trait." But internal functions that accept `&mut ProgressNotifier` could equally accept `&mut dyn ParseProgressObserver` -- the trait is in the shared `hprof-api` crate that all consumers already depend on. The newtype adds method name divergence (`bytes_scanned` vs `on_bytes_scanned`), an extra type to maintain, and no actual benefit. Every method is a trivial 1:1 delegation.

The "eliminates monomorphisation" argument (line 136) is irrelevant -- `&mut dyn` already does dynamic dispatch. The newtype doesn't add or remove any monomorphisation.

---

### 10. `extract_heap_segment_with_progress` removal leaves a dead import

**Severity: Low**

**Spec ref:** Task 3, lines 500-501.
**Source ref:** `heap_extraction.rs:11` -- imports `maybe_report_progress`.

After removing all `maybe_report_progress` call sites from `heap_extraction.rs`, the import on line 11 becomes dead code. The spec doesn't mention this cleanup. Will cause a compiler warning.

---

## Summary

| # | Issue | Severity |
|---|-------|----------|
| 1 | Scan bar stalls during extraction, UX regression, violates AC-6 | High |
| 2 | `Engine::from_file` example won't compile (temporary lifetime) | High |
| 3 | `on_names_resolved` done values jump by chunk_size, not 1-to-N | Medium |
| 4 | `maybe_report_progress` body ambiguous (relative vs absolute) | Medium |
| 5 | Final `on_bytes_scanned` fires after all segment events | Medium |
| 6 | Summary line moves from stdout to stderr silently | Low |
| 7 | AC-8 invites hardcoded segment count | Low |
| 8 | `CliProgressObserver` belongs in `hprof-cli`, not `hprof-tui` | Low |
| 9 | `ProgressNotifier` newtype adds no value | Low |
| 10 | Dead `maybe_report_progress` import after removal | Low |
