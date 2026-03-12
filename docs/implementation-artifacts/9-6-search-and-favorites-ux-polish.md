# Story 9.6: Search & Favorites UX Polish

Status: review

## Story

As a user,
I want to select a thread after searching without canceling the search first, to navigate from a
pinned item directly to its object, to unpin from the favorites panel directly, and to toggle
object ID display on any object,
so that the search, favorites, and inspection workflows are friction-free.

## Acceptance Criteria

1. **AC1 – Select thread while search is active:**
   Given the user has typed a search query and the filtered thread list is shown,
   When the user presses Enter (or ArrowDown/Up moves the selection and the user then presses
   Enter),
   Then the thread is focused and the stack frame view opens — the search bar remains visible but
   focus moves to the stack frames panel.
   The existing progressive Esc behavior in StackFrames is preserved:
   Esc collapses active collections → then exits expansions → then returns to ThreadList.
   When Esc results in returning to ThreadList (the final step of the existing Esc chain),
   the filter remains active — this story adds only filter-preservation to the existing flow,
   not a new Esc step.
   When the user then presses Esc from the thread list while the filter is active,
   Then the filter is cleared and the full thread list is restored.

2. **AC2 – Navigate from favorites to object (`g`):**
   Given the user is in the favorites panel and has a pinned item selected,
   When the user presses `g`,
   Then the tool navigates to the correct thread in the stack frame view and attempts best-effort
   frame positioning — focus moves to the stack frames panel.
   If the source thread is no longer found, a warning is shown and no navigation occurs.
   Note: cursor positioning at the exact frame is best-effort; if the frame cannot be matched,
   the cursor lands at the top of the stack.

3. **AC3 – Unpin directly from favorites panel (`f`):**
   Given the user is in the favorites panel and has a pinned item selected,
   When the user presses `f`,
   Then the item is unpinned and removed from the favorites panel without any navigation to the
   stack frame view.
   When the last item is unpinned and the panel becomes empty:
   - If a stack is already loaded (`stack_state.is_some()`) → focus moves to StackFrames.
   - Otherwise → focus moves to ThreadList.
   *(Note: core unpin is already implemented as of 9.5. Tasks here are focus-on-empty behaviour
   and test coverage.)*

4. **AC4 – Toggle object ID display (`i`):**
   Given any expanded object node in the stack frame view,
   When the user presses `i`,
   Then the hex object ID is shown/hidden inline after the class name (e.g. `MyClass @ 0x1A2B3C`
   when on, `MyClass` when off) — the toggle applies globally to all nodes in the current render,
   not just the selected one.
   Note: `i` is scoped to the stack frames panel. Pressing `i` in any other panel is a no-op.
   The key binding is reserved globally in `input.rs` (no other panel may use `i` for another
   purpose until this is revisited).
   Note: `show_object_ids` affects only the stack frames render. Object IDs are never shown in
   the favorites panel, regardless of toggle state.
   Note: `show_object_ids` is an in-session toggle only — it is not persisted to configuration.

## Tasks / Subtasks

