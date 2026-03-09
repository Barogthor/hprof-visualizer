# Code Review: ParseProgressObserver Trait Implementation

**Reviewer:** Claude (adversarial review)
**Date:** 2026-03-09
**Scope:** Diff introducing `hprof-api` crate with `ParseProgressObserver` trait, replacing closure-based progress reporting.

---

## Findings

### 1. `gc_root_skip_size` wrong for `GcRootJniGlobal` (1 ID instead of 2)

**Severity:** High
**Validity:** Real (pre-existing bug, not introduced by this diff)

In `hprof_primitives.rs:87`:
```rust
GcRootJniGlobal | GcRootThreadBlock => Some(id),
```

Per the hprof spec, `GC_ROOT_JNI_GLOBAL` (0x01) has **two** ID fields: `object_id` + `jni_global_ref_id`. The correct skip size is `2 * id_size`. The same bug exists in `hprof_file.rs:419-420` (`skip_sub_record`). When the parser encounters JNI global roots, it under-skips, misaligning all subsequent sub-records in the same segment. This corrupts heap extraction for any dump containing JNI globals.

---

### 2. `gc_root_skip_size` wrong for `GcRootJniLocal` when `id_size=4`

**Severity:** High
**Validity:** Real (pre-existing bug)

In `hprof_primitives.rs:88`:
```rust
GcRootJniLocal => Some(2 * id),
```

Per the hprof spec, `GC_ROOT_JNI_LOCAL` (0x02) has: `object_id` (ID) + `thread_serial` (u32) + `frame_number` (u32) = `id_size + 8`. The code uses `2 * id_size` which equals 8 for `id_size=4` instead of the correct 12. For `id_size=8` the values happen to coincide (16 = 16), masking the bug on 64-bit JVMs. Same bug in `skip_sub_record` in `hprof_file.rs:422`.

---

### 3. Module docstring references old `progress_fn` callback

**Severity:** Low
**Validity:** Real

`crates/hprof-parser/src/indexer/first_pass/mod.rs` lines 14-18:
```
//! Progress is reported via the `progress_fn` callback,
//! which receives the current byte offset (relative to
//! `data`) every [`PROGRESS_REPORT_INTERVAL`] bytes...
```

This is stale. The function now takes a `ProgressNotifier`, the offset is absolute (not relative to `data`), and there are three distinct signal types. Should be updated to reflect the new `ProgressNotifier` API.

---

### 4. `CliProgressObserver::finish()` produces `inf`/`NaN` on zero-duration parse

**Severity:** Medium
**Validity:** Real

`crates/hprof-cli/src/progress.rs:59`:
```rust
let gb_per_sec = self.total_bytes as f64
    / elapsed.as_secs_f64()
    / 1_073_741_824.0;
```

