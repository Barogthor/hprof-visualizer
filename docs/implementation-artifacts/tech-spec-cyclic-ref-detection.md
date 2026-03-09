---
title: 'Cyclic Reference Detection in Object Expansion'
slug: 'cyclic-ref-detection'
created: '2026-03-09'
status: 'implementation-complete'
stepsCompleted: [1, 2, 3, 4]
tech_stack: [rust, ratatui]
files_to_modify:
  - crates/hprof-tui/src/views/stack_view.rs
  - crates/hprof-tui/src/app.rs
code_patterns:
  - StackCursor enum variants for tree navigation
  - expansion_state() keyed by object_id globally
  - emit_object_children + build_object_items mirror recursion
test_patterns:
  - Unit tests in stack_view.rs module tests section
  - make_var_object_ref / make_frame test helpers
---

# Tech-Spec: Cyclic Reference Detection in Object Expansion

**Created:** 2026-03-09

## Overview

### Problem Statement

When expanded objects form cycles (self-cycle A→A or indirect cycle A→B→...→A), `emit_object_children` and `build_object_items` in `stack_view.rs` automatically recurse up to 16 levels deep because `expansion_state()` is indexed by `object_id` globally. An expand action produces repeated copies of the object's fields, making the UI confusing and unusable.

Real-world case: Thread → `parkBlocker` → BlockingCoroutine → `blockedThread` → Thread (already Expanded) → renders Thread's fields again → recurses 16 levels.

### Solution

Pass a `HashSet<u64>` visited set through both recursive rendering functions. When an `object_id` is already in the ancestor set, render a terminal leaf marker instead of recursing. Two distinct markers:

- **Self-cycle** (field.id == current object_id): `fieldName:   ↻ ClassName @ 0xABCD [self-ref]`
- **Indirect cycle** (field.id in ancestor visited set): `fieldName:   ↻ ClassName @ 0xABCD [cyclic]`

Both are non-expandable terminal leaves. Keep the existing depth-16 guard as a safety net.

### Scope

**In Scope:**
- Visited set (`HashSet<u64>`) passed through `emit_object_children` and `build_object_items`
- Self-cycle detection (field.id == object_id) with `[self-ref]` marker
- Indirect cycle detection (field.id in visited set) with `[cyclic]` marker
- New `StackCursor::OnCyclicNode` variant for cyclic nodes (non-actionable)
- Unit tests for self-cycle and indirect cycle rendering and cursor generation

**Out of Scope:**
- Object address display on all objects (future action item)
- Toggle addresses on/off (future action item)

## Context for Development

### Codebase Patterns

- `StackState` owns `object_phases: HashMap<u64, ExpansionPhase>` and `object_fields: HashMap<u64, Vec<FieldInfo>>` — both keyed by global `object_id`
- Two parallel recursive functions must be updated in sync: `emit_object_children` (cursor list) and `build_object_items` (render list)
- `FieldValue::ObjectRef { id, class_name, entry_count }` carries all needed data for the cyclic marker
- Existing test helpers: `make_frame()`, `make_var_object_ref()` in the `#[cfg(test)]` module
- Every `StackCursor` variant with `frame_idx` must be handled in: `selected_frame_id()`, `toggle_expand()` collapse reset, and the `build_items()` frame-highlight `matches!` macro
- `selected_field_ref_id()` and `selected_field_string_id()` only match `OnObjectField` via `if let` — `OnCyclicNode` is structurally excluded (returns `None`), no change needed
- `app.rs` Enter handler — wildcard catch-all exists but is fragile; an explicit `OnCyclicNode` arm is needed to document intent and prevent future regressions
- `collapse_object_recursive()` removes `object_phases` entries; if cursor is on an `OnCyclicNode` when a parent object is collapsed, the `toggle_expand()` cursor reset (which resets to `OnFrame`) handles it

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `stack_view.rs` — `StackCursor` enum | Add `OnCyclicNode` variant |
| `stack_view.rs` — `emit_object_children()` | Add `&mut HashSet<u64>` param, check visited before recursion |
| `stack_view.rs` — `build_object_items()` | Add `&mut HashSet<u64>` param, check visited before recursion + render marker |
| `stack_view.rs` — `selected_frame_id()` | Add `OnCyclicNode` to `frame_idx` extraction pattern |
| `stack_view.rs` — `toggle_expand()` | Add `OnCyclicNode` to cursor reset pattern on collapse |
| `stack_view.rs` — `build_items()` frame-highlight `matches!` | Add `OnCyclicNode` to frame selection highlight pattern |
| `app.rs` — Enter handler match | Add explicit `OnCyclicNode { .. } => return None` arm before wildcard |
| `engine.rs` — `FieldValue::ObjectRef` | Read-only reference — no changes needed |

