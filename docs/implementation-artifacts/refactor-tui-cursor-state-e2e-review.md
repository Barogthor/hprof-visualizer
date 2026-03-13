# Story 9.11: Post-Navigation E2E Fixes

Status: pending-review

## Story

As a user,
I want the TUI to not crash on cyclic object graphs, to keep the correct thread in view
after navigating from Favorites, and to see a compact frame label in pinned items,
So that day-to-day inspection is stable and readable.

## Acceptance Criteria

1. **AC1 – No stack overflow on indirect cyclic object graphs:**
   Given a thread whose stack contains an object with an indirect cyclic reference
   through a collection (e.g. `Thread → ThreadGroup → Object[] → Thread`),
   When the user expands the object tree in the stack view or within a pinned snapshot,
   Then no stack overflow occurs — rendering stops at depth 16 with the cycle rendered
   up to that point.

2. **AC2 – Background stack view follows `g` navigation from Favorites:**
   Given the Favorites panel is focused and the user presses `g` on a pin that belongs
   to Thread A (while Thread B was previously selected),
   When focus returns or the user switches panels,
   Then the background stack view shows Thread A's stack, not the previously previewed
   Thread B.

3. **AC3 – Compact frame label in pinned item headers:**
   Given a pinned item that was captured inside a stack frame,
   When the Favorites panel is rendered,
   Then the frame portion of the header label reads `ClassName.method()` — without the
   `[FileName.java:Line]` source location suffix.

## Tasks / Subtasks

- [x] **Task 1 – Depth guard for collection rendering (AC1)**
  - [x] 1.1 Add `depth: usize` parameter to `append_collection_items`,
        `append_collection_items_inner`, and `append_collection_entry_item` in
        `crates/hprof-tui/src/views/tree_render.rs`.
  - [x] 1.2 Add `if depth >= 16 { return; }` guard at the top of
        `append_collection_items_inner`.
  - [x] 1.3 Thread `depth` through all call sites: top-level callers pass `0`,
        `append_collection_entry_obj` passes `depth + 1`, recursive callers propagate
        `depth`.
  - [x] 1.4 Remove the separate `depth + 1 < 16` guard in `append_collection_entry_obj`
        (now redundant with the centralised guard in `append_collection_items_inner`).

- [x] **Task 2 – Background stack follows Favorites navigation (AC2)**
  - [x] 2.1 In `crates/hprof-tui/src/app/mod.rs`, change the `else` branch of the
        stack-view render block to prefer `stack_state` over `preview_stack_state`
        when `stack_state` is `Some`.

- [x] **Task 3 – Compact frame label in Favorites (AC3)**
  - [x] 3.1 Add `format_frame_label_short` to
        `crates/hprof-tui/src/views/stack_view/format.rs` — returns
        `ClassName.method()` with no source location.
  - [x] 3.2 Re-export `format_frame_label_short` from
        `crates/hprof-tui/src/views/stack_view/mod.rs`.
  - [x] 3.3 Use `format_frame_label_short` in `PinnedSnapshot::frame_context()`
        in `crates/hprof-tui/src/favorites.rs`.

## Files Changed

- `crates/hprof-tui/src/views/tree_render.rs` — depth threading (Task 1)
- `crates/hprof-tui/src/app/mod.rs` — stack state preference (Task 2)
- `crates/hprof-tui/src/views/stack_view/format.rs` — `format_frame_label_short` (Task 3)
- `crates/hprof-tui/src/views/stack_view/mod.rs` — re-export (Task 3)
- `crates/hprof-tui/src/favorites.rs` — use short label (Task 3)
