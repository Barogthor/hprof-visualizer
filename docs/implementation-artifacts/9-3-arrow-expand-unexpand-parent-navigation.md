# Story 9.3: ArrowRight/Left Expand, Unexpand & Parent Navigation

Status: done

## Story

As a user,
I want ArrowRight to expand a collapsed node and ArrowLeft to unexpand an expanded node or
navigate to the parent when nothing is expanded,
So that I can navigate the object tree with standard tree-control keyboard conventions.

## Acceptance Criteria

**AC1** — Given the cursor is on a collapsed, expandable node
When the user presses ArrowRight
Then the node expands (equivalent to pressing Enter on a collapsed expandable node)

**AC2** — Given the cursor is on a node whose immediate expandable content is currently
expanded (i.e., the object, frame, or entry object directly under the cursor is in
`Expanded` phase, or the field under the cursor has an active collection in
`collection_chunks`)
When the user presses ArrowLeft
Then that expanded content collapses — the cursor stays on the same row

**AC3** — Given the cursor is on a node that has no expanded children (leaf or already
collapsed)
When the user presses ArrowLeft
Then the cursor moves to the logical parent node in the tree

**AC4** — Given the cursor is on a top-level node (no parent — `OnFrame` or `NoFrames`)
When the user presses ArrowLeft
Then the action is a no-op (no crash, no cursor movement)

**AC5** — Given the help panel is visible
When it is rendered
Then ArrowRight (Expand) and ArrowLeft (Unexpand / Go to parent) are listed in the keymap

**AC6** — Given all existing tests
When `cargo test --all` is run
Then zero failures — no regressions

## Out of Scope

- **ArrowRight on already-expanded node:** no-op (not: "move to first child"). Right only
  expands; it never moves the cursor. To descend after expanding, use ArrowDown. This
  avoids ambiguity for Loading/Failed nodes that have no meaningful "first child", and
  keeps Right/Left as pure expand/collapse axes independent of cursor movement.
- **ArrowLeft from inside a collection (OnCollectionEntry, OnChunkSection) collapsing
  the whole collection:** Left arrow only navigates the cursor to the parent field; the
  collection remains expanded. The Escape key handles full collection collapse (existing
  behavior, unchanged). Note: this intentionally differs from frames and objects, where
  Left does collapse. Collections require Escape by design — see Dev Notes for rationale.
- **Thread list panel:** Left/Right arrows have no binding in the thread list (only in
  the stack frames view). Out of scope.
- **Favorites panel:** Left/Right arrow bindings in the favorites panel are out of scope
  (covered by story 9.8).
- **`cursor_collection_id()` bug with `field_path: []`:** When a collection is opened from
  `OnVar` (story 9.2 path), `cursor_collection_id()` incorrectly restores to
  `OnObjectField { field_path: [] }`. That is a story 9.2 defect. Story 9.3's
  `parent_cursor()` does NOT share this bug (it correctly returns `OnVar` when
  `field_path` is empty).

## Tasks / Subtasks

### 1. Add `Right` and `Left` to `InputEvent` (AC1–AC4)

- [x] In `crates/hprof-tui/src/input.rs`, add two variants to `InputEvent` (after `Down`,
  before `Home`):
  ```rust
  /// Expand the item at the current cursor position (stack view only).
  Right,
  /// Unexpand the current node, or navigate to its logical parent.
  Left,
  ```
- [x] In `from_key()`, add two new arms (after the `Down` arm, before `Home`):
  ```rust
  (KeyCode::Right, _) => Some(InputEvent::Right),
  (KeyCode::Left, _) => Some(InputEvent::Left),
  ```
  Note: both arms use `_` for modifiers (no modifier restriction) — consistent with Up/Down.
- [x] Update the existing test `from_key_maps_arrow_keys` in `input.rs` to cover Right and
  Left:
  ```rust
  assert_eq!(
      from_key(key(KeyCode::Right, KeyModifiers::NONE)),
      Some(InputEvent::Right)
  );
  assert_eq!(
      from_key(key(KeyCode::Left, KeyModifiers::NONE)),
      Some(InputEvent::Left)
  );
  ```

### 2. Add `parent_cursor()` to `StackState` (AC3)

