# Story 10.1: Progress Fidelity — Heap Segment Scan

Status: done

## Story

As a user,
I want the progress bar to reflect the real file position during
the first-pass scan, even when heap segments are skipped via
cursor jumps,
so that I can see the tool is working and estimate how long
loading will take.

## Acceptance Criteria

1. **AC1 – Progress after heap segment skip:**
   Given a heap dump where heap segments represent >90% of
   total bytes,
   When the first-pass scan processes a `HeapDump` or
   `HeapDumpSegment` record,
   Then `maybe_report_progress` is called after the cursor
   jump. The progress bar reflects the new file position
   whenever the bytes or time throttle threshold is
   satisfied (4 MB elapsed or 1 second elapsed).

2. **AC2 – Progress callback on every heap segment:**
   Given a 70 GB dump with multiple heap segments (each
   segment is at most ~4 GB due to `u32` payload length),
   When the scan jumps over each segment,
   Then `maybe_report_progress` is called after each jump.
   Since each heap segment payload is >= 4 MB (the bytes
   throttle interval), the call fires and advances the bar
   at every segment boundary. This eliminates the freeze
   regardless of elapsed time.
   **Note:** NFR13 ("update every 2s") is a consequence of
   the per-segment callback — not directly testable in unit
   tests (no real clock). Cursor jumps are O(1) and do not
   trigger the 1-second time throttle. Verified by manual
   testing on large dumps.

3. **AC3 – No regression on small dumps:**
   Given a small dump (< 100 MB),
   When scanned,
   Then behavior is identical to current (no regression, no
   extra overhead).

**FRs covered:** FR57
**NFRs verified:** NFR13

## Tasks / Subtasks

