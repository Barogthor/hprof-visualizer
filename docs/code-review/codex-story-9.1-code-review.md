# Code Review — Story 9.1

- Story: `docs/implementation-artifacts/9-1-expand-state-bugs-and-failed-resolve-non-navigable.md`
- Reviewer: Codex
- Date: 2026-03-10
- Scope read: `crates/hprof-tui/src/app.rs`, `crates/hprof-tui/src/views/stack_view.rs`, `crates/hprof-tui/src/views/tree_render.rs`, `crates/hprof-tui/src/views/favorites_panel.rs`
- Validation commands run:
  - `cargo test -p hprof-tui` (185 passed)
  - `cargo clippy -p hprof-tui --all-targets -- -D warnings` (clean)

## Findings

### HIGH

1. Failed local-variable label format diverges from story AC/task wording.
   - Story task states failed label format should be `"! {class_name} — {error_message}"`.
   - Current implementation prefixes `"local variable: "` in failed var text.
   - Evidence:
     - `docs/implementation-artifacts/9-1-expand-state-bugs-and-failed-resolve-non-navigable.md:69`
     - `docs/implementation-artifacts/9-1-expand-state-bugs-and-failed-resolve-non-navigable.md:71`
     - `crates/hprof-tui/src/views/tree_render.rs:116`

### MEDIUM

2. Failed collection-entry rows use `!` prefix but not error styling.
   - AC2 requires failed rows to use `THEME.error_indicator` (red fg).
   - Collection-entry row style is always computed via `field_value_style(&entry.value)`, which returns default style for `ObjectRef` without inline value.
   - As a result, failed collection entries can render with `!` prefix but non-red text.
   - Evidence:
     - `docs/implementation-artifacts/9-1-expand-state-bugs-and-failed-resolve-non-navigable.md:25`
     - `crates/hprof-tui/src/views/tree_render.rs:312`
     - `crates/hprof-tui/src/views/tree_render.rs:318`
     - `crates/hprof-tui/src/views/stack_view.rs:324`

3. Git-vs-story traceability mismatch for this review run.
   - Story File List claims app/view files were changed for Story 9.1.
   - Current git working tree shows no staged/unstaged changes for those files (`git diff --name-only` and `git diff --cached --name-only` empty).
   - This prevents verifying story claims against uncommitted diff in this session and weakens the workflow's required traceability check.
   - Evidence:
     - `docs/implementation-artifacts/9-1-expand-state-bugs-and-failed-resolve-non-navigable.md:218`
     - runtime command output: `git diff --name-only` (empty), `git diff --cached --name-only` (empty)

## Fix Follow-up

- Applied fixes in `crates/hprof-tui/src/views/tree_render.rs`:
  - Failed var label now renders `! Class — error` (no `local variable:` prefix)
  - Failed collection entries now render inline stored error and red error style
- Added regression tests:
  - `failed_var_label_uses_short_class_without_local_variable_prefix`
  - `failed_collection_entry_shows_error_message_inline`
- Re-validation:
  - `cargo test -p hprof-tui` -> 187 passed
  - `cargo clippy -p hprof-tui --all-targets -- -D warnings` -> clean

## Outcome

- Result: Approved after fixes
- Counts (open): 0 High, 0 Medium, 0 Low
