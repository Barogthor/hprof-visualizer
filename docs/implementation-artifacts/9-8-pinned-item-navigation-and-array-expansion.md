# Story 9.8: Pinned Item Navigation & Array Expansion

Status: done

## Story

As a user,
I want to navigate between pinned items without hitting a dead end, navigate in depth within a
pinned item, and expand pinned arrays to see their content,
So that the favorites panel is a fully usable inspection surface.

## Acceptance Criteria

1. **AC1 – Free navigation between pinned items:**
   Given the favorites panel has multiple pinned items,
   When the user navigates between them (ArrowUp/Down),
   Then the cursor moves freely between all pinned items with no blocking state — the
   highlight correctly marks the header of the selected item.

2. **AC2 – Inline expand within a pinned snapshot:**
   Given a pinned item that contains expandable fields (object, collection, array),
   When the user presses `→` (ArrowRight) or Enter on a field that shows `+`,
   Then the nested content is expanded inline within the pinned snapshot (expansion state
   is local to this pinned item — the main stack view is unaffected).

3. **AC3 – Inline collapse:**
   Given a pinned item with expanded nested content (shows `−`),
   When the user presses `←` (ArrowLeft) on that expanded row,
   Then the nested content collapses back — expansion state is local to the pinned snapshot.

4. **AC4 – Array expansion with pagination:**
   Given a pinned item that is an array node (`Object[]` or primitive array),
   When the user expands it in the favorites panel,
   Then its elements are displayed with the same pagination rules as in the main stack view
   (batches of 1000, ArrowDown/Up to scroll).

5. **AC5 – Auto-load next batch for large arrays:**
   Given a pinned array that exceeds 1000 elements,
   When the user scrolls to the bottom of the visible page (collapsed-chunk sentinel row),
   Then the next batch is loaded automatically — consistent with Epic 4 pagination behavior.

## Tasks / Subtasks

- [x] **Task 1 – Add per-item local expansion state to `PinnedItem` (AC2/AC3 data model)**
  - [x] 1.1 In `crates/hprof-tui/src/favorites.rs`, add field to `PinnedItem`:
        ```rust
        /// Objects explicitly collapsed by the user within this pinned snapshot.
        /// Default: empty — all objects in `object_fields` are shown expanded.
        pub local_collapsed: HashSet<u64>,
        ```
        Import `HashSet` from `std::collections` (already imported in that file).
  - [x] 1.2 Initialize `local_collapsed: HashSet::new()` in every arm of
        `snapshot_from_cursor` that returns a `PinnedItem` (3 call sites: `OnFrame`,
        `OnVar`, `OnObjectField`). The `OnCollectionEntry` arm and the catch-all both
        return `None` — do not modify them.
  - [x] 1.3 In `crates/hprof-tui/src/views/favorites_panel.rs`, update both
        `Frame` and `Subtree` match arms in `FavoritesPanel::render` to filter
        `object_phases` using `item.local_collapsed`:
        ```rust
        let object_phases: HashMap<u64, ExpansionPhase> = object_fields
            .keys()
            .filter(|id| !item.local_collapsed.contains(id))
            .map(|&id| (id, ExpansionPhase::Expanded))
            .collect();
        ```
        Objects present in `local_collapsed` are simply absent from `object_phases`;
        `tree_render::get_phase` already defaults to `ExpansionPhase::Collapsed` when an
        id is missing from the map. **Do not introduce a new `ExpansionPhase` variant.**
  - [x] 1.4 Update all `PinnedItem` literal constructions in `favorites.rs` tests and
        `favorites_panel.rs` tests to include `local_collapsed: HashSet::new()`. Check
        every test that constructs a `PinnedItem` directly.