- [x] Task 1: Add `maybe_report_progress` call in heap
        segment branch (AC: #1, #2)
  - [x] 1.1 In `record_scan.rs:130-137`, apply the fix
        shown in the "Fix Pattern" code block below. Add
        `maybe_report_progress` after `set_position`, before
        `continue`. See Dev Notes for exact code.
  - [x] 1.2 Verify the existing `reported_any` logic at the
        end of `scan_records` (lines 206-208) still works
        correctly. The catch-all fires when
        `!reported_any || cursor_pos > last_progress_bytes`.
        After the fix, for a heap-only dump `reported_any`
        becomes `true` and `last_progress_bytes ==
        cursor_position` after the last segment — so the
        catch-all does NOT fire. This is correct: the final
        position was already reported by the fix. Verify via
        Test 2.2.

- [x] Task 2: Tests (AC: #1, #2, #3)
  - [x] 2.1 Unit test: construct a binary blob with header +
        1 STRING record + 1 large `HeapDumpSegment` record
        (payload >= `PROGRESS_REPORT_INTERVAL` i.e. 4 MB) +
        1 STRING record. Run `scan_records` (via `run_fp_with_
        test_observer`). Assert `BytesScanned` events include
        positions AFTER the heap segment skip — not just the
        positions of the STRING records. Specifically:
        `events` must contain at least one `BytesScanned(pos)`
        where `pos >= heap_segment_end`.
        **Critical:** the heap segment payload must be >= 4 MB
        to exceed `PROGRESS_REPORT_INTERVAL`, otherwise the
        throttle suppresses the callback and the test passes
        for the wrong reason. The blob must be allocated at
        full size (`vec![0u8; total_size]`) because
        `scan_records` validates `payload_end <= data.len()`
        (line 111) before jumping. A header declaring 4 MB
        with a shorter actual blob will break with "payload
        end exceeds file size". ~4 MB allocation is fine for
        a unit test.
  - [x] 2.2 Unit test: construct a blob with ONLY 2 heap
        segments (no structural records), each with payload
        >= `PROGRESS_REPORT_INTERVAL`. Allocate the full
        blob size. Run `scan_records`.
        Assert **exactly 2** `BytesScanned` events, both
        from the new code path (one per segment). The catch-
        all at line 207 does NOT fire in this scenario:
        after the fix sets `last_progress_bytes =
        cursor_position` for the last segment, the catch-all
        condition `!reported_any || cursor_pos >
        last_progress_bytes` is false.
        **Before the fix:** only 1 event (from the catch-
        all, since `reported_any` was always false for heap-
        only dumps). This validates the frozen-bar scenario.
  - [x] 2.3 Regression test: construct a small blob with 1
        heap segment whose payload is exactly 1 byte (well
        under `PROGRESS_REPORT_INTERVAL`). Allocate the full
        declared size. Run `scan_records`. Assert exactly
        **1** `BytesScanned` event at position = end of blob
        (record_header_size + 1). This is the catch-all
        event — the fix's `maybe_report_progress` call does
        not fire (below bytes threshold, and O(1) cursor
        jump does not exceed the 1-second time threshold).
        Validates no regression on small dumps.
  - [x] 2.4 Unit test: construct a blob with multiple heap
        segments (each >= `PROGRESS_REPORT_INTERVAL`)
        interleaved with structural records. Assert
        `BytesScanned` events are monotonically increasing
        and include positions after each segment skip.
  - [x] 2.5 Unit test: same as 2.1 but using `HeapDump`
        tag (`0x0C`) instead of `HeapDumpSegment` (`0x1C`).
        The `matches!` covers both, but a dedicated test
        ensures coverage of the non-segmented variant.

## Dev Notes

### Root Cause

In `record_scan.rs:130-137`, the `HeapDump` /
`HeapDumpSegment` branches call
`cursor.set_position(payload_end)` and then `continue`
without calling `maybe_report_progress`. On a 70 GB dump
where heap segments represent ~95% of bytes, the scan_bar
stays frozen at ~2-5% (the position of the last STRING
record) until the loop finishes.

```rust
// Current code (record_scan.rs:130-137):
if matches!(
    tag,
    RecordTag::HeapDump | RecordTag::HeapDumpSegment
) {
    ctx.result.heap_record_ranges.push(HeapRecordRange {
        payload_start: payload_start as u64,
        payload_length: header.length as u64,
    });
    cursor.set_position(payload_end as u64);
    continue; // ← BUG: no maybe_report_progress call
}
```

### Fix Pattern

The fix mirrors the existing pattern used for unknown tags
(lines 147-157) and structural records (lines 194-202):
after advancing the cursor, call `maybe_report_progress`
with the new position.

```rust
// Fixed code:
if matches!(
    tag,
    RecordTag::HeapDump | RecordTag::HeapDumpSegment
) {
    ctx.result.heap_record_ranges.push(HeapRecordRange {
        payload_start: payload_start as u64,
        payload_length: header.length as u64,
    });
    cursor.set_position(payload_end as u64);
    let pos = cursor.position() as usize;
    reported_any |= maybe_report_progress(
        pos,
        ctx.base_offset,
        &mut ctx.last_progress_bytes,
        &mut ctx.last_progress_at,
        notifier,
    );
    continue;
}
```

### `maybe_report_progress` Throttling

`maybe_report_progress` already throttles by both bytes
(4 MB interval) and time (1 second max). Adding it in the
heap segment branch does NOT cause excessive callbacks —
most heap segments are large enough that only 1 call fires
per segment. On small dumps with small segments, the
throttle prevents any extra overhead.

### Key Files to Modify

| File | Purpose |
|------|---------|
| `crates/hprof-parser/src/indexer/first_pass/record_scan.rs` | Add `maybe_report_progress` call after heap segment cursor jump (lines 130-137) |
| `crates/hprof-parser/src/indexer/first_pass/tests.rs` | New tests validating progress events after heap segment skips |

### Test Infrastructure

Tests use the existing `run_fp_with_test_observer` helper
(tests.rs:33-40) which returns a `TestObserver` with a
`Vec<ProgressEvent>`. Filter for
`ProgressEvent::BytesScanned(_)` variants.

To construct test blobs, use the existing binary builder
pattern from the test file: `WriteBytesExt` to write hprof
headers, record tags, and payload lengths. Use
`RecordTag::HeapDumpSegment` (tag `0x1C`) for heap segment
records.

**Binary layout reference:**
- `run_first_pass` takes data **immediately after** the
  hprof file header (see `mod.rs` docstring). Test blobs
  must NOT include the file header — start directly at
  the first record.
- record header: 1-byte tag + 4-byte timestamp (zeros)
  + 4-byte payload length = **9 bytes**
- `header.length` is `u32` — max payload per record is
  ~4 GB. A 70 GB dump has 29+ heap segment records.

`run_fp_with_test_observer` calls
`run_first_pass(data, id_size, 0, &mut notifier)` —
`base_offset` is always **0** in tests. All
`BytesScanned(pos)` values are absolute offsets into the
test blob (no offset adjustment needed in assertions).

The `test-utils` feature flag must be enabled for
`TestObserver` and `run_fp_with_test_observer` to be
available. Tests should be gated with
`#[cfg(feature = "test-utils")]` if they use these helpers.

Add `PROGRESS_REPORT_INTERVAL` to the existing
`hprof_primitives` import in `tests.rs`:
```rust
use super::hprof_primitives::{
    PARALLEL_THRESHOLD, PROGRESS_REPORT_INTERVAL,
    gc_root_skip_size, parse_class_dump,
    primitive_element_size, skip_n,
};
```

`HeapDumpEnd` (tag `0x2C`) is an unrelated tag handled by
the unknown-tag branch (lines 139-157) which already calls
`maybe_report_progress`. It is not in scope for this story.

### Scope Boundaries

This story fixes **only** the scan_records progress hole
(trou L1). Two other progress holes exist in the loading
pipeline but are **explicitly out of scope:**

- **extract_all** (first parallel batch completes without
  `segment_completed` signal) → addressed by stories
  10.2 / 10.3
- **sort_offsets + seg_builder.finish()** (post-extraction
  silence) → addressed by story 10.4

Do NOT attempt to fix these in this story.

### Architecture Note

The first-pass runs **synchronously on the main thread**
(`main.rs:109-114`). The progress bar works because
`indicatif::MultiProgress` has its own internal render
thread. The `on_bytes_scanned` callback updates the bar
position, and indicatif redraws automatically. No threading
refactor is needed for this fix.

### Test Blob Size Guardrail

`maybe_report_progress` throttles at 4 MB
(`PROGRESS_REPORT_INTERVAL`) and 1 second
(`PROGRESS_REPORT_MAX_INTERVAL`). Tests that validate
the new callback **must** use heap segment payloads
>= 4 MB in their declared `header.length`.

**Important:** `scan_records` validates
`payload_end <= data.len()` (line 111) before jumping.
A header declaring 4 MB with a shorter actual blob will
**not** reach the heap segment branch — it will break
with "payload end exceeds file size". The blob must be
allocated at full declared size (e.g.
`vec![0u8; record_header + 4MB_payload]` where
`record_header = 9`). Note: test blobs do not include the
hprof file header — `run_first_pass` receives data
starting at the first record. ~4 MB allocation is
acceptable for unit tests.

### Tag Coverage

Both `HeapDump` (`0x0C`) and `HeapDumpSegment` (`0x1C`)
are matched by the same `matches!` branch. jvisualvm
dumps use `0x1C` exclusively, but `0x0C` is valid per
spec. Test 2.5 covers the `HeapDump` variant explicitly.

### Project Structure Notes

- Single file change (`record_scan.rs`) + test additions
- No new modules, no new dependencies
- No changes to public API or `ProgressNotifier` trait
- Aligns with existing progress reporting pattern in the
  same function

### References

- [Source: docs/report/large-dump-ux-observations-2026-03-14.md#L1]
- [Source: crates/hprof-parser/src/indexer/first_pass/record_scan.rs:130-137]
- [Source: crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs:37-55 — maybe_report_progress]
- [Source: crates/hprof-parser/src/indexer/first_pass/tests.rs:33-40 — run_fp_with_test_observer]
- [Source: crates/hprof-api/src/progress.rs:100-117 — TestObserver]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

None — clean implementation, no debug issues.

### Completion Notes List

- Added `maybe_report_progress` call after `cursor.set_position` in the `HeapDump`/`HeapDumpSegment` branch of `scan_records`, mirroring the existing pattern used for unknown tags and structural records.
- The fix updates `reported_any` so the catch-all at end-of-loop correctly skips when the last segment already reported its position.
- 5 tests added covering: mixed records (2.1), heap-only dumps (2.2), small segment regression (2.3), interleaved monotonic ordering (2.4), and HeapDump tag variant (2.5).
- Test 2.2 confirmed the bug: before the fix, only 1 BytesScanned event (catch-all) instead of 2 (one per segment).
- All 855 existing tests pass, clippy clean.

### File List

- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs` — added `maybe_report_progress` call in heap segment branch
- `crates/hprof-parser/src/indexer/first_pass/tests.rs` — added `PROGRESS_REPORT_INTERVAL` import + 5 new progress tests
- `docs/implementation-artifacts/sprint-status.yaml` — updated 10-1 status to review

## Change Log

- 2026-03-14: Story 10.1 implemented — fixed progress bar freeze during heap segment scan by adding `maybe_report_progress` call after cursor jumps over HeapDump/HeapDumpSegment records. 5 unit tests added.
- 2026-03-14: Code review fixes — added `#[cfg(feature = "test-utils")]` gate on `build_record_header` helper and `RECORD_HEADER_SIZE` constant; documented timing assumption in test 2.2 docstring; added `sprint-status.yaml` to File List.
