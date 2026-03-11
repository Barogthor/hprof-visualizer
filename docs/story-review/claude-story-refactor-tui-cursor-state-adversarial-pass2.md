# Adversarial Review — Second Pass
## Tech-Spec: Refactor TUI Navigation — CursorState<Id> Composition
**Date:** 2026-03-11
**Reviewer:** Claude (adversarial second pass)

---

## Summary

7 findings remain after the first round of fixes. 2 are Critical (will fail to compile or
silently misbehave), 3 are High (incorrect semantics or spec-to-code mismatch), 2 are Medium
(misleading spec wording that invites misimplementation).

---

## Findings

### 1. [CRITICAL] `sync_favorites_list_state` fix is semantically wrong

**Spec instruction (Task 4, app/mod.rs):**
> Adapter `sync_favorites_list_state` : remplacer l'écriture directe
> `self.favorites_list_state.list_state.select(sel)` par
> `self.favorites_list_state.set_selected_index(sel.unwrap_or(0))`

**Actual function (app/mod.rs:217–224):**
```rust
fn sync_favorites_list_state(&mut self) {
    let sel = if self.pinned.is_empty() {
        None
    } else {
        Some(self.pinned_cursor)
    };
    self.favorites_list_state.list_state.select(sel);
}
```

The function passes `None` to `list_state.select()` when `pinned` is empty, which tells
ratatui to deselect entirely. The spec's replacement `set_selected_index(sel.unwrap_or(0))`
would call `set_selected_index(0)` when `pinned` is empty — which selects index 0 on an
empty list. This is semantically different and can cause a visual artifact (stale highlight).

`set_selected_index` must either accept `Option<usize>` to preserve the `None`/deselect
path, or the callsite must remain a conditional:
```rust
if self.pinned.is_empty() {
    // leave nav cursor as-is; ratatui must show no selection
} else {
    self.favorites_list_state.set_selected_index(self.pinned_cursor);
}
```
The spec does not acknowledge this distinction. Any developer following the spec verbatim
produces a regression in the empty-list case.

---

### 2. [CRITICAL] `set_visible_height` type mismatch in `StackState` migration