- [x] **Task 1 – Two-stage search Esc (AC1)**
  - [x] 1.1 Before implementing, grep the **entire codebase** (not just tests) for all call
        sites of `activate_search` and `deactivate_search` to identify every caller whose
        behaviour will change: `grep -rn "activate_search\|deactivate_search" crates/`.
        Update all affected call sites and tests as part of this task.
  - [x] 1.2 Verify that `SearchBackspace` handling in `handle_thread_list_input` uses
        `String::pop()` on the filter string, NOT `truncate(filter.len() - 1)`. The latter
        corrupts multi-byte UTF-8 characters.
  - [x] 1.3 Make `activate_search()` idempotent: if `search_active` is already `true`, no-op.
        Do NOT reset `filter` on re-activation — the input box must pre-populate with the
        existing filter so the user can refine rather than retype.
        File: `crates/hprof-tui/src/views/thread_list.rs`
  - [x] 1.4 Change `ThreadListState::deactivate_search()` to preserve the active filter (remove
        the implicit `apply_filter("")` call it currently makes).
        File: `crates/hprof-tui/src/views/thread_list.rs`
  - [x] 1.5 Add `pub fn clear_filter(&mut self)` to `ThreadListState` that calls
        `apply_filter("")`. Verify that `apply_filter("")` calls `sync_or_select_first()` so
        the cursor lands on the first available thread when the full list re-appears.
  - [x] 1.6 In `handle_thread_list_input()`, handle `InputEvent::Escape` in this exact order
        (add inline comment `// ORDER MATTERS: deactivate before clear`):
        a. `is_search_active()` → `deactivate_search()` (exit input mode, keep filter)
        b. `!filter().is_empty()` → `clear_filter()`
        c. Otherwise → no-op
        File: `crates/hprof-tui/src/app/mod.rs`
  - [x] 1.7 Extract the "open stack for selected thread" logic from the non-search Enter handler
        (app.rs:368-373) into a private method `open_stack_for_selected_thread(&mut self, serial: u32)`.
        The method takes `serial: u32` explicitly — it does NOT read thread list selection
        state internally, so it can be safely called from any context (Enter flow, `g` flow).
        Update the original call site at app.rs:368-373 to call the new method (pass the
        currently-selected serial). Then handle `InputEvent::Enter` when `is_search_active()`
        → `deactivate_search()` then call `open_stack_for_selected_thread(selected_serial)`.
        No code duplication.
        File: `crates/hprof-tui/src/app/mod.rs`
  - [x] 1.8 Update help panel to document the two-stage Esc behaviour.

- [x] **Task 2 – `g` navigate-to-source in favorites (AC2)**
  - [x] 2.1 Add `InputEvent::NavigateToSource` variant in `crates/hprof-tui/src/input.rs`.
        Before binding `KeyCode::Char('g')`, verify it is not already mapped in `from_key()`.
        If `g` is taken, use `KeyCode::Char('G')` (Shift+G) and update help panel accordingly.
  - [x] 2.2 In `handle_favorites_input()`, handle `InputEvent::NavigateToSource`:
        a. Guard: `if self.pinned.is_empty() { return; }`.
        b. Retrieve item via `.get(idx)` (not `[idx]`); return early if `None`.
        c. Extract `thread_name` from the item's `PinKey`.
        d. Search `self.engine.list_threads()` for threads whose `name == thread_name`:
           - Zero matches → emit status bar warning `"Thread '{name}' no longer found"`, return.
           - Multiple matches → use first match, emit warning
             `"Multiple threads named '{name}' — navigated to first match"`.
           - One match → proceed silently.
        e. **Requires Task 1.7 to be complete first.**
           Call `open_stack_for_selected_thread(matched_serial)` with the serial found in step d.
        f. Best-effort frame positioning: scan `stack_state.flat_items()` for a frame whose
           `frame_id` matches the PinKey's `frame_id`; if found, move cursor there; else leave
           at top. Before implementing, verify that `frame_id` in `PinKey` is the same field
           as what `flat_items()` exposes — document the outcome in Completion Notes.
        File: `crates/hprof-tui/src/app/mod.rs`
  - [x] 2.3 Update help panel to list `g` / `G` as "Go to source" when focus is Favorites.

- [x] **Task 3 – Fix AC3 focus-on-empty behaviour and add regression tests**
  - [x] 3.1 Verify that `f` in favorites panel unpins without navigating to the stack view
        (currently implemented at `app/mod.rs:242-251`).
  - [x] 3.2 Update the empty-panel focus logic at `app/mod.rs:242-251`: when the last item is
        unpinned, set focus to `Focus::StackFrames` if `self.stack_state.is_some()`, else
        `Focus::ThreadList`.
        Rationale: when a stack is already loaded, returning to the thread list is disruptive —
        the user's natural next action is to continue inspecting the stack. ThreadList is only
        the right destination when no stack context exists.
  - [x] 3.3 Add parametric test `favorites_f_last_item_empty_panel_focus` covering both
        branches: `stack_loaded = true` → focus StackFrames; `stack_loaded = false` → focus
        ThreadList.

