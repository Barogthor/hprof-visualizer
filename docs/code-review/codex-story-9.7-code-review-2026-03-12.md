# Code Review Report — Story 9.7

- **Story**: `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md`
- **Story Key**: `9-7-help-footer-context-and-visibility`
- **Reviewer**: Codex
- **Date**: 2026-03-12

## Scope Reviewed

- Claimed File List from story:
  - `crates/hprof-tui/src/views/help_bar.rs`
  - `crates/hprof-tui/src/app/mod.rs`
- Story requirements reviewed (ACs, tasks/subtasks, Definition of Done, Dev Agent Record)
- Verification commands executed:
  - `cargo test --all` (all suites green)
  - `cargo clippy --all-targets -- -D warnings` (green)
  - `cargo fmt -- --check` (green)

## Findings

### MEDIUM — Git working tree and story File List are out of sync for this review context

- **Evidence**:
  - Git shows untracked files unrelated to story File List: `docs/implementation-artifacts/9-8-pinned-item-navigation-and-array-expansion.md`, `docs/story-review/claude-story-9-8-adversarial-review.md`, `docs/story-review/claude-story-9-8-adversarial-review-2.md`
  - Story File List claims only two source files changed: `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:390`
- **Details**: Current working tree state does not represent a clean, story-scoped delta.
- **Impact**: Harder to audit exact story change set from `git diff` alone.
- **Recommended fix**: Review from a clean branch/worktree or document cross-story concurrent edits explicitly.

### HIGH — Story marked review while mandatory validation tasks remain unchecked

- **Evidence**:
  - `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:232`
  - `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:233`
  - `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:234`
  - `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:236`
  - `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:238`
  - `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:244`
- **Details**: Task 4 contains unchecked manual verification items, while Definition of Done explicitly requires all Task 1–4 checkboxes ticked.
- **Impact**: Story readiness claim is inconsistent with its own completion contract.
- **Recommended fix**: Run the manual checks and update checkboxes/results, or move `Status` back to `in-progress` until completed.

### HIGH — Task 1.3 completion claim conflicts with implemented help entry count/content

- **Evidence**:
  - Story requirement says `ENTRY_COUNT remains 17` and scope excludes adding 9.6 keys in this story: `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:83`, `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:321`
  - Implementation has `ENTRY_COUNT = 19` and includes `g` / `i` entries: `crates/hprof-tui/src/views/help_bar.rs:17`, `crates/hprof-tui/src/views/help_bar.rs:42`, `crates/hprof-tui/src/views/help_bar.rs:43`
  - Task 1.3 is checked as done: `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:54`
- **Details**: The marked task completion does not match its literal acceptance text.
- **Impact**: Auditability and story traceability are weakened.
- **Recommended fix**: Either revise Task 1.3 wording to reflect the merged 9.6 baseline, or uncheck/rework the task to match the current requirement set.

### MEDIUM — Misleading test name no longer matches behavior

- **Evidence**:
  - Test name says 13/17 but asserts 14 with 19 entries: `crates/hprof-tui/src/views/help_bar.rs:172`, `crates/hprof-tui/src/views/help_bar.rs:173`
- **Details**: The test still passes, but its name describes obsolete expectations.
- **Impact**: Future maintainers may misread intent and debugging context.
- **Recommended fix**: Rename test to reflect current expectation (e.g., `required_height_returns_fourteen_for_nineteen_entries`).

### MEDIUM — Dev Agent Record overstates unchanged-test claim

- **Evidence**:
  - Claim: existing tests unchanged: `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md:382`
  - Existing test behavior changed in file: `crates/hprof-tui/src/views/help_bar.rs:172`
- **Details**: Record says existing tests were unchanged, but at least one existing test assertion semantics changed.
- **Impact**: Historical implementation notes become less reliable.
- **Recommended fix**: Correct Dev Agent Record wording to distinguish newly added tests vs modified legacy tests.

## AC Validation Snapshot

- **AC1 (context-aware dimming)**: Implemented (`HelpContext`, masks, context-based dimming path present).
- **AC2 (9.3/9.4 shortcuts present)**: Implemented (ArrowLeft/Right and camera keys present in entries).
- **AC3 (`cargo test` no regressions)**: Verified locally (`cargo test --all` passed).

## Outcome

- **Decision**: Changes requested
- **Story status recommendation**: `in-progress` until HIGH findings are resolved or story text is aligned with merged baseline reality.

## Post-Review Remediation (Auto-Fix)

- Applied automatically after review:
  - Story status moved to `in-progress` in `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md` and synced in `docs/implementation-artifacts/sprint-status.yaml`.
  - Story text aligned to current merged baseline (`ENTRY_COUNT = 19`, line/height expectations updated).
  - Test renamed to `required_height_returns_fourteen_for_nineteen_entries` in `crates/hprof-tui/src/views/help_bar.rs`.
  - Dev Agent Record wording corrected to avoid claiming unchanged tests.
- Remaining blocker:
  - Manual smoke checks in Task 4 are still unchecked and require an interactive terminal run.
- Re-validation after fixes:
  - `cargo test --all` ✅
  - `cargo clippy --all-targets -- -D warnings` ✅
  - `cargo fmt -- --check` ✅
