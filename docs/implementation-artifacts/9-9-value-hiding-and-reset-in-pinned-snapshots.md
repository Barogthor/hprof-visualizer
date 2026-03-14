# Story 9.9: Value Hiding & Reset in Pinned Snapshots

Status: done

## Story

As a user,
I want to hide individual values from a pinned snapshot and reveal them on demand,
So that I can reduce noise in the favorites panel and focus on the fields that matter.

## Acceptance Criteria

1. **AC1 – Hide a field or variable:**
   Given the cursor is on a field or variable row within a pinned snapshot in the favorites panel,
   When the user presses `h`,
   Then that row is completely removed from the display (line freed, no placeholder)
   without removing the pin or affecting any other data.

2. **AC2 – Reveal hidden rows via `H` toggle:**
   Given a pinned snapshot has one or more hidden fields,
   When the user presses `H`,
   Then all hidden rows appear as `▪ [hidden: var[N]]` / `▪ [hidden: fieldName]`
   placeholders, navigable with the cursor.

3. **AC3 – Restore a hidden field individually:**
   Given the snapshot is in reveal mode (`H` active) and the cursor is on a placeholder,
   When the user presses `h`,
   Then that field is restored to visible at its original position.

4. **AC4 – `H` toggles reveal mode off:**
   Given reveal mode is active,
   When the user presses `H` again,
   Then placeholders are hidden again (rows removed from display).

5. **AC5 – Scope:**
   Only instance fields and Frame local variables can be hidden.
   Static fields and collection entries are unaffected (`h` is a no-op on those rows).

6. **AC6 – Help panel reflects new shortcuts:**
   Given the help panel is open,
   When focus is on the favorites panel,
   Then `h` (Hide / show field) and `H` (Reveal / hide hidden) appear active in the
   keymap; they are dimmed when focus is on the thread list or stack frames.

## Tasks / Subtasks

- [x] **Task 1 – Add `HideKey` and `hidden_fields` to `PinnedItem` (data model)**
  - [x] 1.1 In `crates/hprof-tui/src/favorites.rs`, add a new public enum immediately
        above `PinnedItem`:
        ```rust
        /// Identifies a renderable row that can be hidden within a pinned snapshot.
        ///
        /// Only instance fields and Frame local variables are in scope.
        /// Static fields are excluded for simplicity.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum HideKey {
            /// A local variable row in a `Frame` snapshot: index in the `variables` vec.
            Var(usize),
            /// An instance field of an expanded object:
            /// (`parent_id`, `field_idx` in that object's `FieldInfo` vec).
            Field { parent_id: u64, field_idx: usize },
        }
        ```
  - [x] 1.2 Add `pub hidden_fields: HashSet<HideKey>` to `PinnedItem`, placed after
        `local_collapsed` (added in Story 9.8):
        ```rust
        pub struct PinnedItem {
            pub thread_name: String,
            pub frame_label: String,
            pub item_label: String,
            pub snapshot: PinnedSnapshot,
            pub key: PinKey,
            pub local_collapsed: HashSet<u64>,
            pub hidden_fields: HashSet<HideKey>,  // NEW
        }
        ```
  - [x] 1.3 The single `PinnedItem` construction site is
        `PinnedItemFactory::make_pinned_item` in `favorites.rs`. Add
        `hidden_fields: HashSet::new()` to the struct literal there — no other
        arms to update.
  - [x] 1.4 Update ALL test literal constructions of `PinnedItem` in `favorites.rs`
        and `favorites_panel/tests.rs` to include `hidden_fields: HashSet::new()`.
        Search those two files for `PinnedItem {` to find every direct construction
        site.

