# Adversarial Review — Story 9.8: Pinned Item Navigation & Array Expansion

**Reviewer:** Claude (adversarial pass)
**Date:** 2026-03-12
**Status:** 15 issues found

---

## Issues

### 1. `collect_row_metadata` omits the `(no fields)` and `(no locals)` sentinel rows

**Location:** Task 3.1, traversal-order spec — "Edge cases" list

The story's traversal-order description says "each child field emits 1 row". It never accounts
for the `(no fields)` row that `append_object_children` emits when `field_list.is_empty()`,
nor the `(no locals)` row that the `Frame` branch emits when `vars.is_empty()`. Both are
single `ListItem`s emitted by the real renderer. `collect_row_metadata` is required to mirror
the renderer exactly; if it skips these, `row_count` is 1 short for every empty-field object.
The consequence is that `abs_row()` undershoots, and the ratatui selection highlight is one
row off for every item rendered after an object with no fields.

The four "most common drift sources" explicitly listed in Task 3.1 do not include this case,
which is by far the most likely to occur in practice (e.g., `Object` root in a
`Subtree` snapshot that has already been fully traversed).

---

### 2. `collect_row_metadata` must also count the `truncated` warning row

**Location:** Task 3.1; also Task 1.3

The render loop in `FavoritesPanel::render` (and in the current code) unconditionally pushes
an extra `ListItem` when `truncated == true`:

```rust
if *truncated {
    items.push(ListItem::new(Line::from(Span::styled(
        "  [!] snapshot partiel — trop d'objets",
        THEME.error_indicator,
    ))));
}
```

This row appears **before** the tree content. `collect_row_metadata` is never told to account
for it. Any `PinnedItem` with `truncated = true` will have a `row_count` that is 1 short,
misaligning every subsequent toggle target. The story does not mention this anywhere — not in
the edge-case list, not in the traversal-order spec, not in the debug assert verification.

---

### 3. `PinChunkSentinel` struct is defined, then immediately retracted — leaving an
    unresolvable naming conflict in Task 5

**Location:** Tasks 5.2 and 5.5 — directly contradictory

Task 5.2 says:

> Use a dedicated type: `pub(crate) struct PinChunkSentinel { ... }`

Task 5.5 then says:

> Returns a plain tuple — no `PinChunkSentinel` struct exported outside the module.
> `app/mod.rs` destructures the tuple directly.

These two instructions cannot both be followed. Task 5.2 introduces a named struct and
instructs the dev to store `Vec<HashMap<usize, (u64, usize)>>` in `FavoritesPanelState`
in the same breath — a contradiction within a single task. A developer who reads Task 5.2
first will define the struct; one who reads 5.5 first will use a tuple. The final signatures
of `current_chunk_sentinel` and `update_row_metadata` depend on which path is taken.

The story should pick one and remove the other. The tuple form (5.5) is simpler and consistent
with 5.2's final data-type declaration — but the struct definition in 5.2 is dead weight that
will cause a dead-code Clippy warning.

---

### 4. `update_row_metadata` is called at the *start* of each render, but the story says
    row metadata is used immediately by the *same* render's `abs_row()` call

**Location:** Tasks 2.6, 3.2, and 3.3

Task 2.6 says `update_row_metadata` is "called at the start of each render (so navigation can
use last-frame metadata — lag of one frame is imperceptible)". Task 3.2 says to call it
"before building `items`". Task 3.3 then says to call `state.abs_row()` *after* building
items to set the ratatui list selection for the *current* frame.

If `update_row_metadata` stores last-frame metadata, then the `abs_row()` call at the end of
the same render uses the freshly-computed current-frame metadata — not last-frame data.
The statement "lag of one frame is imperceptible" applies to **navigation** (keypresses
between frames), but the `list_state.select(Some(state.abs_row()))` at the bottom of render
correctly uses current-frame row counts. The story conflates these two uses, making the
intended timing semantics ambiguous: is `update_row_metadata` updating state for the current
render or the next? The dev must decide which is correct and the story should state this
explicitly.

---

### 5. `move_up` landing-row formula uses `unwrap_or(0).saturating_sub(1)` — silently
    lands on row 0 for items with `row_count = 0`

**Location:** Task 2.7

The story specifies:

```
sub_row = row_counts.get(selected_item).copied().unwrap_or(0).saturating_sub(1)
```

If `row_counts[selected_item]` is `0` (which is impossible by Task 2.4's invariant but can
occur during the brief window before `update_row_metadata` populates a newly added item),
`unwrap_or(0).saturating_sub(1)` returns `0`. This silently puts the cursor at `sub_row = 0`
rather than signalling a problem. The story simultaneously promises in Task 2.4 that
`row_counts.resize(len, 1)` prevents this window, yet Task 2.7 uses `unwrap_or(0)` which
implies a `0` value is possible. The inconsistency is a maintenance trap: if Task 2.4's
padding is ever changed to `0` the move formula breaks silently.