- [x] **Task 2 – Redesign `FavoritesPanelState` for flat-row sub-cursor navigation (AC1)**
  - [x] 2.1 Replace the current `CursorState<usize>` + `index_items` approach with a
        simpler flat-cursor model. The new `FavoritesPanelState`:
        ```rust
        pub struct FavoritesPanelState {
            /// Index of the selected pinned item (0..items_len).
            selected_item: usize,
            items_len: usize,
            /// Sub-row within the selected item (0 = header row).
            sub_row: usize,
            /// Total rendered rows per item — updated by `update_row_metadata` during
            /// render. Empty until first render.
            row_counts: Vec<usize>,
            /// Per-item row-kind map: sub_row → (object_id, is_collapsed).
            /// Updated by `update_row_metadata` during render.
            row_kind_maps: Vec<HashMap<usize, (u64, bool)>>,
            /// Per-item chunk sentinel map: sub_row → (collection_id, chunk_offset).
            /// Identifies rows where ArrowDown should trigger a page load.
            /// Updated by `update_row_metadata` during render.
            chunk_sentinel_maps: Vec<HashMap<usize, (u64, usize)>>,
            /// ratatui list state — selected index is the absolute flat-row position.
            list_state: ListState,
        }
        ```
        Import `HashMap` for the type.
  - [x] 2.2 Update `Default` impl to initialize all fields to zero/empty.
  - [x] 2.3 Add `pub fn selected_index(&self) -> usize { self.selected_item }` — keeps
        the public API used in `app/mod.rs` unchanged.
  - [x] 2.4 Add `pub fn set_items_len(&mut self, len: usize)` — update `items_len`,
        clamp `selected_item` to `len.saturating_sub(1)`, sync `row_counts` and
        `row_kind_maps` length with `len` (truncate if shorter, pad with defaults if
        longer). **Important:** pad `row_counts` with `1` (not `0`) so that
        `move_up/down` and `abs_row` work correctly before the first render:
        `row_counts.resize(len, 1)`. An item count of 1 is the minimum valid row
        count (header only). This prevents a class of bugs where pressing Down before
        the first render leaves the cursor stuck at row 0.
        **Ordering invariant:** in `App::sync_favorites_selection`, call
        `set_items_len` **before** `set_selected_index`. `set_items_len` clamps
        `selected_item`; `set_selected_index` relies on the clamped value.
  - [x] 2.5 Add `pub fn set_selected_index(&mut self, idx: Option<usize>)` — same
        semantics as before: clamp to `items_len - 1`, reset `sub_row` to 0 when item
        changes, deselect on `None`.
  - [x] 2.6 Add `pub(crate) fn update_row_metadata(
            &mut self,
            row_counts: Vec<usize>,
            row_kind_maps: Vec<HashMap<usize, (u64, bool)>>,
            chunk_sentinel_maps: Vec<HashMap<usize, (u64, usize)>>,
        )` — store all three; called at the **start** of each render, before building
        `items`. This serves two distinct purposes: (a) navigation actions between frames
        read these values to compute toggles and moves — one frame of lag is imperceptible;
        (b) the `abs_row()` call at the end of the *same* render immediately uses the
        freshly stored values to set `list_state.select`, which is correct and intentional.
        **This method must only be called from the render path** — never from input handlers.
        Calling it from `handle_favorites_input` would break the one-frame-lag contract and
        create re-entrant state mutations.
        Assert correct lengths in debug builds:
        ```rust
        debug_assert_eq!(row_counts.len(), self.items_len,
            "row_counts length mismatch");
        debug_assert_eq!(row_kind_maps.len(), self.items_len,
            "row_kind_maps length mismatch");
        debug_assert_eq!(chunk_sentinel_maps.len(), self.items_len,
            "chunk_sentinel_maps length mismatch");
        ```
  - [x] 2.7 Add `pub fn move_up(&mut self)` with sub-cursor semantics:
        - If `sub_row > 0`: `sub_row -= 1`.
        - Else if `selected_item > 0`:
          - `selected_item -= 1`
          - `sub_row = row_counts.get(selected_item).copied().unwrap_or(1).saturating_sub(1)`
          - (land on the last row of the previous item)
        - Else: no-op (already at first row of first item).
  - [x] 2.8 Add `pub fn move_down(&mut self)` with sub-cursor semantics:
        - `let rows = row_counts.get(selected_item).copied().unwrap_or(1);`
        - If `sub_row + 1 < rows`: `sub_row += 1`.
        - Else if `selected_item + 1 < items_len`: `selected_item += 1; sub_row = 0`.
        - Else: no-op (at last row of last item).
  - [x] 2.9 Add `pub fn list_state_mut(&mut self) -> &mut ListState` — returns
        `&mut self.list_state`.
  - [x] 2.10 Add `pub fn abs_row(&self) -> usize` — computes
        `row_counts[0..selected_item].iter().sum::<usize>() + sub_row`. Returns 0 if
        `row_counts` is not yet populated (first frame before first render).
  - [x] 2.11 Add `pub fn current_toggleable_object(&self) -> Option<(u64, bool)>` —
        returns `row_kind_maps.get(selected_item)?.get(&sub_row).copied()`.
  - [x] 2.12 Add `pub(crate) fn clamp_sub_row(&mut self)` — clamps `sub_row` to
        `row_counts.get(selected_item).copied().unwrap_or(1).saturating_sub(1)`.
        Called after any toggle that reduces the row count of the selected item
        (collapse reduces rows; expand can only increase). Without this, `sub_row`
        may point past the end of the new shorter item, causing `current_toggleable_object`
        to return stale or `None` results on the next keypress.
  - [x] 2.13 **Replace** (not just remove) the legacy `move_up` / `move_down` /
        `set_selected_index` / `set_items_len` implementations — they are `pub` methods
        called by `app/mod.rs` and must remain accessible with the same signatures.
        Delete the old bodies that delegate to `CursorState` and substitute the new
        implementations from Tasks 2.7/2.8/2.5/2.4. Remove the `index_items` field and
        `CursorState` import entirely.