For a tiny file parsed in sub-microsecond time, `as_secs_f64()` returns `0.0`, producing `Infinity`. The output becomes `Loaded in 0.0ns (inf GB/s)`. Not a crash (Rust doesn't panic on float division by zero), but the UX is poor. Fix with a guard: if `elapsed < 1us`, skip the throughput or display "N/A".

---

### 5. `scan_bar.finish_and_clear()` called twice when segments exist

**Severity:** Low
**Validity:** Real

In `CliProgressObserver`:
- `on_segment_completed` calls `self.scan_bar.finish_and_clear()` inside `get_or_insert_with` (line 74).
- `finish()` calls `self.scan_bar.finish_and_clear()` again (line 51).

The double call is harmless (indicatif handles it gracefully) but redundant.

---

### 6. `segment_bar` finished in callback but not taken -- `finish()` finishes it again

**Severity:** Low
**Validity:** Real

When `done == total` in `on_segment_completed` (line 90-92), the bar is finished but remains as `Some(...)` in `self.segment_bar`. Later, `finish()` calls `self.segment_bar.take()` and calls `finish_and_clear()` a second time. Again harmless but wasteful. Could `take()` in the callback.

---

### 7. `name_bar` never finished inside `on_names_resolved`

**Severity:** Low
**Validity:** Real

Unlike `on_segment_completed` which finishes the bar when `done == total`, `on_names_resolved` never checks for completion. The bar is only cleaned up in `finish()`. If `finish()` is not called (error path), the bar remains visually active. In the current code `finish()` is always called, so no practical impact.

---

### 8. `on_names_resolved` called from engine layer, not parser layer

**Severity:** Medium
**Validity:** Real (design concern)

The trait is named `ParseProgressObserver` but:
- `on_bytes_scanned` and `on_segment_completed` are called from `hprof-parser`
- `on_names_resolved` is called from `hprof-engine` (`engine_impl.rs:272`)

This means the observer spans two architectural layers. The parser's `run_first_pass` function signature accepts a `ProgressNotifier` that it passes through but never calls `names_resolved` on. The engine creates a second `ProgressNotifier` wrapping the same observer after `from_path_with_progress` returns. This works but the trait name is misleading -- it is not purely a "parse" progress observer.

---

### 9. `open_hprof_file_with_progress` docstring references old `progress_fn`

**Severity:** Low
**Validity:** Real

`hprof-engine/src/lib.rs` lines 62-64:
```
/// `progress_fn(bytes)` -- absolute file offset every 4 MiB...
```
Parameter is now `observer: &mut dyn ParseProgressObserver`. The docstring references a removed parameter name.

---

### 10. `TestObserver` exists but is never used in integration tests

**Severity:** Medium
**Validity:** Real

The `TestObserver` and `ProgressEvent` types in `hprof-api` are gated behind `feature = "test-utils"` and correctly activated in `[dev-dependencies]`. However, no test actually uses `TestObserver` to verify the event sequence from a real parse. All observer tests use ad-hoc `CountingObserver` structs instead.

This means:
- `TestObserver` is dead test infrastructure
- No test verifies the event ordering contract (BytesScanned monotonically increasing, followed by SegmentCompleted 1..N, followed by NamesResolved)
- No test verifies that `on_segment_completed` is called exactly `heap_record_ranges.len()` times

---

### 11. `ProgressNotifier` adds no value over `&mut dyn ParseProgressObserver`

**Severity:** Low
**Validity:** Undecided

`ProgressNotifier` is a newtype over `&mut dyn ParseProgressObserver` that provides identical methods with slightly different names (`bytes_scanned` vs `on_bytes_scanned`). The spec's justification ("avoids re-importing the trait") is weak -- all crates already depend on `hprof-api`. The "eliminates monomorphisation" argument is irrelevant since `&mut dyn` already does dynamic dispatch. The newtype introduces naming divergence and an extra type to maintain for no functional benefit.

However, it does serve as a clear "this is the internal plumbing type" marker, and the naming divergence (dropping the `on_` prefix) arguably improves call-site readability. This is a style/taste issue.

---

### 12. `ProgressNotifier` is `!Send` -- correct but undocumented invariant

**Severity:** Low
**Validity:** Undecided

`ProgressNotifier` wraps `&mut dyn ParseProgressObserver` which is `!Send + !Sync`. This correctly prevents progress reporting from worker threads (segment extraction uses `par_iter().collect()` then reports from the main thread). Any future attempt to report from workers will fail at compile time. This is good defensive design but the invariant is not documented.

---

### 13. `engine_impl.rs:216` has broken doc comment

**Severity:** Low
**Validity:** Real

Line 216 in the diff output:
```
/ See [`Engine::from_file`].
```
This is `/ ` (single slash) instead of `/// ` (triple slash doc comment). This is a formatting error that makes the line a regular comment instead of a doc comment.

Similarly, lines 194-196:
```
/ - [`HprofError::MmapFailed`]...
/ - [`HprofError::UnsupportedVersion`]...
/ - [`HprofError::TruncatedRecord`]...
```
These are also single-slash comments instead of triple-slash doc comments.

---

## Summary

| # | Finding | Severity | Validity |
|---|---------|----------|----------|
| 1 | `GcRootJniGlobal` skip size wrong (1 ID instead of 2) | High | Real (pre-existing) |
| 2 | `GcRootJniLocal` skip size wrong for `id_size=4` | High | Real (pre-existing) |
| 3 | Module docstring still references old `progress_fn` | Low | Real |
| 4 | Division by zero in `finish()` for instant completion | Medium | Real |
| 5 | `scan_bar.finish_and_clear()` called twice | Low | Real |
| 6 | `segment_bar` finished twice | Low | Real |
| 7 | `name_bar` never finished inside callback | Low | Real |
| 8 | Observer trait spans two architectural layers | Medium | Real |
| 9 | Stale `progress_fn` reference in engine lib.rs docstring | Low | Real |
| 10 | No integration test using `TestObserver` | Medium | Real |
| 11 | `ProgressNotifier` newtype adds no value | Low | Undecided |
| 12 | `ProgressNotifier` is `!Send` (undocumented) | Low | Undecided |
| 13 | Broken doc comments (single `/` instead of `///`) | Low | Real |

### Actionable items for this diff:
1. **Fix stale docstrings** (#3, #9) -- quick text updates
2. **Guard against zero-duration division** (#4) -- add `if elapsed > Duration::ZERO` check
3. **Add integration test using `TestObserver`** (#10) -- parse a real file, assert event sequence
4. **Fix broken doc comments** (#13) -- `/ ` to `/// `
5. **Minor cleanup** of double-finish patterns (#5, #6, #7)

### Pre-existing bugs surfaced during review (separate fix recommended):
- **GC root skip sizes (#1, #2)** -- will cause silent data corruption when parsing files with JNI global roots on 32-bit JVMs (`id_size=4`), and JNI globals on any JVM. These bugs exist in both `gc_root_skip_size` and `skip_sub_record`.