**Spec (Task 3, state.rs):**
> `set_visible_height(h: usize)` → `self.nav.set_visible_height(h as usize)`
> (cast depuis `u16` : `stack_area.height.saturating_sub(2)` est déjà `u16` dans
> `app/mod.rs` lignes 897, 900 — le cast `as usize` est fait dans l'appelant)

**Actual callers (app/mod.rs:897, 900):**
```rust
ss.set_visible_height(stack_area.height.saturating_sub(2));
self.preview_stack_state.set_visible_height(stack_area.height.saturating_sub(2));
```

**Actual current signature (state.rs:572):**
```rust
pub fn set_visible_height(&mut self, h: u16) {
    self.visible_height = h;
}
```

The callers pass `u16` (result of `saturating_sub(2)` on a `u16`). The spec says
`CursorState::set_visible_height` takes `usize`, and says "le cast `as usize` est fait
dans l'appelant". But the callers in `app/mod.rs` are not shown being updated with
`as usize`. The spec's Task 2 / Task 3 do not include an instruction to add `as usize` at
lines 897 and 900. The developer will get a type error at both callers unless they add
`stack_area.height.saturating_sub(2) as usize` themselves — which is not stated.

This is a compile error: the spec omits the callsite cast update for the StackState
`set_visible_height` callers in `app/mod.rs`.

---

### 3. [HIGH] `SearchableList::render` fix — `state.nav` is a private field accessible only within the module, but the spec's framing implies it requires an accessor

**Spec (Task 2, thread_list.rs):**
> Adapter `SearchableList::render` : `state.list_state` (ligne 267) →
> `state.nav.list_state_mut()` ; `state.filtered_serials` reste accessible
> (champ public conservé — uniquement `list_state` migre dans `nav`)

The render impl is in the same file as `ThreadListState` (both in `thread_list.rs`).
`SearchableList::render` already accesses private fields `state.filtered_serials` and
`state.list_state` directly (lines 247, 267). After migration, `state.nav` would also be
a private field in the same module, so `state.nav.list_state_mut()` compiles fine.

**However**, the spec says `filtered_serials` "reste accessible (champ public conservé)".
This is factually wrong: `filtered_serials` is currently a private field (no `pub` in the
struct at line 28). It is accessible in render only because render is in the same module.
The spec's claim that it is "public" is a false rationale that could mislead the developer
into thinking the visibility rules differ for `filtered_serials` vs `nav`. The actual reason
both work is same-module access, not pub visibility.

---

### 4. [HIGH] `StackCursor::NoFrames` orphan — the `resync_cursor_after_collapse` codepath sets `self.cursor` directly without going through `set_cursor_and_sync`

**Spec (Task 3, state.rs):**
> `resync_cursor_after_collapse` → `self.nav.sync(&self.flat_items())`

But `resync_cursor_after_collapse` (state.rs:449–498) assigns `self.cursor` directly in
multiple places (e.g. `self.cursor = fallback;`, `self.cursor = StackCursor::OnFrame(...)`)
before calling `sync_list_state()`. After migration, these direct assignments to
`self.cursor` are gone — `cursor` lives inside `self.nav`. The spec correctly says to call
`self.nav.sync(&self.flat_items())` at the end, but it does not address how to replace the
intermediate `self.cursor = fallback` assignments within the method body.

The developer must replace those with `self.nav.set_cursor_and_sync(fallback, &flat)` or
with `self.nav.cursor = fallback` (if cursor is made accessible). The spec says nothing
about how to migrate these intermediate direct assignments — only the final
`sync_list_state()` call is mentioned. This is an omission that will cause a compile error
(`self.nav.cursor` is private).

The same issue exists inside `set_expansion_failed` (state.rs:402–412) where `self.cursor`
is assigned directly before `sync_list_state()`. The spec lists this as simply
`self.nav.sync(&self.flat_items())` but does not explain how to replace the direct cursor
mutations inside the method.

---

### 5. [HIGH] AC-10 references `FavoritesPanelState::new()` which does not exist

**AC-10:**
> Given `FavoritesPanelState::new()` avec cursor initial 0 ...

`FavoritesPanelState` derives `Default` (favorites_panel.rs:35) and has no `new()` method.
The test must use `FavoritesPanelState::default()`. After migration, a `new()` is not
specified in the Task 4 implementation instructions either. The AC cannot be verified
as written — the method call would not compile.

---

### 6. [MEDIUM] `state.cursor = X` migration in tests.rs: 4 sites vs actual count

**Spec (Task 3, tests.rs):**
> Remplacer `state.cursor = SomeVariant` (assignation directe, 4 sites) par
> `state.set_cursor(SomeVariant)`

Actual direct assignments in `tests.rs`: lines 443, 463, 1068, 1142 — that is indeed 4
sites. However, there are also many read-only accesses (`assert_eq!(state.cursor, ...)`)
at lines 37, 45, 53, 65, 82, 102, 111, 114, 117, 207, 213, 222, 250, 256, 335, 343, 368,
443, 463, 892, 898, 904, 911, 917, 1068, 1082, 1084, 1089, 1096, 1102, 1142, 1162, 1164,
1235, 1237, 1249, 1251, 1260, 1272, 1484 — all of which also need to become `state.cursor()`.

The spec says "lectures `state.cursor` → `state.cursor()` partout" (AC-9 bullet 1), so it
does acknowledge the reads. But Task 3's action description only explicitly calls out the 4
write sites, and the read migration is only mentioned in the AC. A developer reading Task 3
linearly could miss the ~40 read-site changes. The count is accurate but the description
is incomplete relative to the scope of work.

---

### 7. [MEDIUM] `toggle_pin` adds to `pinned` without calling `sync_favorites_list_state` on the add path

**app/mod.rs:207–215:**
```rust
fn toggle_pin(&mut self, item: PinnedItem) {
    if let Some(pos) = self.pinned.iter().position(|p| p.key == item.key) {
        self.pinned.remove(pos);
        self.pinned_cursor = self.pinned_cursor.min(self.pinned.len().saturating_sub(1));
    } else {
        self.pinned.push(item);    // <— no sync here
    }
    self.sync_favorites_list_state();  // called after both branches
}
```

The call at line 214 is after the if/else, so it covers the add path. This is correct in
the current code. **However**, the spec's instruction for Task 4 says to replace
`self.pinned_cursor = ...` mutations with `set_selected_index(...)`. The spec says:
> Remplacer toutes les mutations `self.pinned_cursor = ...` par
> `self.favorites_list_state.set_selected_index(...)`

The mutation on line 210 (`self.pinned_cursor = self.pinned_cursor.min(...)`) is followed
immediately by `sync_favorites_list_state()`. After migration, the developer must call
`set_selected_index(new_idx)` instead, which — per the spec — presumably also updates the
underlying `ListState`. But `sync_favorites_list_state()` is still called at line 214 after
both branches. If `set_selected_index` updates `ListState` internally, calling
`sync_favorites_list_state()` again would overwrite that update with `self.pinned_cursor`
— which no longer exists.

The spec does not say `sync_favorites_list_state` should be deleted as part of Task 4. If
it is kept, it will conflict with `set_selected_index`. If it is deleted, the spec must say
so explicitly. This is an unresolved inconsistency.

---

## Non-Findings (verified clean)

- `set_cursor_and_sync` is consistently described in Task 1 and Task 3, and its usage in
  `StackState::set_cursor` is coherent. The method definition and the single callsite align.
- `NoFrames` is checked in `app/mod.rs:561` via a match arm (`| StackCursor::NoFrames =>
  return None`). After migration, `CursorState::new(StackCursor::NoFrames)` makes
  `NoFrames` the stored cursor. The match in `app/mod.rs` pattern-matches on the value
  returned by `state.cursor()` — this remains valid since `NoFrames` is still a variant of
  `StackCursor`. No breakage.
- The 4 callsites of `page_down(h)` / `page_up(h)` at lines 306, 311, 352, 357 are all
  correctly identified.
- `widget.rs:38` — `state.list_state` is currently accessed directly; `state.list_state_mut()`
  will compile because `widget.rs` and `state.rs` are in the same `stack_view` module and
  `list_state_mut()` will be a public method on `CursorState`.