- [x] **Task 2 – Add `hidden_fields` support to `render_variable_tree`**
  - [x] 2.1 In `crates/hprof-tui/src/views/tree_render.rs`, add a `hidden_fields`
        field to `RenderCtx` as an `Option`. The struct already has 7 fields
        including `object_errors` and `snapshot_mode`; add `hidden_fields` as the
        last field:
        ```rust
        struct RenderCtx<'a> {
            object_fields: &'a HashMap<u64, Vec<FieldInfo>>,
            object_static_fields: &'a HashMap<u64, Vec<FieldInfo>>,
            collection_chunks: &'a HashMap<u64, CollectionChunks>,
            object_phases: &'a HashMap<u64, ExpansionPhase>,
            object_errors: &'a HashMap<u64, String>,   // already present
            show_object_ids: bool,
            snapshot_mode: bool,                       // already present
            hidden_fields: Option<&'a HashSet<HideKey>>,  // NEW — None = no hiding
        }
        ```
        Add `use crate::favorites::HideKey;` at the top of the file.

  - [x] 2.2 Add `hidden_fields: Option<&'a HashSet<HideKey>>` as the last parameter
        of `render_variable_tree`. Pass it through to the `RenderCtx` construction.

        Using `Option` rather than a bare reference keeps the API self-documenting:
        `None` means "hiding not applicable here" and callers that don't use the
        feature don't need to know about `HideKey` or allocate an empty set.

        **Callers to update:**
        - `crates/hprof-tui/src/views/stack_view/state.rs` (line ~1737,
          `flat_items`): pass `None`. No import of `HideKey` needed.
        - `favorites_panel/mod.rs` (4 call sites in `collect_row_metadata`
          debug asserts and `FavoritesPanel::render`): pass
          `Some(&item.hidden_fields)`.

        In the hide-check sites (Tasks 2.3 and 2.4), unwrap with a short-circuit:
        ```rust
        let is_hidden = ctx.hidden_fields
            .map(|s| s.contains(&key))
            .unwrap_or(false);
        if is_hidden { … continue; }
        ```

  - [x] 2.3 In the `TreeRoot::Frame` branch of `render_variable_tree`, the
        non-empty vars branch currently reads:
        ```rust
        for var in vars {
            append_var(var, "  ", &ctx, &mut items);
        }
        ```
        Replace with an indexed variant that checks `hidden_fields`:
        ```rust
        for (var_idx, var) in vars.iter().enumerate() {
            let key = HideKey::Var(var_idx);
            let is_hidden = ctx.hidden_fields
                .map(|s| s.contains(&key))
                .unwrap_or(false);
            if is_hidden {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("  \u{25AA} [hidden: var[{}]]", var_idx),
                    THEME.null_value,
                ))));
                continue;
            }
            append_var(var, "  ", &ctx, &mut items);
        }
        ```
        The `▪` bullet (U+25AA) visually marks placeholder rows without taking up
        a `+`/`-` column. `var[N]` is more readable than a bare index.

  - [x] 2.4 In `append_fields_expanded` (the actual function name — `append_object_children`
        does not exist), inside the `ExpansionPhase::Expanded` arm, the loop over
        instance fields currently reads:
        ```rust
        for field in field_list {
            if let FieldValue::ObjectRef { id, class_name, .. } = &field.value
                && visited.contains(id)
            { /* cyclic check */ }
            let (child_phase, child_unavailable) =
                render_single_field(field, indent, ctx, items, static_ctx);
            /* recursion … */
        }
        ```
        Replace with an indexed variant. Add the hidden-field guard as the **first**
        statement of each iteration (before the cyclic check):
        ```rust
        for (field_idx, field) in field_list.iter().enumerate() {
            let hide_key = HideKey::Field { parent_id: object_id, field_idx };
            let is_hidden = ctx.hidden_fields
                .map(|s| s.contains(&hide_key))
                .unwrap_or(false);
            if is_hidden {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!("{indent}  \u{25AA} [hidden: {}]", field.name),
                    THEME.null_value,
                ))));
                continue;  // children suppressed — no recursion
            }
            // existing cyclic check, render_single_field, recursion (unchanged)
        }
        ```
        **Ordering invariant:** the guard must be FIRST — before the cyclic check and
        before `render_single_field`. If placed after the cyclic check, a cyclic hidden
        field would emit `[cyclic]` instead of the placeholder, causing a row-count
        mismatch with `collect_row_metadata` and cursor drift.

        **Scope limit:** `hidden_fields` applies only to instance fields in
        `append_fields_expanded` and Frame variables in `render_variable_tree`.
        Static fields (`append_static_items`) are not affected and require no changes.

