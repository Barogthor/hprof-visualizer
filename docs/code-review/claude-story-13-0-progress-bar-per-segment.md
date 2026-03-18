# Code Review — Story 13.0: Progress Bar Per-Segment

**Date:** 2026-03-18
**Reviewer:** Claude Sonnet 4.6 (Amelia — Dev Agent)
**Story File:** `docs/implementation-artifacts/13-0-progress-bar-intra-segment.md`
**Status after review:** done

---

## Summary

| Severity | Count | Fixed |
|----------|-------|-------|
| 🔴 High | 1 | ✅ |
| 🟡 Medium | 3 | ✅ |
| 🟢 Low | 3 | 1 fixed, 2 deferred |

---

## Git vs Story Discrepancies

- `docs/planning-artifacts/epics.md` modified in git but not listed in story File List → **Fixed** (added to File List).

---

## 🔴 HIGH — AC #2 Violated: Drain Outside Scope

**File:** `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs`

The drain loop (`rx.into_iter().collect()`) was placed **after** `rayon::in_place_scope`, not inside it. Since `in_place_scope` waits for all spawned tasks before returning, by the time the drain ran all workers had already finished. Progress events fired in a burst after each batch, not concurrently during extraction — functionally equivalent to the original `par_iter().collect()` for dumps with a single batch (MemoryBudget::Unlimited).

**Fix:** Moved the drain loop **inside** the scope closure using `rx.iter()` (non-consuming borrow). Progress is reported as each segment arrives from the channel, while slower workers are still active. Merge is deferred to after the scope and sorted by `payload_start` to maintain `SegmentFilterBuilder`'s offset-order requirement.

```rust
// Before: drain outside scope (not concurrent)
rayon::in_place_scope(|s| { ...; drop(tx); });
let mut batch_results: Vec<_> = rx.into_iter().collect();

// After: drain inside scope (concurrent with workers)
rayon::in_place_scope(|s| {
    ...;
    drop(tx);
    for (start, payload_len, result) in rx.iter() {
        bytes_done += payload_len;
        notifier.heap_bytes_extracted(bytes_done, total_heap_bytes);
        batch_results.push((start, payload_len, result));
    }
});
batch_results.sort_unstable_by_key(|(start, _, _)| *start);
for (_, _, result) in batch_results.drain(..) { result.merge_into(ctx); }
```

---

## 🟡 MEDIUM — Test 5.6 Exercised Sequential Path, Not Parallel

**File:** `crates/hprof-parser/src/indexer/first_pass/tests.rs:2743`

Original fixture: 2 × 4-byte instances → well below `PARALLEL_THRESHOLD`. The test always took the sequential path and never exercised the `drop(tx)` deadlock guard.

**Fix:** Replaced with a dedicated 2-thread pool (`rayon::ThreadPoolBuilder::new().num_threads(2)`) and a 34 MB fixture (2 × 17 MB prim arrays, above `PARALLEL_THRESHOLD`). The parallel path is now deterministically triggered, exercising the `drop(tx)` guard inside `in_place_scope`.

---

## 🟡 MEDIUM — `on_segment_completed` Docstring Referenced Removed Code

**File:** `crates/hprof-api/src/progress.rs:33`

Docstring said "incremented in the main thread after `par_iter().collect()`" — the old implementation that story 13.0 replaced.

**Fix:** Updated to "incremented on the main thread after each segment result is merged."

---

## 🟡 MEDIUM — Git Discrepancy: `epics.md` Not in File List

`docs/planning-artifacts/epics.md` appeared in `git diff --name-only` but was absent from the story File List.

**Fix:** Added to story File List.

---

## 🟢 LOW — CLI Module Docstring Inaccurate (Fixed)

**File:** `crates/hprof-cli/src/progress.rs:1`

Said "four indicatif bars: scan bytes, **segment completion**, phase spinners, and name resolution." `segment_bar` was removed in this story; `extraction_bar` (bytes-based) was added.

**Fix:** Updated docstring to describe actual bars: scan bytes, heap bytes extracted, phase spinners, name resolution.

---

## 🟢 LOW — `segment_entry_points` Dedup is O(n²) (Deferred)

**File:** `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs:306`

Linear scan per entry point in `merge_segment_result`. Harmless at ≤29 segments. Deferred — not in scope for this story.

---

## 🟢 LOW — ADR Not Created (Deferred to Merge)

Story Dev Notes explicitly defer `docs/adr/adr-parallel-extraction-progress.md` to merge time. Not a review blocker.

---

## Verdict

All ACs implemented and verified. All HIGH and MEDIUM issues fixed. 436/436 tests pass. Clippy clean.