- [x] In `crates/hprof-tui/src/views/stack_view/state.rs`, add a method to `StackState`.
  Place it near `cursor_collection_id()` (~line 620):
  ```rust
  /// Returns the logical parent cursor for the current position, or `None` if at
  /// the top level (`OnFrame` or `NoFrames`).
  ///
  /// Does NOT modify state — only computes where the cursor should go.
  ///
  /// Parent relationships:
  /// - `OnVar` → `OnFrame`
  /// - `OnObjectField { path: [x] }` → `OnVar`
  /// - `OnObjectField { path: [x, y, ...] }` → `OnObjectField` with last element dropped
  /// - `OnObjectLoadingNode` / `OnCyclicNode` → same rule as `OnObjectField`
  /// - `OnCollectionEntry { field_path: [] }` → `OnVar` (collection opened from var)
  /// - `OnCollectionEntry { field_path: [x, ...] }` → `OnObjectField { field_path }`
  /// - `OnChunkSection` → same rule as `OnCollectionEntry`
  /// - `OnCollectionEntryObjField { obj_field_path: [x] }` → `OnCollectionEntry`
  /// - `OnCollectionEntryObjField { obj_field_path: [x, y, ...] }` → truncate obj_field_path
  pub fn parent_cursor(&self) -> Option<StackCursor> {
      match &self.cursor {
          StackCursor::NoFrames | StackCursor::OnFrame(_) => None,
          StackCursor::OnVar { frame_idx, .. } => {
              Some(StackCursor::OnFrame(*frame_idx))
          }
          StackCursor::OnObjectField { frame_idx, var_idx, field_path }
          | StackCursor::OnObjectLoadingNode { frame_idx, var_idx, field_path }
          | StackCursor::OnCyclicNode { frame_idx, var_idx, field_path } => {
              // len() == 0: cursor restored by cursor_collection_id() after a collection
              //   was collapsed from OnVar context (story 9.2 edge case). Parent = OnVar.
              // len() == 1: normal depth-1 field. Parent = OnVar.
              // Both cases correctly return OnVar — the condition `<= 1` is intentional.
              if field_path.len() <= 1 {
                  Some(StackCursor::OnVar {
                      frame_idx: *frame_idx,
                      var_idx: *var_idx,
                  })
              } else {
                  let parent_path =
                      field_path[..field_path.len() - 1].to_vec();
                  Some(StackCursor::OnObjectField {
                      frame_idx: *frame_idx,
                      var_idx: *var_idx,
                      field_path: parent_path,
                  })
              }
          }
          StackCursor::OnChunkSection {
              frame_idx,
              var_idx,
              field_path,
              ..
          }
          | StackCursor::OnCollectionEntry {
              frame_idx,
              var_idx,
              field_path,
              ..
          } => {
              if field_path.is_empty() {
                  Some(StackCursor::OnVar {
                      frame_idx: *frame_idx,
                      var_idx: *var_idx,
                  })
              } else {
                  Some(StackCursor::OnObjectField {
                      frame_idx: *frame_idx,
                      var_idx: *var_idx,
                      field_path: field_path.clone(),
                  })
              }
          }
          StackCursor::OnCollectionEntryObjField {
              frame_idx,
              var_idx,
              field_path,
              collection_id,
              entry_index,
              obj_field_path,
          } => {
              if obj_field_path.len() <= 1 {
                  Some(StackCursor::OnCollectionEntry {
                      frame_idx: *frame_idx,
                      var_idx: *var_idx,
                      field_path: field_path.clone(),
                      collection_id: *collection_id,
                      entry_index: *entry_index,
                  })
              } else {
                  let parent_obj_path =
                      obj_field_path[..obj_field_path.len() - 1].to_vec();
                  Some(StackCursor::OnCollectionEntryObjField {
                      frame_idx: *frame_idx,
                      var_idx: *var_idx,
                      field_path: field_path.clone(),
                      collection_id: *collection_id,
                      entry_index: *entry_index,
                      obj_field_path: parent_obj_path,
                  })
              }
          }
      }
  }
  ```
  This method is pure (no side effects). It is always safe to call; callers decide what to
  do with the result.

### 3. Add Right and Left handlers in `app.rs` (AC1–AC4)

