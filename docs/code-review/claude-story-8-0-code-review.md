# Code Review: Story 8.0 — Profiling Infrastructure

**Date:** 2026-03-08
**Reviewer:** Amelia (Dev Agent — Claude Opus 4.6)
**Story File:** `docs/implementation-artifacts/8-0-profiling-infrastructure.md`

## Summary

Story 8.0 adds criterion benchmarks and tracing-chrome instrumentation for
first-pass performance profiling. The implementation was functional but had
several issues around API surface, unnecessary dependencies, and misleading
benchmarks.

**Issues Found:** 4 High, 5 Medium, 2 Low
**Issues Fixed:** 4 High, 5 Medium
**Final Status:** done

## Findings

### HIGH (all fixed)

| ID | Issue | Fix |
|----|-------|-----|
| H1 | `trace.json` and `profile.json.gz` not gitignored — risk of accidental commit | Added to `.gitignore` |
| H2 | `tracing-chrome` + `tracing-subscriber` declared as optional deps in `hprof-parser` but never used there (only in `hprof-cli`) | Removed from `hprof-parser/Cargo.toml`; kept `dev-profiling = ["dep:tracing"]` only |
| H3 | `bench_string_parsing` and `bench_heap_extraction` are identical copies of `bench_first_pass_total` — fake granularity | Removed; kept only `first_pass_total` (per-phase via tracing spans in Perfetto) |
| H4 | `bench_segment_filter_build` adds 1 ID per heap range (~15 IDs) — not representative of real workload (~5M IDs) | Removed; real segment_filter_build timing comes from tracing spans |

### MEDIUM (all fixed)

| ID | Issue | Fix |
|----|-------|-----|
| M1 | `header_end()` in bench + `hprof_file.rs` duplicates `parse_header()` offset calculation | Added `records_start: usize` to `HprofHeader`; removed both `header_end()` functions |
| M2 | `IndexResult::percent_indexed()` was `pub(crate)` with `#[allow(dead_code)]` while type is `pub` | Made `pub`, removed allow |
| M3 | `segment` module exposed as `pub mod` unnecessarily (only for benches that are now removed) | Reverted to `pub(crate) mod segment`; all types/methods back to `pub(crate)` |
| M4 | Story File List missing `Cargo.lock`, workspace `Cargo.toml`, `sprint-status.yaml`, `header.rs`, `hprof_file.rs` | Updated File List with all changed files |
| M5 | `[profile.bench] debug = true` in workspace Cargo.toml undocumented | Documented in File List |

### LOW (not fixed — cosmetic)

| ID | Issue | Notes |
|----|-------|-------|
| L1 | `load_bench_file()` called once per bench function (redundant file reads) | Acceptable for a single bench function now; would matter with multiple benches |
| L2 | `#[derive(Default)]` on `SegmentFilterBuilder` was added for clippy on public API | Removed with visibility revert (M3); manual `new()` restored |

## Files Changed During Review

- `.gitignore` — added profiling artifacts
- `crates/hprof-parser/Cargo.toml` — removed unused optional deps, simplified feature
- `crates/hprof-parser/src/header.rs` — added `records_start` field to `HprofHeader`
- `crates/hprof-parser/src/hprof_file.rs` — use `header.records_start`, removed `header_end()`
- `crates/hprof-parser/src/indexer/mod.rs` — `segment` back to `pub(crate)`, `percent_indexed` now `pub`, `segment_filters` field `pub(crate)`
- `crates/hprof-parser/src/indexer/segment.rs` — all types/methods reverted to `pub(crate)`, manual `new()`
- `crates/hprof-parser/benches/first_pass.rs` — simplified to single `first_pass_total` bench
- `docs/implementation-artifacts/8-0-profiling-infrastructure.md` — updated File List, status → done
- `docs/implementation-artifacts/sprint-status.yaml` — 8-0 → done

## Verification

- 359 tests pass (3+78+183+93+2)
- clippy clean (0 warnings)
- `cargo build` (no features) — no tracing deps compiled
- `cargo build --features dev-profiling` — builds successfully
- `cargo bench --bench first_pass --no-run` — compiles
