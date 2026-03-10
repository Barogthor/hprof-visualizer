# Code Review Report — Story 6.2

- Story: `docs/implementation-artifacts/6-2-loading-indicators-and-status-bar-warnings.md`
- Reviewer: Codex
- Date: 2026-03-10
- Outcome: Changes Requested

## Git vs Story Discrepancy

- Git working tree is clean in this review context, while story `File List` claims multiple changed files; current workspace evidence cannot corroborate those changes from uncommitted diffs.

## Findings

1. **High** — Initial collection page load lacks delayed loading indicator (AC1)
   - `poll_pages()` only sets chunk loading when `offset > 0`, so slow first-page loads never display a loading marker.
   - Reference: `crates/hprof-tui/src/app.rs:575`

2. **High** — Memory log double-counts skeleton bytes (AC6)
   - `memory_used()` includes skeleton bytes, but render logs it as `cache` and also logs `skeleton_bytes()` separately.
   - References: `crates/hprof-engine/src/engine.rs:291`, `crates/hprof-engine/src/engine_impl.rs:817`, `crates/hprof-tui/src/app.rs:666`

3. **Medium** — Warning count and last-warning text can diverge (AC5 partial)
   - Count combines indexing warnings + session warnings; latest text only reads session warnings.
   - References: `crates/hprof-tui/src/app.rs:749`, `crates/hprof-tui/src/app.rs:736`

4. **Low** — Story task tracking inconsistency
   - Task 4 first subtask remains unchecked even though symbols exist in code.
   - References: `docs/implementation-artifacts/6-2-loading-indicators-and-status-bar-warnings.md:163`, `crates/hprof-tui/src/app.rs:37`

## AC Validation Snapshot

- AC1: **Partial** (object expansion path covered; initial collection page loading indicator gap)
- AC2: **Implemented**
- AC3: **Implemented**
- AC4: **Implemented**
- AC5: **Partial** (warning count/text mismatch edge case)
- AC6: **Partial** (log format path present; cache metric semantics incorrect)