- [x] **Task 3 – Track field rows in `collect_row_metadata` and `FavoritesPanelState`**
  - [x] 3.1 **Read `favorites_panel/mod.rs` in full before editing.** The actual
        implementation delegates to `MetadataCollector` (a private struct). The
        `RowMetadata` type alias and `collect_row_metadata` function are the external
        API; `MetadataCollector` and its methods implement the traversal.

        **Step A — extend the type alias and `MetadataCollector`:**
        ```rust
        type FieldRowMap = HashMap<usize, (HideKey, bool)>;
        type RowMetadata = (usize, RowKindMap, ChunkSentinelMap, FieldRowMap);  // was 3-tuple
        ```
        Add `field_row_map: FieldRowMap` to `MetadataCollector`. Initialize to
        `HashMap::new()` in `MetadataCollector::new`. Extend `into_parts` to return
        the 4-tuple.

        Add `use crate::favorites::HideKey;` in `favorites_panel/mod.rs` alongside
        other imports from `crate::favorites`.

        **Step B — traversal additions (must mirror `append_fields_expanded` and the
        Frame loop in `render_variable_tree` exactly):**

        In `collect_frame_rows` (mirrors `TreeRoot::Frame`): replace the plain
        `for var in vars` call-loop with an indexed version. If the var's `HideKey::Var`
        is in `item.hidden_fields`, emit 1 placeholder row via `self.push_row()` and
        `continue`; do NOT call `collect_var_row`. Otherwise record
        `(HideKey::Var(var_idx), false)` at the row returned by `push_row()` before
        delegating to `collect_var_row`.

        Note: `HideKey::Var` is **only** produced in the `Frame` branch. The `Subtree`
        root object is never wrapped in a `HideKey::Var` entry.

        In `collect_object_children_rows` (mirrors `append_fields_expanded`): inside the
        `ExpansionPhase::Expanded` arm, replace the plain `for field in field_list` loop
        with an indexed variant. Add the hidden-field guard as the **first statement**
        of each iteration (before the cyclic check):
        - Record `(HideKey::Field { parent_id: object_id, field_idx }, is_hidden)` at
          the pushed row.
        - If `is_hidden=true`: emit 1 row via `push_row()` and `continue`
          immediately — the `continue` must skip the cyclic check, `child_phase`
          computation, collection branch, and recursive `collect_object_children_rows`.
          Any fallthrough past `continue` emits spurious child rows that
          `render_variable_tree` does not emit, causing row-count drift.

        Static field rows and header/separator rows: NOT added to `field_row_map`.

        **`truncated` warning row offset:** if `item.snapshot.truncated == true`,
        `render_variable_tree` emits 1 extra warning row immediately after the header
        (before any variable/field rows). `collect_row_metadata` must account for this.
        Concretely: after the header row accounting (`current_row = 1`) and **before
        entering the variable/field loop**, insert:
        ```rust
        if item.snapshot.truncated {
            current_row += 1; // skip the truncated-warning row
        }
        ```
        Failing to do this shifts every `field_row_map` entry by -1 relative to the
        actual rendered rows — pressing `h` hides the wrong field. Mirror the exact
        same logic as the `truncated` handling in the 9.8 `collect_row_metadata`
        implementation.

        **Row-count invariant still holds:** a hidden row is always replaced by
        exactly 1 placeholder row, so row count for a hidden primitive field is
        unchanged. For a hidden ObjectRef field that was expanded (N child rows),
        the row count decreases by N — the placeholder row replaces 1 field row and
        N child rows with just 1 row. `collect_row_metadata` must mirror this
        accurately to prevent cursor misalignment.

        Update the `debug_assert_eq!` guard inside `collect_row_metadata` (added in
        Story 9.8) to account for this: the assertion compares the computed
        `row_count` against what `render_variable_tree` produces. Keep the assertion
        as is — it will catch drift automatically.

  - [x] 3.2 Add `field_row_maps: Vec<FieldRowMap>` to `FavoritesPanelState`, after
        `chunk_sentinel_maps`. Initialize as `Vec::new()` in `Default`.
        ```rust
        pub struct FavoritesPanelState {
            selected_item: usize,
            items_len: usize,
            sub_row: usize,
            row_counts: Vec<usize>,
            row_kind_maps: Vec<HashMap<usize, (u64, bool)>>,
            chunk_sentinel_maps: Vec<HashMap<usize, (u64, usize)>>,
            field_row_maps: Vec<HashMap<usize, (HideKey, bool)>>,  // NEW
            list_state: ListState,
        }
        ```

  - [x] 3.3 Extend `update_row_metadata` to accept and store `field_row_maps`:
        ```rust
        pub(crate) fn update_row_metadata(
            &mut self,
            row_counts: Vec<usize>,
            row_kind_maps: Vec<RowKindMap>,
            chunk_sentinel_maps: Vec<ChunkSentinelMap>,
            field_row_maps: Vec<FieldRowMap>,  // NEW
        )
        ```
        Add `debug_assert_eq!(field_row_maps.len(), self.items_len, ...)`.
        **Do NOT** add a `resize_with` call inside `update_row_metadata` — that method
        receives fully-computed data and must not pad it. Instead, extend
        `set_items_len` (which already resizes `row_counts`, `row_kind_maps`, and
        `chunk_sentinel_maps`) to also resize `self.field_row_maps`:
        ```rust
        self.field_row_maps.resize_with(len, HashMap::new);
        ```
        This keeps all Vec-resizing logic in one place.

  - [x] 3.4 Update the call site in `FavoritesPanel::render`: `collect_row_metadata`
        is called once **per item** inside the existing render loop that already
        collects `all_row_counts`, `all_row_kind_maps`, and `all_chunk_sentinel_maps`.
        Extend that same loop to unpack the 4-tuple and push the new `field_row_map`
        into an `all_field_row_maps: Vec<FieldRowMap>`. Pass `all_field_row_maps`
        to `update_row_metadata`.

  - [x] 3.5 Add `pub fn field_key_at_cursor(&self) -> Option<(HideKey, bool)>` to
        `FavoritesPanelState`:
        ```rust
        pub fn field_key_at_cursor(&self) -> Option<(HideKey, bool)> {
            self.field_row_maps
                .get(self.selected_item)?
                .get(&self.sub_row)
                .copied()
        }
        ```
        The name `field_key_at_cursor` accurately reflects that it returns the key for
        whatever row is currently under the cursor — whether visible or already hidden.
        The second element `is_hidden` lets the `h` handler branch between insert and
        remove (toggle behavior). The header row has no entry in `field_row_maps`, so
        `None` is returned for it automatically.