- [x] In `crates/hprof-tui/src/app/mod.rs`, inside `handle_stack_frames_input()`,
  add two new arms in the outer `match event { ... }` block.

  **Place them after `InputEvent::PageUp` (~line 448) and before `InputEvent::Enter`.**
  The outer match ends with `_ => {}` (line ~642). The new `Right` and `Left` arms MUST
  appear before this catch-all — Rust matches arms in order and the `_ => {}` would
  silently swallow them otherwise. Verify after insertion that no `_ => {}` arm precedes
  the new arms.

  **Right arrow — expand collapsed node:**
  ```rust
  InputEvent::Right => {
      // Same as Enter for expandable nodes, except:
      //   - Expanded phase → no-op (do NOT collapse on Right)
      //   - Loading / Failed / non-expandable → no-op
      enum RightCmd {
          ExpandFrame(u64),
          StartObj(u64),       // covers both root-var and nested-field expansion
          StartCollection(u64, u64),
          StartEntryObj(u64),
          LoadChunk(u64, usize, usize),
      }
      let cmd = self.stack_state.as_ref().and_then(|s| {
          Some(match s.cursor().clone() {
              StackCursor::OnFrame(_) => {
                  let fid = s.selected_frame_id()?;
                  if s.is_expanded(fid) {
                      return None;
                  }
                  RightCmd::ExpandFrame(fid)
              }
              StackCursor::OnVar { .. } => {
                  let oid = s.selected_object_id()?;
                  // CONDITIONAL ON STORY 9.2:
                  // Before writing this block, run:
                  //   rg -n "fn selected_var_entry_count" \
                  //     crates/hprof-tui/src/views/stack_view.rs
                  //
                  // If it EXISTS → include the block below as-is.
                  // If it does NOT exist → omit it entirely (do not add a TODO comment).
                  //
                  // selected_var_entry_count() returns Some(n) for ArrayList, Object[],
                  // int[], etc. Right expands it as a collection (same as Enter after 9.2).
                  if let Some(ec) = s.selected_var_entry_count() {
                      if s.collection_chunks.contains_key(&oid) {
                          return None; // already expanded — no-op on Right
                      }
                      return Some(RightCmd::StartCollection(oid, ec));
                  }
                  // END CONDITIONAL BLOCK
                  match s.expansion_state(oid) {
                      ExpansionPhase::Collapsed => RightCmd::StartObj(oid),
                      _ => return None,
                  }
              }
              StackCursor::OnObjectField { .. } => {
                  if let Some((cid, ec)) = s.selected_field_collection_info() {
                      if s.collection_chunks.contains_key(&cid) {
                          return None; // already expanded
                      }
                      return Some(RightCmd::StartCollection(cid, ec));
                  }
                  let nested_id = s.selected_field_ref_id()?;
                  match s.expansion_state(nested_id) {
                      ExpansionPhase::Collapsed => RightCmd::StartObj(nested_id),
                      _ => return None,
                  }
              }
              StackCursor::OnChunkSection { .. } => {
                  if let Some((cid, co, cl)) = s.selected_chunk_info() {
                      match s.chunk_state(cid, co) {
                          Some(ChunkState::Collapsed) => RightCmd::LoadChunk(cid, co, cl),
                          _ => return None,
                      }
                  } else {
                      return None;
                  }
              }
              StackCursor::OnCollectionEntry { .. } => {
                  let oid = s.selected_collection_entry_ref_id()?;
                  match s.expansion_state(oid) {
                      ExpansionPhase::Collapsed => RightCmd::StartEntryObj(oid),
                      _ => return None,
                  }
              }
              StackCursor::OnCollectionEntryObjField { .. } => {
                  let oid = s.selected_collection_entry_obj_field_ref_id()?;
                  match s.expansion_state(oid) {
                      ExpansionPhase::Collapsed => RightCmd::StartEntryObj(oid),
                      _ => return None,
                  }
              }
              StackCursor::OnCyclicNode { .. }
              | StackCursor::OnObjectLoadingNode { .. }
              | StackCursor::NoFrames => return None,
          })
      });
      match cmd {
          Some(RightCmd::ExpandFrame(fid)) => {
              let vars = self.engine.get_local_variables(fid);
              if let Some(s) = &mut self.stack_state {
                  s.toggle_expand(fid, vars);
              }
          }
          Some(RightCmd::StartObj(oid)) => {
              self.start_object_expansion(oid);
          }
          Some(RightCmd::StartCollection(cid, ec)) => {
              let limit = (ec as usize).min(100);
              let chunks = CollectionChunks {
                  total_count: ec,
                  eager_page: None,
                  chunk_pages: compute_chunk_ranges(ec)
                      .into_iter()
                      .map(|(o, _)| (o, ChunkState::Collapsed))
                      .collect(),
              };
              if let Some(s) = &mut self.stack_state {
                  s.collection_chunks.insert(cid, chunks);
              }
              self.start_collection_page_load(cid, 0, limit);
          }
          Some(RightCmd::StartEntryObj(oid)) => {
              self.start_object_expansion(oid);
          }
          Some(RightCmd::LoadChunk(cid, offset, limit)) => {
              self.start_collection_page_load(cid, offset, limit);
          }
          None => {}
      }
  }
  ```

  **Left arrow — unexpand if expanded, else navigate to parent:**
  ```rust
  InputEvent::Left => {
      enum LeftCmd {
          CollapseFrame(u64),
          CollapseObj(u64),
          CollapseNestedObj(u64),
          CollapseCollection(u64),
          CollapseEntryObj(u64),
          NavigateToParent(StackCursor),
      }
      let cmd = self.stack_state.as_ref().and_then(|s| {
          Some(match s.cursor().clone() {
              StackCursor::OnFrame(_) => {
                  let fid = s.selected_frame_id()?;
                  if s.is_expanded(fid) {
                      LeftCmd::CollapseFrame(fid)
                  } else {
                      return None; // top-level, no-op (AC4)
                  }
              }
              StackCursor::OnVar { .. } => {
                  // Primitives (String, int, bool, etc.) have no object_id.
                  // selected_object_id() returns None for them — go straight to parent.
                  let Some(oid) = s.selected_object_id() else {
                      return Some(LeftCmd::NavigateToParent(s.parent_cursor()?));
                  };
                  // Check collection expansion first (story 9.2 path)
                  if s.collection_chunks.contains_key(&oid) {
                      return Some(LeftCmd::CollapseCollection(oid));
                  }
                  match s.expansion_state(oid) {
                      ExpansionPhase::Expanded => LeftCmd::CollapseObj(oid),
                      _ => LeftCmd::NavigateToParent(s.parent_cursor()?),
                  }
              }
              StackCursor::OnObjectField { .. } => {
                  if let Some((cid, _)) = s.selected_field_collection_info() {
                      if s.collection_chunks.contains_key(&cid) {
                          return Some(LeftCmd::CollapseCollection(cid));
                      }
                  }
                  if let Some(nested_id) = s.selected_field_ref_id() {
                      if s.expansion_state(nested_id) == ExpansionPhase::Expanded {
                          return Some(LeftCmd::CollapseNestedObj(nested_id));
                      }
                  }
                  LeftCmd::NavigateToParent(s.parent_cursor()?)
              }
              StackCursor::OnCollectionEntry { .. } => {
                  let oid = s.selected_collection_entry_ref_id()?;
                  if s.expansion_state(oid) == ExpansionPhase::Expanded {
                      LeftCmd::CollapseEntryObj(oid)
                  } else {
                      // Navigate to parent field WITHOUT collapsing the collection
                      LeftCmd::NavigateToParent(s.parent_cursor()?)
                  }
              }
              StackCursor::OnCollectionEntryObjField { .. } => {
                  let oid = s.selected_collection_entry_obj_field_ref_id()?;
                  if s.expansion_state(oid) == ExpansionPhase::Expanded {
                      LeftCmd::CollapseEntryObj(oid)
                  } else {
                      LeftCmd::NavigateToParent(s.parent_cursor()?)
                  }
              }
              StackCursor::OnChunkSection { .. } => {
                  LeftCmd::NavigateToParent(s.parent_cursor()?)
              }
              StackCursor::OnObjectLoadingNode { .. }
              | StackCursor::OnCyclicNode { .. } => {
                  LeftCmd::NavigateToParent(s.parent_cursor()?)
              }
              StackCursor::NoFrames => return None,
          })
      });
      match cmd {
          Some(LeftCmd::CollapseFrame(fid)) => {
              if let Some(s) = &mut self.stack_state {
                  s.toggle_expand(fid, vec![]);
              }
          }
          Some(LeftCmd::CollapseObj(oid)) => {
              self.pending_expansions.remove(&oid);
              if let Some(s) = &mut self.stack_state {
                  s.collapse_object_recursive(oid);
              }
          }
          Some(LeftCmd::CollapseNestedObj(oid)) => {
              self.pending_expansions.remove(&oid);
              if let Some(s) = &mut self.stack_state {
                  s.collapse_object(oid);
              }
          }
          Some(LeftCmd::CollapseCollection(cid)) => {
              if let Some(s) = &mut self.stack_state {
                  s.collection_chunks.remove(&cid);
              }
              self.pending_pages.retain(|&(id, _), _| id != cid);
          }
          Some(LeftCmd::CollapseEntryObj(oid)) => {
              self.pending_expansions.remove(&oid);
              if let Some(s) = &mut self.stack_state {
                  s.collapse_object_recursive(oid);
              }
          }
          Some(LeftCmd::NavigateToParent(parent)) => {
              if let Some(s) = &mut self.stack_state {
                  s.set_cursor(parent);
              }
          }
          None => {}
      }
  }
  ```

  **Verify before writing:**
  ```
  # Confirm imports already present (compute_chunk_ranges is at app.rs:32)
  rg -n "use.*StackCursor\|use.*ExpansionPhase\|compute_chunk_ranges" \
    crates/hprof-tui/src/app.rs

  # Confirm selected_chunk_info() exists on StackState
  rg -n "fn selected_chunk_info" crates/hprof-tui/src/views/stack_view.rs
  ```
  If `selected_chunk_info()` does not exist, search for how the Enter handler resolves
  `OnChunkSection` info (`rg -n "OnChunkSection" crates/hprof-tui/src/app.rs`) and mirror
  the exact same accessor call used there.

