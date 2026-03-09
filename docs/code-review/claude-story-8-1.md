# Code Review — Story 8.1: FxHashMap, Pre-allocation & all_offsets Optimization

**Reviewer:** Claude Opus 4.6 (Dev Agent — Amelia)
**Date:** 2026-03-08
**Story file:** `docs/implementation-artifacts/8-1-fxhashmap-pre-allocation-and-all-offsets-optimization.md`

## Summary

**Git vs Story Discrepancies:** 4 files not documented in File List
**Issues Found:** 0 High, 4 Medium, 3 Low

## MEDIUM ISSUES

### M1 — Out-of-scope reformatting in `main.rs` and `segment.rs`

`crates/hprof-cli/src/main.rs` and `crates/hprof-parser/src/indexer/segment.rs` have purely cosmetic changes (signature reformatting). No relation to story 8.1. These files are not in the story's File List. This is noise in the diff that complicates future reviews.

### M2 — Cosmetic reformatting mixed with functional changes in `first_pass.rs`

Several lines in `first_pass.rs` were reformatted (tracing spans, signatures) with no connection to the functional changes. For example, `tracing::info_span!` calls collapsed to single lines, `run_first_pass` signature compacted. Mixing reformatting and functional changes in the same commit makes the diff harder to audit.

### M3 — Incomplete File List in story

4 files changed in git but absent from the story File List:
- `Cargo.lock` (acceptable — auto-generated)
- `crates/hprof-cli/src/main.rs` (reformatting)
- `crates/hprof-parser/src/indexer/segment.rs` (reformatting)
- `crates/hprof-parser/benches/first_pass.rs` (path fix + config)

### M4 — `PreciseIndex::new()` no longer used in production

`new()` creates maps without pre-allocation. Only `with_capacity()` is called from `run_first_pass`. `new()` is only used in `precise.rs` tests. This is not a bug, but it is dead code in production — tests should also use `with_capacity()` to exercise the real code path, or `new()` should be `#[cfg(test)]`.

## LOW ISSUES

### L1 — ZGC/Shenandoah test validates FxHashMap in isolation, not the real pipeline

The test `fxhashmap_handles_zgc_shenandoah_common_high_bits` creates a standalone `FxHashMap` and inserts IDs. It does not go through `run_first_pass`, so it does not verify that the real code path handles ZGC IDs correctly. It is more of a `rustc-hash` unit test than a parser regression test. Acceptable as a smoke test but naming could be more honest.

### L2 — `bench_first_pass_total` does not measure `all_offsets` sort in isolation

The bench calls `run_first_pass` which includes the sort. Good — but the story Dev Notes stated "~200ms for 5M entries" for the sort. There is no way to measure this phase in isolation with the current bench. Not blocking for 8.1 but worth considering for 8.3.

### L3 — Undocumented `sub_record_start - 1` offset

```rust
all_offsets.push((obj_id, (sub_record_start - 1) as u64));
```

The `-1` compensates for `sub_record_start` pointing after the sub-tag byte. This is not documented anywhere. The same logic existed before the change, but it remains fragile.