### Technical Decisions

- **Visited set approach**: `HashSet<u64>` passed through recursion, current `object_id` inserted before iterating fields. Covers both self-cycles and indirect cycles
- Self-cycle is a special case: `field.id == object_id` (parent is in visited set by definition) — detected first for distinct `[self-ref]` label
- Indirect cycle: `field.id` found in visited set but `field.id != object_id` — labeled `[cyclic]`
- Both markers are terminal leaves in cursor generation and rendering — Enter on them is a no-op
- Initial callers (`flat_items`, `build_items` top-level loops) create an empty `HashSet::new()` — the root object_id is inserted by the first recursive call to `emit_object_children`/`build_object_items`
- `&mut HashSet<u64>` is needed (not `&HashSet`) because the visited set is mutated during DFS: insert before iterating fields, remove after (backtracking)
- Visited set is scoped per variable root — each var's `emit_object_children`/`build_object_items` call gets a fresh `HashSet::new()`. If the same cyclic structure is reachable from two vars, both independently detect cycles. This is correct behavior
- Address format: `0x{:X}` uppercase hex, consistent with Java tooling conventions
- **Option A chosen**: new `StackCursor::OnCyclicNode { frame_idx, var_idx, field_path }` variant — explicit, testable, no dispersed logic. Trade-off: no `target_id` field stored — if a future feature needs the cycle target object_id, it must re-derive it by walking `field_path`. Acceptable for current scope; add `target_id: u64` later if needed
- Detection is TUI-side (not engine-side) — engine returns raw data, TUI decides rendering. Consistent with current architecture
- Multi self-ref: each self-referencing field gets its own cyclic marker independently
- `object_id` in hprof = Java heap address, formatted as `0x{:X}`
- **Failure mode insight**: explicit `OnCyclicNode` arm required in `app.rs` Enter handler — wildcard is fragile if match arms are reordered later
- **Failure mode insight**: `flat_items` must emit `OnCyclicNode` entries — otherwise navigation skips the cyclic marker row
- **Failure mode insight**: cyclic marker indentation must use same formula as normal fields: `" ".repeat(2 + 2 * (parent_path.len() + 1))`
- No risk from `id == 0`: already filtered as `FieldValue::Null` by parser
- **First principles**: cyclic marker must preserve field name: `fieldName: ↻ ClassName @ 0xABCD [self-ref|cyclic]` — same pattern as normal fields (`fieldName: value`)
- **Short name**: cyclic marker uses `rsplit('.').next()` for class name (e.g. `Thread` not `java.lang.Thread`) — consistent with `format_object_ref_collapsed`
- **First principles**: toggle prefix for cyclic node is `"  "` (space, like primitives), not `"+ "` — it is a non-expandable leaf
- **Known limitation (accepted)**: `expansion_state` is global per `object_id` — an object referenced from multiple sites expands/collapses synchronously. This is consistent with JVisualVM behavior and acceptable by design
- **Verified safe (no change needed)**: `collect_descendants` and `string_ids_in_subtree` already use `HashSet<u64>` visited — self-cycles are handled correctly during collapse cleanup
- **Style**: cyclic marker uses `theme::SEARCH_HINT` when unselected (informational node, not an error), `theme::SELECTED` when selected — consistent with loading/no-fields nodes
- **Required tests**: (1) `flat_items` with self-ref emits `OnCyclicNode` not 16x `OnObjectField`, (2) `flat_items` with indirect cycle A→B→A emits `OnCyclicNode` at B's ref back to A, (3) `build_items` renders `↻` marker with `[self-ref]` for self-cycle and `[cyclic]` for indirect, (4) Enter on `OnCyclicNode` is no-op (structurally guaranteed by `selected_field_ref_id` matching only `OnObjectField`), (5) object with 2 self-ref fields produces 2 distinct `OnCyclicNode` entries, (6) `move_down`/`move_up` correctly traverse cyclic nodes without skipping

## Implementation Plan

### Tasks

- [x] Task 1: Add `OnCyclicNode` variant to `StackCursor`
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action: Add variant `OnCyclicNode { frame_idx: usize, var_idx: usize, field_path: Vec<usize> }` to the `StackCursor` enum after `OnObjectLoadingNode`
  - Notes: Triggers compile errors for all incomplete match arms — use the compiler to find every site that needs updating