### 4. Update `help_bar.rs` (AC5)

- [x] In `crates/hprof-tui/src/views/help_bar.rs`, update `ENTRY_COUNT` from `11` to `13`
  and add two new entries to the `ENTRIES` constant:
  ```rust
  const ENTRY_COUNT: u16 = 13;

  const ENTRIES: &[(&str, &str)] = &[
      ("q / Ctrl+C", "Quit"),
      ("Esc", "Go back / cancel search"),
      ("Tab", "Cycle panel focus"),
      ("\u{2191} / \u{2193}", "Move selection"),
      ("PgUp / PgDn", "Scroll one page"),
      ("Home / End", "Jump to first / last"),
      ("Enter", "Expand / confirm"),
      ("\u{2192}", "Expand node"),                          // ADD
      ("\u{2190}", "Unexpand / go to parent"),              // ADD
      ("f", "Pin / unpin favorite (Story 7.1)"),
      ("F", "Focus favorites panel (Story 7.1)"),
      ("s or /", "Open search (thread list only)"),
      ("?", "Toggle help panel"),
  ];
  ```
  Unicode: `\u{2192}` = `→`, `\u{2190}` = `←`.
- [x] Update the two failing tests in `help_bar.rs` to reflect the new counts:
  Formula: `required_height()` = `2 (borders) + 1 (padding) + div_ceil(ENTRY_COUNT, 2) (rows) + 1 (separator)`.
  With 13 entries: `2 + 1 + 7 + 1 = 11`.
  ```rust
  // Tests to update:
  // - required_height_returns_ten_for_eleven_entries:
  //     rename → required_height_returns_eleven_for_thirteen_entries
  //     update assertion: assert_eq!(required_height(), 11)
  // - entry_count_constant_matches_entries_slice: no rename needed; passes automatically
  // - build_rows_produces_correct_line_count:
  //     update assertion: assert_eq!(rows.len(), 9)
  //     (1 padding + div_ceil(13,2)=7 rows + 1 separator = 9 lines)
  ```
  Rename the test accordingly:
  ```rust
  fn required_height_returns_eleven_for_thirteen_entries() {
      assert_eq!(required_height(), 11);
  }
  fn build_rows_produces_correct_line_count() {
      // 1 padding + ceil(13/2) + 1 separator = 1 + 7 + 1 = 9
      assert_eq!(rows.len(), 9);
  }
  ```

