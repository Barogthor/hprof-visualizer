# Adversarial Review — Story 7.1: Favorites Panel

**Artifact reviewed:** `docs/implementation-artifacts/7-1-favorites-panel.md`
**Reviewer:** Claude (adversarial mode)
**Date:** 2026-03-10

---

## Findings

### 1. `frame.id` vs `frame.frame_id` — field name is wrong in Task 3.1

Task 3.1 bullet for `OnFrame` says:

> `frame_id = state.frames()[frame_idx].id`

The actual `FrameInfo` struct (defined in `crates/hprof-engine/src/engine.rs:68`)
uses `pub frame_id: u64`, not `.id`. The Dev Notes section repeats the same error:

> `let frame = &state.frames()[frame_idx]; let frame_id = frame.id;`

A developer following this literally will get a compile error. The correct access is
`frame.frame_id`. This same wrong name appears in the sub-bullet for `OnFrame` in
Task 3.3 and again in the "Résolution `frame_idx → frame_id`" note.

---

### 2. `vars_for` method does not exist — Dev Notes snippet references a phantom API

The field-path resolution snippet in Dev Notes (the "snapshot_from_cursor : résolution
du field_path" section) calls:

```rust
let var = &state.vars_for(frame.id)?[var_idx];
```

No such method exists on `StackState`. The actual storage is
`self.vars: HashMap<u64, Vec<VariableInfo>>`, keyed by `frame_id`. The correct access
is `state.vars.get(&frame.frame_id)`. Additionally that field is private (`vars` has no
accessor listed in Task 3.1). The story either needs to add a
`pub(crate) fn vars(&self) -> &HashMap<u64, Vec<VariableInfo>>` accessor to the Task 3.1
list, or must use the private field from within the same crate — but the snippet as
written will not compile regardless.

---

### 3. `stack_state` is `Option<StackState>` — Task 5.1 ignores the unwrap risk

`App.stack_state` is `Option<StackState>` (line 80 of `app.rs`). Task 5.1 instructs the
developer to call `self.stack_state.cursor()` and `&self.stack_state` directly with no
mention of the `Option`. There is no guidance on how to handle the `None` case. A naive
implementation will either `unwrap()` (panic risk) or fail to compile. The story must
explicitly state `self.stack_state.as_ref().map(|s| ...)` or note that
`handle_stack_frames_input` is only reachable when `stack_state` is `Some`.

---

### 4. `active_thread_name()` helper: existence and source are unverified

Task 5.1 tells the developer to call `self.active_thread_name()` and to "create this
method helper on `App` if it doesn't exist." Examining `app.rs` (lines 1–180), no such
method exists and neither does any stored `active_thread_name` field. The story does not
say where the name comes from: is it retrieved from the engine via the selected thread's
serial? From `StackState`? The app struct has `thread_list: ThreadListState` but stores
no thread name directly. This is not a minor omission — the developer has to reverse-engineer
the data flow from scratch.

---

### 5. `OnChunkSection` is silently excluded from pin scope with no AC mention

AC #1–#3 cover `OnFrame`, `OnVar`, and `OnObjectField`. The "out of scope" note in AC#3
explicitly excludes `OnCollectionEntry` and `OnCollectionEntryObjField`. However,
`OnChunkSection` also silently returns `None` in Task 3.3, with zero mention in any
acceptance criterion. The user story claims the user can pin "any value (stack frame,
variable, or object field)." Whether `OnChunkSection` should be pinnable is left
unresolved. If it is intentionally excluded, that exclusion belongs in the AC, not buried
in a code comment in Task 3.3.

---

### 6. `PinnedSnapshot::Frame` includes `variables: Vec<VariableInfo>` — cloning strategy unspecified

Task 2.3 defines `PinnedSnapshot::Frame { variables: Vec<VariableInfo>, ... }`. Task 3.3
says to capture "all variables + subtree_snapshot per root var." `VariableInfo` is defined
in `hprof-engine`. There is no instruction on whether each variable's value tree must be
recursively walked or whether only the root `object_fields` reachable from the frame's
vars need to be collected via `subtree_snapshot`. If there are 50 variables and each is
an expanded ObjectRef, the developer must call `subtree_snapshot` 50 times and merge the
results — but the document says only "subtree_snapshot per root var" with no merge
semantics. The `SNAPSHOT_OBJECT_LIMIT = 500` applies per-call or globally? Unspecified.
A developer could reasonably implement this in two incompatible ways.

---

### 7. `FavoritesPanelState` lifetime conflicts with `StatefulWidget` implementation in ratatui

Task 7.2 prescribes:

```rust
pub struct FavoritesPanelState<'a> {
    pub pinned: &'a [PinnedItem],
    pub pinned_cursor: usize,
    pub list_state: &'a mut ListState,
}
```

And states: "`FavoritesPanel` implémente `StatefulWidget<State = FavoritesPanelState<'a>>`"

The ratatui `StatefulWidget` trait is defined as:
```rust
pub trait StatefulWidget {
    type State;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State);
}
```

The `State` associated type has no lifetime parameter — it is a concrete type. Binding
`State = FavoritesPanelState<'a>` requires a Generic Associated Type (GAT) or an `impl
StatefulWidget` that carries its own lifetime. Ratatui's `StatefulWidget` does not use
GATs. The story prescribes a pattern that either will not compile or requires an
uncomfortably complex workaround. No mention of this constraint. Task 7.2 should either
provide a compilable signature or replace the lifetime reference with owned data.

---

### 8. `TreeRoot::Subtree { root_id }` rendering requires `object_phases` — synthesis rule is incomplete