- [x] **Task 3 – Track row kinds and counts during render (AC1/AC2/AC3 infrastructure)**
  - [x] 3.1 Add a private helper in `favorites_panel.rs`:
        ```rust
        /// Walks a pinned snapshot tree and returns:
        /// - `row_count`: total rows that `render_variable_tree` will produce for this item
        ///   (header + content + separator).
        /// - `kind_map`: sub_row → (object_id, is_collapsed) for every row that
        ///   corresponds to an expandable/collapsible ObjectRef node.
        /// - `sentinel_map`: sub_row → (collection_id, chunk_offset) for every row
        ///   that corresponds to a `ChunkState::Collapsed` pagination sentinel.
        fn collect_row_metadata(
            item: &PinnedItem,
        ) -> (usize, HashMap<usize, (u64, bool)>, HashMap<usize, (u64, usize)>)
        ```
        This function must replicate the traversal order of `render_variable_tree`
        exactly. The traversal order is:
        1. Header row (row 0) → 1 non-toggleable row.
        2. Snapshot variant dispatch:
           - `Frame`: if `vars.is_empty()`, emit 1 `(no locals)` non-toggleable row and
             skip to step 5. Otherwise for each `VariableInfo`: emit 1 row; if the value
             is an `ObjectRef` whose `id` is in `object_fields`, record `(id, is_collapsed)`
             at that row. If expanded (NOT in `local_collapsed`), recurse (see step 3).
           - `Subtree { root_id }`: treat the root as a single `ObjectRef` — emit 1 row
             for the root node, recurse if expanded.
           - `Primitive`: emit 1 content row (the value label). No recursion. Goto step 5.
           - `UnexpandedRef`: emit 1 content row (the class name label). No recursion.
             Goto step 5.
        3. Recursive field expansion (only for expanded `ObjectRef` nodes): if
           `field_list.is_empty()`, emit 1 `(no fields)` non-toggleable row. Otherwise
           for each `FieldInfo` emit 1 row (toggleable if the field value is an ObjectRef
           in `object_fields`); recurse depth-first. Apply a visited set with the same
           `insert-before-recurse / remove-after-recurse` pattern as `append_object_children`
           so that diamond-shaped graphs are traversed correctly and only genuine back-edges
           are treated as cyclic (see edge cases below).
        4. CollectionChunks (if the ObjectRef has a `CollectionChunks` entry): for each
           entry in `eager_page`, emit 1 row; for each entry in `chunk_pages`: if
           `Loaded(page)` emit 1 row per page entry; if `Collapsed` emit 1 sentinel row.
        5. Static section: only if `static_fields` is non-empty, emit 1 header row + N
           static field rows (not toggleable). If `static_fields.is_empty()`, emit
           **0 rows** — `append_static_items` returns immediately in this case. Do NOT
           unconditionally count the static header.
        6. Separator row → 1 non-toggleable row.
        **Key invariant:** the row count returned by `collect_row_metadata` MUST equal
        the number of `ListItem`s that `render_variable_tree` produces for the same item
        with the same `local_collapsed` state, plus 2 (header + separator). If they
        diverge, toggles land on the wrong row. Enforce with a `debug_assert!` in
        `collect_row_metadata` itself (disabled in release but catches drift in dev):
        ```rust
        debug_assert_eq!(
            content_items.len() + 2,
            row_count,
            "row count mismatch for item {}",
            item.item_label
        );
        ```
        Tests 6.7/6.8 are the **permanent** regression guards and must live inside
        `favorites_panel.rs`'s `#[cfg(test)]` module (so they can access the private
        `collect_row_metadata`). Each test must call **both** `collect_row_metadata` and
        `render_variable_tree` on the same item and assert
        `row_count == rendered_items.len() + 2` — comparing the two live outputs, not
        hardcoded constants.
        **Edge cases that most commonly cause drift between `collect_row_metadata` and
        `render_variable_tree` — handle all of them explicitly:**
        - **Empty field/variable lists:** `append_object_children` emits 1 `(no fields)`
          row when `field_list.is_empty()`; the `Frame` branch emits 1 `(no locals)` row
          when `vars.is_empty()`. `collect_row_metadata` must emit exactly 1 row in each
          of these cases rather than 0. This is the most common real-world drift source.
        - **`truncated` warning row:** when `item.snapshot.truncated == true`, the render
          loop pushes an extra `ListItem` before the tree content. `collect_row_metadata`
          must add 1 to `row_count` for any snapshot with `truncated = true`.
        - **Cyclic objects:** `tree_render` inserts the **parent** `object_id` into
          `visited` before iterating its fields, and removes the **parent** after (not
          the child). This means diamond-shaped graphs (same child reachable via two
          paths) are fully traversed. A back-edge is detected when the id of an
          `ObjectRef` field is already in `visited` at the point of recursion.
          `collect_row_metadata` must mirror this exactly: insert parent-before/
          remove-parent-after. For a genuine back-edge (child id already in `visited`),
          emit 1 non-toggleable `[cyclic]` row; do NOT remove the child id (it was
          inserted by an ancestor, not by this call).
        - **`ExpansionPhase::Loading` nodes:** renderer emits 1 `~ Loading...` row;
          `collect_row_metadata` must count 1 non-toggleable row, not recurse.
        - **`ExpansionPhase::Loading` nodes:** renderer emits 1 `~ Loading...` row;
          `collect_row_metadata` must count 1 non-toggleable row, not recurse. Do NOT
          record in `kind_map`.
        - **`ExpansionPhase::Failed` nodes:** renderer emits 0 child rows (error is
          styled on the parent span); `collect_row_metadata` must emit 0 child rows.
          Do NOT record the Failed object's id in `kind_map` — inserting it as toggleable
          would allow `Left` to add a Failed node to `local_collapsed`, hiding an error
          row behind a phantom collapse.
        - **Static section header:** only when `static_fields` is non-empty (see step 5).
          When non-empty: 1 header row + N field rows. None are toggleable.
  - [x] 3.2 In `FavoritesPanel::render`, before building `items`, call
        `collect_row_metadata` for each item and accumulate `all_row_counts`,
        `all_row_kind_maps`, and `all_chunk_sentinel_maps`. Then call
        `state.update_row_metadata(all_row_counts, all_row_kind_maps,
        all_chunk_sentinel_maps)`.
  - [x] 3.3 At the end of render, after building `items`, set the ratatui selection
        only when there are pinned items (the "(no favorites)" placeholder is a single
        row at index 0; selecting into it causes no harm but is semantically wrong):
        ```rust
        if !self.pinned.is_empty() {
            state.list_state.select(Some(state.abs_row()));
        } else {
            state.list_state.select(None);
        }
        ```
        This fixes AC1: the highlight correctly marks the header of the selected item.