- [x] **Task 4 – Handle `h` and `H` in `handle_favorites_input` (AC1–AC4)**
  - [x] 4.1 In `crates/hprof-tui/src/app/mod.rs`, add `use crate::favorites::HideKey;`
        at the top of the file (alongside existing favorites imports).

  - [x] 4.2 In `handle_favorites_input`, add two new arms. Place them **before** any
        catch-all `InputEvent::SearchChar(_)` arm:
        ```rust
        // NOTE: 'h'/'H' are intentionally handled as SearchChar rather than dedicated
        // InputEvent variants. Adding HideField/ResetHidden to InputEvent would map
        // 'h' globally, breaking thread-list incremental search (the from_key
        // catch-all produces SearchChar for any unbound letter). Focus-based dispatch
        // ensures these arms execute only when the favorites panel is focused.
        // h — hide / show field (AC1, AC2)
        InputEvent::SearchChar('h') => {
            if let Some((key, is_hidden)) =
                self.favorites_list_state.field_key_at_cursor()
            {
                let idx = self.favorites_list_state.selected_index();
                if let Some(item) = self.pinned.get_mut(idx) {
                    if is_hidden {
                        item.hidden_fields.remove(&key);
                    } else {
                        item.hidden_fields.insert(key);
                    }
                    // row_counts are one frame stale — clamp defensively so
                    // sub_row does not point past the (now shorter) item.
                    self.favorites_list_state.clamp_sub_row();
                }
            }
        }
        // H — reset all hidden fields in current snapshot (AC3, AC4)
        InputEvent::SearchChar('H') => {
            let idx = self.favorites_list_state.selected_index();
            if let Some(item) = self.pinned.get_mut(idx) {
                item.hidden_fields.clear(); // no-op if already empty (AC4)
                // No clamp_sub_row call: resetting hidden fields only adds rows back
                // (row count can only increase or stay the same). The cursor's
                // current sub_row is therefore still within bounds — no overflow.
            }
        }
        ```

  - [x] 4.3 **Key conflict check — read the actual dispatcher.** Before adding the
        new arms, open `App::handle_input` (or whatever function dispatches
        `InputEvent` to panel handlers) and confirm that the call to
        `handle_favorites_input` is inside a branch conditioned on
        `self.focus == Focus::Favorites`. If the dispatch is correct, the new arms
        fire only when the favorites panel is focused and thread-list search is
        unaffected. If the dispatch is NOT guarded by focus (unexpected), do NOT
        modify the dispatch logic — that is a pre-existing bug outside this story's
        scope; flag it in the Completion Notes and proceed with the narrowest
        possible fix (guard the two new arms with an explicit `if self.focus !=
        Focus::Favorites { return; }` at the top of `handle_favorites_input`).
        No change to `input::from_key` is needed in either case.

- [x] **Task 5 – Update help bar (AC5)**
  - [x] 5.1 In `crates/hprof-tui/src/views/help_bar.rs`, insert two entries into
        `ENTRIES` immediately after the `("g", "Favorites: go to source", FAV)` entry:
        ```rust
        ("h", "Favorites: hide / show field", FAV),
        ("H", "Favorites: reset hidden fields", FAV),
        ```

  - [x] 5.2 Update `ENTRY_COUNT` from `19` to `21`.

  - [x] 5.3 Rename and update the height test:
        ```rust
        // Old:
        // fn required_height_returns_fourteen_for_nineteen_entries
        // assert_eq!(required_height(), 14);
        //
        // New:
        #[test]
        fn required_height_returns_fifteen_for_twenty_one_entries() {
            // div_ceil(21, 2) = 11; 2 + 1 + 11 + 1 = 15
            assert_eq!(required_height(), 15);
        }
        ```

  - [x] 5.4 Update `build_rows_produces_correct_line_count`:
        ```rust
        // Old: assert_eq!(...len(), 12)  → 1 + 10 + 1
        // New:
        assert_eq!(build_rows(HelpContext::ThreadList).len(), 13); // 1 + 11 + 1
        assert_eq!(build_rows(HelpContext::StackFrames).len(), 13);
        assert_eq!(build_rows(HelpContext::Favorites).len(), 13);
        ```