Task 6.3 passes `object_phases: &HashMap<u64, ExpansionPhase>` to `render_variable_tree`.
Task 7.3 says to synthesise it as `{ id => ExpansionPhase::Expanded }` for all ids in
`snapshot.object_fields`. This is correct for fully-loaded objects, but `PinnedSnapshot`
says "freeze Loading → Collapsed." That means any object that was `Loading` at pin time
is now absent from `object_fields`. The synthesis rule then produces `Expanded` for
everything that is present — fine. But what does the renderer do with a field whose
`FieldValue::ObjectRef` points to an id NOT in `object_phases`? The existing
`stack_view.rs` rendering presumably treats missing ids as `Collapsed`. This fallback is
never stated in the story. If the existing renderer instead panics or renders incorrectly
for absent ids, the snapshot rendering will be silently broken. The story must state the
expected renderer behavior for ids absent from `object_phases`.

---

### 9. Focus state machine diagram is incomplete — `ThreadList → Favorites` is missing

The ASCII diagram in Dev Notes shows:

```
ThreadList ←──Enter──→ StackFrames
     ↕                      ↕
     └──────── F key ──→ Favorites ←────┘
```

But Task 5.3 explicitly states that `FocusFavorites` (the `F` key) should also trigger a
focus transition from the `ThreadList` panel — "même transition focus que 5.1." The diagram
shows no arrow from `ThreadList` to `Favorites` directly. A developer using the diagram as
the authoritative reference will omit that transition. Text and diagram contradict each other.

---

### 10. `push_search_char` method prescribed in Task 5.2 does not exist

Task 5.2 says:

> `self.thread_list.push_search_char('f')` for `ToggleFavorite` [...] `push_search_char(&mut self, c: char)` à créer sur `ThreadListState` si absente

Searching `thread_list.rs`, the existing mechanism is `apply_filter(&str)` — the caller
builds the string, not `ThreadListState`. There is no `push_search_char` method. The story
invents a new API without specifying where it sits in the file, what its full implementation
should be, or whether it should be tested. More importantly, `apply_filter` already does
filtering by calling into the list state; a separate `push_search_char` would duplicate
the filter-mutation responsibility. The story should either instruct the developer to use
the existing `apply_filter` pattern (build the string in the handler, as done for
`SearchChar` at line 153 of `app.rs`) or provide a complete rationale for introducing a
new method with different semantics.

---

### 11. `pinned_cursor` clamping on unpin leaves it at 0 when list becomes empty — behavior is inconsistent

Task 4.7 (toggle_pin) specifies:

> Si présent → retirer + clamp `pinned_cursor`:
> `pinned_cursor = pinned_cursor.min(pinned.len().saturating_sub(1))`

When the last item is removed, `pinned.len()` is 0 after removal, so
`saturating_sub(1)` returns 0 and `pinned_cursor` is set to 0. The Dev Notes code block
for the `Focus::Favorites` unpin handler then does:

```rust
let sel = if self.pinned.is_empty() { None } else { Some(self.pinned_cursor) };
self.favorites_list_state.select(sel);
```

That `None` branch is handled. However Task 4.6 says "`ListState` est utilisé uniquement
pour le scroll ratatui (synchroniser via `list_state.select(Some(pinned_cursor))`)." The
`None` case in Dev Notes contradicts Task 4.6's instruction to always use `Some`. Both
instructions are given to the developer; following one violates the other. The document
must pick one and delete the other.

---

### 12. AC #6 toggle-detection uses "même chemin dans l'arbre" — `PinKey` cannot detect position changes after re-render

AC #6 states that pressing `f` on an already-pinned position unpins it. Detection is done
by `PinKey` equality. `PinKey::Field` includes `var_idx: usize` and `field_path: Vec<usize>`.
If the variable list for a frame changes (e.g., the engine returns a different set of
variables after an async load), `var_idx` 2 today is not the same variable as `var_idx` 2
tomorrow. The story presents this as reliable identity, which it is not — `var_idx` is a
positional index, not a stable semantic id. This is a known limitation that ought to be
acknowledged in the story, or the identity model must be changed to use stable ids (e.g.,
variable name + type). Silently using positional indices as identity will produce incorrect
unpin behavior in edge cases the story does not even acknowledge exist.

---

### 13. `render_variable_tree` signature accepts `object_phases` but the snapshot has none — Task 6.3 passes wrong type

Task 6.3 defines:

```rust
pub(crate) fn render_variable_tree(
    root: TreeRoot<'_>,
    object_fields: &HashMap<u64, Vec<FieldInfo>>,
    collection_chunks: &HashMap<u64, CollectionChunks>,
    object_phases: &HashMap<u64, ExpansionPhase>,
) -> Vec<ListItem<'static>>
```

When called from `FavoritesPanel` (Task 7.3), `object_phases` must be synthesised on the
fly from `snapshot.object_fields`. That synthesis produces a temporary `HashMap` inside
the render call. Passing a reference to a locally-constructed map is legal, but it means
the caller must build this map every frame for every pinned item. For 500 objects across
N pinned items this is non-trivial allocation per render tick. There is no acknowledgment
of this overhead nor any suggestion to cache it. This is not fatal, but it contradicts the
project's stated performance sensitivity (Epic 8).

---

### Summary

The document has multiple errors that will cause compile failures (issues 1, 2, 7, 10),
multiple omissions that will cause incorrect runtime behavior (issues 3, 5, 8, 12), and
several internal contradictions that will produce developer confusion (issues 4, 6, 9, 11,
13). It is not ready for development in its current state.