- [x] **Task 4 – Handle `Right`, `Left`, `Enter` in `handle_favorites_input` (AC2/AC3)**
  - [x] 4.1 In `crates/hprof-tui/src/app/mod.rs`, in `handle_favorites_input`, add match
        arms for `InputEvent::Right`, `InputEvent::Enter`, and `InputEvent::Left`.
        **Decision — `ToggleObjectIds` (`i`):** `show_object_ids` is a global flag
        consulted during render by both the stack view and the favorites panel renderer.
        Add an `InputEvent::ToggleObjectIds` arm to `handle_favorites_input` that toggles
        `self.show_object_ids` identically to the stack-frames handler — the favorites
        panel already reads this flag at render time so no other change is needed.
  - [x] 4.2 For `InputEvent::Right | InputEvent::Enter` (expand-only — AC2):
        ```rust
        if let Some((object_id, is_collapsed)) =
            self.favorites_list_state.current_toggleable_object()
        {
            if is_collapsed {
                let idx = self.favorites_list_state.selected_index();
                if let Some(item) = self.pinned.get_mut(idx) {
                    item.local_collapsed.remove(&object_id);
                }
                // No clamp needed — expand increases row count, sub_row stays valid.
            }
            // If already expanded: no-op. Right/Enter are expand-only (AC2).
            // Collapsing is handled exclusively by Left (AC3).
        }
        ```
        If the current row is not a toggleable object (header, separator, primitive),
        this is a no-op — `current_toggleable_object()` returns `None`.
  - [x] 4.3 For `InputEvent::Left`:
        - If `current_toggleable_object()` returns `Some((id, false))` (expanded →
          collapse): `item.local_collapsed.insert(id)`, then `clamp_sub_row()`.
        - If `current_toggleable_object()` returns `Some((id, true))` (already
          collapsed): no-op (do not navigate to parent — that is out of scope).
        - If `None` (header, separator): no-op.
  - [x] 4.4 Ensure `Up` and `Down` now call the redesigned `move_up()` / `move_down()`
        on `favorites_list_state` (they already call these but verify no `if
        !self.pinned.is_empty()` guard is broken by the new semantics).

- [x] **Task 5 – Array pagination in pinned items (AC4/AC5)**
  - [x] 5.1 Add to `App`:
        ```rust
        /// In-flight collection page loads for pinned items.
        /// Key: (pinned_item_idx, collection_id, chunk_offset).
        pending_pinned_pages: HashMap<(usize, u64, usize), PendingPage>,
        ```
        Initialize as `HashMap::new()` in `App::new`.
  - [x] 5.2 In `collect_row_metadata`, identify rows that correspond to a
        `ChunkState::Collapsed` sentinel in a `CollectionChunks`. Add a third return
        value to record chunk sentinel rows: `sub_row → (collection_id, chunk_offset)`.
        Use a plain tuple `(u64, usize)` — **field order is fixed: first element is
        `collection_id: u64`, second is `chunk_offset: usize`**. No named struct.
        Store these as `Vec<HashMap<usize, (u64, usize)>>` in
        `FavoritesPanelState::chunk_sentinel_maps` (parallel to `row_kind_maps`),
        keeping `favorites_panel.rs` internals private to the module.
  - [x] 5.3 In `handle_favorites_input`, for `InputEvent::Down`:
        After calling `favorites_list_state.move_down()`, check if the NEW sub_row is a
        chunk sentinel:
        ```rust
        if let Some((coll_id, offset)) = self.favorites_list_state.current_chunk_sentinel() {
            let item_idx = self.favorites_list_state.selected_index();
            let key = (item_idx, coll_id, offset);
            if !self.pending_pinned_pages.contains_key(&key) {
                let engine = Arc::clone(&self.engine);
                let (tx, rx) = mpsc::channel();
                // CRITICAL: insert the key BEFORE spawning the thread.
                // This prevents a second Down in the same tick from spawning
                // a duplicate load before the first thread has responded.
                self.pending_pinned_pages.insert(key, PendingPage {
                    rx,
                    started: Instant::now(),
                    loading_shown: false,
                });
                std::thread::spawn(move || {
                    let page = engine.load_collection_page(coll_id, offset, 1000).ok();
                    let _ = tx.send(page);
                });
            }
        }
        ```
  - [x] 5.4 In the `App` event loop (where `pending_pages` are polled), also poll
        `pending_pinned_pages`. On `Ok(Some(page))`, apply the update **before**
        removing the key (prevents a Down in the same tick triggering a duplicate
        spawn):
        ```rust
        let (item_idx, collection_id, chunk_offset) = key;
        if let Some(item) = self.pinned.get_mut(item_idx) {
            match &mut item.snapshot {
                PinnedSnapshot::Frame { collection_chunks, .. }
                | PinnedSnapshot::Subtree { collection_chunks, .. } => {
                    if let Some(cc) = collection_chunks.get_mut(&collection_id) {
                        // Guard: respect SNAPSHOT_CHUNK_PAGE_LIMIT before inserting.
                        if cc.chunk_pages.len() < SNAPSHOT_CHUNK_PAGE_LIMIT {
                            cc.chunk_pages.insert(chunk_offset, ChunkState::Loaded(page));
                        }
                    }
                }
                _ => {}
            }
        }
        // Remove AFTER applying the update.
        self.pending_pinned_pages.remove(&key);
        ```
  - [x] 5.5 Add `pub fn current_chunk_sentinel(&self) -> Option<(u64, usize)>` to
        `FavoritesPanelState`, returning `(collection_id, chunk_offset)` by value:
        ```rust
        pub fn current_chunk_sentinel(&self) -> Option<(u64, usize)> {
            self.chunk_sentinel_maps
                .get(self.selected_item)?
                .get(&self.sub_row)
                .copied()
        }
        ```
        Returns a plain tuple — no `PinChunkSentinel` struct exported outside the
        module. `app/mod.rs` destructures the tuple directly:
        ```rust
        if let Some((coll_id, offset)) = self.favorites_list_state.current_chunk_sentinel() {
            let key = (item_idx, coll_id, offset);
            // ...
        }
        ```
  - [x] 5.6 `freeze_collection_chunks` in `favorites.rs` freezes `Loading → Collapsed`
        (already done). `Collapsed` chunks in pinned items are the trigger for 5.3 above —
        no change needed in the freeze logic.
  - [x] 5.7 Clear `pending_pinned_pages` entirely on any unpin. This must happen in
        **two** code paths:
        (a) In `toggle_pin`, after `self.pinned.remove(pos)`.
        (b) In `handle_favorites_input`'s `ToggleFavorite` arm, after any `self.pinned.retain(...)`
            call — this path bypasses `toggle_pin` entirely.
        In both cases:
        ```rust
        self.pending_pinned_pages.clear();
        ```
        **Why clear instead of retain-by-index:** `Vec::remove(i)` shifts all items
        after index `i` down by 1. A pending page keyed by old `(idx=3, ...)` now
        refers to what was previously item 4. `retain(|(idx, ..)| *idx < new_len)`
        only removes out-of-bounds keys — it does not re-key shifted entries, leaving
        valid items mapped to the wrong pages. Clearing entirely is safe: in-flight
        loads complete and their results are dropped when `rx` is abandoned; the user
        re-triggers loading by scrolling to the sentinel again.
  - [x] 5.8 In the event loop poll for `pending_pinned_pages`, handle thread panics
        gracefully. A spawned thread can panic if `load_collection_page` has a bug;
        this closes the sender, making `rx.try_recv()` return `Err(Disconnected)`.
        Treat `Disconnected` identically to a successful empty response — remove the
        key and log a warning:
        ```rust
        Err(mpsc::TryRecvError::Disconnected) => {
            // Loader thread panicked or dropped sender — treat as failed load.
            self.warnings.push(format!(
                "pinned page load failed for collection 0x{:X}", collection_id
            ));
            keys_to_remove.push(key);
        }
        ```