The correct fallback is `unwrap_or(1).saturating_sub(1)` to match Task 2.4's padding value
of `1`.

---

### 6. Test 6.9 makes a claim it cannot enforce: "assert `selected_item` is 0 or 1"

**Location:** Task 6.9

Test 6.9 states:

> `items_len = 2`; call `move_down()`; assert no panic and `selected_item` is 0 or 1
> (either is valid; must not be out of bounds).

This is not a regression guard — it accepts both `0` and `1` as correct, meaning it would
pass even if `move_down()` is a no-op (returns `selected_item = 0`). The actual requirement
from Task 2.4 is that pressing Down before the first render must NOT leave the cursor stuck
at item 0 when there are multiple items — but the test permits exactly that degenerate
behaviour. The test should assert `selected_item == 1` (i.e., that `move_down` actually
moved, using the `row_counts` padding of 1 per item to compute the move correctly). As
written, this test provides zero regression coverage for the bug it claims to guard against.

---

### 7. `toggle_pin` calls `sync_favorites_selection` which calls `set_items_len` then
    `set_selected_index` in the correct order — but the story also mandates a `clear`
    of `pending_pinned_pages` that is nowhere enforced in `sync_favorites_selection`

**Location:** Task 5.7 and `toggle_pin` in `app/mod.rs`

Task 5.7 says to call `self.pending_pinned_pages.clear()` after any unpin, "in `toggle_pin`
after `self.pinned.remove(pos)` or `self.pinned.retain(...)`". The current `toggle_pin`
removes items only via `self.pinned.remove(pos)`. The `handle_favorites_input`
`ToggleFavorite` arm uses `self.pinned.retain(...)` directly (bypassing `toggle_pin`
entirely). The story instructs the clear in `toggle_pin` but the retain-path bypass means
the `pending_pinned_pages.clear()` is never applied from the `ToggleFavorite` arm in
`handle_favorites_input`. The story fails to mention this second removal site — a developer
following the story literally will miss it.

---

### 8. `collect_row_metadata` is specified as `fn collect_row_metadata(item: &PinnedItem)`
    but it must also handle `PinnedSnapshot::Primitive` and `PinnedSnapshot::UnexpandedRef`
    — neither is mentioned in the traversal spec

**Location:** Task 3.1

The traversal-order description exclusively discusses `Frame` and `Subtree` snapshots. The
render loop in `FavoritesPanel::render` also handles `UnexpandedRef` (1 content row) and
`Primitive` (1 content row). `collect_row_metadata` must return a `row_count` of
`1 (header) + 1 (content) + 1 (separator) = 3` for both of these snapshot types.

The story does not say what `collect_row_metadata` should return for these variants. If the
dev naively pattern-matches only `Frame` and `Subtree` and returns `0` (or panics) for the
others, `abs_row()` and `move_down()` will be broken for any pinned primitive or unexpanded
ref. This is a common case — pinning a null variable or an unexpanded object reference
produces exactly these variants.

---

### 9. The `Right`/`Enter` toggle logic in Task 4.2 collapses when `is_collapsed == false`
    — this is the *expand* path, not the collapse path

**Location:** Task 4.2 — logic is inverted

Task 4.2 states:

```rust
if is_collapsed {
    item.local_collapsed.remove(&object_id);
} else {
    item.local_collapsed.insert(object_id);
}
```

`Right`/`Enter` are described as expand actions (AC2). When `is_collapsed == true`, the
object is collapsed and the user wants to expand it → removing from `local_collapsed` is
correct. When `is_collapsed == false`, the object is expanded — pressing `Right`/`Enter`
on an already-expanded row collapses it again. This is **not** what AC2 specifies. AC2
says `Right`/`Enter` on a `+` row expands; AC3 says `Left` on a `-` row collapses. Task
4.2's `else` branch turns `Right`/`Enter` into a toggle-both-ways action, contradicting
AC2 and creating duplicate collapse semantics between `Right` and `Left`.

The `else` branch in Task 4.2 should either be removed (making `Right`/`Enter` expand-only)
or the entire block should be clearly labelled as a toggle. As written it contradicts the
ACs and the Dev Notes ("all objects in `object_fields` are expanded by default — `local_collapsed`
is additive").

---

### 10. `FavoritesPanelState::chunk_sentinel_maps` field type in Task 2.1 vs Task 5.2 is
     never reconciled with `update_row_metadata` signature in Task 2.6

**Location:** Tasks 2.1, 2.6, 5.2

Task 2.1 defines `chunk_sentinel_maps: Vec<HashMap<usize, (u64, usize)>>` in the struct.
Task 2.6 defines `update_row_metadata` taking `chunk_sentinel_maps: Vec<HashMap<usize, (u64,
usize)>>` — consistent so far.

Task 5.2 says to "extend the kind map — or use a dedicated type", then introduces `struct
PinChunkSentinel` with named fields `collection_id` and `chunk_offset`, but says "no dedicated
struct needed, keeping `favorites_panel.rs` internals private — the tuple is `(collection_id,
chunk_offset)`". The result is that `chunk_sentinel_maps` carries a raw `(u64, usize)` tuple
but the story never documents which `u64` is `collection_id` and which `usize` is
`chunk_offset` in the tuple — only the struct would have made this self-evident. The signature
in Task 2.6's assert also uses opaque indices. A developer relying purely on the story (not
the struct) will have to guess the field order, and a swap produces a silent runtime bug where
the wrong page offset is loaded.