- [x] **Task 4 – Object ID toggle (AC4)**
  - [x] 4.1 Add `show_object_ids: bool` field (default `false`) to `App` struct.
        File: `crates/hprof-tui/src/app/mod.rs`
  - [x] 4.2 Add `InputEvent::ToggleObjectIds` variant; bind `KeyCode::Char('i')` in `input.rs`.
  - [x] 4.3 Handle `ToggleObjectIds` in `handle_stack_frames_input()` only — no-op in other
        focus contexts to avoid invisible state changes.
  - [x] 4.4 Add `show_object_ids: bool` to `RenderCtx` (`tree_render.rs:33`) and thread it
        through `render_variable_tree()`'s signature.
        Pass `app.show_object_ids` when calling from `stack_view/mod.rs`.
        Pass `false` (hardcoded) when calling from `favorites_panel.rs` — object IDs are never
        shown in the favorites panel regardless of toggle state (see AC4 scope note).
  - [x] 4.5 In `format_object_ref_collapsed()` (`stack_view/format.rs:70-105`), when
        `show_object_ids && id != 0`, append ` @ 0x{id:X}` after the class name in a dim style.
        Never display `@ 0x0` (null ref). All callers must pass the flag.
  - [x] 4.6 Update help panel to list `i` as "Toggle object IDs".

- [x] **Task 5 – Tests (TDD)**
  - [x] 5.1 `thread_list_esc_in_search_mode_preserves_filter`
  - [x] 5.2 `thread_list_second_esc_clears_filter`
  - [x] 5.3 `thread_list_esc_routing_does_not_clear_filter_from_other_focus`
        — **App-level integration test** in `app/tests.rs`: set `app.focus = Focus::StackFrames`,
        inject Esc event via `app.handle_input()`, assert `thread_list.filter()` is unchanged.
  - [x] 5.4 `thread_list_search_bar_visible_when_filter_active_not_in_input_mode`
        — State predicate test only: assert `!state.filter().is_empty() && !state.is_search_active()`
        after `deactivate_search()`. Do NOT attempt ratatui rendering in this test.
  - [x] 5.5 `thread_list_reopen_search_preserves_existing_filter_in_input`
  - [x] 5.6 `thread_list_clear_filter_on_empty_result_syncs_cursor`
  - [x] 5.7 `thread_list_enter_in_search_mode_deactivates_input_keeps_filter`
  - [x] 5.8 `favorites_navigate_to_source_empty_list_no_panic`
  - [x] 5.9 `favorites_navigate_to_source_zero_match_emits_warning`
        — Assert `app.transient_message.as_deref() == Some("Thread '...' no longer found")`
        after injecting a `NavigateToSource` event with a pinned item whose `thread_name`
        matches no thread in the engine stub.
  - [x] 5.10 `favorites_navigate_to_source_selects_correct_thread`
  - [x] 5.11 `favorites_navigate_to_source_warns_on_duplicate_thread_name`
  - [x] 5.12 `favorites_f_last_item_empty_panel_focus` (parametric over `stack_loaded`)
  - [x] 5.13 `toggle_object_ids_noop_outside_stack_frames_focus`
  - [x] 5.14 `render_object_ref_id_toggle` (parametric: `show=true` / `show=false`)
  - [x] 5.15 `render_object_ref_null_id_never_shows_address`
  - [x] 5.16 `favorites_navigate_to_source_frame_positioning_found` — with a stub stack whose
        `flat_items()` contains a frame matching the PinKey's `frame_id`, assert cursor lands
        on that frame's index (not at 0).
  - [x] 5.17 `esc_from_stack_frames_to_thread_list_preserves_filter` — App-level test:
        set committed filter, set `focus = StackFrames`, simulate the final Esc (go-to-threads)
        step in `handle_stack_frames_input`, assert `thread_list.filter()` is unchanged.