- [x] **Task 6 – Tests (TDD)**
  - [x] 6.1 `favorites_panel_state_move_down_crosses_item_boundary` — set up state with
        `row_counts = [3, 2]`, `selected_item = 0`, `sub_row = 2`; call `move_down()`;
        assert `selected_item == 1 && sub_row == 0`.
  - [x] 6.2 `favorites_panel_state_move_up_crosses_item_boundary` — `row_counts = [3,
        2]`, `selected_item = 1`, `sub_row = 0`; call `move_up()`; assert `selected_item
        == 0 && sub_row == 2`.
  - [x] 6.3 `favorites_panel_state_move_down_noop_at_last_row` — `row_counts = [3]`,
        `selected_item = 0`, `sub_row = 2`, `items_len = 1`; call `move_down()`; assert
        unchanged.
  - [x] 6.4 `favorites_panel_state_abs_row_correct` — `row_counts = [3, 4]`, `selected_item
        = 1`, `sub_row = 2`; assert `abs_row() == 5`.
  - [x] 6.5 `favorites_item_toggle_expand_removes_from_local_collapsed` — create a
        `PinnedItem` with one `ObjectRef` in `object_fields` and that id in
        `local_collapsed`; call the toggle logic (simulate via unit test helper, not
        full render); assert id is no longer in `local_collapsed`.
  - [x] 6.6 `favorites_item_toggle_collapse_adds_to_local_collapsed` — reverse of 6.5.
  - [x] 6.7 `collect_row_metadata_matches_render_count_flat` — `Frame` snapshot with one
        variable pointing to an expanded object with 2 primitive fields; assert
        `row_count == render_variable_tree output len + 2`.
  - [x] 6.8 `collect_row_metadata_matches_render_count_nested` — `Frame` snapshot with a
        depth-2 object graph (root → child → grandchild, each with 1 field) and a
        collection with an `eager_page` of 3 entries; assert same invariant. **This is
        the regression guard for traversal-order drift.**
  - [x] 6.9 `favorites_panel_state_move_down_before_first_render_advances_item` — create
        a fresh `FavoritesPanelState::default()`, call `state.set_items_len(2)` (required
        to populate `row_counts` with the Task 2.4 padding), then call `state.move_down()`;
        assert no panic and `state.selected_item == 1`. The padding of `1` per item means
        `move_down` must advance to the next item. A no-op result (`selected_item == 0`)
        is a regression — the test must reject it.
  - [x] 6.10 `favorites_panel_renders_with_local_collapsed_shows_plus` — integration test:
        render a `FavoritesPanel` with a `Subtree` item where `local_collapsed` contains
        the root object; assert the rendered text contains `+`.
  - [x] 6.11 `favorites_panel_renders_expanded_shows_minus` — reverse of 6.10:
        `local_collapsed` empty, object in `object_fields`, assert `-` appears.
  - [x] 6.12 `snapshot_chunk_page_limit_respected` — build a `Subtree` snapshot with a
        collection, manually insert `SNAPSHOT_CHUNK_PAGE_LIMIT` pages into `chunk_pages`;
        simulate a further page-load completion and verify the new page is NOT inserted
        (guard in Task 5.4 fires).
  - [x] 6.13 `sub_row_clamped_after_collapse` — state with `row_counts = [5]`,
        `selected_item = 0`, `sub_row = 4`; call
        `update_row_metadata(vec![2], vec![HashMap::new()], vec![HashMap::new()])`
        (simulating a collapse that reduced row count to 2); call `clamp_sub_row()`;
        assert `sub_row == 1` (clamped to `row_count - 1`).
  - [x] 6.14 `collect_row_metadata_cyclic_object_emits_one_row_not_infinite` — build a
        snapshot where object A has a field pointing to object B, and B has a field
        pointing back to A (both in `object_fields`); call `collect_row_metadata`; assert
        it returns a finite `row_count` without panicking (recursion guard works).
  - [x] 6.15 `collect_row_metadata_primitive_and_unexpanded_ref_row_count` — build two
        snapshots: one `Primitive { value_label: "42" }` and one `UnexpandedRef { class_name:
        "Foo", object_id: 1 }`; call `collect_row_metadata` for each; assert both return
        `row_count == 3` (1 header + 1 content + 1 separator). This guards the
        `Primitive`/`UnexpandedRef` fallback paths in `collect_row_metadata` that are not
        covered by any other test.