### 5. Add tests (AC1–AC5)

**In `input.rs`** (task 1 already covered above).

**In `stack_view/tests.rs`** — test `parent_cursor()`:
- [x] `parent_cursor_on_frame_returns_none`
- [x] `parent_cursor_on_var_returns_on_frame`
- [x] `parent_cursor_on_object_field_depth1_returns_on_var`
- [x] `parent_cursor_on_object_field_depth2_returns_shallower_field`
- [x] `parent_cursor_on_collection_entry_with_field_path_returns_object_field`
- [x] `parent_cursor_on_collection_entry_with_empty_field_path_returns_on_var`
- [x] `parent_cursor_on_collection_entry_obj_field_returns_collection_entry`
- [x] `parent_cursor_on_collection_entry_obj_field_depth2_returns_shallow_obj_field`

**In `stack_view/tests.rs`** — test Left arrow edge cases:
- [x] `left_on_non_expanded_var_navigates_to_frame`
- [x] `left_on_non_expanded_frame_is_noop`
- [x] `left_on_expanded_var_collapses_not_navigates`
- [x] `left_on_primitive_var_navigates_to_frame` (uses `VariableValue::Null` as non-ObjectRef)
- [x] `right_on_collection_var_dispatches_start_collection`

**In `help_bar.rs`** — tests updated in task 4 above.