- [x] **Task 6 – Tests (TDD)**

  Tests 6.1–6.5 live in the `#[cfg(test)]` module of `favorites.rs`.
  Tests 6.6–6.8 live in `tree_render.rs` (inline `#[cfg(test)]`).
  Tests 6.9–6.13 live in `favorites_panel/tests.rs`.
  Tests 6.14–6.15 live in `help_bar.rs`.
  Tests 6.16–6.17 live in `favorites_panel/tests.rs` and `app/tests.rs`.

  - [x] 6.1 `hide_key_var_and_field_are_distinct` — assert
        `HideKey::Var(0) != HideKey::Field { parent_id: 0, field_idx: 0 }` and that
        both variants can coexist in a `HashSet`.

  - [x] 6.2 `snapshot_from_cursor_initializes_hidden_fields_empty` — call
        `snapshot_from_cursor` on a valid frame cursor position; assert the returned
        `PinnedItem.hidden_fields.is_empty()`. This verifies the constructor
        initializes the field (a missing `hidden_fields: HashSet::new()` in any arm
        would cause a compile error that this test catches before any runtime path).

  - [x] 6.3 `pinned_item_hidden_fields_toggle_hides_and_restores` — take a
        `PinnedItem` from `snapshot_from_cursor`; insert `HideKey::Var(0)` into
        `item.hidden_fields`; assert `item.hidden_fields.contains(&HideKey::Var(0))`;
        remove it; assert `!item.hidden_fields.contains(&HideKey::Var(0))`. Verifies
        the field is mutable and accessible.

  - [x] 6.4 `pinned_item_hidden_fields_reset_clears_multiple` — take a `PinnedItem`;
        insert `HideKey::Var(0)` and `HideKey::Field { parent_id: 1, field_idx: 0 }`;
        call `item.hidden_fields.clear()`; assert `item.hidden_fields.is_empty()`.

  - [x] 6.5 `pinned_item_hidden_fields_reset_noop_when_empty` — take a fresh
        `PinnedItem` (hidden_fields empty); call `item.hidden_fields.clear()`; assert
        no panic and `item.hidden_fields.is_empty()`.

  - [x] 6.6 `render_variable_tree_hidden_var_shows_placeholder` — call
        `render_variable_tree` with `TreeRoot::Frame { vars }` (one
        `VariableValue::Null` var), all maps empty; pass
        `Some(&hidden_fields)` where `hidden_fields` = `{HideKey::Var(0)}`; assert
        the output `Vec<ListItem>` has exactly 1 item whose plain text contains
        `"[hidden:"`.

  - [x] 6.7 `render_variable_tree_not_hidden_var_shows_normal` — same setup but pass
        `None` for `hidden_fields`; assert the item text does NOT contain `"[hidden:"`.

  - [x] 6.8 `render_variable_tree_hidden_field_suppresses_children` — build an
        `object_fields` map with root object `1` having a single `ObjectRef` field
        pointing to child `2` (with 2 primitive fields). Call
        `render_variable_tree` with `TreeRoot::Subtree { root_id: 1 }`,
        `object_phases` fully expanded.
        Note: `render_variable_tree` returns **content items only** — no header or
        separator rows (those are added by `favorites_panel.rs` around the call).
        - **Baseline** (pass `None` for `hidden_fields`): assert `items.len() == 3`
          (1 ObjectRef field row + 2 child primitive rows).
        - **With hide** (pass `Some(&set)` where `set` =
          `{HideKey::Field { parent_id: 1, field_idx: 0 }}`):
          assert `items.len() == 1` (only the placeholder `▪ [hidden: fieldName]`).
        This is the primary regression guard for children suppression.

  - [x] 6.9 `collect_row_metadata_field_row_map_populated` — build a `Frame`
        snapshot with two `VariableValue::Null` variables (primitives, no recursion,
        `truncated = false`); call `collect_row_metadata`; assert:
        - `field_row_map.get(&1)` = `Some(&(HideKey::Var(0), false))` — var[0] at
          sub_row 1 (row 0 is the header).
        - `field_row_map.get(&2)` = `Some(&(HideKey::Var(1), false))` — var[1] at
          sub_row 2.
        - `field_row_map.len() == 2` (no spurious extra entries).

  - [x] 6.10 `collect_row_metadata_hidden_var_row_shows_is_hidden_true` — same as
        6.9 but add `HideKey::Var(0)` to `item.hidden_fields`; assert the entry for
        variable 0's sub_row has `is_hidden = true`.

  - [x] 6.11 `collect_row_metadata_hidden_objectref_row_count_decreases` — build a
        `Subtree` snapshot: root object `1` has a single `ObjectRef` field (field_idx=0)
        pointing to child `2`; child `2` has exactly 2 primitive fields.
        Both objects are in `object_fields`. `local_collapsed` is empty (root's ObjectRef
        field is expanded).
        - **Baseline** (`hidden_fields` empty): assert `row_count == 5`.
          Breakdown: header=1 + ObjectRef field row=1 + 2 primitive child rows=2 +
          separator=1.
        - **With hide** (`hidden_fields` = `{HideKey::Field { parent_id: 1, field_idx: 0 }}`):
          assert `row_count == 3` (header=1 + placeholder=1 + separator=1).
          The 2 child rows must be absent — this is the regression guard for the
          `continue`-must-skip-all-recursion invariant.
        Note: `collect_row_metadata` returns `row_count` = header(1) + content + separator(1),
        while `render_variable_tree` returns content items only. Therefore:
        `row_count == render_variable_tree_output.len() + 2`.
        Call **both** functions on the same snapshot and assert this invariant for
        both cases, following the same pattern as tests 6.7/6.8 from Story 9.8.
        When calling `render_variable_tree`:
        - Baseline: pass `None` for `hidden_fields`.
        - Hidden case: pass `Some(&hide_set)` where `hide_set` =
          `{HideKey::Field { parent_id: 1, field_idx: 0 }}`
          (same key as in `item.hidden_fields`).

  - [x] 6.12 `favorites_panel_state_field_key_at_cursor_correct` — construct a
        `FavoritesPanelState`, manually set `field_row_maps` with one entry
        `sub_row=1 → (HideKey::Var(0), false)`, set `selected_item=0`, `sub_row=1`;
        assert `field_key_at_cursor()` returns `Some((HideKey::Var(0), false))`.

  - [x] 6.13 `favorites_panel_state_field_key_at_cursor_none_for_header` — same
        state, `sub_row=0`; assert `field_key_at_cursor()` returns `None` (header
        row is not in `field_row_maps`).

  - [x] 6.14 `help_bar_h_key_applicable_only_in_favorites` — find the `"h"` entry in
        `ENTRIES`; assert its mask has the `FAV` bit set and `THREAD`/`STACK` bits
        NOT set.

  - [x] 6.15 `help_bar_H_key_applicable_only_in_favorites` — same check for `"H"`.

  - [x] 6.16 `handle_favorites_input_h_noop_when_no_pinned_items` — call
        `handle_favorites_input` with `SearchChar('h')` when `self.pinned` is
        empty; assert no panic and `self.pinned` remains empty. (Moved from Task 4.4
        for visibility.)

  - [x] 6.17 `collect_row_metadata_truncated_offset_shifts_field_row_map` — build a
        `Frame` snapshot with two `VariableValue::Null` variables and set
        `snapshot.truncated = true`; call `collect_row_metadata`; assert:
        - `field_row_map.get(&2)` = `Some(&(HideKey::Var(0), false))` — var[0] is
          at sub_row 2 (row 0 = header, row 1 = truncated-warning, row 2 = first var).
        - `field_row_map.get(&3)` = `Some(&(HideKey::Var(1), false))` — var[1] at
          sub_row 3.
        - `field_row_map.get(&1)` = `None` (no field at the warning row).
        This is the direct regression guard for the `truncated` offset bug: if the
        `current_row += 1` increment is missing, var[0] lands at sub_row 1 and
        pressing `h` on the warning row would incorrectly hide var[0].