- [x] Task 2: Add `OnCyclicNode` to all existing pattern matches
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action: Fix every compile error from Task 1. Known sites:
    - `selected_frame_id()`: add `| StackCursor::OnCyclicNode { frame_idx, .. }` to the `OnObjectField | OnObjectLoadingNode` arm
    - `toggle_expand()` cursor reset: add `| StackCursor::OnCyclicNode { frame_idx, .. }` to the collapse reset pattern
    - `build_items()` frame-highlight `matches!` macro: add `| StackCursor::OnCyclicNode { frame_idx, .. }` to the frame selection highlight pattern
    - Any other sites the compiler flags
  - Notes: Each site already groups `OnObjectField` and `OnObjectLoadingNode` with `|` — just append `OnCyclicNode`. `selected_field_ref_id()` and `selected_field_string_id()` use `if let OnObjectField` — they structurally exclude `OnCyclicNode`, no change needed

- [x] Task 3: Add `&mut HashSet<u64>` parameter to `emit_object_children` and wire visited set
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action:
    - Add `visited: &mut HashSet<u64>` parameter to `emit_object_children` signature
    - At the start of the `Expanded` branch, insert `object_id` into `visited`
    - Before the recursive call, check if `id` is in `visited`:
      - If `id == object_id` → emit `StackCursor::OnCyclicNode` (self-ref), do NOT recurse
      - Else if `visited.contains(&id)` → emit `StackCursor::OnCyclicNode` (indirect cycle), do NOT recurse
      - Else → recurse as before, passing `visited`
    - At the end of the `Expanded` branch, remove `object_id` from `visited` (backtrack)
    - Update the call site in `flat_items()`: create `let mut visited = HashSet::new()` and pass `&mut visited` to `emit_object_children`
  - Notes: The visited set tracks the **ancestor chain**, not all visited nodes globally. Insert before iterating, remove after — standard DFS backtracking

- [x] Task 4: Add `&mut HashSet<u64>` parameter to `build_object_items` and wire visited set
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action:
    - Add `visited: &mut HashSet<u64>` parameter to `build_object_items` signature
    - At the start of the `Expanded` branch, insert `object_id` into `visited`
    - Before the recursive call, check if `id` is in `visited`:
      - If `id == object_id` → render cyclic marker with `[self-ref]` label, do NOT recurse
      - Else if `visited.contains(&id)` → render cyclic marker with `[cyclic]` label, do NOT recurse
      - Else → recurse as before, passing `visited`
    - Cyclic marker format (use local variable to stay under 100-char line limit):
      ```rust
      let marker = format!(
          "↻ {} @ 0x{:X} [{}]",
          short_class_name, id, label,
      );
      // then: format!("{indent}  {field_name}: {marker}")
      ```
      - `short_class_name` via `class_name.rsplit('.').next().unwrap_or(&class_name)`
      - Toggle prefix: `"  "` (space, non-expandable leaf)
      - Style: `theme::SEARCH_HINT` when unselected, `theme::SELECTED` when selected
      - Selection check: match cursor against `StackCursor::OnCyclicNode` with same `frame_idx`, `var_idx`, `field_path`
    - At the end of the `Expanded` branch, remove `object_id` from `visited` (backtrack)
    - Update the call site in `build_items()`: create `let mut visited = HashSet::new()` and pass `&mut visited` to `build_object_items`
  - Notes: Mirror structure of Task 3 for rendering side

- [x] Task 5: Add explicit `OnCyclicNode` arm in `app.rs` Enter handler
  - File: `crates/hprof-tui/src/app.rs`
  - Action: Add `StackCursor::OnCyclicNode { .. } => return None,` arm before the wildcard `_ => return None` at line 270
  - Notes: Explicit no-op documents intent and prevents future regressions if match arms are reordered

- [x] Task 6: Unit tests for self-cycle detection
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action: Add tests in the `#[cfg(test)]` module:
    - `flat_items_self_ref_emits_cyclic_node`: Object 100 with field `ObjectRef { id: 100 }`. Expand object 100, call `flat_items`. Assert contains exactly one `OnCyclicNode`, zero recursive `OnObjectField` for depth > 1
    - `flat_items_multi_self_ref_emits_two_cyclic_nodes`: Object 100 with 2 fields both `ObjectRef { id: 100 }`. Assert 2 `OnCyclicNode` entries
    - `build_items_self_ref_renders_self_ref_marker`: Same setup, call `build_items`. Assert output contains `"↻"` and `"[self-ref]"` exactly once per self-ref field
  - Notes: Use existing helpers `make_frame()`, `make_var_object_ref()`. Manually insert `object_fields` and `object_phases` into `StackState`

