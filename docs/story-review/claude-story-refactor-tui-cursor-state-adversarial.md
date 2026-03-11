# Adversarial Review: tech-spec-refactor-tui-cursor-state.md

**Reviewed:** 2026-03-11
**Spec:** `docs/implementation-artifacts/tech-spec-refactor-tui-cursor-state.md`
**Sources read:** `thread_list.rs`, `stack_view/state.rs`, `favorites_panel.rs`,
`app/mod.rs`, `stack_view/widget.rs`, `stack_view/tests.rs`

---

## Findings

### 1. sync_list_state has 4 callsites, not 3 — Task 3 will silently miss one

**Severity: Critical | Validity: Real**

The spec (Task 3, "Supprimer `sync_list_state` — remplacer ses 3 callsites") claims
`sync_list_state` is called at three places:
`set_expansion_failed`, `resync_cursor_after_collapse`, `toggle_expand`.

Reality: there are **4** callsites (lines 102, 412, 498, 540):
- line 102 — `set_cursor`
- line 412 — `set_expansion_failed`
- line 498 — `resync_cursor_after_collapse`
- line 540 — `toggle_expand`

`set_cursor` is listed separately in Task 3 with the instruction
`self.nav = CursorState::new(c)` + `nav.sync(...)`, so it does get addressed,
but the spec's own claim of "3 callsites" is wrong. A developer following the
task verbatim will look for three, find the three named ones, delete
`sync_list_state`, and the compiler will catch the missed fourth — but only if
`sync_list_state` is deleted. If the developer forgets the delete step and
only adds the three replacements, the stale method stays and is never called
by anyone, meaning `set_cursor` now creates a fresh `CursorState` that is never
synced. This is exactly the kind of off-by-one that silently produces wrong
`list_state` indices.

---

### 2. `StackState::new` uses `StackCursor::NoFrames` — `CursorState::new` requires an initial Id, breaking the empty-frames case

**Severity: Critical | Validity: Real**

`CursorState<Id>` is defined as `new(initial: Id) -> Self`. For `StackState`,
`Id = StackCursor`. When `frames.is_empty()` the current code sets
`cursor = StackCursor::NoFrames` and does not select anything in `list_state`.

After migration, `StackState::new` must call `CursorState::new(...)` with some
`StackCursor`. There is no valid non-orphan `StackCursor` for the empty case.
The spec does not address this. The developer has two bad options:
- `CursorState::new(StackCursor::NoFrames)` — compiles, but `NoFrames` is never
  in `flat_items()`, so every navigation call sees an orphan cursor from the
  start, and the `list_state` index is always `None`.
- Add a branch that conditionally constructs a `CursorState` — requires making
  `StackState.nav` an `Option<CursorState<StackCursor>>`, a bigger API change
  not mentioned anywhere.

The existing code has clean semantics for the empty case. The spec's
`CursorState::new(initial: Id)` API cannot represent "no cursor". This is a
design gap that will cause a regression for threads with zero stack frames.

---

### 3. `selected_serial()` post-migration contract is semantically broken for the orphan cursor case

**Severity: High | Validity: Real**

The spec says (Task 2):

> `selected_serial()` : `if filtered_serials.is_empty() { None } else { Some(*self.nav.cursor()) }`

With `CursorState<u32>` the cursor always holds a `u32`. After `apply_filter`
with zero results (AC-8c case), the spec says:
> "Sinon (vide) : ne pas réinitialiser `nav` (cursor orphelin, `selected_serial()` → `None`)"

The guard `if filtered_serials.is_empty() { None }` does return `None` —
correct for the empty-list case. But consider: filter changes from "worker"
(serials 2,3 visible) to "xyz" (empty), then back to "" (all 3 visible). The
cursor is still holding the last serial (say 2), which is now valid again.
`apply_filter` sequence 2 (`sync` if cursor ∈ filtered_serials) handles this
correctly.

However, the render path in `thread_list.rs` (line 247-267) accesses
`state.filtered_serials` directly — a `pub(super)` field. After migration
`filtered_serials` stays (it is not removed), but `list_state` is no longer a
direct field; it is `self.nav.list_state_mut()`. The render calls
`StatefulWidget::render(list, list_area, buf, &mut state.list_state)` (line 267).
The spec says `list_state` is replaced by the private field inside `CursorState`,
exposed only via `list_state_mut()`. The render code — which is inside
`SearchableList::render`, i.e., in the same file — still needs access. The spec
does not mention adapting the render callsite in `thread_list.rs`. The developer
will hit a compile error that is not mentioned in any task or AC.

---

### 4. Task 3 spec for `set_cursor` resets navigation history — losing visible_height

**Severity: High | Validity: Real**

Current:
```rust
pub fn set_cursor(&mut self, new_cursor: StackCursor) {
    self.cursor = new_cursor;
    self.sync_list_state();
}
```

Spec says (Task 3):
> `set_cursor(c)` : `self.nav = CursorState::new(c)` + `nav.sync(&self.flat_items())`