- [x] **Task 7 – Validation**
  - [x] `cargo test --all` — zero failures.
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo fmt -- --check`
  - [x] `cargo build` — verify `stack_view.rs` compiles without modification after
        passing `None` to `render_variable_tree` (the signature change is transparent:
        `None` produces identical output to before).
  - [x] Verify existing `stack_view` tests pass without modification.
  - [x] Manual smoke: pin an item with several fields → focus favorites → navigate
        to a field row → press `h` → field is replaced by `▪ [hidden: name]`
        placeholder → press `h` on placeholder → field re-appears.
  - [x] Manual smoke: hide 2–3 fields → press `H` → all fields visible again.
  - [x] Manual smoke: press `H` on a snapshot with no hidden fields → no crash, no
        visual change (AC4).
  - [x] Manual smoke: press `?` while favorites focused → `h` and `H` entries appear
        active; switch focus to thread list → those entries are dimmed.
  - [x] **Regression 9.8:** navigate between multiple pinned items with Up/Down →
        header of each item correctly highlighted (flat-cursor model intact).
  - [x] **Regression 9.8:** expand/collapse fields in favorites with Right/Left →
        still works after `FavoritesPanelState` changes.
  - [x] Manual smoke: hide a field on item 1 → navigate to item 2 (ArrowDown) →
        navigate back to item 1 → the field is still hidden (persisted on `PinnedItem`).
  - [x] **Regression 9.6:** press `g` in favorites → navigate to source thread/frame.
  - [x] **Regression 9.7:** press `?` → context dimming active.

## Definition of Done

All Task 1–7 checkboxes ticked. 17 new tests (6.1–6.17) pass. Clippy and fmt clean.
Manual smokes pass (including 9.6, 9.7, 9.8 regression checks). Story status → `review`.

## Dev Notes

### Dependency on Story 9.8

9.9 builds directly on 9.8. The following elements introduced in 9.8 are used here:
- `PinnedItem.local_collapsed: HashSet<u64>` — 9.9 adds `hidden_fields` alongside it.
- `FavoritesPanelState` flat-cursor design (`selected_item`, `sub_row`, `row_kind_maps`,
  `chunk_sentinel_maps`, `update_row_metadata`, `collect_row_metadata`, `clamp_sub_row`).
- `handle_favorites_input` arms for Right/Left/Enter.

**Do NOT start 9.9 until 9.8 is merged.** Read `handle_favorites_input` in the current
branch state before adding Task 4 arms — 9.8 added `Right`, `Left`, `Enter`, and
`Down`/chunk-sentinel arms. Do not accidentally overwrite them.

**`RowMetadata` type alias and `collect_row_metadata` return type change from 3-tuple to
4-tuple.** Tests in `favorites_panel/tests.rs` that destructure the return value of
`collect_row_metadata` as a 3-tuple will fail to compile once Task 3.1 is applied.
Before running tests, search `favorites_panel/tests.rs` for `collect_row_metadata`
destructuring patterns and update them to bind the new `field_row_map` fourth element
(use `_` if unused in that test).

### Why `HideKey` is an enum, not a plain tuple

`(u64, usize)` is ambiguous: for a Frame variable the `u64` has no natural meaning
(frame_id could work but leaks frame-level concepts into the hide model). An enum
variant clearly distinguishes the two cases and makes match arms self-documenting.
It also prevents subtle bugs where `Var(0)` and `Field { parent_id: 0, field_idx: 0 }`
would collide in a `HashSet<(u64, usize)>`.

### Row count change when hiding an expanded ObjectRef field

Hiding a primitive field: 1 row → 1 placeholder row. Row count unchanged.
Hiding an expanded ObjectRef field with N visible child rows:
  N+1 rows → 1 placeholder row. Row count decreases by N.
Hiding a collapsed ObjectRef field: 1 row → 1 placeholder row. Row count unchanged.

This means `collect_row_metadata` (via `MetadataCollector`) must reflect the same
logic as `render_variable_tree` / `append_fields_expanded`:
when `hidden_fields` contains the field key, emit exactly 1 row and skip all recursion.
The one-frame lag of `row_counts` (updated at render, read at next keypress) is the same
as for `row_kind_maps` in 9.8 — harmless because `clamp_sub_row()` is called after any
toggle that could shorten an item.

**Known limitation of `clamp_sub_row` after hide:** `clamp_sub_row` only corrects
upper-bound overflow. If the user hides a field at a sub_row *above* the current cursor,
the item shrinks in the upper portion and `sub_row` keeps its old value — which now
refers to a different row than before (shifted by the collapse). This is a one-frame
misalignment: the next render recomputes `row_kind_maps` and `field_row_maps` from
updated state, so the following keypress acts on the correct row. No data corruption
occurs. Acceptable for P2.

Conversely, hiding a row *below* the cursor only removes rows below the current
position — `sub_row` remains valid and `clamp_sub_row` is a no-op (no drift).

### `h` key does not require a new `InputEvent` variant

`h` and `H` are mapped by `input::from_key` to `SearchChar('h')` and `SearchChar('H')`
via the printable-character catch-all. Adding `HideField`/`ResetHidden` variants would
prevent `h` from functioning as a search character in the thread list (where search mode
is active). Instead, `handle_favorites_input` intercepts `SearchChar('h')` and
`SearchChar('H')` before any catch-all — these arms fire only when focus is on the
favorites panel, leaving thread-list search fully intact.

### Independence of `hidden_fields` and `local_collapsed`

These two states are fully orthogonal and never interact:

- `local_collapsed: HashSet<u64>` — set of object IDs whose **children** are hidden.
  The field row itself remains visible (shows `+`). Managed by `←` / `→`.
- `hidden_fields: HashSet<HideKey>` — set of field/var rows that are **entirely
  hidden** (replaced by a placeholder). Children are suppressed as a side-effect.
  Managed by `h`.

Pressing `H` (reset) calls `hidden_fields.clear()` only. `local_collapsed` is
untouched. Consequence:
- A field that was **collapsed then hidden**: after `H`, it reappears as **collapsed**
  (`+`) — the collapse state is preserved.
- A field that was **expanded then hidden**: after `H`, it reappears as **expanded**
  (`-`) — the expansion state is preserved.

This is the correct and intended behavior: `H` restores visibility, not expansion
state. Do not add any interaction between `hidden_fields.clear()` and `local_collapsed`.

### Static fields are out of scope

`append_static_items` is not modified. Static fields are non-interactive rows (not in
`row_kind_maps` per Story 9.8). Adding hide support for them would require a third
key variant in `HideKey` and is not called out in the ACs. Defer if desired.

### `Option<&HashSet<HideKey>>` instead of a bare reference

`render_variable_tree` takes `hidden_fields: Option<&HashSet<HideKey>>`. The `Option`
avoids forcing every caller (including `stack_view.rs`) to know about `HideKey` or
maintain a static empty set. `stack_view.rs` passes `None` — zero cost, zero import.
`favorites_panel.rs` passes `Some(&item.hidden_fields)`.

This resolves the API coupling concern: the shared function signature signals that
hiding is an optional overlay, not a core concern of the renderer. Future callers that
do not need hiding pass `None` without any boilerplate.

### Files changed

| File | Change |
|------|--------|
| `crates/hprof-tui/src/favorites.rs` | Add `HideKey` enum; add `hidden_fields` to `PinnedItem`; update `make_pinned_item` and tests |
| `crates/hprof-tui/src/views/tree_render.rs` | Add `hidden_fields` to `RenderCtx` and `render_variable_tree`; add placeholder rendering in Frame loop and `append_fields_expanded` |
| `crates/hprof-tui/src/views/favorites_panel/mod.rs` | Add `FieldRowMap` alias; extend `MetadataCollector` and `collect_row_metadata` to 4-tuple; add `field_row_maps` to `FavoritesPanelState`; add `field_key_at_cursor`; update `update_row_metadata` and `set_items_len` |
| `crates/hprof-tui/src/app/mod.rs` | Add `SearchChar('h')` and `SearchChar('H')` arms in `handle_favorites_input` |
| `crates/hprof-tui/src/views/help_bar.rs` | Add 2 entries; update `ENTRY_COUNT` 19→21; update tests |
| `crates/hprof-tui/src/views/stack_view/state.rs` | Update `render_variable_tree` call site in `flat_items` to pass `None` |

No changes to `input.rs`, `engine` crate, or parser crates.

### References

- `crates/hprof-tui/src/favorites.rs` — `PinnedItem`, `PinnedSnapshot`,
  `PinnedItemFactory::make_pinned_item`, `snapshot_from_cursor`
- `crates/hprof-tui/src/views/tree_render.rs` — `render_variable_tree`, `RenderCtx`,
  `append_fields_expanded` (field loop inside `ExpansionPhase::Expanded`),
  `TreeRoot::Frame` vars loop (~line 89)
- `crates/hprof-tui/src/views/favorites_panel/mod.rs` — `MetadataCollector`,
  `collect_row_metadata`, `FavoritesPanelState`, `update_row_metadata`,
  `set_items_len`, `FavoritesPanel::render`
- `crates/hprof-tui/src/views/favorites_panel/tests.rs` — test construction sites
  for `PinnedItem` literals (update for `hidden_fields`)
- `crates/hprof-tui/src/views/stack_view/state.rs` — `flat_items` (~line 1737),
  `render_variable_tree` call site to update to pass `None`
- `crates/hprof-tui/src/app/mod.rs` — `handle_favorites_input`
- `crates/hprof-tui/src/views/help_bar.rs` — `ENTRIES`, `ENTRY_COUNT`
- `docs/implementation-artifacts/9-8-pinned-item-navigation-and-array-expansion.md`
  (prerequisite story — read before starting)
- `docs/planning-artifacts/epics.md` (Story 9.9, FR54–FR56)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- `HideKey` import removed from `app/mod.rs` — not needed since `field_key_at_cursor()`
  returns the enum value without the caller needing to name the type.
- `#[allow(clippy::too_many_arguments)]` added to `render_variable_tree` — function has
  8 params after adding `hidden_fields`; refactoring into a context struct is deferred.
