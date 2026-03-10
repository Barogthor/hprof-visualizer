# Code Review — Story 7.2 Theme System

Date: 2026-03-10
Reviewer: Codex (Adversarial Review)
Story: `docs/implementation-artifacts/7-2-theme-system.md`

## Outcome

Changes Requested.

## Verified Checks

- `cargo test` passed (all crates + doctests)
- `cargo clippy --all-targets -- -D warnings` passed
- No inline `Color::*` usages found under `crates/hprof-tui/src/views/*.rs`
- No `Color::Rgb(...)` or `Color::Indexed(...)` found under `crates/hprof-tui/src/*.rs`

## Findings

### HIGH — AC3 mismatch for `char` value coloring

- Story AC3 requires string/char values to use `THEME.string_value` (`docs/implementation-artifacts/7-2-theme-system.md:29`).
- Implementation maps `FieldValue::Char(_)` to `THEME.primitive_value` (`crates/hprof-tui/src/views/stack_view.rs:1117`).
- Impact: acceptance criterion is not fully met for `char` values.

### CRITICAL — Task marked done but manual validation explicitly deferred

- Task 6 marks manual validation as complete (`docs/implementation-artifacts/7-2-theme-system.md:93`).
- Dev record says AC5 manual validation was deferred to reviewer (`docs/implementation-artifacts/7-2-theme-system.md:335`).
- Impact: task status is inconsistent with actual execution evidence.

### CRITICAL — Task 3 subtask claim does not match implementation for toggle styling

- Subtask claims expand indicators are themed (`docs/implementation-artifacts/7-2-theme-system.md:77`).
- In object/field tree rendering, toggle characters (`+`/`-`) are rendered inside the full row string and styled by row value style, not `THEME.expand_indicator` (`crates/hprof-tui/src/views/stack_view.rs:1646`, `crates/hprof-tui/src/views/stack_view.rs:1651`).
- Same pattern for variable rows (`crates/hprof-tui/src/views/stack_view.rs:1486`, `crates/hprof-tui/src/views/stack_view.rs:1496`).
- Impact: completed subtask overstates implementation scope.

### HIGH — Story file list cannot be validated against current git state

- Story File List claims code and tracking files were changed (`docs/implementation-artifacts/7-2-theme-system.md:339`).
- Current git working tree has no corresponding staged/unstaged changes for these files.
- Impact: review traceability is incomplete from current branch state (cannot verify story file list via git delta).

## Notes

- `Theme` struct and `THEME` constant are present and centralized (`crates/hprof-tui/src/theme.rs:38`, `crates/hprof-tui/src/theme.rs:81`).
- Thread list, stack view, and status bar use `THEME` imports (`crates/hprof-tui/src/views/thread_list.rs:19`, `crates/hprof-tui/src/views/stack_view.rs:19`, `crates/hprof-tui/src/views/status_bar.rs:15`).

## Auto-fix follow-up (2026-03-10)

- `FieldValue::Char(_)` mapping updated to `THEME.string_value` in `value_style()`.
- Stack/object toggle prefixes in `stack_view.rs` now render with `THEME.expand_indicator` while preserving selection highlight.
- Story status moved to `in-progress` with AC5 manual visual validation explicitly left pending.