### 6. Run validation

- [x] `cargo test --all` — zero failures (233 passed)
- [x] `cargo clippy --all-targets -- -D warnings` — zero warnings

## Dev Notes

### Why Right = Enter-for-Collapsed only

The enter handler for `OnVar` with `Expanded` dispatches `CollapseObj`. The Right arrow
must NOT collapse — that would be counterintuitive. Right arrow only expands; Left arrow
only collapses. This gives the user two independent axes of control.

### ADR: `Cmd`/`RightCmd`/`LeftCmd` duplication

The execution arms of the three handlers (Enter, Right, Left) are intentionally
duplicated. Local enums explicitly document which variants each handler actually uses —
avoiding implicit coupling from a shared module-level `Cmd` enum.
If in the future ≥4 variants share identical execution logic across handlers, extract a
helper `fn execute_tree_cmd(cmd, app)`. Do not do this prematurely.

### Left arrow: no collection auto-collapse from inside

**Left = structural navigation (cursor moves up). Escape = level close (collection
destroyed + cursor restored). These two semantics must not be merged.**

The Escape key already collapses the whole collection and returns to the parent field.
ArrowLeft inside a collection (e.g., `OnCollectionEntry`) merely moves the cursor to the
parent field without touching the collection. This avoids accidental collapse when the
user just wants to move focus upward. The user still has Escape if they want to close the
collection.

Exception: if the cursor is on `OnCollectionEntry` AND the entry's object IS expanded
(`StartEntryObj` was dispatched for it), then Left DOES collapse that entry object —
because the entry node itself is "expanded", and Left unexpands it. The collection
remains open.

### `parent_cursor()` is pure — no flat_items walk

`parent_cursor()` does NOT verify that the returned cursor is in `flat_items()`. It
computes the structural parent from cursor metadata alone. The caller (`app.rs`) should
call `set_cursor(parent)` unconditionally — `set_cursor` already calls `sync_list_state`
which realigns the ratatui `ListState`.

**Scroll visibility:** `sync_list_state()` calls `list_state.select(Some(idx))`. Ratatui's
`List` widget automatically scrolls to keep the selected item visible on the next render.
No manual scroll offset needed — verified at `stack_view.rs:1168`.

**If `parent_cursor()` returns a cursor not in `flat_items()`** (e.g., transient async
state): `flat_index()` returns `None` → `list_state.select(None)` → no crash, no
corruption. Resolves on the next navigation event. Acceptable.

**`compute_chunk_ranges` import:** already imported at `app.rs:32` — no action needed.

Exception: `OnObjectLoadingNode { field_path: [] }` → `OnVar { ... }`. The var still exists
even if the loading node is active. Safe.

### Watch out: Left on primitive `OnVar` — `selected_object_id()` returns `None`

`VariableValue::Primitive`, `VariableValue::StringValue`, and any other non-ObjectRef
variant causes `selected_object_id()` to return `None`. The Left handler MUST handle
this before the `expansion_state` check. The fix is the early-return guard:

```rust
let Some(oid) = s.selected_object_id() else {
    return Some(LeftCmd::NavigateToParent(s.parent_cursor()?));
};
```

Without this guard, Left is a silent no-op on primitive local variables — the user
cannot navigate back to the frame level from a primitive var using the keyboard.

### Watch out: Right on collection `OnVar` — interop with story 9.2

If story 9.2 is already implemented when this story is developed, `VariableValue::ObjectRef`
will have an `entry_count: Option<u64>` field and `selected_var_entry_count()` will exist.
The Right handler's `OnVar` arm MUST call `selected_var_entry_count()` BEFORE falling
through to `expansion_state`, otherwise Right on an ArrayList/Object[] dispatches
`StartObj` → async `expand_object` → `Failed` state (array IDs are not instances).