---

### 11. The `visited` set for cyclic-object detection in `collect_row_metadata` must mirror
     `append_object_children`'s exact insert-then-remove pattern — the story does not
     specify this

**Location:** Task 3.1, "Cyclic objects" edge case

The real renderer (`append_object_children`) does `visited.insert(object_id)` at the *start*
of the `Expanded` arm and `visited.remove(&object_id)` at the *end* (after recursing children
and static items). This remove-after-recurse pattern means an object can appear in multiple
disjoint branches of the tree without being detected as cyclic — only genuine back-edges are
cyclic.

The story's cyclic-object edge case says only "apply the same guard" without specifying
whether `collect_row_metadata` must also remove the id after recursion. If the dev omits the
`visited.remove`, the function will incorrectly treat any object appearing more than once in
different subtrees as cyclic, producing a row count that is too low (emitting 1 `[cyclic]`
row instead of the full child set). Tests 6.7/6.8 will not catch this because they use a
simple linear chain, not a diamond-shaped graph.

---

### 12. No test covers `UnexpandedRef` or `Primitive` snapshot types in the new
     `FavoritesPanelState` row-count path

**Location:** Task 6 — test coverage gaps

The 14 new tests (6.1–6.14) cover `Frame`/`Subtree` snapshots in 6.7, 6.8, 6.10, 6.11.
Not one test verifies `row_count` or navigation behaviour for `UnexpandedRef` or `Primitive`
snapshots. Given issue #8 above (the traversal spec ignores these variants), the combination
of a missing spec and missing tests means a bug in the `collect_row_metadata` fallback paths
will ship without a single test to catch it.

---

### 13. `clamp_sub_row` is specified to clamp using `unwrap_or(1)` — inconsistency with
     Task 2.7 which uses `unwrap_or(0)`

**Location:** Task 2.12 vs Task 2.7

Task 2.12 says:

> clamps `sub_row` to `row_counts.get(selected_item).copied().unwrap_or(1).saturating_sub(1)`

Task 2.7 says:

> `sub_row = row_counts.get(selected_item).copied().unwrap_or(0).saturating_sub(1)`

Both `unwrap_or` fallback values differ (1 vs 0). The story gives no rationale for the
inconsistency. For a "before first render" state both should produce the same result if Task
2.4's padding of `1` holds — but if either is wrong the other masked it. This is a latent
bug waiting for the padding invariant to break.

---

### 14. The `ToggleObjectIds` arm is missing from the list of events that `handle_favorites_input`
     must preserve

**Location:** Task 4.1 / Dev Notes "CRITICAL before starting Task 4"

The story warns developers to preserve the `NavigateToSource` (`g`) arm from Story 9.6. It
does not mention the `ToggleObjectIds` (`i`) arm, which is present in `handle_stack_frames_input`
and dispatched globally before the focus branch. However, the current `handle_input` only
handles `ToggleHelp` globally — `ToggleObjectIds` is handled only inside
`handle_stack_frames_input` and is therefore silently dropped when focus is on favorites.
The story does not specify whether `i` should work while the favorites panel is focused. If
it should (which is the intuitive expectation since the favorites panel also renders with
`show_object_ids`), Task 4.1 must add an arm for it; if it should not, the story should
say so. The omission leaves the dev to guess.

---

### 15. Test 6.13 description says `update_row_metadata([2], ...)` but the function takes
     three `Vec` arguments — the ellipsis hides mandatory parameters

**Location:** Task 6.13

The test specifies:

> call `update_row_metadata([2], ...)` (simulating a collapse that reduced row count to 2)

`update_row_metadata` takes `(row_counts: Vec<usize>, row_kind_maps: Vec<HashMap<usize,
(u64, bool)>>, chunk_sentinel_maps: Vec<HashMap<usize, (u64, usize)>>)`. The `...` hides
two required arguments. For a test document that is supposedly prescriptive enough to be
written test-first, leaving mandatory arguments to the developer's imagination is unacceptable.
The correct call is `update_row_metadata(vec![2], vec![HashMap::new()],
vec![HashMap::new()])`. Without this precision, a developer who gets the `row_kind_maps`
or `chunk_sentinel_maps` argument wrong will have a test that compiles (it's valid Rust) but
verifies the wrong invariant.