- [x] Task 7: Unit tests for indirect cycle detection
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action: Add tests:
    - `flat_items_indirect_cycle_emits_cyclic_node`: Object A(100) has field → B(200), B has field → A(100). Expand both. Assert `flat_items` contains `OnCyclicNode` at B's back-reference to A, not 16 levels of recursion
    - `build_items_indirect_cycle_renders_cyclic_marker`: Same setup. Assert output contains `"↻"` and `"[cyclic]"` (not `[self-ref]`) for B's reference back to A
  - Notes: Both A and B must have `ExpansionPhase::Expanded` and entries in `object_fields`

- [x] Task 8: Unit test for navigation across cyclic nodes
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action: Add test:
    - `move_down_up_across_cyclic_node`: Object 100 with fields [Int, ObjectRef(100), Int]. Expand. Call `move_down` repeatedly. Assert cursor visits: OnObjectField (Int) → OnCyclicNode (self-ref) → OnObjectField (Int). Then `move_up` back through the same sequence without skipping
  - Notes: Validates that `flat_items` correctly includes `OnCyclicNode` in the navigation order

- [x] Task 9: Regression test for acyclic multi-level tree + depth guard interaction
  - File: `crates/hprof-tui/src/views/stack_view.rs`
  - Action: Add tests:
    - `flat_items_acyclic_tree_no_cyclic_nodes`: Object A(100) → B(200) → C(300), all expanded, no cycles. Assert zero `OnCyclicNode` in `flat_items`, all fields rendered as normal `OnObjectField`
    - `depth_guard_still_active_with_visited_set`: Verify that the depth-16 guard is checked **before** the visited set check in both `emit_object_children` and `build_object_items` — a non-cyclic tree deeper than 16 levels still stops at 16
  - Notes: Ensures visited set wiring does not break existing behavior or disable the safety net

### Acceptance Criteria

- [x] AC1: Given an expanded object A with a field pointing to itself (id == A), when the stack view renders, then the self-referencing field displays `↻ ClassName @ 0xABCD [self-ref]` as a terminal leaf with no nested children
- [x] AC2: Given expanded objects A→B→A (indirect cycle), when the stack view renders, then B's back-reference to A displays `↻ ClassName @ 0xABCD [cyclic]` as a terminal leaf with no nested children
- [x] AC3: Given cursor positioned on an `OnCyclicNode`, when user presses Enter, then nothing happens (no expansion, no crash, cursor stays)
- [x] AC4: Given an object with 2 fields both self-referencing, when expanded, then both fields show distinct `[self-ref]` markers independently
- [x] AC5: Given an expanded object tree with no cycles, when the stack view renders, then behavior is identical to current (no regression)
- [x] AC6: Given any cyclic marker displayed, when the class name is a FQCN (e.g. `java.lang.Thread`), then only the short name is shown (e.g. `Thread`)
- [x] AC7: Given cursor on an `OnCyclicNode`, when user navigates up/down with arrow keys, then the cursor moves correctly to the previous/next item in the flat list without skipping the cyclic node

## Additional Context

### Dependencies

- No external library dependencies — `HashSet` is in `std::collections` (already imported in `stack_view.rs`)
- No engine API changes — detection is purely TUI-side

### Testing Strategy

- **Unit tests** (Tasks 6-9): 8 test cases covering self-ref, indirect cycle, multi self-ref, render markers, cursor generation, navigation, acyclic regression, and depth guard interaction
- **Navigation test**: verify `move_up`/`move_down` correctly traverse cyclic nodes in `flat_items` output
- **Manual testing**: Open RustRover dump, navigate to a Thread with `parkBlocker`, expand the chain, verify cyclic marker appears instead of 16-level recursion
- **Regression**: Run full `cargo test` suite — all existing tests must pass unchanged

### Notes

- **Known limitation (accepted)**: `expansion_state` is global per `object_id` — an object referenced from multiple disjoint subtrees expands/collapses synchronously. Consistent with JVisualVM behavior
- **Future action items**: (1) Display object address on all ObjectRef fields, (2) Toggle address display on/off
- **Visited set is DFS ancestor chain**: insert before iterating fields, remove after — NOT a global "all visited" set. An object can appear in multiple unrelated subtrees without false-positive cycle detection
