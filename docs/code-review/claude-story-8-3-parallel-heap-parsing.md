# Code Review — Story 8.3: Parallel Heap Segment Parsing

**Reviewer:** Claude Opus 4.6
**Date:** 2026-03-08
**Story:** `docs/implementation-artifacts/8-3-parallel-heap-segment-parsing.md`
**Commit:** `48e8c3c feat: Story 8.3 — parallel heap segment parsing via rayon`

## Summary

**Git vs Story Discrepancies:** 2 found
**Issues Found:** 0 Critical, 3 Medium, 2 Low

## Git vs Story File List

| File | Status |
|------|--------|
| `Cargo.lock` | In git, not in story File List (auto-generated, acceptable) |
| `docs/implementation-artifacts/sprint-status.yaml` | In git, not in story File List |

## MEDIUM Issues

### M1: Duplicated sub-record skip logic (DRY violation)

The sub-record tag skip logic (tags `0x01`–`0x09`, plus the variable-length `0x21`/`0x22`/`0x23` skip patterns) is copy-pasted across **4 functions**:

1. `extract_class_dumps_only` (L748–828)
2. `extract_heap_segment_parallel` (L855–989)
3. `subdivide_segment` (L1028–1087)
4. `extract_heap_object_ids` (L1136–1302)

The fixed-size tags (`0x01`–`0x09`) have identical skip sizes in all 4 functions. The variable-length tags (`0x21`–`0x23`) have identical skip logic in `extract_class_dumps_only` and `subdivide_segment`.

**Impact:** Any future change to sub-record parsing (new sub-tag, size fix) must be synchronized across 4 locations. Bug risk increases linearly with copy count.

**Suggested fix:** Extract a `skip_sub_record(cursor, sub_tag, id_size) -> bool` helper for the common fixed-size tags, and a `skip_variable_sub_record(cursor, sub_tag, id_size) -> bool` for the full skip (used by `extract_class_dumps_only` and `subdivide_segment`). The extraction functions that DO parse (not just skip) keep their custom arms.

### M2: No pre-allocation for parallel worker Vecs

`HeapSegmentResult` Vecs are created with `Vec::new()` (L845–850) — zero initial capacity. For a 16 MB sub-divided chunk with potentially hundreds of thousands of sub-records, this causes repeated reallocations.

**Contrast:** The main `all_offsets` Vec at L135 uses `Vec::with_capacity((data.len() / 80).min(8_000_000))` — Story 8.1 added this optimization. The parallel workers don't benefit from it.

**Suggested fix:** Estimate capacity from `payload.len()`:
```rust
let est = payload.len() / 40;
all_offsets: Vec::with_capacity(est),
filter_ids: Vec::with_capacity(est),
```

### M3: Warning cap not enforced in parallel path

The sequential path uses `push_warning()` (L85–91) which enforces `MAX_WARNINGS = 100`. The parallel path (`extract_heap_segment_parallel`) pushes warnings directly to `result.warnings` with no cap (L860, L869, L878, etc.). During merge (L517), `result.warnings.extend(seg_result.warnings)` also bypasses the cap.

**Practical impact:** Low — each worker typically produces 0–1 warnings (break-on-error pattern). But it's an inconsistency that could matter on severely corrupted large dumps with many segments.

**Suggested fix:** Use the same `push_warning` helper, or apply a cap during merge.

## LOW Issues

### L1: `small_file_uses_sequential_path` test is not path-discriminating

The test (L2709–2731) only asserts "something was parsed" (`!class_dumps.is_empty() || heap_record_ranges.len() > 0`). It doesn't verify the **sequential** path was taken rather than parallel. Both paths produce identical results by design, so the assertion passes regardless.

**Suggested fix:** Add a metric or use `PARALLEL_THRESHOLD` assertion on data size as the test does (L2721), which is sufficient to prove the sequential branch was entered at L199 (`!defer_heap_extraction`).

### L2: Story File List incomplete

The File List in the Dev Agent Record omits `Cargo.lock` and `sprint-status.yaml`. These are expected side effects but should be documented for traceability.

## AC Validation

| AC | Status | Evidence |
|----|--------|----------|
| AC1: Parallel >= 32 MB | IMPLEMENTED | `PARALLEL_THRESHOLD` at L44, `par_iter` at L497 |
| AC2: Sequential < 32 MB | IMPLEMENTED | `else` branch at L520, inline path at L199 |
| AC3: CLASS_DUMP pre-pass | IMPLEMENTED | `extract_class_dumps_only` at L743, called at L473–479 |
| AC4: Per-worker filter IDs | IMPLEMENTED | `filter_ids: Vec<(usize, u64)>` in HeapSegmentResult, merged L512–514 |
| AC5: Per-worker offset merge | IMPLEMENTED | `all_offsets` Vec, concatenated L511, sorted L546 |
| AC6: Sub-division > 16 MB | IMPLEMENTED | `subdivide_segment` at L1002, `SUB_DIVIDE_THRESHOLD` at L48 |
| AC7: All tests pass | IMPLEMENTED | 369 tests pass, clippy clean, fmt clean |

## Task Completion Audit

All 8 tasks (17 subtasks) marked `[x]` — all verified against implementation. No false claims.
