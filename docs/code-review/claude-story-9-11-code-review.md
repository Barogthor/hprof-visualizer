# Code Review — Story 9-11: Go-to-Pin Async Navigation & Scroll-to-Target

**Reviewer:** Amelia (Dev Agent — Code Review)
**Date:** 2026-03-14
**Model:** claude-opus-4-6
**Story file:** `docs/implementation-artifacts/9-11-go-to-pin-async-and-scroll-fix.md`
**Commit:** `744794f`

## Summary

Story 9-11 implements non-blocking go-to-pin navigation with progressive
scroll-to-cursor, spinner feedback, cancel support, and stale-context
retry. 12 unit/integration tests cover the async walk lifecycle.

## Git vs Story Discrepancies

| Discrepancy | Severity | Resolution |
|---|---|---|
| `theme.rs` modified in commit but absent from File List | MEDIUM | **Fixed** — added to File List |

## Findings

### MEDIUM (fixed)

| # | Issue | File | Resolution |
|---|---|---|---|
| M1 | `theme.rs` missing from File List | story file | Added `theme.rs` + `help_bar.rs` to File List |
| M2 | Task 3.5 help footer "Esc: Cancel navigation" not implemented — `HelpContext` had no `Navigating` variant | `help_bar.rs`, `app/mod.rs` | Added `HelpContext::Navigating` variant, override Esc label in `build_rows`, pass from `render()` when `navigating_to_pin` is true. 3 tests added |
| M3 | Story documents `StaleRestart` variant but code uses `Continue` (in-frame cap). Stale restart handled in `resume_pending_navigation` instead | story file, `app/mod.rs:93-100` | Updated story Task 1.1 and Dev Notes code block to document `Continue` variant |
| M4 | `resume_pending_navigation` suffix invariant (`remaining_path` is suffix of `original_path`) not documented | `app/mod.rs:611-614` | Added invariant comment |

### LOW (not fixed — cosmetic)

| # | Issue | File |
|---|---|---|
| L1 | No test for spinner tick rotation (`wrapping_add` + `% 10` char selection) | `status_bar.rs` |
| L2 | `position_cursor_and_scroll` fallback to first `At(_)` is silent (no log/warning) | `app/mod.rs:639` |
| L3 | `expand_object_sync` marked `#[allow(dead_code)]` — may accumulate tech debt | `app/mod.rs:218` |

## AC Validation

| AC | Status | Evidence |
|---|---|---|
| AC1 — Non-blocking go-to-pin | IMPLEMENTED | `navigate_walk` defers via `PendingNavigation`; test 5.1 |
| AC2 — Progressive visual feedback | IMPLEMENTED | `position_cursor_and_scroll` called after each step |
| AC3 — Spinner during async waits | IMPLEMENTED | `navigating_to_pin` + `spinner_tick` in `StatusBar`; test `navigating_to_pin_shows_spinner_in_status_bar` |
| AC4 — Scroll to final target | IMPLEMENTED | `scroll_to_cursor()` in upper third; tests 5.2, 5.3, 5.12 |
| AC5 — Partial navigation with deferred completion | IMPLEMENTED | `PendingNavigation` struct + poll resume; tests 5.1, 5.10 |
| AC6 — Cancel via Escape | IMPLEMENTED | Escape handler clears pending nav; test 5.4. Help footer now shows "Cancel navigation" (M2 fix) |
| AC7 — No regression on small dumps | IMPLEMENTED | Sync fast path when all steps cached; test 5.5 |
| AC8 — Navigation failure graceful | IMPLEMENTED | `Ok(None)` in poll clears nav + shows error; test 5.8 |
| AC9 — Stale context triggers retry | IMPLEMENTED | `prereq_expanded` check in `resume_pending_navigation`; test 5.7 |

## Task Audit

All 5 tasks (19 subtasks) marked `[x]` — verified against implementation.
Task 3.5 was marked complete but not implemented (M2 fix applied).

## Verdict

**DONE** — all HIGH and MEDIUM issues fixed. 3 LOW issues left as-is (cosmetic).

## Files Changed by Review

- `crates/hprof-tui/src/views/help_bar.rs` — `HelpContext::Navigating` + Esc override + 3 tests
- `crates/hprof-tui/src/app/mod.rs` — pass `Navigating` context + invariant comment
- `docs/implementation-artifacts/9-11-go-to-pin-async-and-scroll-fix.md` — File List + AwaitedResource docs
- `docs/implementation-artifacts/sprint-status.yaml` — 9-11 → done
