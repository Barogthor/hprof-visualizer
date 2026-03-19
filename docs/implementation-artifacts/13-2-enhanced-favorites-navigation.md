# Story 13.2: Enhanced Favorites Navigation

Status: review

## Story

As a user,
I want to unexpand a pinned item, navigate between pinned items with
shortcuts, and batch-unexpand multiple levels at once anywhere in the tree,
so that managing complex pinned snapshots is efficient.

## Background

Three distinct ergonomics improvements for the favorites panel and the
stack view:

1. **Batch unexpand** (`Shift+Left`): currently `Left` in the favorites
   panel collapses only the node at the cursor row. If the cursor is deep
   inside a subtree, the user must press `Left` many times to reach a
   clean state. `Shift+Left` should collapse the root of the current
   pinned item in one keystroke (from anywhere in that item). In the
   stack view, `Shift+Left` should collapse the entire current frame tree
   (equivalent to pressing Left up to the frame root and collapsing it).

2. **Jump between pinned items** (`[` / `]`): the favorites panel can
   contain many pinned items. When each item has expanded children, the
   cursor navigates through every sub-row, making it slow to reach the
   next item header. `[` / `]` should jump directly to the previous/next
   pinned item root, skipping nested content.

3. **Help panel update**: the new shortcuts must appear in the help bar
   when the favorites panel is focused.

> **Note sur le choix de `Shift+Left` :** Ce binding est provisoire.
> Certains terminaux (Windows/WSL) interceptent `Shift+Left` avant
> crossterm. Story 13.5 (AZERTY/QWERTY keymapping) devra confirmer ou
> réviser ce choix. Si `Shift+Left` ne passe pas en pratique, le fallback
> sera un `SearchChar` dédié (e.g. `c` pour "collapse") intercepté en
> focus stack/favorites. **Ce fallback est explicitement hors scope 13.2
> et délégué à 13.5.** AC #1 et #2 sont conditionnels à la disponibilité
> du binding dans le terminal cible.

## Acceptance Criteria

1. **Given** any row within a pinned item in the favorites panel
   **When** the user presses `Shift+Left`
   **Then** the root object/frame of that pinned item is inserted into
   `local_collapsed`, visually collapsing the entire item tree in one
   operation. The cursor moves to the item's header row (sub_row = 0).

2. **Given** an expanded node in the stack view
   **When** the user presses `Shift+Left`
   **Then** the current frame is fully collapsed (equivalent to
   navigating to the frame header and pressing `Left`). The cursor is
   placed at the frame header row.

3. **Given** multiple pinned items in the favorites panel
   **When** the user presses `]`
   **Then** the cursor jumps to the next pinned item's header row
   (sub_row = 0, selected_item += 1). No-op if already on the last item.

4. **Given** multiple pinned items in the favorites panel
   **When** the user presses `[`
   **Then** the cursor jumps to the previous pinned item's header row
   (sub_row = 0, selected_item -= 1). No-op if already on the first item.

5. **Given** the help panel rendered with focus on the favorites panel
   **When** the user presses `?`
   **Then** `Shift+←` and `[ / ]` entries appear, dimming inactive
   entries as usual.

6. **Given** existing tests
   **When** running `cargo test` and
   `cargo clippy --all-targets -- -D warnings`
   **Then** all pass with zero regressions.

## Tasks / Subtasks