If story 9.2 is NOT yet implemented: `selected_var_entry_count()` does not exist — omit
that branch and add a `// TODO(9.2)` comment. The existing behavior (StartObj fails on
arrays) is not a regression introduced by this story.

### Watch out: Left/Right routing — confirm `handle_stack_frames_input` is focus-gated

Verify that `handle_stack_frames_input` is only called when `focus == Focus::StackFrames`.
A routing bug could send Left/Right to the stack handler while the favorites panel is
focused, causing unexpected cursor changes:
```
rg -n "handle_stack_frames_input\|Focus::StackFrames" crates/hprof-tui/src/app.rs
```

### Story 9.2 interaction

Story 9.2 adds `selected_var_entry_count()` and dispatches `StartCollection` from `OnVar`.
Story 9.3's Right handler for `OnVar` currently dispatches `StartObj` for any collapsed
`ObjectRef`. If story 9.2 is implemented first, the `OnVar` Enter handler will already
intercept collection vars before reaching the ObjectRef phase dispatch. Story 9.3's Right
handler for `OnVar` should mirror this: check `selected_var_entry_count()` first (if
story 9.2 is already done), then fall back to `expansion_state` check.

The Left handler for `OnVar` already checks `collection_chunks.contains_key(&oid)` first,
which handles the story 9.2 path correctly regardless of order.

If story 9.3 is implemented BEFORE story 9.2: Right arrow on a collection-var dispatches
`StartObj` (same as Enter currently), which will fail with a `Failed` state — same as the
current behavior. No regression. Story 9.2 will fix both Enter and Right for OnVar
collections.

### Collapse commands reuse exact same app logic

The Left handler's `CollapseObj`, `CollapseNestedObj`, `CollapseCollection`,
`CollapseEntryObj`, `CollapseFrame` arms are identical to the corresponding arms in the
Enter handler. They are duplicated inline (not extracted to a shared function) to keep
`handle_stack_frames_input` readable. If the Enter handler is ever refactored, update
the Left handler in tandem.

### `RightCmd` and `LeftCmd` are local enums

Define them inside the `InputEvent::Right` and `InputEvent::Left` arms (same pattern as
the existing `Cmd` enum inside the `InputEvent::Enter` arm at ~line 452). This keeps them
scoped and avoids polluting the module with one-use types.

### help_bar.rs `required_height` test name update

The test is named `required_height_returns_ten_for_eleven_entries` and asserts `== 10`.
After adding 2 entries (→ 13 total), the height becomes 11. Rename the test AND update
the assertion. The existing `entry_count_constant_matches_entries_slice` test will catch
a mismatch between `ENTRY_COUNT` and `ENTRIES.len()` at compile time automatically.

### Project Structure

| File | Change | Tasks |
|------|--------|-------|
| `crates/hprof-tui/src/input.rs` | Add `Right`, `Left` variants; map `KeyCode::Right/Left`; update test | 1 |
| `crates/hprof-tui/src/views/stack_view.rs` | Add `parent_cursor()` method; add unit tests | 2, 5 |
| `crates/hprof-tui/src/app.rs` | Add `InputEvent::Right` and `InputEvent::Left` arms in `handle_stack_frames_input` | 3 |
| `crates/hprof-tui/src/views/help_bar.rs` | Add 2 entries, update `ENTRY_COUNT`, fix tests | 4 |

### References