- [x] **Task 6 – Validation**
  - [x] `cargo test --all` (includes pre-existing thread_list tests from stories 3.x / 7.x)
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo fmt -- --check`
  - [x] Manual smoke: search threads → press Enter → filter stays visible → Esc clears filter
  - [x] Manual smoke: pin an item → favorites → `g` → correct thread/frame opens
  - [x] Manual smoke: expand object → `i` → object ID appears (dim); `i` again → ID hidden

## Definition of Done

Story is complete when all Task 1–6 checkboxes are ticked, all 17 tests in Task 5 pass
(including all parametric cases), clippy and fmt report zero issues, and the three manual smoke
checks succeed. Set status to `review` and open a code-review pass.

## Dev Notes

### AC1 – Two-stage search state machine

`ThreadListState` currently has a single `search_active: bool` at
`crates/hprof-tui/src/views/thread_list.rs:33`. There is no separation between "user is typing"
and "filter is active but input mode is off".

New state semantics:
- `search_active = true` → user is currently typing; keyboard captured by search input box.
- `search_active = false && !filter.is_empty()` → filter is committed but not in input mode; the
  search bar is still rendered (see `thread_list.rs:196-212`) so the user sees the active filter.
- `search_active = false && filter.is_empty()` → no filter, search bar hidden.

Key change: `deactivate_search()` **must not** call `apply_filter("")`. Add a separate
`clear_filter()` for explicit clearing.

**This two-stage Esc is intentional per FR46 — do not "fix" it to a single Esc.** The design
allows the user to open the stack view while keeping the thread filter active, which is the
primary UX goal of this story.

Escape routing in `handle_thread_list_input()` (app.rs:274-385):
```
InputEvent::Escape if is_search_active() => deactivate_search(),            // exits input mode only
InputEvent::Escape if !filter().is_empty() => clear_filter(),               // clears committed filter
InputEvent::Escape => { /* no active filter, no-op */ }
```

**Critical guard:** this handler is only reached when `focus == Focus::ThreadList`. Esc pressed
while focus is on StackFrames or Favorites is routed to their own handlers and **never** reaches
`handle_thread_list_input()`. There is therefore no risk of clearing the filter from the wrong
focus — but the implementation must not relax this routing invariant.

**Esc in StackFrames — filter preservation only.** The existing progressive Esc behavior in
`handle_stack_frames_input()` (app.rs:419-446) is unchanged: collapse collection → exit
expansion → return to ThreadList. This story adds only one thing: when the "return to ThreadList"
branch executes, the thread list filter must be left intact (i.e., do not call `clear_filter()`
from that branch). No new Esc step is added to StackFrames.

### AC2 – Status bar warning mechanism

Warnings are emitted via a transient message field on `App`. Check whether `App` already has a
`transient_message: Option<String>` (or equivalent) field used in `status_bar.rs` rendering.
If it exists, set `self.transient_message = Some("...")` and it will be shown for one render
cycle. If it does not exist, add `transient_message: Option<String>` to `App` and render it in
`status_bar.rs` alongside the existing `warning_count` display — clear it after one render by
setting back to `None` at the start of the next event loop tick.

Do **not** use the parser `warnings()` mechanism (`NavigationEngine::warnings()`) for transient
UI messages — that list is for parse-time warnings only and is never cleared.

Reference: `crates/hprof-tui/src/views/status_bar.rs`

### AC2 – Navigate to source: frame_id vs thread_serial

`PinKey` stores `thread_name: String` and `frame_id: u64` but **not** `thread_serial`
(`favorites.rs:22-40`). Navigation requires:

1. Find `thread_serial` by matching `engine.list_threads()` on `name == thread_name`.
   - Zero matches → silently no-op (thread no longer in dump).
   - Multiple matches → use first match, emit status bar warning (see Task 2.2d).
   - One match → proceed silently.
2. Select that thread and load its stack by reusing `open_stack_for_selected_thread()` (Task 1.4).
3. Best-effort cursor position: scan `stack_state.flat_items()` for a frame whose `frame_id`
   matches the PinKey's `frame_id`. If found, set cursor to that item's index.
   If not found, leave at top — do not panic or error.

There is **no need** to position at the exact var/field within the frame for this story. Frame-
level navigation is sufficient.

**Primitive and non-object snapshots:** A `PinnedSnapshot::Primitive` or any `PinKey::Var` where
the pinned item is a scalar value still stores `thread_name` + `frame_id`. Navigation to the
correct thread and frame succeeds. "Highlighting the position" for a primitive means landing on
that frame's header row — there is no object node to select. This is acceptable and matches the
best-effort contract stated in AC2.

**Note on duplicate thread names:** jvisualvm dumps can produce multiple threads with the same
display name (e.g. `Thread-1` appears in many dumps). Storing `thread_serial` in `PinKey` would
eliminate the ambiguity entirely, but this requires a breaking change to the `PinKey` struct and
re-validation of the deduplication tests at `favorites.rs:434-473`. This is explicitly deferred
to a future story — the warning-on-multiple-matches approach in Task 2.2c is the safe minimum.

### AC4 – RenderCtx threading

`RenderCtx` is a private struct in `tree_render.rs:33`. It is constructed at `tree_render.rs:58`
by `render_variable_tree()` and never leaves that module. Adding `show_object_ids: bool` to it
and to `render_variable_tree()`'s parameters is the cleanest path.

`render_variable_tree()` is called from:
- `crates/hprof-tui/src/views/stack_view/mod.rs` (stack view render)
- `crates/hprof-tui/src/views/favorites_panel.rs` (pinned snapshot render)

Both callers have access to `App`'s state, so passing `app.show_object_ids` is straightforward.

`format_object_ref_collapsed()` is at `crates/hprof-tui/src/views/stack_view/format.rs:70-105`.
It currently returns `"ClassName"` or `"ClassName (N entries)"`. When `show_object_ids = true`,
the return should be `"ClassName @ 0x1A2B3C"` or `"ClassName (N entries) @ 0x1A2B3C"`.

ObjectRef ID is available everywhere a `FieldValue::ObjectRef { id, .. }` is matched — no engine
call required.

**Primary use case:** The object ID toggle exists to detect whether two references point to the
same object (same hex ID = same Java instance). It is not intended for navigating by memory
address.

**Rendering style:** Render the ` @ 0x...` suffix using dedicated semantic theme styling for
object metadata (dim/muted). Keep cyclic/self-ref rows on their own semantic theme entry as
well, even if the current visual values match.

**`show_object_ids` is an in-session toggle only.** Do NOT persist it to the TOML config file.
It resets to `false` on every launch.

**Stale IDs in favorites:** Object IDs displayed inside pinned snapshots reflect the state at
pin time, not the live heap. This is expected and correct — snapshots are frozen by design. The
IDs remain useful for identity comparison within a single session.

**Null guard:** `id == 0` means an unresolved or null reference. Never render `@ 0x0`. The guard
`if show_object_ids && id != 0` must be applied in `format_object_ref_collapsed()`. This also
applies in `FavoritesPanel` where `UnexpandedRef` already renders `@ 0x{id:X}` — verify that
path also skips id == 0.

### AC3 – Partially implemented; focus-on-empty requires update

`f` in favorites → `InputEvent::ToggleFavorite` → `handle_favorites_input()` at `app.rs:242-251`
already removes the selected item. The current code sets focus to `ThreadList` when the panel
becomes empty. Task 3.2 changes this to conditional: `StackFrames` if a stack is loaded, else
`ThreadList`. The regression test (Task 3.3) must cover both branches.

### Project Structure Notes

| File | Change |
|------|--------|
| `crates/hprof-tui/src/input.rs` | Add `NavigateToSource`, `ToggleObjectIds` to `InputEvent` |
| `crates/hprof-tui/src/app/mod.rs` | Two-stage Esc, Enter-in-search, `g` handler, `i` toggle, `show_object_ids` field |
| `crates/hprof-tui/src/app/tests.rs` | New tests for AC1, AC2, AC3, AC4 |
| `crates/hprof-tui/src/views/thread_list.rs` | `deactivate_search` no longer clears filter; add `clear_filter()` |
| `crates/hprof-tui/src/views/tree_render.rs` | Add `show_object_ids` to `RenderCtx` and `render_variable_tree()` |
| `crates/hprof-tui/src/views/stack_view/format.rs` | `format_object_ref_collapsed` shows ID when flag is set |
| `crates/hprof-tui/src/views/stack_view/mod.rs` | Pass `show_object_ids` to `render_variable_tree` |
| `crates/hprof-tui/src/views/favorites_panel.rs` | Pass `false` for `show_object_ids` to `render_variable_tree` |
| `crates/hprof-tui/src/views/help_panel.rs` (or equivalent) | Update keymap entries |
| `crates/hprof-tui/src/views/status_bar.rs` | Render `transient_message` if present; clear after one tick |

### References

- `docs/planning-artifacts/epics.md` (Story 9.6, FR46–FR49)
- `docs/planning-artifacts/architecture.md` (Frontend Architecture, FR10 Note)
- `docs/implementation-artifacts/9-5-static-fields-in-stack-view.md` (RenderCtx refactor)
- `crates/hprof-tui/src/views/thread_list.rs` (search state machine)
- `crates/hprof-tui/src/app/mod.rs:274-385` (`handle_thread_list_input`)
- `crates/hprof-tui/src/app/mod.rs:229-260` (`handle_favorites_input`)
- `crates/hprof-tui/src/favorites.rs:22-40` (`PinKey` enum)
- `crates/hprof-tui/src/views/tree_render.rs:33-40` (`RenderCtx`)
- `crates/hprof-tui/src/views/stack_view/format.rs:70-105` (`format_object_ref_collapsed`)
- `crates/hprof-tui/src/views/status_bar.rs` (transient message / warning display)

## Dev Agent Record

### Agent Model Used

openai/gpt-5.3-codex

### Debug Log References

- `cargo test -p hprof-tui --lib`
- `cargo test --all`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`

