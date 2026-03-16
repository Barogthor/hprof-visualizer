# Code Review — Story 10.4: Investigate Post-Extraction RAM Spike

**Date:** 2026-03-15
**Reviewer:** Amelia (Dev Agent, claude-sonnet-4-6)
**Story file:** `docs/implementation-artifacts/10-4-investigate-post-extraction-ram-spike.md`
**Story status:** review → see conclusion

---

## Git vs Story File List Discrepancy

| File | In Git diff | In File List |
|------|-------------|--------------|
| `crates/hprof-parser/src/indexer/mod.rs` | ✅ | ✅ |
| `crates/hprof-parser/src/indexer/first_pass/mod.rs` | ✅ | ✅ |
| `crates/hprof-parser/src/indexer/first_pass/tests.rs` | ✅ | ✅ |
| `docs/planning-artifacts/epics.md` | ✅ | ✅ |
| `docs/implementation-artifacts/sprint-status.yaml` | ✅ | ✅ |
| `docs/report/test-split-categorization-2026-03-13.md` | ✅ | ❌ |

**1 undocumented change** found in git.

---

## AC Validation

| AC | Status | Evidence |
|----|--------|----------|
| AC#1 — structured profiling data per fixture | ✅ IMPLEMENTED | `all_fixtures_profiling` test, `DiagnosticInfo` struct, debug logs in `sort_offsets()` and `finish()` |
| AC#2 — decision documented with explicit thresholds | ✅ IMPLEMENTED | Findings section in story file; `epics.md` root cause updated |
| AC#3 — `manual_large_dump_profiling` via `HPROF_BENCH_FILE` | ✅ IMPLEMENTED | `tests.rs:2976` |

---

## Task Audit

All tasks marked `[x]`. Verification:

- **Task 1.1** `sort_offsets` span: ✅ `mod.rs:222-226` — scoped block with `info_span!("sort_offsets")`
- **Task 1.2** `segment_filter_build` span placement: ✅ `mod.rs:239-241` — wraps `ctx.finish()`
- **Task 1.3** debug logs: ✅ in `sort_offsets()` (len+capacity) and `finish()` (segment count)
- **Task 1.4** `DiagnosticInfo` struct + feature gate: ✅ `indexer/mod.rs:36-47`, captured before `thread_resolution`
- **Task 2.1–2.6** profiling test: ✅ complete with structured log format
- **Task 3.1–3.3** CI tests: ✅ both passing (`cargo test --features test-utils post_extraction`)
- **Task 4.1–4.7** manual test + findings: ✅ documented, decision ACCEPT, no story 10.5

---

## 🔴 CRITICAL ISSUES

None found.

---

## 🟡 MEDIUM ISSUES

### M1 — Undocumented file change in story File List

`docs/report/test-split-categorization-2026-03-13.md` is modified in the git working tree but absent from the story File List.

**File:** `docs/implementation-artifacts/10-4-investigate-post-extraction-ram-spike.md`
**Action:** Add the file to the File List, or confirm the change predates this story.

---

### M2 — Profiling tests hardcode `id_size=8`, never read from file header

Both `all_fixtures_profiling` and `manual_large_dump_profiling` always call
`run_fp(data, 8)` with a hardcoded id_size. The hprof file header stores the
actual id_size (4 or 8 bytes) at bytes `[null_pos+1 .. null_pos+5]`, but the
tests skip this field.

For the current real-world fixtures (all 8-byte ID) this is harmless. But if a
4-byte ID dump is ever profiled via `HPROF_BENCH_FILE`, the test will silently
produce wrong object counts and memory figures without any error.

**Files:** `tests.rs:2886,2894,2999,3008`
**Recommendation:** Parse id_size from header bytes, or at minimum add a comment
that this is a known limitation restricted to 8-byte ID dumps.

---

### M3 — `_seg_filter_span` not scoped in a block (inconsistent with `_sort_span`)

`sort_offsets` is instrumented inside a scoped block so the span drops immediately
after the call:

```rust
// mod.rs:222-226 — correctly scoped
{
    #[cfg(feature = "dev-profiling")]
    let _sort_span = tracing::info_span!("sort_offsets").entered();
    ctx.sort_offsets();
}
```

`segment_filter_build` is NOT scoped, so the span's guard lives until function
return:

```rust
// mod.rs:239-241 — unscoped
#[cfg(feature = "dev-profiling")]
let _seg_filter_span = tracing::info_span!("segment_filter_build").entered();
ctx.finish()
```

Functionally equivalent since `ctx.finish()` is the last statement, but if a
future refactor adds code after `ctx.finish()`, it would silently be attributed
to the `segment_filter_build` span. Apply consistent scoping.

**File:** `crates/hprof-parser/src/indexer/first_pass/mod.rs:239-242`

---

## 🟢 LOW ISSUES

### L1 — `diagnostics_fields_present` does not assert `precise_index_heap_bytes > 0`

The test validates `offsets_len > 0` and `capacity >= len` but does not check
that `precise_index_heap_bytes` is populated. Even a single-instance dump should
result in at least one HashMap entry in `PreciseIndex`, giving a non-zero value.
A missing assertion leaves the `MemorySize` trait path untested.

**File:** `tests.rs:2750-2763`

---

### L2 — Header null-byte fallback in profiling tests is silent

```rust
let hdr_end = raw.iter().position(|&b| b == 0).unwrap_or(18) + 1 + 4 + 8;
```

If no null byte is found (malformed file), the code silently uses offset 31 and
proceeds to parse garbage as hprof records. An `eprintln!` + `continue` / `return`
would make this visible.

**Files:** `tests.rs:2886, 2999`

---

### L3 — 70 GB extrapolation uses hardcoded 1.5× capacity multiplier

```rust
offsets_capacity: (objects_70gb as f64 * 1.5) as usize,
```

This ignores the baseline's actual measured waste ratio. Using
`(objects_70gb as f64 * (b.objects_cap as f64 / b.objects as f64)) as usize`
would extrapolate from the real observed ratio rather than a magic constant.

**File:** `tests.rs:2946`

---

## Test Results

```
test post_extraction_tests::diagnostics_fields_present ... ok
test post_extraction_tests::waste_ratio_bounded_on_synthetic_dump ... ok
test post_extraction_tests::all_fixtures_profiling ... ignored
test post_extraction_tests::manual_large_dump_profiling ... ignored
```

Clippy: clean (no warnings).

---

## Conclusion

All ACs are implemented and CI tests pass. The implementation is solid.
Issues M1–M3 should be addressed before marking done. L1–L3 are optional
quality improvements.

**Recommended story status:** `in-progress` pending resolution of M1–M3.