`CursorState::new(c)` initialises `visible_height = 1`. Calling `set_cursor`
at any point after the first render will clobber the `visible_height` that was
set by `set_visible_height` during render. After the reset, the next
`move_page_down` will jump only 1 row regardless of panel height.

`set_cursor` is called at 15+ sites in `app/mod.rs` (expansion done callbacks,
toggle-expand, collection page loads). Every one of them will silently break
page navigation until the next render call re-invokes `set_visible_height`.
Since `set_visible_height` is only called once per frame (during render), and
input is processed in the same frame loop, this will manifest as page navigation
being stuck at 1 for the rest of that frame.

The fix is `self.nav.set_cursor_value(c); self.nav.sync(...)` — i.e., changing
only the `cursor` field without touching `visible_height` — but `CursorState`
has no such method in the spec. The spec's `set_cursor` design is wrong.

---

### 5. Task 4: `App` field is `favorites_list_state`, not `favorites_state` — spec uses the wrong name

**Severity: High | Validity: Real**

The spec (Task 4) says:
> Remplacer tous les accès `self.pinned_cursor` par
> `self.favorites_state.selected_index()` ou méthodes sur `FavoritesPanelState`

The actual field in `App` is `favorites_list_state: FavoritesPanelState`
(line 104). The spec consistently references `self.favorites_state` which does
not exist. A developer following the spec literally will write code that does
not compile until they discover the correct name themselves.

---

### 6. `sync_favorites_list_state` in `App` directly writes `self.favorites_list_state.list_state.select(sel)` — not addressed by spec

**Severity: High | Validity: Real**

`App::sync_favorites_list_state` (line 217) directly accesses:
```rust
self.favorites_list_state.list_state.select(sel);
```

After Task 4 migration, `FavoritesPanelState` will hold `nav: CursorState<usize>`
and `list_state` will be inside `CursorState` (private). The spec says to
"supprimer `pinned_cursor` de `App`" and redirect accesses through
`selected_index()`, but `sync_favorites_list_state` writes to `list_state`
directly, not through a navigation method. This function also exists at 4
callsites (`toggle_pin`, `handle_favorites_input` x3). The spec does not
mention deleting or rewriting `sync_favorites_list_state`, nor does it say what
replaces the direct `list_state.select()` write. The developer has no guidance
here and will either leave stale code or make a wrong choice.

---

### 7. Tests that assign `state.cursor = ...` directly cannot be fixed with syntactic `state.cursor` → `state.cursor()` substitution

**Severity: High | Validity: Real**

The spec says for Task 3 / AC-9:
> remplacer `state.cursor` par `state.cursor()` partout (syntaxe uniquement —
> aucune logique de test modifiée)

This claim is false for at least 4 tests in `stack_view/tests.rs`:

- line 443: `state.cursor = StackCursor::OnObjectField { ... }`
- line 463: `state.cursor = StackCursor::OnObjectField { ... }`
- line 1068: `state.cursor = StackCursor::OnObjectField { ... }`
- line 1142: `state.cursor = StackCursor::OnObjectField { ... }`

These are **writes** to `state.cursor`, not reads. After migration, `cursor`
lives inside `CursorState` (private field). Replacing `state.cursor =` with
`state.cursor() =` is not valid Rust (you cannot assign to a method call
returning `&Id`). These tests will require a new setter method (e.g.,
`state.set_cursor_for_test(...)`) or the test logic must use `set_cursor()`,
which per finding #4 resets `visible_height`. The spec calls this "syntaxe
uniquement" — it is not.

Also: `cursor` is `pub(super)`, visible to sibling modules in `stack_view`. The
tests live in `stack_view/tests.rs` which is `mod tests` inside `stack_view`,
so they can access `pub(super)`. After moving `cursor` inside `CursorState`
(private), this access breaks regardless of visibility. The spec does not
acknowledge this.

---

### 8. `StackState::set_visible_height` takes `u16`, spec changes it to `usize` — existing call in `app/mod.rs` passes `u16`

**Severity: Medium | Validity: Real**

Current signature: `pub fn set_visible_height(&mut self, h: u16)`
The app calls: `ss.set_visible_height(stack_area.height.saturating_sub(2))`
where `stack_area.height` is `u16` — so `saturating_sub(2)` returns `u16`.