### Completion Notes List

- Implemented AC1 two-stage Esc flow in thread list: first Esc exits search input while keeping filter, second Esc clears committed filter via `clear_filter()`; Enter in active search now opens stack while preserving filter.
- Added `open_stack_for_selected_thread(&mut self, serial: u32)` and reused it in both normal Enter flow and favorites `g` navigation flow to remove duplication.
- Added favorites `g` navigation (`NavigateToSource`) with guarded `.get(idx)` access, thread name lookup, duplicate-name warning, missing-thread warning, and best-effort frame positioning by matching `PinKey.frame_id` against `StackState::flat_items()` frame cursors.
- Updated favorites unpin-empty behavior to route focus to `StackFrames` when a stack is loaded, otherwise `ThreadList`.
- Implemented AC4 toggle: `show_object_ids` app flag, `ToggleObjectIds` input (`i`) scoped to stack-frames focus only, render-context threading with `show_object_ids` in `tree_render`, and favorites hardcoded to `false` for object IDs.
- Added object-id formatting tests and null guard (`id == 0` never renders `@ 0x0`), plus favorites unexpanded-ref null guard.
- Added/updated tests for all Task 5 cases (AC1/AC2/AC3/AC4), including search Esc behavior, search reopen persistence, favorites navigate-to-source variants, frame positioning, and object-id toggle scope.
- `PinKey.frame_id`/`StackState::flat_items()` compatibility check: frame matching uses `StackCursor::OnFrame(frame_idx)` then resolves `StackState::frames()[frame_idx].frame_id`; this is the same frame ID domain used in `PinKey` variants.
- Follow-up polish from manual QA: `g` now synchronizes ThreadList selection with navigated thread and performs best-effort cursor positioning to the pinned `PinKey` location (`Frame`/`Var`/`Field`) by expanding required path objects synchronously.
- Fixed deep object-id rendering scope: `show_object_ids` now applies beyond first-level locals to nested object-field rows in stack tree rendering (still disabled in favorites).
- Added dedicated theme roles `object_id_hint` and `cyclic_ref` to clarify styling intent for object metadata vs null/cyclic contexts.

### File List

- `crates/hprof-tui/src/app/mod.rs`
- `crates/hprof-tui/src/app/tests.rs`
- `crates/hprof-tui/src/input.rs`
- `crates/hprof-tui/src/views/favorites_panel.rs`
- `crates/hprof-tui/src/views/help_bar.rs`
- `crates/hprof-tui/src/views/stack_view/format.rs`
- `crates/hprof-tui/src/views/stack_view/state.rs`
- `crates/hprof-tui/src/views/stack_view/widget.rs`
- `crates/hprof-tui/src/views/thread_list.rs`
- `crates/hprof-tui/src/views/tree_render.rs`
- `docs/implementation-artifacts/9-6-search-and-favorites-ux-polish.md`
- `docs/implementation-artifacts/sprint-status.yaml`
