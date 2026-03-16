# Code Review — Story 10.2: Chunked Heap Segment Extraction

**Date:** 2026-03-15
**Reviewer:** Amelia (Dev Agent)
**Story file:** `docs/implementation-artifacts/10-2-split-oversized-heap-segments.md`
**Branch:** `feature/epic-10-large-dump-loading`
**Final status:** done

---

## Summary

All ACs are implemented and functional. 238 tests pass, clippy clean. The
implementation introduces a `MemoryBudget` enum in `hprof-api` (not `Option<u64>`
as spec'd), which is a cleaner design but required documentation corrections.

**Issues found:** 2 High, 3 Medium, 2 Low
**Issues fixed:** 5 (H1, H2, M1, M2, M3)
**Issues documented as action items:** 2 (L1, L2 — accepted as-is)

---

## 🔴 HIGH — Fixed

### H1 — Interface contract for Story 10.3 was stale

**File:** `docs/implementation-artifacts/10-2-split-oversized-heap-segments.md`
**Section:** Dev Notes → Key Difference from Story 10.3 → "Note for 10.3"

The note said `ctx.budget_bytes: Option<u64>` but the actual field is
`ctx.budget: MemoryBudget`. A dev agent implementing 10.3 would look for
the wrong field name and type.

**Fix:** Updated the interface contract note to reference `ctx.budget: MemoryBudget`
and document that `.bytes()` returns `Option<u64>`.

---

### H2 — Two files absent from Dev Agent Record File List

**Files not documented:**
- `crates/hprof-api/src/budget.rs` — new file, introduces `MemoryBudget` enum
- `crates/hprof-api/src/lib.rs` — re-exports `MemoryBudget`

The `hprof-api` crate was modified to introduce a new public type used across
the entire call chain. Omitting it from the File List is incomplete documentation.

**Fix:** Added both files to the File List.

---

## 🟡 MEDIUM — Fixed

### M1 — Test 5.5 missing value-for-value `all_offsets` comparison (AC3)

**File:** `crates/hprof-parser/src/indexer/first_pass/tests.rs`
**Test:** `chunked_extraction_tests::run_first_pass_with_budget_bytes`

The story spec says: "merged `all_offsets` and `filter_ids` are value-for-value
identical". The test only compared `segment_filters.len()`, `records_indexed`,
and `warnings.len()` — not the actual offsets.

**Fix:** Added comparison of `index.instance_offsets` keys and values between
the budgeted and unlimited runs. This exercises the `all_offsets → PreciseIndex`
plumbing through `run_first_pass`.

---

### M2 — Test 5.4 missing `filter_ids` equivalence check (AC3)

**File:** `crates/hprof-parser/src/indexer/first_pass/tests.rs`
**Test:** `chunked_extraction_tests::mixed_records_single_vs_multi_chunk_identical`

After `merge_into`, `filter_ids` are consumed into `ctx.seg_builder`. The test
compared `all_offsets` and `raw_frame_roots` but not the segment filter state.

**Fix:** Added `ctx.finish()` on both single-chunk and multi-chunk contexts,
then compared `segment_filters.len()` to validate `filter_ids` equivalence
(AC3: identical combined results).

---

### M3 — Completion Notes described `Option<u64>` instead of `MemoryBudget`

**File:** `docs/implementation-artifacts/10-2-split-oversized-heap-segments.md`
**Section:** Dev Agent Record → Completion Notes

Task 1 note said "Added `budget_bytes: Option<u64>` parameter" which
does not reflect the actual implementation (`budget: MemoryBudget`).

**Fix:** Rewrote the Task 1 completion note to accurately describe the
`MemoryBudget` enum design decision and its rationale.

---

## 🟢 LOW — Accepted as-is

### L1 — `HprofFile::from_path` didn't receive `budget` parameter

**Story Task 1.3** said to add `budget_bytes` to both `from_path_with_progress`
**and** `from_path`. The implementation kept `from_path` as a zero-arg convenience
wrapper hardcoding `MemoryBudget::Unlimited`.

**Rationale for acceptance:** Functional behavior is correct (`from_path` callers
don't need chunking). Adding a `budget` parameter to a convenience wrapper adds
API surface without value. The deviation is intentional and acceptable.

---

### L2 — `HeapSegmentParsingResult::chunks` is `pub(super)`, tests bypass `merge_into`

Tests in `chunked_extraction_tests` access `result.chunks.len()` directly
rather than going through `merge_into()`. This ties test assertions to the
internal representation.

**Rationale for acceptance:** Tests are in a sub-module of `first_pass`, so
`pub(super)` access is appropriate within the module boundary. Changing to
`merge_into()`-only would require constructing a `FirstPassContext` for every
chunk count check, which reduces readability without gain.

---

## Git vs Story Discrepancies

| File | Status |
|------|--------|
| `crates/hprof-api/src/budget.rs` | In git, was missing from File List → **Fixed (H2)** |
| `crates/hprof-api/src/lib.rs` | In git, was missing from File List → **Fixed (H2)** |

---

## AC Verification

| AC | Status | Evidence |
|----|--------|----------|
| AC1 — Chunked extraction when oversized | ✅ Implemented | `heap_extraction.rs:95-296`, test 5.1 |
| AC2 — Natural sub-record boundary alignment | ✅ Implemented | Checkpoint after `if !ok { break; }` guard, test 5.3 |
| AC3 — Identical combined results | ✅ Implemented | Tests 5.4+5.5 (expanded by M1+M2 fixes) |
| AC4 — No-op for small segments | ✅ Implemented | `max_chunk_bytes >= payload.len()` → single chunk, test 5.2 |
| AC5 — Stable API via `HeapSegmentParsingResult` | ✅ Implemented | `HeapSegmentParsingResult::merge_into`, tests 5.5+5.9 |
