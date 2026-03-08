# Code Review Report — Story 3.6

- Story: `docs/implementation-artifacts/3-6-lazy-value-string-loading.md`
- Story status at review time: `review`
- Reviewer: Claude (Amelia dev agent)
- Date: 2026-03-07
- Outcome: **Changes Requested**

## Scope Reviewed

- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-parser/src/hprof_file.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/app.rs`
- `crates/hprof-tui/src/views/status_bar.rs`
- `crates/hprof-tui/src/theme.rs`

## Git vs Story File List

Working tree clean. All story files committed in `c29eb26`. Discrepancy count: **0**.

## Acceptance Criteria Audit

1. AC1 (String placeholder before navigation): **Implemented** — `stack_view.rs:591`
2. AC2 (Enter loads async, display updates): **Partially Implemented** — async + truncation done; UTF-16 surrogate pairs are lossy
3. AC3 (unresolved warning visible in status bar count): **NOT Implemented** — warning collected in `app_warnings` but never reflected in status bar
4. AC4 (no duplicate loads while loading): **Implemented** — `selected_field_string_id()` returns None for Loading phase; Enter is no-op
5. AC5 (collapse clears string state): **Partially Implemented** — maps cleared in `StackState`, but in-flight `pending_strings` receivers not removed from `App`

## Findings

### [HIGH] H1 — AC3 violation: `app_warnings` not reflected in status bar count

- `App::warning_count` is captured once from `engine.warnings().len()` at construction (`app.rs:75`)
- `poll_strings()` appends to `app_warnings` when a backing array is unresolved (`app.rs:372-374`)
- `render()` passes `warning_count: self.warning_count` to `StatusBar` (`app.rs:487`) — static, never updated
- Result: unresolved string warnings are invisible in the UI, violating AC3

Fix: `app.rs:487` → `warning_count: self.warning_count + self.app_warnings.len()`

### [MEDIUM] M1 — `pending_strings` not cancelled on object collapse (AC5 gap)

- `CollapseObj`/`CollapseNestedObj` remove pending object expansion (`app.rs:287, 294`) and call `collapse_object_recursive` which clears `string_phases/values/errors` in `StackState`
- But `app.pending_strings` is NOT cleaned up for in-flight StringRef receivers of the collapsed subtree
- When the thread completes, `poll_strings()` calls `set_string_loaded(sid, val)` → re-inserts state into the already-cleared maps
- Consequence: if the parent object is re-expanded (same object ID), the StringRef appears as `Loaded` immediately without user action — phantom state

### [MEDIUM] M2 — No visual distinction between `Unloaded` and `Loading` for StringRef

- `format_field_value` (stack_view.rs:586-591) renders both `Unloaded` and `Loading` as `String = "..."`
- Object expansion shows `~ Loading...` during async load; string loading shows nothing different
- Users cannot tell if loading is in progress — inconsistent with the `OnObjectLoadingNode` UX pattern

### [MEDIUM] M3 — AC4 cursor movement is not tested

- AC4 says: "no additional load is started" when pressing Enter again **or moving the cursor**
- `enter_on_loading_string_ref_is_noop` (app.rs:1092) only covers the Enter case
- No test verifies that `move_up()`/`move_down()` while a StringRef is Loading does not trigger a new spawn
- The behavior is implicitly correct but not regression-protected

## Low

### [LOW] L1 — DRY: `find_prim_array` and `find_instance` share 40 lines of copy-pasted logic

- Segment filtering loop (candidate_segs, overlaps, bounds) is identical in both functions (`hprof_file.rs:136-229`)
- Only the inner scanner differs; extracting `find_in_heap_segments<T, F>` would halve the surface
- Story devnotes explicitly said "mirror find_instance exactly" — intentional but technical debt

### [LOW] L2 — `enter_on_loading_string_ref_is_noop` test lacks `pending_strings` assertion

- `app.rs:1092-1115`: test asserts phase stays `Loading` but does not assert `app.pending_strings.is_empty()`
- Adding the assertion would guard against a regression where a duplicate spawn is initiated
