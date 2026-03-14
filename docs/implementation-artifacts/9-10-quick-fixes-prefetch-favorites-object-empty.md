# Story 9.10: Quick Fixes — Prefetch Favoris + Object[0] Empty

Status: done

## Story

As a user,
I want empty collections (Object[0]) to display "(empty)" without a misleading expand toggle,
and I want the favorites panel to not show unloadable chunk sentinels,
so that the UI accurately reflects what can and cannot be expanded.

## Bugs Fixed

### N9 — Chunk sentinels in favorites snapshots

**Problem:** Scrolling in the favorites panel showed chunk sentinel rows
(`+ [100...199]`) for paginated collections. These sentinels triggered
background loads on Down that looped indefinitely since favorites are
frozen snapshots that cannot fetch new pages.

**Fix:** Chunk sentinels for unloaded chunks are now completely hidden
in snapshot mode (both render and metadata collector). Prefetch logic
removed from the favorites Down handler (Right/Enter never had prefetch).

### N10 — Object[0] displayed as expandable

**Problem:** Empty arrays/collections (`entry_count: Some(0)`) rendered
with a `+` toggle. Pressing Enter/Right triggered `expand_object()`
which failed with a "Failed to resolve" warning in the status bar.

**Fix:**
- `format_object_ref_collapsed` / `format_entry_value_text`: `Some(0)` renders "(empty)"
- `object_ref_state`: returns `(None, false)` for empty collections (no toggle)
- `append_var`: handles `phase == None` with blank toggle `"  "` instead of `"+"`
- `selected_field_ref_id`, `selected_static_field_ref_id`, `selected_collection_entry_ref_id`:
  exclude `entry_count: Some(0)` so the code doesn't fall through to `StartObj`
- `ec == 0` guards added in Right and Enter handlers for all 4 `StartCollection` paths

## Acceptance Criteria

1. **AC1 – Empty collections show "(empty)" label:**
   Given a variable or field is an `ObjectRef` with `entry_count: Some(0)`,
   it renders as `ClassName (empty)` without any expand toggle.

2. **AC2 – Enter/Right on empty collections is a no-op:**
   Given the cursor is on an empty collection,
   When the user presses Enter or Right,
   Then nothing happens (no expansion, no warning, no page load).

3. **AC3 – Unloaded chunk sentinels hidden in favorites:**
   Given a pinned snapshot contains a paginated collection,
   Then only loaded chunk pages are shown; unloaded chunks are invisible.

4. **AC4 – No prefetch in favorites panel:**
   Given the focus is on the favorites panel,
   Then Down/Right/Enter never trigger background page loads.

## Files Modified

| File | Change |
|------|--------|
| `crates/hprof-tui/src/app/mod.rs` | Remove favorites prefetch; add `ec == 0` guards in Right/Enter |
| `crates/hprof-tui/src/app/tests.rs` | 4 new tests (empty collection var/field, snapshot sentinels) |
| `crates/hprof-tui/src/views/favorites_panel/mod.rs` | Skip unloaded chunks in metadata collector |
| `crates/hprof-tui/src/views/stack_view/format.rs` | `Some(0)` → "(empty)" in 2 format functions + 2 tests |
| `crates/hprof-tui/src/views/stack_view/state.rs` | Exclude `entry_count: Some(0)` from 3 `*_ref_id()` functions |
| `crates/hprof-tui/src/views/tree_render.rs` | `object_ref_state` early return; `append_var` None phase; skip unloaded chunks in snapshot mode + 1 test |

## Dev Notes

- The root cause of the "Failed to resolve" warning on Object[0] was subtle:
  `selected_field_collection_info()` already filtered `ec > 0`, so the `ec == 0`
  guard in the handler was never reached. The code fell through to
  `selected_field_ref_id() → StartNestedObj → expand_object()` which failed.
  The fix required both the handler guards AND the `*_ref_id()` exclusions.
- Favorites snapshots are frozen by design. The chunk sentinel infrastructure
  (`ChunkSentinelMap`, `current_chunk_sentinel()`) is still in place for
  potential future live-loading but is currently never populated.