- `#[allow(non_snake_case)]` added to `help_bar_H_key_applicable_only_in_favorites` test
  to preserve the test name's clarity.
- **UX rework (post-initial-impl):** Hide semantics changed from placeholder-on-hide to
  completely-remove-on-hide. `h` removes the line; `H` toggles `show_hidden` mode which
  reveals placeholders navigable by cursor; `h` on a placeholder restores. `PinnedItem`
  gained `show_hidden: bool` (init `false`). `RenderOptions` gained `show_hidden: bool`.
  `MetadataCollector` mirrors the renderer: hidden rows are absent when `show_hidden=false`,
  emit a placeholder row when `show_hidden=true`.

### File List

- `crates/hprof-tui/src/favorites.rs` — `HideKey`, `hidden_fields`, `show_hidden` on `PinnedItem`
- `crates/hprof-tui/src/views/tree_render.rs` — `show_hidden` in `RenderOptions`/`RenderCtx`, hide logic
- `crates/hprof-tui/src/views/favorites_panel/mod.rs` — `MetadataCollector` + render calls
- `crates/hprof-tui/src/views/favorites_panel/tests.rs` — tests 6.9–6.13, 6.17
- `crates/hprof-tui/src/views/stack_view/state.rs` — `render_variable_tree` call updated
- `crates/hprof-tui/src/views/help_bar.rs` — `h`/`H` entries, `ENTRY_COUNT` 19→21
- `crates/hprof-tui/src/app/mod.rs` — `h`/`H` handlers in `handle_favorites_input`
- `crates/hprof-tui/src/app/tests.rs` — test 6.16
- `docs/implementation-artifacts/sprint-status.yaml`
- `docs/implementation-artifacts/9-9-value-hiding-and-reset-in-pinned-snapshots.md`