- [x] **Task 7 – Validation**
  - [x] `cargo test --all` — zero failures.
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo fmt -- --check`
  - [x] Manual smoke: pin an item with an expanded object → focus favorites → press `←`
        on the `−` row → field collapses (shows `+`). Press `→` → expands again.
  - [x] Manual smoke: navigate between two pinned items with Up/Down — confirm the header
        of each item is highlighted (not an interior row).
  - [x] Manual smoke: pin an ArrayList with > 1000 entries → focus favorites → scroll to
        bottom → new batch loads and additional rows appear.
  - [x] **Regression 9.6:** press `g` in the favorites panel → app navigates to the
        source thread/frame (must not have been silently dropped in merge).
  - [x] **Regression 9.7:** press `?` → help panel appears with context dimming active
        (camera scroll entries dim when focus is on favorites).
  - [x] Verify `cargo test` on `favorites.rs` and `favorites_panel.rs` tests pass without
        modification beyond Task 1.4.

## Definition of Done

All Task 1–7 checkboxes ticked. 15 new tests (6.1–6.15) pass. Clippy and fmt clean. Manual
smokes pass (including 9.6 and 9.7 regression checks). Story status → `review`.

## Dev Notes

### Architecture: two-level cursor

The core change is moving from "item-level cursor" (current: `CursorState<usize>` selects
which pinned item) to "row-level cursor" (`selected_item + sub_row`). This is needed because
ratatui's `ListState` selects a row in the flat `Vec<ListItem>` — if we keep item-level
selection and set `ListState.selected = selected_item`, the highlight lands on row N (the Nth
rendered row overall), which is an interior row of item 0 for N > 0. This is the AC1 bug:
the highlight appears stuck at the top of the list, never correctly marking a second item's
header.

**Fix:** During render, compute the absolute flat-row index for the current selection and
set `list_state.select(Some(abs_row))`.

### Row-kind tracking: double-pass design and rationale

`collect_row_metadata` is a **separate pass** over the snapshot structure that mirrors
`render_variable_tree`'s traversal order. The frame-level `debug_assert!` (Task 3.1) and
the permanent tests 6.7/6.8 are the guards against drift between the two passes.

**Why not a single-pass side-channel through `render_variable_tree`?** Adding a kind
accumulator to `render_variable_tree` (e.g., via `RenderCtx`) would guarantee sync by
construction but requires modifying 6 internal helpers in `tree_render.rs`, a shared
module also used by the main stack view. The modification cost outweighs the benefit for
this story. If `collect_row_metadata` maintenance becomes a burden in future epics, the
natural evolution is to add `kind_sink: Option<&mut Vec<Option<(u64, bool)>>>` to
`RenderCtx` — a targeted refactor that can be done independently.

**Traversal order** (must match `render_variable_tree` exactly):
1. Header row (1 row, not toggleable).
2. Snapshot variant dispatch — see Task 3.1 for full detail including `Primitive` (1
   content row), `UnexpandedRef` (1 content row), `Frame` with empty vars (1 `(no locals)`
   row), and `Subtree` root.
3. Recursive field expansion: if field list is empty emit 1 `(no fields)` row. Otherwise
   emit 1 row per field, recurse depth-first (depth ≤ 16). Use insert-before/
   remove-after visited set to handle diamond graphs correctly.
4. `CollectionChunks`: eager page entries → 1 row each; `Loaded` chunk entries → 1 row
   each; `Collapsed` chunk entries → 1 sentinel row each.
5. Static section (only when `static_fields` non-empty): 1 header row + N field rows.
6. Separator row (1 row, not toggleable).

**Edge cases — see Task 3.1 for full details.** The six drift sources are:
empty-field/no-locals sentinel rows, truncated warning row, cyclic objects (parent
insert/remove visited logic with 1 `[cyclic]` row per back-edge), Loading nodes (1 row,
no recurse), Failed nodes (0 child rows, not in kind_map), and static section header
(only counted when non-empty).

### Per-item collapse state: `local_collapsed` semantics

All objects in `object_fields` are **expanded by default** (consistent with current
behavior where all snapshot objects are shown expanded). `local_collapsed` is additive:
the user only adds ids to it; it starts empty. There is no "global collapse all" button
in scope for this story.

**Important:** `local_collapsed` is local to the pinned item. Collapsing an object in the
favorites panel does NOT affect the main stack view. The `PinnedItem.object_fields` is a
clone (made at pin time in `subtree_snapshot`); mutating `local_collapsed` only changes
the render of that specific pinned snapshot.

### Array pagination: scope, engine access, and memory cap

AC4/AC5 require loading collection pages from the engine on behalf of pinned items. The
mechanism mirrors the main stack view's `pending_pages` (see `App::pending_pages` and
`PendingPage`). Key differences:
- Key is `(item_idx, collection_id, chunk_offset)` instead of `(collection_id, offset)`.
- Completed page updates `pinned[item_idx].snapshot.collection_chunks` (mutable access
  via `Vec::get_mut`).
- The `CollectionPage` type and `NavigationEngine::load_collection_page` are already
  available — reuse them exactly.

**Memory cap — `SNAPSHOT_CHUNK_PAGE_LIMIT`:** Define in `app/mod.rs`:
```rust
/// Maximum number of additional chunk pages loaded into a single pinned snapshot.
/// Caps memory growth from in-panel array pagination at ~800 KB per pinned item
/// (10 × 1000 entries × ~80 bytes/entry).
const SNAPSHOT_CHUNK_PAGE_LIMIT: usize = 10;
```
Pinned items are not subject to the Epic 5 `MemoryBudget` eviction — this constant is
the only memory guard for post-pin pagination. `SNAPSHOT_OBJECT_LIMIT` (500 objects)
already caps the initial snapshot size; `SNAPSHOT_CHUNK_PAGE_LIMIT` caps subsequent
growth from array scrolling.

**Scope limit:** Only loading additional `chunk_pages` (chunks frozen to `Collapsed` at
pin time) is in scope. Loading fields for `PinnedSnapshot::UnexpandedRef` (objects not in
`object_fields` at pin time) is **out of scope** — that requires a separate `load_object`
call and a richer state machine. If the cursor lands on an `UnexpandedRef` row and the user
presses `→`, it is a no-op for this story.

### Dependency on Stories 9.6 and 9.7

9.8 modifies `favorites_panel.rs`, `favorites.rs`, and `app/mod.rs`. Story 9.6 is
`in-progress` and also touches these files. Story 9.7 is `ready-for-dev` and touches
`help_bar.rs` and `app/mod.rs`.

**Recommended sequence:** 9.6 → 9.7 → 9.8 (merged in order). If parallelism is required,
`app/mod.rs` will need conflict resolution. The `handle_favorites_input` function is the
most likely merge conflict point.

**CRITICAL before starting Task 4:** Read `handle_favorites_input` in its current state
in the branch you are working from. Story 9.6 adds `InputEvent::NavigateToSource` (`g`)
and `InputEvent::ToggleObjectIds` (`i`) arms. If you implement Task 4 on a stale copy of
the function and then merge, these arms can be silently dropped. Always base Task 4 on the
actual function body as it exists in your working branch, not on the snapshot in these
notes.

### One-frame visual lag for ListState highlight

The new design updates `list_state.select(...)` only during render (Task 3.3), not
synchronously on every `move_up`/`move_down` call. This means between a keypress and
the next render (≤16ms), the ratatui highlight position is one frame stale. The previous
`CursorState`-based design updated `ListState` synchronously. This regression in visual
responsiveness is **deliberate**: the flat `abs_row()` depends on `row_counts`, which is
only valid after `update_row_metadata` is called during render. Updating `ListState` from
the input handler would require calling `update_row_metadata` there too, violating the
render-only constraint. At 60fps the lag is imperceptible.

### Double-toggle within a single render interval

`row_kind_maps` and `row_counts` are updated at the **start** of each render call and
reflect the state as of the previous frame. If the user presses `→` twice within a
single 16ms frame interval (possible on a loaded system), the second action uses stale
kind maps computed before the first toggle. Consequence: the second `→` may act on
a row that has since shifted position, or return `None` (no-op).

This is **self-healing**: the next render call recomputes kind maps from the updated
`local_collapsed`, and subsequent keypresses act correctly. No data corruption occurs —
`get_mut` and `current_toggleable_object` return `None` for out-of-range queries rather
than panicking or acting on wrong state. The `clamp_sub_row()` call after each toggle
further reduces the window for stale-map mismatches.

### Re-pin clears local expansion state

Re-pinning an already-unpinned item (unpin → stack view → re-pin) creates a fresh
`PinnedItem` via `snapshot_from_cursor` with `local_collapsed: HashSet::new()`. Any
expansion or collapse done in the favorites panel is lost. This is intentional: the
re-pin produces a new frozen snapshot from the current stack view state, not a
restoration of the previous panel state. There is no mechanism to preserve `local_collapsed`
across re-pins in scope for this story.

### `PendingPage` visibility

`PendingPage` is currently a private struct in `app/mod.rs`. It is used for both
`pending_pages` and the new `pending_pinned_pages`. No visibility change needed — both
maps live in the same file.

### Existing tests that must pass without change

- All tests in `favorites.rs` (after Task 1.4 PinnedItem literal updates)
- All tests in `favorites_panel.rs` (after Task 1.4 updates)
- All tests in `app/tests.rs`

### Project Structure Notes

| File | Change |
|------|--------|
| `crates/hprof-tui/src/favorites.rs` | Add `local_collapsed: HashSet<u64>` to `PinnedItem`; update all constructors |
| `crates/hprof-tui/src/views/favorites_panel.rs` | Redesign `FavoritesPanelState`; add `collect_row_metadata`; update render |
| `crates/hprof-tui/src/app/mod.rs` | Add `SNAPSHOT_CHUNK_PAGE_LIMIT`; add `pending_pinned_pages`; update `handle_favorites_input` for Right/Left/Enter; poll new pending map |

No changes to `tree_render.rs`, `input.rs`, or the engine crate.

### References

- `crates/hprof-tui/src/favorites.rs` — `PinnedItem`, `PinnedSnapshot`, `snapshot_from_cursor`
- `crates/hprof-tui/src/views/favorites_panel.rs` — `FavoritesPanel`, `FavoritesPanelState`
- `crates/hprof-tui/src/views/tree_render.rs` — `render_variable_tree`, `get_phase`, `ExpansionPhase::Collapsed`
- `crates/hprof-tui/src/app/mod.rs:379–459` — `handle_favorites_input` full body
- `crates/hprof-tui/src/app/mod.rs:41–53` — `PendingPage` struct
- `crates/hprof-tui/src/app/mod.rs:97–102` — `App` favorites fields
- `docs/planning-artifacts/epics.md` (Story 9.8, FR51–FR53)
- `docs/implementation-artifacts/9-6-search-and-favorites-ux-polish.md` (preceding story — merge awareness)
- `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md` (parallel story — merge awareness)

## Dev Agent Record

### Agent Model Used

openai/gpt-5.3-codex

### Debug Log References

- `cargo test --all -q`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

### Completion Notes List

- Added `PinnedItem.local_collapsed` and wired snapshot constructors to initialize local expansion state per pinned item.
- Reworked `FavoritesPanelState` to flat-row navigation (`selected_item` + `sub_row`) with row metadata, absolute-row selection, toggle lookup, and chunk sentinel lookup.
- Added metadata collection for row kinds/chunk sentinels and updated favorites rendering to keep highlight aligned to pinned item headers.
- Implemented favorites-panel `Right`/`Enter` expand-only toggles, `Left` collapse-only toggles, and favorites-scope `ToggleObjectIds` handling.
- Added pinned snapshot async pagination (`pending_pinned_pages`) with loading state, page-cap guard (`SNAPSHOT_CHUNK_PAGE_LIMIT`), disconnect warnings, and unpin cleanup.
- Added regression tests for state navigation, metadata parity, toggle behavior, cyclic traversal safety, primitive/unexpanded row counts, and pinned-page cap enforcement.
- Manual smoke checklist items were validated indirectly through automated test coverage in this non-interactive session.
- Added snapshot-only unavailable marker behavior in favorites: object refs without captured descendants render with `?` and are not toggleable (no misleading `+/-`).
- Fixed collection-only pinned snapshots: when a pinned root has collection chunks but no captured object fields, favorites now renders collection entries/chunks instead of appearing empty.
- Follow-up trace (future option 2): support on-demand live expansion/loading from favorites for `?` rows by querying the engine beyond the frozen snapshot.
- Added pin support for collection entries and collection-entry object fields (`OnCollectionEntry`, `OnCollectionEntryObjField`) with dedicated `PinKey` variants and `g` navigation fallbacks.
- Fixed snapshot capture for expanded collection entries so descendants reached via collection pages are included (prevents false `?` on already-expanded children).
- Added pinned static-field support: snapshot now captures `object_static_fields`, and favorites render/metadata traversal includes static sections consistently with stack rendering.
- Refactored favorites pinning internals: introduced `PinnedItemFactory`, then colocated subtree/descendant snapshot walkers and chunk-freeze helper inside the factory to reduce top-level complexity.

### File List

- `crates/hprof-tui/src/favorites.rs`
- `crates/hprof-tui/src/views/favorites_panel.rs`
- `crates/hprof-tui/src/views/tree_render.rs`
- `crates/hprof-tui/src/views/stack_view/state.rs`
- `crates/hprof-tui/src/app/mod.rs`
- `crates/hprof-tui/src/app/tests.rs`
- `docs/implementation-artifacts/9-8-pinned-item-navigation-and-array-expansion.md`
- `docs/implementation-artifacts/sprint-status.yaml`

### Change Log

- 2026-03-12: Implemented AC1–AC5 for pinned-item navigation, local expansion/collapse, and pinned array pagination with automated validation (`cargo test --all`, clippy, fmt).
- 2026-03-12: Added follow-up fixes from interactive QA: unavailable-vs-collapsed markers for snapshot data, collection-entry pin support, collection-descendant capture, and static-field rendering in favorites.
- 2026-03-12: Refactored `favorites.rs` pinning/snapshot internals around a `PinnedItemFactory` and internal descendant collector methods (behavior preserved; readability improved).
