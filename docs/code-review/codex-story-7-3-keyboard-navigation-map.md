## Code Review Report — Story 7.3

- Story: `docs/implementation-artifacts/7-3-keyboard-navigation-map.md`
- Reviewer: Codex (dev agent)
- Date: 2026-03-10
- Outcome: **Resolved after fixes applied in review pass**

### Scope Reviewed

- `crates/hprof-tui/src/input.rs`
- `crates/hprof-tui/src/app.rs`
- `crates/hprof-tui/src/views/help_bar.rs`
- `crates/hprof-tui/src/views/mod.rs`
- Supporting context: `docs/planning-artifacts/architecture.md`, `docs/planning-artifacts/ux-design-specification.md`, `docs/planning-artifacts/epics.md`

### Git vs Story Discrepancies

1. `git status --porcelain` reports only one untracked file:
   - `docs/story-review/claude-story-7-3-keyboard-navigation-map.md`
2. Story File List claims changes in six files, but there are no staged/unstaged diffs for those files in the current worktree.

Impact: this review validates current source behavior, but cannot use current `git diff` as evidence of Story 7.3 implementation deltas.

### Findings

#### HIGH-1 — `Tab` does not cycle focus from ThreadList when search is active (AC4 violation)

- Requirement: AC4 states `Tab` from `Focus::ThreadList` with active `stack_state` moves focus to `Focus::StackFrames`.
- Current behavior: in the search-active branch, `InputEvent::Tab` is not handled and falls through to no-op.
- Evidence:
  - `crates/hprof-tui/src/app.rs:259` (search-active branch)
  - `crates/hprof-tui/src/app.rs:317` (`_ => {}` default in search-active branch)

#### HIGH-2 — `Tab` is no-op in Favorites panel, contradicting “complete keyboard map across all TUI panels”

- Requirement: story goal requires consistent keyboard navigation map across all panels; help panel advertises `Tab` as “Cycle panel focus”.
- Current behavior: favorites handler has no `InputEvent::Tab` branch, so `Tab` is ignored when focus is `Favorites`.
- Evidence:
  - `crates/hprof-tui/src/app.rs:219` (`handle_favorites_input`)
  - `crates/hprof-tui/src/app.rs:247` (`_ => {}` default)
  - `crates/hprof-tui/src/views/help_bar.rs:23` (`Tab` documented as global cycle)

#### MEDIUM-1 — Story/File List traceability mismatch with repository state

- Story lists:
  - `crates/hprof-tui/src/input.rs`
  - `crates/hprof-tui/src/app.rs`
  - `crates/hprof-tui/src/views/mod.rs`
  - `crates/hprof-tui/src/views/help_bar.rs`
  - `docs/implementation-artifacts/7-3-keyboard-navigation-map.md`
  - `docs/implementation-artifacts/sprint-status.yaml`
- Current `git diff --name-only` and `git diff --cached --name-only` are empty.
- Impact: impossible to reconcile “what changed for 7.3” from current git state alone.

#### LOW-1 — Missing targeted tests for `Tab` behavior in edge focus states

- Existing tests cover:
  - ThreadList no stack_state no-op
  - ThreadList with stack_state to StackFrames
  - StackFrames to ThreadList
- Missing tests:
  - `Tab` while search is active in ThreadList
  - `Tab` when focus is Favorites
- Evidence:
  - `crates/hprof-tui/src/app.rs:2336`
  - `crates/hprof-tui/src/app.rs:2366`

### Acceptance Criteria Status

- AC1 (`?` toggles non-overlay bottom panel): **Implemented**
- AC2 (keymap completeness in help panel): **Partial** (documentation present, but behavior inconsistency for `Tab` in Favorites)
- AC3 (`s` activates search in thread list non-search mode): **Implemented**
- AC4 (`Tab` cycles focus TL <-> StackFrames): **Partial** (fails in ThreadList search-active branch)
- AC5 (no-op for unbound keys): **Implemented**
- AC6 (`q` exits from panel contexts): **Implemented**
- AC7 (no regressions: test/clippy/fmt): **Not re-verified in this review run**

### Recommended Fixes

1. In `handle_thread_list_input` search-active branch, handle `InputEvent::Tab` by calling `self.cycle_focus()`.
2. In `handle_favorites_input`, define `InputEvent::Tab` behavior (e.g., cycle back to `ThreadList` or through `StackFrames`) and align with help text.
3. Add tests for both missing `Tab` scenarios.
4. Re-run `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` after fixes.

### Resolution Applied

- Fixed `Tab` handling in thread-list search-active mode (`handle_thread_list_input`).
- Fixed `Tab` handling in favorites panel (`handle_favorites_input`).
- Extended focus cycle to include favorites when visible and return to thread list.
- Added regression tests:
  - `tab_from_thread_list_with_search_active_moves_to_stack_frames`
  - `tab_from_favorites_cycles_to_thread_list`
- Validation after fixes:
  - `cargo test` ✅
  - `cargo clippy --all-targets -- -D warnings` ✅
  - `cargo fmt --check` ✅
