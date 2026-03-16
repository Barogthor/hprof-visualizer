# Adversarial Review — Story 10.1: Progress Fidelity — Heap Segment Scan

Date: 2026-03-14
Reviewer: Claude (adversarial)
Artifact: `docs/implementation-artifacts/10-1-progress-fidelity-heap-segment-scan.md`

---

## Findings

1. **Test 2.2 description is factually wrong about which events fire.**
   The story says "Assert at least 2 BytesScanned events: one from the fix (after segment jump) and
   one from the final catch-all." This is incorrect. The catch-all at lines 206-208 fires when
   `!reported_any || cursor_pos > last_progress_bytes`. After the fix, `reported_any = true` and
   `last_progress_bytes == cursor_position` (because `maybe_report_progress` sets
   `last_progress_bytes = pos` when it fires, and `pos = cursor.position()` at that moment = end of
   last segment = end of data for a heap-only blob). The catch-all condition evaluates to `false ||
   false` — it does NOT fire. Both events come from the fix, not one from each. The assertion
   description is wrong and will mislead the developer into asserting the wrong thing.

2. **Binary layout reference includes the hprof file header, which should not be in test blobs.**
   The Dev Notes section "Binary layout reference" describes the full hprof file header (`JAVA
   PROFILE 1.0.2\0` + id_size + timestamp). But `run_first_pass` explicitly takes "raw bytes
   starting at the first record (immediately after the hprof file header)" — confirmed in
   `mod.rs:178`. Test blobs for `run_fp_with_test_observer` must NOT include the file header. The
   note saying "31 bytes for id_size=4" for the header is irrelevant noise that could waste
   significant developer time if they construct a blob starting with the hprof header magic bytes.

3. **`PROGRESS_REPORT_INTERVAL` is missing from the `#[cfg(feature = "test-utils")]` import block.**
   The new tests require `PROGRESS_REPORT_INTERVAL` to size their test blobs correctly. The current
   import in `tests.rs:15-17` only imports `PARALLEL_THRESHOLD, gc_root_skip_size,
   parse_class_dump, primitive_element_size, skip_n` from `hprof_primitives`. The constant is
   `pub(super)` so it is accessible, but the developer must add it to the existing import list. The
   story never mentions this.

4. **Task 1.2 is a non-actionable "verify" task with no test coverage.**
   Task 1.2 says "Verify the existing `reported_any` logic at lines 206-208 still works correctly."
   This is vague — there is no unit test that specifically targets the `reported_any` catch-all
   interaction, and no concrete assertion is specified. After the fix, `reported_any` changes
   semantics for heap-only dumps (it becomes true where it was false), which changes the catch-all
   behavior. This behavioral change is underdocumented and untested. Test 2.2 was supposed to cover
   it but is incorrectly described (see finding #1).

5. **Test 2.3 assertion is underspecified.**
   Task 2.3 says "Assert the final `bytes_scanned` catch-all fires." No concrete assertion is given
   — assert one event? Assert the event position equals the end of the blob? Assert
   `events.len() == 1`? The developer has no clear target. A small-segment test should assert
   exactly 1 event at the end-of-data position, but the story doesn't say this.

6. **No test for the case where `maybe_report_progress` fires via the time-based throttle.**
   All 5 tests rely exclusively on the bytes-based throttle path (`pos - last >= 4 MB`). The
   time-based path (`elapsed >= 1 second`) is never exercised. For large dumps where segments are
   spaced < 4 MB apart but time > 1 second elapses, only the time path fires. This is admittedly
   hard to test without time injection, but the story doesn't acknowledge the gap.

7. **AC1 conflates calling `maybe_report_progress` with actually updating the progress bar.**
   AC1 says "maybe_report_progress is called after the cursor jump, and the progress bar reflects
   the new file position." But `maybe_report_progress` is throttled — it can be called without
   calling `bytes_scanned` if both the byte and time thresholds are unmet. AC1 as written implies
   the bar always updates after every heap segment, which is false. The AC should distinguish
   between the call and the actual update.

8. **NFR13 ("update at least every 2 seconds") is not testable but the story's reasoning for why it
   is satisfied is thin.**
   The story says NFR13 is "a consequence of the fix" verified by "manual testing on large dumps."
   But it provides no manual test procedure or pass/fail criteria. A 4 GB heap segment takes O(ns)
   for a cursor jump — the 1-second time throttle does not guarantee the bar updates every 2
   seconds if multiple large segments are scanned faster than 1 second (which they are, since cursor
   jumps are O(1)). NFR13 is satisfied per-segment, not per-elapsed-time. The reasoning should be
   explicit: "one `BytesScanned` per segment guarantees the bar advances at each segment boundary,
   eliminating the freeze regardless of elapsed time."

9. **The fix pattern's `let pos = cursor.position() as usize` is redundant after
   `cursor.set_position(payload_end as u64)`.**
   After `cursor.set_position(payload_end as u64)`, `cursor.position() == payload_end as u64`
   always. The `pos` binding is therefore always `payload_end`. While mirroring the existing unknown-
   tag pattern is intentional, the story's Dev Notes do not note this redundancy, which means a
   developer might wonder if `cursor.position()` can differ from `payload_end` and write incorrect
   tests that assert `pos != payload_end`.

10. **Story has no test for the scenario where heap segments are NOT the last records in the file.**
    Tests 2.1–2.5 all end with a heap segment (or have heap-only blobs). If structural records follow
    the last heap segment, the catch-all at line 207 does fire (because `cursor_pos >
    last_progress_bytes` after processing post-heap structural records). Test 2.1 constructs
    `header + STRING + HeapDumpSegment + STRING` which does have a trailing STRING, but it
    primarily asserts the heap segment produces an event — it does not assert the interaction
    between the trailing STRING's progress report and the catch-all. This edge case is untested.

11. **The story hardcodes "~4 MB allocation is fine for a unit test" without acknowledging CI
    memory pressure.**
    Tests 2.1–2.5 require allocating 4+ MB blobs per test, with Test 2.2 and 2.4 requiring two
    segments = 8+ MB each. On memory-constrained CI runners, this adds noticeable heap allocation
    to the test suite. The story does not suggest using the minimum payload (exactly 4 MB, not
    "4 MB+"), nor does it consider whether a smaller `PROGRESS_REPORT_INTERVAL` mock or const
    override would be cleaner than allocating real 4 MB buffers in tests.

12. **The `Dev Agent Record` section retains the `{{agent_model_name_version}}` template
    placeholder.**
    Minor but indicates the story template was not fully materialized. This placeholder will appear
    verbatim in the record unless the dev agent fills it in, which is easy to miss.
