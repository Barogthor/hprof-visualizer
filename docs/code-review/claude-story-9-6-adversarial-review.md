# Code Review — Story 9.6: Search & Favorites UX Polish

**Date:** 2026-03-12
**Story status at review:** `review`
**Branch:** `feature/epic-9-navigation-data-fidelity`
**Agent:** Amelia (Dev Agent) — adversarial review
**Tests:** 309 passed, 0 failed (`cargo test -p hprof-tui --lib`)

---

## Git vs Story Discrepancies

Git shows no uncommitted tracked changes — all story files are committed.
Story File List matches git state. **0 discrepancies.**

---

## 🔴 HIGH ISSUES

### H1 — AC4 partial: collection entry `ObjectRef` rows ignore `show_object_ids`

**File:** `crates/hprof-tui/src/views/tree_render.rs:746`
**File:** `crates/hprof-tui/src/views/stack_view/format.rs:141-174`

AC4 states: *"the toggle applies globally to all nodes in the current render"*. However,
`format_entry_line` (called at `tree_render.rs:746`) delegates to `format_entry_value_text`
which has no `show_object_ids` parameter and never appends the `@ 0x...` suffix. Collection
entry `ObjectRef` rows (e.g. ArrayList items) will never show IDs regardless of toggle state.

The Completion Notes only mention "nested object-field rows" — collection entry rows were
apparently overlooked. `format_entry_line` signature is:

```rust
pub(crate) fn format_entry_line(
    entry: &EntryInfo,
    indent: &str,
    value_phase: Option<&ExpansionPhase>,
) -> String
```

It needs a `show_object_ids: bool` parameter, and `format_entry_value_text` must be updated
similarly.

**No test covers this gap** — `render_object_ref_id_toggle` only tests
`format_object_ref_collapsed`, not `format_entry_line`.

---

## 🟡 MEDIUM ISSUES

### M1 — Test names don't match story spec (5.5, 5.6)

Story Definition of Done requires specific test function names. Actual names differ:

| Story spec | Actual function | File |
|---|---|---|
| `thread_list_reopen_search_preserves_existing_filter_in_input` | `reopen_search_preserves_existing_filter` | `thread_list.rs:406` |
| `thread_list_clear_filter_on_empty_result_syncs_cursor` | `clear_filter_restores_full_list_and_selects_first` | `thread_list.rs:393` |

Functional coverage is present; naming prevents cross-referencing story ACs from test output.

### M2 — `navigate_stack_cursor_to_pin_key` blocks UI thread for deep field paths

**File:** `crates/hprof-tui/src/app/mod.rs:177-295`

For `PinKey::Field` variants, the function calls `expand_object_sync` in a loop for each
depth level of `field_path`. `expand_object_sync` invokes `self.engine.expand_object()` on
the main thread (no worker thread, no timeout). On a large hprof, a deep object graph (e.g.
`field_path.len() == 5`) could hang the TUI event loop.

The async `start_object_expansion` path exists but is not used here. This is the correct
trade-off for frame-level cursor positioning, but should be documented as a known limitation
to avoid a "freeze" regression report later.

### M3 — Dev Agent Record lists non-existent model `openai/gpt-5.3-codex`

**File:** `docs/implementation-artifacts/9-6-search-and-favorites-ux-polish.md:354`

```
### Agent Model Used
openai/gpt-5.3-codex
```

This is not a real model name. Story metadata should accurately reflect the implementation
agent for audit traceability.

---

## 🟢 LOW ISSUES

### L1 — Help bar Esc entry describes only thread-list behavior

**File:** `crates/hprof-tui/src/views/help_bar.rs:22`

```rust
("Esc", "Search off -> clear filter -> back"),
```

In `Focus::StackFrames`, Esc does: collapse active collection → cancel loading expansion →
return to ThreadList. The help text only describes the thread-list two-stage Esc. A user in
the stack view who presses Esc expecting "search off" behavior will be confused. Should be
something like: `"Back / collapse / clear filter"` to be panel-agnostic.

### L2 — `ui_status` overloaded for two independent message types

**File:** `crates/hprof-tui/src/app/mod.rs:104`

`ui_status: Option<String>` serves both "terminal too narrow" messages (line 583) and
navigate-to-source warnings (lines 437, 441). A concurrent width-resize event could
overwrite a nav warning before the user sees it, or vice versa. Low risk in practice but
the field design couples unrelated concerns.

---

## AC Validation Summary

| AC | Status | Evidence |
|----|--------|---------|
| AC1 — Select thread while search active, two-stage Esc | ✅ IMPLEMENTED | `app/mod.rs:468-479`, tests 5.1–5.7 |
| AC2 — `g` navigate from favorites to source | ✅ IMPLEMENTED | `app/mod.rs:411-448`, tests 5.8–5.11, 5.16 |
| AC3 — `f` unpin from favorites, focus-on-empty logic | ✅ IMPLEMENTED | `app/mod.rs:391-408`, test 5.12 |
| AC4 — `i` toggle object ID — field rows | ✅ IMPLEMENTED | `tree_render.rs:39,66`, `format.rs:86-90` |
| AC4 — `i` toggle object ID — **collection entry rows** | ❌ MISSING | `format_entry_line` has no `show_object_ids` param |

---

## Task Completion Audit

All tasks marked `[x]`. Spot checks:

- **Task 1.3** `activate_search` is idempotent: `thread_list.rs:139-143` ✓
- **Task 1.4** `deactivate_search` preserves filter: `thread_list.rs:146-148` ✓
- **Task 1.5** `clear_filter` calls `apply_filter("")`: `thread_list.rs:151-153` ✓
- **Task 1.6** ORDER MATTERS comment present: `app/mod.rs:469` ✓
- **Task 1.7** `open_stack_for_selected_thread(serial: u32)`: `app/mod.rs:151-158` ✓
- **Task 4.3** `ToggleObjectIds` only in `handle_stack_frames_input`: `app/mod.rs:1359-1361` ✓
- **Task 4.4** `show_object_ids` in `RenderCtx`: `tree_render.rs:39` ✓; favorites hardcoded `false` ✓
- **Task 5** All 17 tests present and passing ✓ (with name deviations noted in M1)

---

## Recommendation

**Story status: `in-progress`** — H1 must be resolved before `done`.

Priority:
1. **Fix H1** — add `show_object_ids: bool` to `format_entry_line` and `format_entry_value_text`,
   propagate through `tree_render.rs:746`, add a test `render_collection_entry_id_toggle`.
2. **Fix M1** — rename the two test functions to match story spec names.
3. **Address M2** — add a code comment documenting the synchronous-expansion caveat.
4. **Fix M3** — correct the agent model field in the story file.