- [Source: docs/planning-artifacts/epics.md#Story 9.3] — ACs and description
- [Source: crates/hprof-tui/src/input.rs:11] — `InputEvent` enum
- [Source: crates/hprof-tui/src/input.rs:48] — `from_key()` match arms
- [Source: crates/hprof-tui/src/views/stack_view.rs:115] — `StackCursor` enum variants
- [Source: crates/hprof-tui/src/views/stack_view.rs:175] — `StackState` struct fields
- [Source: crates/hprof-tui/src/views/stack_view.rs:414] — `set_cursor()`
- [Source: crates/hprof-tui/src/views/stack_view.rs:620] — `cursor_collection_id()`
- [Source: crates/hprof-tui/src/views/stack_view.rs:654] — `expansion_state()`
- [Source: crates/hprof-tui/src/views/stack_view.rs:809] — `is_expanded()`
- [Source: crates/hprof-tui/src/app.rs:394] — `handle_stack_frames_input()`
- [Source: crates/hprof-tui/src/app.rs:450] — Enter handler with local `Cmd` enum
- [Source: crates/hprof-tui/src/app.rs:399] — Escape handler / collection collapse pattern
- [Source: crates/hprof-tui/src/views/help_bar.rs:17] — `ENTRY_COUNT` constant
- [Source: crates/hprof-tui/src/views/help_bar.rs:20] — `ENTRIES` constant

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Implemented `Right`/`Left` InputEvent variants and key mappings in `input.rs`
- Added `parent_cursor()` pure method to `StackState` in `state.rs`
- Added `InputEvent::Right` and `InputEvent::Left` arms in `handle_stack_frames_input()`;
  adapted from story spec: `s.collection_chunks` → `s.expansion.collection_chunks`,
  `StartCollection` carries restore cursor like Enter handler
- Updated `help_bar.rs`: ENTRY_COUNT 11→13, added → Expand / ← Unexpand entries
- Added 13 unit tests for `parent_cursor()` and Left/Right edge cases
- `VariableValue::Null` used as non-ObjectRef stand-in for primitive local test
- Code-review follow-up: Right now mirrors Enter for collection-entry collection paths,
  and Left now navigates to parent for non-ObjectRef collection-entry leaves
- Added 4 app-level regression tests for Right/Left collection-entry parity and
  parent navigation fallback behavior

### File List

- `crates/hprof-tui/src/input.rs`
- `crates/hprof-tui/src/views/stack_view/state.rs`
- `crates/hprof-tui/src/views/stack_view/tests.rs`
- `crates/hprof-tui/src/app/mod.rs`
- `crates/hprof-tui/src/app/tests.rs`
- `crates/hprof-tui/src/views/help_bar.rs`
- `docs/code-review/codex-story-9-3-adversarial-review.md`
- `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md`
- `docs/implementation-artifacts/sprint-status.yaml`

## Senior Developer Review (AI)

### Reviewer

Codex (2026-03-11)

### Findings Summary

- Initial adversarial review found 4 high-severity behavior gaps around
  Right/Left handling for collection-entry nodes.
- Follow-up implementation now aligns Right behavior with Enter for collection
  entry collection branches and restores Left parent navigation on non-expandable
  leaves.
- All Story 9.3 ACs verified as implemented with passing tests.

### Actions Applied

- Fixed `InputEvent::Right` in `handle_stack_frames_input()` for:
  - `StackCursor::OnCollectionEntry` collection dispatch parity
  - `StackCursor::OnCollectionEntryObjField` collection dispatch parity
- Fixed `InputEvent::Left` in `handle_stack_frames_input()` for:
  - non-`ObjectRef` `OnCollectionEntry` parent navigation fallback
  - non-`ObjectRef` `OnCollectionEntryObjField` parent navigation fallback
- Added 4 regression tests in `crates/hprof-tui/src/app/tests.rs` to lock behavior.

### Verification

- `cargo test --all`: pass
- `cargo clippy --all-targets -- -D warnings`: pass

## Change Log

- 2026-03-11 (Codex): Applied adversarial review fixes for Right/Left parity and
  parent navigation fallback, added app-level regression tests, set story status
  to `done`, and synced sprint status.
- 2026-03-12 (claude-sonnet-4-6): Post-release bug fixes from user testing with
  deeply nested collections (array inside custom type inside outer list):
  - `tree_render.rs`: collection/array fields in expanded objects showed wrong
    `+`/`-` indicator — fixed by checking `collection_chunks` in
    `append_object_children` and `append_collection_entry_obj`
  - `state.rs`: `parent_cursor()` for `OnCollectionEntry`/`OnChunkSection` now
    consults `collection_restore_cursors[collection_id]` first, so Left from an
    inner collection entry correctly lands on `OnCollectionEntryObjField` instead
    of `OnObjectField`
  - `app/mod.rs`: Left handler for `OnCollectionEntryObjField` now checks
    `collection_chunks` before object phases, so a second Left from the array
    field row collapses the inner collection instead of navigating to its parent
  - Added 2 regression tests:
    `parent_cursor_on_collection_entry_uses_restore_cursor_for_nested_collection`,
    `left_on_collection_entry_obj_field_with_open_collection_detects_collapse`
