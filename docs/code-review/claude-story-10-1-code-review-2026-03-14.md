# Code Review — Story 10.1: Progress Fidelity — Heap Segment Scan

**Reviewer:** Amelia (Dev Agent — Claude Opus 4.6)
**Date:** 2026-03-14
**Story:** `docs/implementation-artifacts/10-1-progress-fidelity-heap-segment-scan.md`
**Outcome:** Approved with fixes applied

## AC Validation

| AC  | Status      | Evidence |
|-----|-------------|----------|
| AC1 | IMPLEMENTED | `record_scan.rs:136-143` — `maybe_report_progress` called after `cursor.set_position` in heap segment branch |
| AC2 | IMPLEMENTED | Test 2.2 confirms exactly 2 `BytesScanned` events for 2 segments (one per segment boundary) |
| AC3 | IMPLEMENTED | Test 2.3 confirms 1-byte segment produces only the catch-all event — no regression |

## Task Audit

All 7 tasks/subtasks marked `[x]` — all verified as genuinely complete.

## Git vs Story File List

- `sprint-status.yaml` modified but was missing from File List — **fixed**

## Findings

### MEDIUM (fixed)

**M1 — `sprint-status.yaml` missing from File List**
The sprint-status change (`ready-for-dev` -> `review`) was part of this story's work but undocumented in the Dev Agent Record File List.
**Fix:** Added to File List.

**M2 — Test 2.2 implicit timing dependency undocumented**
`progress_heap_only_dump_two_segments` asserts exactly 2 events. This relies on synchronous execution where only the bytes throttle (4 MB) fires, never the time throttle (1s). Not documented.
**Fix:** Added docstring explaining the synchronous execution assumption.

**M3 — `build_record_header` and `RECORD_HEADER_SIZE` not feature-gated**
These test helpers are only used by `#[cfg(feature = "test-utils")]` tests but were not themselves gated, creating dead code without the feature.
**Fix:** Added `#[cfg(feature = "test-utils")]` to both.

### LOW (not fixed — informational)

**L1 — Test 2.1 weak assertion (only `any >= heap_seg_end`)**
Could verify exact event count for stronger coverage. Acceptable as-is since test 2.2 covers the exact-count scenario.

**L2 — Test 2.5 weak assertion (`!is_empty()`)**
Could assert exactly 1 event. Acceptable since the variant coverage is the primary goal.

**L3 — Redundant `pos` variable in fix**
`let pos = cursor.position() as usize` after `cursor.set_position(payload_end as u64)` — always equals `payload_end`. Consistent with the codebase pattern in other branches.

## Verification

- 860 tests pass (including 5 new story 10.1 tests)
- Clippy clean (`--all-targets -D warnings`)
- No regressions