- [x] Task 1: Add `BatchCollapseSubtree` InputEvent and key binding (AC: #1, #2)
  - [x] 1.1 In `crates/hprof-tui/src/input.rs`, add a new variant
    `BatchCollapseSubtree` to `InputEvent`.
    Map `Shift+Left` BEFORE the plain `Left` arm:
    ```rust
    (KeyCode::Left, mods) if mods.contains(KeyModifiers::SHIFT) =>
        Some(InputEvent::BatchCollapseSubtree),
    ```
    Note: `Shift+Left` is currently unmapped (only `Shift+Up`/`Shift+Down`
    are mapped to camera scroll). The new arm must come before
    `(KeyCode::Left, _) => Some(InputEvent::Left)`.
    **Side-effect:** `mods.contains(SHIFT)` is true for `CONTROL|SHIFT`
    too, so `Ctrl+Shift+Left` will also map to `BatchCollapseSubtree`.
    This is acceptable and consistent with the camera-scroll pattern
    (`Ctrl/Shift+Up` both map to the same event). Do NOT change to
    `mods == KeyModifiers::SHIFT` — that would break keyboards that send
    extra modifier bits for the same physical key.
  - [x] 1.2 Add unit test: `from_key(Shift+Left)` →
    `Some(InputEvent::BatchCollapseSubtree)`.
  - [x] 1.3 Add unit test: `from_key(Left)` still →
    `Some(InputEvent::Left)` (regression guard).
  - [x] 1.4 Add unit test: `assert_ne!(from_key(Shift+Left), Some(InputEvent::Left))`
    — explicit guard against the arm ordering bug where Shift+Left
    silently falls into the plain `Left` catch-all.
  - [x] 1.5 Add unit test: `from_key(Ctrl+Shift+Left)` →
    `Some(InputEvent::BatchCollapseSubtree)` (documents the accepted
    dual-modifier behaviour).

- [x] Task 2: Handle `BatchCollapseSubtree` in the stack view (AC: #2)
  - [x] 2.1 In `handle_stack_frames_input`
    (`crates/hprof-tui/src/app/mod.rs`), add an arm for
    `InputEvent::BatchCollapseSubtree`:
    Resolve the current frame_id from `stack_state` (extract the first
    segment of the current cursor's path — `PathSegment::Frame(fid)`).
    Call `s.collapse_object_recursive(&frame_path)` where `frame_path`
    is `NavigationPathBuilder::frame_only(fid)`. This calls
    `expansion.collapse_at_path` AND `resync_cursor_after_collapse`,
    ensuring the cursor lands on a valid row after collapse.
    **Do NOT use `toggle_expand(fid, vec![])` directly** — it does not
    call `resync_cursor_after_collapse`, leaving the cursor pointing at
    a row that no longer exists in `flat_items()`.
  - [x] 2.2 **Order is critical:** cancel pending expansions BEFORE
    calling `collapse_object_recursive`. An async response arriving
    between the two calls would re-insert the path into
    `expansion_phases` after it was removed.
    ```rust
    // Step 1 — cancel ALL in-flight expansions FIRST.
    self.pending_expansions.clear();
    // Step 2 — then collapse.
    s.collapse_object_recursive(&frame_path);
    ```
    Clearing all pending expansions is simpler than filtering by frame
    and safe: the frame is fully collapsed so no in-flight expansion
    is useful. Expansions in other frames re-trigger naturally if the
    user navigates back to them.
  - [x] 2.3 Place the arm BEFORE the `_ => {}` fallthrough, alongside
    the other `InputEvent` arms in `handle_stack_frames_input`.
    Add a standalone arm — do not merge with the existing `Left` arm.
  - [x] 2.4 Unit test: calling `BatchCollapseSubtree` from a deeply
    expanded frame → frame appears unexpanded in `flat_items()` AND
    the cursor returned by `nav.cursor()` is present in `flat_items()`
    (cursor is valid, no out-of-bounds).

- [x] Task 3: Handle `BatchCollapseSubtree` in the favorites panel (AC: #1)

  **Design:** Free function
  `fn batch_collapse_paths(item: &PinnedItem) -> Vec<NavigationPath>`
  returns the paths to insert into `local_collapsed`:
  - `PinnedSnapshot::Frame { .. }` → `collect_row_metadata` calls
    `collect_frame_rows(variables, 0)` with a **hardcoded `frame_id = 0`**,
    so all var paths in the metadata use `FrameId(0)`. The paths inserted
    into `local_collapsed` must use the same synthetic `FrameId(0)`, not
    the real fid from `key.nav_path` — otherwise `phase_for_path` will
    never find them. Return `Frame(0)/Var(i)` for every variable index
    `0..variables.len()`.
  - `PinnedSnapshot::Subtree { root_id, .. }` → root path is the
    synthetic root path built in `collect_row_metadata`:
    `NavigationPathBuilder::new(FrameId(*root_id), VarIdx(0)).build()`.
  - `PinnedSnapshot::Primitive { .. }` | `PinnedSnapshot::UnexpandedRef { .. }`
    → no tree to collapse; no-op.

  - [x] 3.1 Add free function `fn batch_collapse_paths(item: &PinnedItem)
    -> Vec<NavigationPath>` in
    `crates/hprof-tui/src/views/favorites_panel/mod.rs`.
    Pure function, no mutation — caller does `item.local_collapsed.extend(paths)`.
    Easier to test in isolation.

    **Per snapshot type:**
    - `Frame { variables, .. }` → `local_collapsed` is consulted via
      `phase_for_path(object_id, path)` which operates on object-level
      paths, NOT frame-level paths. Inserting a frame-only path has NO
      effect. Instead, return `Frame(0)/Var(i)` for every variable
      index `0..variables.len()`, regardless of variable type (primitives
      are harmless — their path is never looked up in `phase_for_path`).
      **Use `FrameId(0)`, NOT the real fid from `key.nav_path`:**
      `collect_row_metadata` calls `collect_frame_rows(variables, 0)` with
      a hardcoded 0, so `phase_for_path` looks up `Frame(0)/Var(i)` paths.
      Using the real fid from the pin key would silently produce paths that
      never match and the collapse would have no visual effect.
      ```rust
      (0..variables.len())
          .map(|i| NavigationPathBuilder::new(FrameId(0), VarIdx(i)).build())
          .collect()
      ```
    - `Subtree { root_id, .. }` → return a single-element vec:
      `vec![NavigationPathBuilder::new(FrameId(*root_id), VarIdx(0)).build()]`
      This MUST match the synthetic root path built in
      `collect_row_metadata` (line ~890).
    - `Primitive` / `UnexpandedRef` → return `vec![]`.

    **Imports:** `NavigationPathBuilder`, `FrameId`, `VarIdx` are
    already imported in `favorites_panel/mod.rs`. No additions needed.
  - [x] 3.2 In `handle_favorites_input`, add arm for
    `InputEvent::BatchCollapseSubtree`:
    - Get `idx = favorites_list_state.selected_index()`.
    - If `pinned.get_mut(idx)` exists (`pinned` may be empty — guard
      via `get_mut` return value, not a separate `items_len` check):
      - `let paths = batch_collapse_paths(item);`
      - `item.local_collapsed.extend(paths);`
      - Set `favorites_list_state.sub_row = 0` (jump to header).
      - `favorites_list_state.clamp_sub_row()`.
    - If `pinned` is empty, `get_mut` returns `None` — entire arm is a
      no-op, no state mutation.
    - No-op if `Primitive` / `UnexpandedRef` (returns empty vec).
  - [x] 3.3 Unit test: `BatchCollapseSubtree` on a `Subtree` snapshot →
    `local_collapsed` contains exactly
    `NavigationPathBuilder::new(FrameId(*root_id), VarIdx(0)).build()`.
    Verify by equality, not just non-empty.
  - [x] 3.4 Unit test: `BatchCollapseSubtree` on a `Primitive` snapshot →
    `local_collapsed` remains empty (no-op).
  - [x] 3.5 Unit test: `BatchCollapseSubtree` on a `Frame` snapshot with
    N variables → `local_collapsed` contains exactly N paths of the form
    `NavigationPathBuilder::new(FrameId(0), VarIdx(i)).build()` for
    `i in 0..N` (**`FrameId(0)`, the synthetic id used by
    `collect_frame_rows`**, not the real fid from the pin key). Verify
    that NO frame-only path is inserted, and that NO path using the real
    fid is inserted (both would have no visual effect).
  - [x] 3.6 Unit test: `BatchCollapseSubtree` on a `Frame` snapshot with
    0 variables → `local_collapsed` remains empty (empty range, no-op).
    `sub_row` is reset to 0 and `clamp_sub_row()` is called regardless.
  - [x] 3.7 Regression test: after `BatchCollapseSubtree`, pressing
    `Right` while `sub_row == 0` (header row) must be a no-op —
    `current_toggleable_object()` returns `None` for the header row and
    `local_collapsed` must remain unchanged. This guards the documented
    limitation (see Dev Notes) from being accidentally "fixed" by a
    refactor that makes `Right` on the header re-expand.
  - [x] 3.8 Integration test: call `handle_favorites_input(BatchCollapseSubtree)`
    on an `App` with a pinned `Subtree` snapshot. Verify **both** that
    `pinned[0].local_collapsed` contains the expected root path AND that
    `favorites_list_state.sub_row == 0`. This catches wiring bugs in the
    handler (e.g., calling `batch_collapse_paths` but forgetting to extend
    `local_collapsed`, or setting the wrong `sub_row`) that the pure-function
    tests 3.3–3.7 cannot detect.

- [x] Task 4: Jump between pinned items in `FavoritesPanelState` (AC: #3, #4)
  - [x] 4.1 Add `pub fn jump_to_prev_pin(&mut self)` on
    `FavoritesPanelState`
    (`crates/hprof-tui/src/views/favorites_panel/mod.rs`):
    - If `selected_item > 0`: `selected_item -= 1; sub_row = 0`.
    - Call `clamp_sub_row()` after modifying `sub_row`.
    - Else: no-op (already on first pin).
    - **Do NOT update `self.list_state` directly** — it is the sole
      responsibility of `render`. Same convention as `move_up`/`move_down`.
    - **Bounds: always use `self.items_len` — never `self.row_counts.len()`.**
      `row_counts` may be stale between render cycles (updated in
      `update_row_metadata` called from `render`). `items_len` is kept
      in sync by `set_items_len` at every render entry point.
  - [x] 4.2 Add `pub fn jump_to_next_pin(&mut self)`:
    - If `selected_item + 1 < items_len`: `selected_item += 1; sub_row = 0`.
    - Call `clamp_sub_row()` after modifying `sub_row`.
    - Else: no-op (already on last pin).
    - Same bound rule: use `self.items_len`, not `self.row_counts.len()`.
  - [x] 4.3 Unit test: `jump_to_prev_pin` on item 2 (0-indexed) →
    `selected_item == 1, sub_row == 0`.
  - [x] 4.4 Unit test: `jump_to_prev_pin` on item 0 → stays at 0.
  - [x] 4.5 Unit test: `jump_to_next_pin` on item 0 (of 3) →
    `selected_item == 1, sub_row == 0`.
  - [x] 4.6 Unit test: `jump_to_next_pin` on last item → stays.
  - [x] 4.7 Unit test: `jump_to_next_pin` when cursor has sub_row > 0
    → after jump, `sub_row == 0`.
  - [x] 4.8 Unit test: `jump_to_next_pin` with `items_len == 0` and
    `row_counts` empty → no panic, no state change. Validates that the
    `items_len` guard protects against stale-`row_counts` edge cases.

- [x] Task 5: Wire `[` / `]` in `handle_favorites_input` (AC: #3, #4)
  - [x] 5.1 In `handle_favorites_input`, add:
    ```rust
    InputEvent::SearchChar('[') => {
        self.favorites_list_state.jump_to_prev_pin();
    }
    InputEvent::SearchChar(']') => {
        self.favorites_list_state.jump_to_next_pin();
    }
    ```
    Place these BEFORE the generic `_ => {}` fallthrough.
  - [x] 5.2 `[` / `]` arrive as `SearchChar` because `input.rs` maps
    unbound printable keys to `SearchChar`. This is the established
    pattern (same as `h`/`H` for hide). No changes needed to `input.rs`
    for these keys.
    **Future conflict note:** If a search/filter mode is ever added to
    the favorites panel, `[`/`]` must only be intercepted for navigation
    when search is NOT active — following the same pattern as
    `is_search_active()` guard in `handle_thread_list_input`. Add a
    `// FUTURE: guard with is_search_active() if favorites search is added`
    comment at the intercept site (mandatory).
    **AZERTY note:** On AZERTY keyboards, `[`/`]` may require `AltGr`
    and may be inaccessible without a modifier key. To be revisited
    during story 13.5 (AZERTY/QWERTY keymapping).
    **Stale `items_len` risk:** If `jump_to_next_pin` seems to select
    a non-existent item during testing, check that `ToggleFavorite` in
    `handle_favorites_input` calls `set_items_len(self.pinned.len())`
    BEFORE `sync_favorites_selection()`. Without this, `items_len` stays
    stale until the next render, making bounds checks unreliable.
  - [x] 5.3 Handler-level test: call `handle_favorites_input(SearchChar('['))`
    and `handle_favorites_input(SearchChar(']'))` when `self.pinned` is
    empty → no panic, `favorites_list_state` state unchanged. This
    exercises the dispatch path through the handler (not just the state
    methods tested in 4.4/4.8).

- [x] Task 6: Update help bar (AC: #5)
  - [x] 6.1 In `crates/hprof-tui/src/views/help_bar.rs`, update
    `ENTRY_COUNT` from `21` to `23`.
  - [x] 6.2 Add two new entries to `ENTRIES`:
    ```rust
    ("Shift+\u{2190}", "Batch collapse subtree", STACK | FAV),
    ("[ / ]",          "Favorites: jump to prev/next pin", FAV),
    ```
    Insert after the existing `←` entry
    (`"\u{2190}", "Unexpand / go to parent", STACK`).
  - [x] 6.3 Update the row count assertions in `help_bar.rs` tests.
    `ENTRY_COUNT` = total number of entries in `ENTRIES` (used by
    `required_height()` to size the layout slot — counts ALL entries,
    not just visible ones for a given context).
    `build_rows(ctx)` iterates **all** entries regardless of context —
    it dims inapplicable ones but does NOT omit them. Therefore
    `build_rows` always returns the same number of `Line`s for every
    context. The formula is: 1 padding + `ceil(ENTRIES.len() / 2)` + 1
    separator. Update both independently:
    - `ENTRY_COUNT`: 21 → 23 (2 new entries added to `ENTRIES`).
    - `build_rows(ctx).len()` for **all three contexts**: 13 → 14
      (`ceil(23/2) = 12`; 1 + 12 + 1 = 14).
    **Do NOT write different expected values for ThreadList, StackFrames,
    and Favorites** — they are always identical.
    Updating `ENTRY_COUNT` without fixing the `build_rows` assertions
    will cause test failures; updating `build_rows` counts without
    `ENTRY_COUNT` will cause layout sizing regression.
    Also rename test `required_height_returns_fifteen_for_twenty_one_entries`
    to `required_height_returns_sixteen_for_twenty_three_entries` and
    update its assertion from 15 to 16.
  - [x] 6.4 Add unit tests for the new entries:
    - `Shift+←` entry: applicable in STACK and FAV, not THREAD.
    - `[ / ]` entry: applicable only in FAV.
  - [x] 6.5 Verify that the existing `entry_count_constant_matches_entries_slice`
    test in `help_bar.rs` still passes after updating `ENTRY_COUNT` to 23.
    Do NOT add a duplicate — the test already exists and asserts
    `ENTRY_COUNT as usize == ENTRIES.len()`.
  - [x] 6.6 Before updating counts, verify the semantics of `build_rows`:
    confirm it returns one element per `ENTRIES` item (not one per
    rendered line). If entries with long labels wrap to 2 lines, the
    actual rendered height differs from `len()`. Adjust expected counts
    only after confirming the function's unit of return.
  - [x] 6.7 Verify `required_height()` stays viable with 23 entries.
    The actual formula is `2 + 1 + div_ceil(ENTRY_COUNT, 2) + 1`.
    With 23 entries: `2 + 1 + 12 + 1 = 16` lines. On a 24-line terminal
    this leaves 8 lines for the main content — acceptable. Update the
    doc-comment in `required_height()` from the current "15" to "16".
    No other code change needed; this is a verification step only.

- [x] Task 7: Final verification (AC: #6)
  - [x] 7.1 `cargo test` — all green.
  - [x] 7.2 `cargo clippy --all-targets -- -D warnings` — clean.
  - [x] 7.3 `cargo fmt -- --check` — clean.
  - [x] 7.4 Manual test with `assets/heapdump-visualvm.hprof`:
    1. Pin 3 items with expanded nested content.
    2. Navigate into a nested item (sub_row > 0) → press `Shift+Left`
       → tree collapses, cursor on header row.
    3. Press `]` twice → cursor jumps to 3rd pin header. Press `[` → 2nd pin.
    4. In stack view, expand several levels → press `Shift+Left` →
       entire frame collapses.
    5. Press `?` with favorites focused → `Shift+←` and `[ / ]`
       appear in the help bar.

## Dev Notes

### Current Favorites State Architecture

```
FavoritesPanelState {
    selected_item: usize,       // index in pinned[]
    sub_row: usize,             // 0 = header row, 1..N = content rows
    row_counts: Vec<usize>,     // total rows per item (incl. header + separator)
    ...
}
```

`abs_row()` = `sum(row_counts[0..selected_item]) + sub_row` → maps to the
ratatui `ListState` absolute row.

`jump_to_next_pin` / `jump_to_prev_pin` simply modify `selected_item` and
reset `sub_row = 0`. `clamp_sub_row()` is a no-op when `sub_row == 0`.

### Key Paths for Snapshot Roots

`PinnedItem.key.nav_path` = the cursor path at pin time (e.g.
`Frame(1)/Var(0)/Field(2)/Field(0)`). The FRAME segment is always the
first segment. To get the frame-only root path:
```rust
let segs = item.key.nav_path.segments();
if let Some(PathSegment::Frame(fid)) = segs.first() {
    NavigationPathBuilder::frame_only(*fid)
}
```

For `PinnedSnapshot::Subtree`, the synthetic root path used in
`collect_row_metadata` is:
```rust
NavigationPathBuilder::new(FrameId(*root_id), VarIdx(0)).build()
```
The same root path must be used in `root_path_for_snapshot` for
`local_collapsed` insertion to match what `phase_for_path` looks up.

**Limitation : ré-expand après batch collapse depuis le header (sub_row = 0) :**
Après `Shift+Left`, le curseur est placé sur le header row (sub_row = 0).
`Right` sur le header est un no-op car `current_toggleable_object()`
retourne `None` pour le header row (non présent dans `row_kind_maps`).
L'utilisateur ne peut donc pas ré-expand directement depuis le header.
**Workaround documenté :** presser `↓` pour descendre sur la première
ligne de contenu, puis `Right` pour ré-expand.
**Backlog :** Ajouter dans une story future la possibilité que `Right`
sur le header retire le root path de `local_collapsed` si présent,
permettant un ré-expand en une touche depuis le header. Hors scope 13.2.

**Comportement de `local_collapsed` après batch collapse :**
Batch collapse insère UNIQUEMENT le root path dans `local_collapsed`.
Les paths enfants déjà collapsed (ajoutés par des `Left` précédents)
restent en place — ce qui est correct : si l'utilisateur re-expand la
racine (retire le root de `local_collapsed` via `Right`), les nœuds
enfants réapparaissent dans leur état précédent (certains collapsed,
d'autres non). Ce comportement est intentionnel et ne doit pas être
"corrigé" en effaçant `local_collapsed` lors du batch collapse.

### Stack View Frame Collapse

**Asymétrie intentionnelle stack view vs favorites :**
`Shift+Left` dans le stack view collapse le FRAME ENTIER (toute la
hiérarchie sous le frame courant). Dans les favoris, il collapse
l'ITEM COURANT uniquement (pas tous les pinned items). Ces deux
granularités sont différentes mais cohérentes : dans chaque panel,
`Shift+Left` collapse l'unité logique racine visible (frame vs item).

**Use `collapse_object_recursive`, NOT `toggle_expand`.**

`toggle_expand(fid, vec![])` collapses the frame but does NOT call
`resync_cursor_after_collapse()`. After collapse, the cursor may point
to a row that no longer exists in `flat_items()` — causing the UI to
select the wrong row or panic on the next render.

`collapse_object_recursive(&frame_path)` calls `collapse_at_path` AND
`resync_cursor_after_collapse`, which falls back to the nearest valid
ancestor. This is the correct path.

To implement `BatchCollapseSubtree` in the stack view:
```rust
if let Some(s) = &mut self.stack_state {
    // Extract frame_id from the first segment of the current cursor.
    if let RenderCursor::At(path) | RenderCursor::LoadingNode(path) = s.nav.cursor() {
        if let Some(PathSegment::Frame(fid)) = path.segments().first() {
            let frame_path = NavigationPathBuilder::frame_only(*fid);
            s.collapse_object_recursive(&frame_path);
        }
    }
}
```
Note: `collapse_object_recursive` is already `pub` on `StackState`
(state.rs:967). No new API needed.

### input.rs Ordering

`Shift+Left` arm must come BEFORE the catch-all `(KeyCode::Left, _)` arm
in `from_key`. The existing Shift+Up/Down pattern is the model:
```rust
(KeyCode::Up, mods)
    if mods.contains(KeyModifiers::CONTROL) || mods.contains(KeyModifiers::SHIFT) =>
    Some(InputEvent::CameraScrollUp),
(KeyCode::Up, _) => Some(InputEvent::Up),
```
Apply the same pattern for Left:
```rust
(KeyCode::Left, mods) if mods.contains(KeyModifiers::SHIFT) =>
    Some(InputEvent::BatchCollapseSubtree),
(KeyCode::Left, _) => Some(InputEvent::Left),
```
`Shift+Left` with `CONTROL` also maps to `BatchCollapseSubtree` because
`mods.contains(SHIFT)` is true for `CONTROL|SHIFT`. This is intentional
and consistent with the camera-scroll pattern.

### Anti-Patterns to Avoid

- Do NOT use `selected_item` directly in `handle_favorites_input` to
  implement jump; call `jump_to_prev_pin` / `jump_to_next_pin` and
  keep all bounds logic inside `FavoritesPanelState`.
- Do NOT insert child paths into `local_collapsed` for batch collapse —
  only insert the root path. `render_variable_tree` stops recursing
  into children of collapsed nodes, so inserting the root is sufficient
  to hide the entire subtree. (`phase_for_path` itself only does a
  direct `contains()` lookup — no ancestor walk.)
- Do NOT forget to call `clamp_sub_row()` after any operation that
  modifies `selected_item` or `sub_row` directly.
- Do NOT change `ENTRY_COUNT` without updating the row count assertions
  in `help_bar.rs` tests — they will break.
- Do NOT use `toggle_expand(fid, vec![])` for stack view batch collapse —
  it skips cursor resync and leaves the cursor pointing at a stale row.
  Use `collapse_object_recursive` instead.
- Do NOT use `item.key.nav_path` as the root path for `Subtree`
  snapshots — it may be a deep path that doesn't match the synthetic
  root used in `collect_row_metadata`. Always use
  `NavigationPathBuilder::new(FrameId(*root_id), VarIdx(0)).build()`.
- Do NOT use the real fid from `item.key.nav_path` when building
  `Frame` snapshot collapse paths. `collect_frame_rows` uses a hardcoded
  `frame_id = 0`, so all var paths in metadata are `Frame(0)/Var(i)`.
  Inserting `Frame(real_fid)/Var(i)` into `local_collapsed` silently
  does nothing. Always use `FrameId(0)` for Frame snapshot collapse.
- Do NOT add a `[ / ]` → `InputEvent` variant in `input.rs` — these
  chars must stay as `SearchChar` so that the thread-list search input
  continues to work. Context-based dispatch (focus == Favorites) is the
  correct isolation mechanism.

### Project Structure Notes

All changes are in `crates/hprof-tui/`:
- `src/input.rs` — new variant + Shift+Left mapping
- `src/app/mod.rs` — new arms in `handle_favorites_input` and
  `handle_stack_frames_input`
- `src/views/favorites_panel/mod.rs` — `jump_to_prev_pin`,
  `jump_to_next_pin`, `root_path_for_snapshot`
- `src/views/help_bar.rs` — 2 new entries, updated counts

No changes needed in `hprof-engine` or `hprof-parser`.

### References

- [Source: crates/hprof-tui/src/input.rs:66-118] — `from_key` with
  existing Shift+Up/Down pattern
- [Source: crates/hprof-tui/src/app/mod.rs:858-1012] —
  `handle_favorites_input` with all existing arms
- [Source: crates/hprof-tui/src/views/favorites_panel/mod.rs:66-250] —
  `FavoritesPanelState` with `move_up`, `move_down`, `selected_item`
- [Source: crates/hprof-tui/src/views/favorites_panel/mod.rs:680-819] —
  `collect_row_metadata` with synthetic root path for Subtree snapshots
- [Source: crates/hprof-tui/src/favorites.rs:89-115] — `PinnedItem`
  with `local_collapsed`, `key.nav_path`
- [Source: crates/hprof-tui/src/views/stack_view/expansion.rs:77-106] —
  `collapse_at_path` recursive collapse
- [Source: crates/hprof-tui/src/views/help_bar.rs:17-48] —
  `ENTRY_COUNT`, `ENTRIES`, context bitmasks
- [Source: docs/planning-artifacts/epics.md] — Epic 13, Story 13.2 (FR67)
- [Source: docs/implementation-artifacts/13-1-collapsible-static-fields-section.md]
  — Previous story in epic

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

N/A

### Completion Notes List

- Task 1: `BatchCollapseSubtree` variant removed during iteration — bindings changed to simple keys (`c`, `b`, `n`) dispatched via `SearchChar` to avoid Shift/AltGr issues on WSL/AZERTY.
- Task 2: `c` in stack view = progressive expand from cursor position. First press expands frame, subsequent presses expand collapsed objects under cursor. Uses `collapsed_expandable_at` + `expand_target_at_path` to walk NavigationPath segments through vars, fields, and collection entries.
- Task 3: `c` in favorites = clear `local_collapsed` (expand all). `batch_collapse_paths()` free function kept for Left-collapse use. `reset_sub_row()` added on `FavoritesPanelState`.
- Task 4: `jump_to_prev_pin()` / `jump_to_next_pin()` on `FavoritesPanelState`. 6 unit tests.
- Task 5: `b`/`n` wired as `SearchChar` dispatch in `handle_favorites_input`. 1 handler-level test.
- Task 6: Help bar updated: `c` (expand, STACK+FAV), `b/n` (prev/next pin, FAV). `ENTRY_COUNT` 21→23.
- Task 7: All tests pass, clippy clean, fmt clean.
- Post-review fixes:
  - `expand_target_at_path`: fixed `seg == segs.last()` value comparison bug (duplicate Field segments matched prematurely) → replaced with index-based `is_last` check.
  - `expand_target_at_path`: fixed `Some(_) => return None` on collection fields — now traverses through collection fields to reach `CollectionEntry` segments.
  - `expand_target_at_path`: added `CollectionEntry` segment handling to resolve ObjectRef entries in collections.
  - `collapsed_expandable_at`: stops at collection boundaries — collections skipped in descendant search, paths crossing `CollectionEntry` in the delta are filtered out.
  - Collections > 100 entries skipped by `c` handler (manual open via Enter/Right required).
  - `emit_collection_entry_obj_children` (+ 2 sibling methods): fixed "no fields" phantom nodes — distinguishes `object_fields` absent (show LoadingNode) vs empty `Vec` (genuinely no fields, skip silently).
  - Engine `expand_object`: added re-check after decode to prevent LRU re-insertion panic on concurrent expand calls.
  - `set_expansion_done_at_path`: added `nav.sync()` to keep cursor aligned after async expansion completes.
  - Realistic scenario test covering full Universe structure (17 fields, 5 collections) validating collection boundary enforcement.

### Change Log

- 2026-03-19: Implemented story 13.2 — progressive expand, favorites expand/jump, help bar
- 2026-03-19: Post-review: fixed collection boundary traversal, "no fields" phantoms, LRU race, cursor sync

### File List

- `crates/hprof-tui/src/app/mod.rs` — `c` progressive expand handler (stack view + favorites), `b`/`n` pin jump, `ExpandTarget` import, `HashSet` import
- `crates/hprof-tui/src/app/tests.rs` — integration tests (expand on subtree, b/n empty pinned)
- `crates/hprof-tui/src/views/favorites_panel/mod.rs` — `batch_collapse_paths()`, `jump_to_prev_pin()`, `jump_to_next_pin()`, `reset_sub_row()`
- `crates/hprof-tui/src/views/favorites_panel/tests.rs` — `jump_pin_tests` (6), `batch_collapse_tests` (6)
- `crates/hprof-tui/src/views/help_bar.rs` — `ENTRY_COUNT` 23, `c` + `b/n` entries, updated tests
- `crates/hprof-tui/src/views/stack_view/mod.rs` — re-export `ExpandTarget`
- `crates/hprof-tui/src/views/stack_view/state.rs` — `ExpandTarget` enum, `expand_target_at_path`, `collapsed_expandable_at`, `mark_path_expanded`, `sync_nav`, `object_id_at_path` → `expand_target_at_path` rewrite, `emit_*` "no fields" fix, `set_expansion_done_at_path` nav sync
- `crates/hprof-tui/src/views/stack_view/tests.rs` — `expand_target_at_collection_entry_path`, `expand_target_at_collection_entry_child_field`, realistic Universe scenario (5 tests)
- `crates/hprof-engine/src/engine_impl/mod.rs` — `expand_object` re-check cache after decode
- `docs/implementation-artifacts/sprint-status.yaml` — story status
- `docs/implementation-artifacts/13-2-enhanced-favorites-navigation.md` — this file