The spec says `CursorState` uses `visible_height: usize` (not `u16`, to "avoid
casts"). After migration, `set_visible_height(h: usize)` will require the caller
in `app/mod.rs` to add `as usize` at lines 897 and 900. The spec mentions this
only in the context of `thread_list_height` removal (line 152: `as usize`) but
not for the two stack state callsites. A developer adapting `app/mod.rs` for
Task 2 may not notice the type change until they handle Task 3, causing confusion
about whether to cast at the callsite or change the stored type.

---

### 9. `FavoritesPanel` widget struct still takes `pinned_cursor: usize` — Task 4 render callsite in `app/mod.rs` is incomplete

**Severity: Medium | Validity: Real**

The spec (Task 4) says:
> Supprimer `pinned_cursor: usize` du widget struct `FavoritesPanel`

The render callsite in `app/mod.rs` line 949:
```rust
FavoritesPanel {
    focused: fav_focused,
    pinned: &self.pinned,
    pinned_cursor: self.pinned_cursor,
},
```

The spec tells the developer to remove `pinned_cursor` from the widget struct
and from `App`. What it does not say is what the widget renderer should use
instead to decide which item is highlighted. Currently the widget uses
`pinned_cursor` to drive the visual selection. After migration, the cursor lives
in `FavoritesPanelState.nav`, but `FavoritesPanel::render` receives only
`&mut FavoritesPanelState` — the highlight logic must be rewritten to read from
`state.nav.list_state_mut()`. The spec is silent on how `FavoritesPanel::render`
should access the cursor post-migration, leaving the visual selection broken.

---

### 10. AC-5 / AC-6 / AC-6b: `move_page_down` semantics inconsistency

**Severity: Medium | Validity: Undecided**

AC-5: cursor on 0, `visible_height = 3`, items `[0..9]` → cursor becomes 3.
AC-6b: cursor on 0, `visible_height = 1` (default), items `[0,1,2]` → cursor
becomes 1.

These are consistent (advance by `visible_height`). However, the existing
`StackState::move_page_down` (line 583) does:
```rust
let target = (current + self.visible_height as usize).min(flat.len() - 1);
```
and the existing `ThreadListState::page_down(n)` does:
```rust
let next = (current + n).min(self.filtered_serials.len() - 1);
```

Both clamp at `len - 1`. The spec's CursorState should match. The ACs do
confirm this, but the spec text (Notes section) says `saturating_sub(1)` on
`len` for *up* moves, without explicitly specifying the down-clamp formula.
This is consistent but the asymmetric phrasing ("saturating_sub(1) sur la
longueur" mentioned only for length guards) could lead a developer to write
`flat.len().saturating_sub(1)` unnecessarily in the down path, making the code
slightly opaque. Low risk but worth flagging.

---

### 11. `selected_serial()` existing tests call `page_down(n)` — these will break post-migration without spec guidance

**Severity: Medium | Validity: Real**

The existing unit tests in `thread_list.rs` (lines 378-405) call:
```rust
state.page_down(3);
state.page_up(3);
state.page_up(10);
state.page_down(10);
```
with explicit `n: usize` arguments.

After migration these methods become parameterless. The tests must either be
rewritten to call `set_visible_height(n)` before `page_down()`, or the expected
values change. The spec (AC-9) only covers `stack_view/tests.rs` and describes
those changes as "syntaxe uniquement". The `thread_list.rs` tests require
**logic changes** (add `set_visible_height` calls) which the spec does not
mention at all.

---

### 12. `apply_filter` migration step 2 reads `*nav.cursor()` which may panic if filter matches but cursor is `NoFrames`-equivalent

**Severity: Low | Validity: Undecided**

For `ThreadListState`, the spec's step 2 says:
> Si `filtered_serials` contient `*nav.cursor()` : `nav.sync(&filtered_serials)`

Since `ThreadListState` uses `CursorState<u32>`, the cursor is always a valid
`u32` serial after construction (no `NoFrames` analog). However, the orphan
case (step 4: filter yields empty) leaves `nav.cursor()` pointing at a serial
that is no longer in `filtered_serials`. This is intentional per the spec.
The concern is that step 2's check `filtered_serials.contains(nav.cursor())`
performs a linear scan of `filtered_serials` — same as the current code — so
no regression. This is Undecided / Low as it's a pre-existing O(n) scan that
the spec doesn't make worse.

---

## Summary Table

| # | Severity | Validity | Short Description |
|---|----------|----------|-------------------|
| 1 | Critical | Real | sync_list_state has 4 callsites not 3; set_cursor callsite not in named list |
| 2 | Critical | Real | CursorState::new requires Id; StackCursor::NoFrames case unaddressed |
| 3 | High | Real | Render path accesses state.list_state directly — not adapted in any task |
| 4 | High | Real | set_cursor resets visible_height to 1 via CursorState::new; breaks page nav |
| 5 | High | Real | Wrong App field name: `favorites_state` vs actual `favorites_list_state` |
| 6 | High | Real | sync_favorites_list_state writes list_state directly — not addressed in spec |
| 7 | High | Real | Tests write state.cursor = ...; not a syntactic change, needs a new setter |
| 8 | Medium | Real | set_visible_height type change u16→usize not noted for stack state callsites |
| 9 | Medium | Real | FavoritesPanel render highlight logic left undefined post pinned_cursor removal |
| 10 | Medium | Undecided | page_down clamping formula description asymmetric, low risk |
| 11 | Medium | Real | thread_list tests use page_down(n); need logic changes not mentioned by spec |
| 12 | Low | Undecided | apply_filter step 2 O(n) scan — pre-existing, spec doesn't worsen it |
