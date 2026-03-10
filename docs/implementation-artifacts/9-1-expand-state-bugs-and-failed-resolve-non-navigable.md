# Story 9.1: Fix Expand State Bugs & Failed-to-Resolve Inline Error Style

Status: done

> **File slug note:** the filename keeps the original slug (`non-navigable`) for sprint-status
> compatibility. The title reflects the final approach (inline error style).

## Story

As a user,
I want nodes that fail to resolve to be displayed inline with an error style, and expand state
to remain consistent after a resolution failure,
So that navigation never gets stuck in a broken state and errors are visible at a glance.

## Acceptance Criteria

**AC1** — Given a node whose object resolution returned an error
When the user presses Enter on that node
Then nothing happens (no re-expansion, no crash, phase stays `Failed`)

**AC2** — Given a node in `Failed` state
When it is rendered in the tree
Then its expand-toggle prefix shows `"! "` instead of `"+ "`, its text includes the stored
error message (e.g. `"! ArrayList — Failed to resolve object"`), and the row is styled with
`THEME.error_indicator` (Color::Red foreground)

**AC3** — Given a node in `Failed` state
When the user navigates with ArrowUp / ArrowDown
Then the cursor CAN land on the node (it remains in `flat_items`), but Enter is a no-op

> **Deliberate divergence from epics.md:** the original epic AC said "cursor skips over it".
> This story changes that to "cursor CAN land on it, Enter is a no-op" — see Dev Notes.

**AC4** — Given a node in `Failed` state
When rendered
Then NO separate child row appears below the node — the error is communicated entirely
through the inline prefix + color + label on the parent node

**AC5** — Given any `StackState` configuration
When `flat_items()` and `build_items()` are both called
Then their lengths are equal (`flat_items().len() == build_items().len()`)

**AC6** — Given all existing tests
When `cargo test --all` is run
Then zero failures — no regressions

## Out of Scope

- **Help bar update:** new behavior (inert `"! "` prefix) does not require a keymap entry;
  explicitly deferred to story 9.7 if deemed necessary.
- **Thread search impact:** search filters thread names only (not stack frame content);
  the `"! "` prefix on Failed nodes does not affect search results.
- **Retry mechanism:** `cancel_expansion()` exists and could support future retry UX;
  this story does not add it — see Dev Notes for rationale.

## Tasks / Subtasks

- [x] Fix Enter re-triggering expansion on Failed nodes (AC1)
  - [x] In `app.rs`, `handle_stack_frames_input` → Enter handler: find all arms matching
        `ExpansionPhase::Collapsed | ExpansionPhase::Failed` and split them:
        `Collapsed` keeps its `Cmd::*` dispatch; add `ExpansionPhase::Failed => return None`
  - [x] Locate all sites at implementation time:
        `rg "ExpansionPhase::Failed" crates/hprof-tui/src/app.rs`
- [x] Change Failed node visual: `"! "` prefix + stored error message + error color,
      no extra child row (AC2, AC4)
  - [x] Locate prefix computation sites:
        `rg '"[+\-~] "' crates/hprof-tui/src/views/`
        Add `Some(ExpansionPhase::Failed) => "! "` arm at each site
  - [x] When building the label for a Failed node, retrieve the stored error from
        `object_errors` (always populated by `set_expansion_failed`) and format as
        `"! {class_name} — {error_message}"`. Use `const FAILED_LABEL_SEP: &str = " — "`
        defined once in `stack_view.rs` to avoid string scatter.
  - [x] Locate row-style computation sites:
        `rg "field_value_style|row_style|error_indicator" crates/hprof-tui/src/views/`
        At each site for an expandable `ObjectRef`, override style with
        `THEME.error_indicator` when the ref object's phase is `Failed`
  - [x] Delete both arms that emit the orphan child row:
        `rg -n "Failed to resolve" crates/hprof-tui/src/views/`
        Remove the `ExpansionPhase::Failed => { items.push(...) }` arm at each match
  - [x] In `emit_object_children` (`stack_view.rs`): add exhaustive no-op arm:
        ```rust
        ExpansionPhase::Failed => {
            // Error state is styled on the parent node — no child row emitted here.
        }
        ```
  - [x] Verify `tree_render.rs` match expressions on `ExpansionPhase` are exhaustive:
        `rg "ExpansionPhase" crates/hprof-tui/src/views/tree_render.rs`
        Add `Failed => {}` no-op arms where missing
- [x] Favorites panel — no change needed (AC4 scope clarification)
  - [x] `favorites_panel.rs` builds `object_phases` from snapshot data mapping only
        `Expanded` states; `Failed` phase is never present in a `PinnedItem` snapshot
        (snapshots are frozen at pin time, before any failure). No render change needed.
        Confirm by running: `rg "ExpansionPhase" crates/hprof-tui/src/views/favorites_panel.rs`
- [x] Add unit tests (AC1–AC5)
  - [x] `enter_on_failed_var_is_noop` — set object to `Failed` via `set_expansion_failed`,
        call `expansion_state(oid)` → assert `ExpansionPhase::Failed`;
        assert `flat_items()` contains `OnVar { .. }` for that object (cursor lands on it)
  - [x] `enter_on_failed_collection_entry_is_noop` — expand a collection, call
        `set_expansion_failed` on one entry's object id → assert its `expansion_state`
        is still `Failed` after the call; assert `flat_items()` still contains
        `OnCollectionEntry { .. }` for that entry (not removed from navigation)
  - [x] `failed_var_label_uses_stored_error_message` — `build_items()` on a state with a
        Failed var → use `item_text()` helper (see existing tests at line ~1804) →
        assert text contains `"! "` prefix and the error string passed to
        `set_expansion_failed`; assert `items.len() == flat_items().len()` (AC5, no extra row)
  - [x] `failed_var_style_is_error_indicator` — `build_items()` on a state with a Failed var →
        use `rendered_fg_at()` helper → assert fg color equals `Color::Red`
  - [x] `flat_items_build_items_equal_length_invariant` — assert `flat_items().len() ==
        build_items().len()` for state configurations: (b) one frame collapsed,
        (c) one frame expanded / var Failed, (d) one frame expanded / var Expanded with fields,
        (f) two frames — one collapsed, one expanded with a Failed nested field
- [x] Run `cargo test --all` — zero failures
- [x] Run `cargo clippy --all-targets -- -D warnings` — zero warnings

## Dev Notes

### Chosen Approach — Inline Error State (Chemin W)

Rather than emitting a separate child row, the parent node itself changes appearance
when its expansion phase is `Failed`:

- **Prefix:** `"! "` (ASCII, consistent with `"+ "`, `"- "`, `"~ "` used elsewhere)
- **Label:** `"! {class_name} — {error_message}"` using the message stored in `object_errors`
- **Color:** `THEME.error_indicator` = `Color::Red` foreground (defined in `theme.rs`, always set)
- **Interactivity:** node stays in `flat_items` → cursor can land on it; Enter = no-op

**Why cursor lands on Failed node (not skip):** keeping the node in `flat_items` ensures
`flat_items().len() == build_items().len()` at all times. No extra helper, no offset
calculation. The node is functionally inert (Enter = no-op), which is equivalent UX.

**Deliberate divergence from epics.md AC:** original epic said "cursor skips over it".
Changed to "cursor lands, Enter is no-op" to preserve the 1:1 invariant (AC5).

### Why Retry is Not Added

`cancel_expansion(object_id)` resets phase to `Collapsed`. A future story could add a
"retry" key (e.g. `r`) that calls `cancel_expansion` then dispatches `StartObj`. This
story does not add it because current failure reasons (object absent from file, BinaryFuse8
false positive) are deterministic — retry on a static file always produces the same result.

### `item_style()` Test Helper Pattern

Existing tests use `item_text(item)` to extract text. For style assertions, add an
analogous helper in the test module:

```rust
fn item_style(item: ListItem<'static>) -> Style {
    item.content
        .lines
        .first()
        .and_then(|l| l.spans.first())
        .map(|s| s.style)
        .unwrap_or_default()
}
```

### Favorites Panel — Confirmed No Change Needed

`favorites_panel.rs` constructs `object_phases: HashMap<u64, ExpansionPhase>` from
`PinnedSnapshot::object_fields`, mapping each expanded object id to `Expanded`.
`Failed` is never inserted into this map because `PinnedItem` snapshots are captured
at pin time via `snapshot_from_cursor` — before any resolution failure occurs on the
pinned snapshot path. Confirmed by: `rg "ExpansionPhase" crates/hprof-tui/src/views/favorites_panel.rs`.

### Key Invariant (AC5)

`flat_items().len() == build_items().len()` at all times. Every visual row must have a
corresponding `StackCursor`, and vice versa. This story enforces it by removing the
orphan Failed child rows from the render. Future non-navigable row types must either
appear in both `flat_items` and `build_items`, or in neither.

### Project Structure

- `crates/hprof-tui/src/app.rs` — Enter handler in `handle_stack_frames_input`
- `crates/hprof-tui/src/views/stack_view.rs` — `flat_items()` (pub), `build_items()` (pub),
  `emit_object_children`, `object_errors`, prefix/style computation,
  inline `#[cfg(test)]` module with `item_text()` helper (~line 1804)
- `crates/hprof-tui/src/views/tree_render.rs` — field row render helpers
- `crates/hprof-tui/src/views/favorites_panel.rs` — confirmed: no change needed
- `crates/hprof-tui/src/theme.rs` — `THEME.error_indicator = Color::Red` (always set)

### References

- [Source: docs/planning-artifacts/epics.md#Story 9.1] — original ACs
- [Source: crates/hprof-tui/src/views/stack_view.rs:1167] — `build_items()` (pub)
- [Source: crates/hprof-tui/src/views/stack_view.rs:1804] — `item_text()` helper pattern
- [Source: crates/hprof-tui/src/views/stack_view.rs:869] — `flat_items()`
- [Source: crates/hprof-tui/src/views/stack_view.rs:921] — `emit_object_children`
- [Source: crates/hprof-tui/src/views/favorites_panel.rs:92] — `object_phases` construction
- [Source: crates/hprof-tui/src/theme.rs:23] — `error_indicator = Color::Red`
- [Source: crates/hprof-tui/src/app.rs] — Enter handler, Failed arms

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Impl: split 4 `Collapsed | Failed` arms in `app.rs` Enter handler → Failed returns None (AC1)
- Impl: added `FAILED_LABEL_SEP` const in `stack_view.rs`; `emit_object_children` Failed arm
  is now no-op (AC4/AC5)
- Impl: `format_entry_line` Failed toggle changed from `"+ "` to `"! "`
- Impl: `render_variable_tree` / `append_var` / `append_object_children` /
  `append_collection_entry_obj` — added `object_errors` param; Failed phase now shows
  `"! class — error"` label + `THEME.error_indicator` style; orphan child rows removed
- Impl: `favorites_panel.rs` passes `&HashMap::new()` (Failed never in snapshot)
- Tests: updated old test `build_items_failed_expansion_shows_error_message_with_correct_indent`
  → new name `build_items_failed_expansion_shows_error_inline_on_var_row`
- Tests added: `enter_on_failed_var_is_noop`, `enter_on_failed_collection_entry_is_noop`,
  `failed_var_label_uses_stored_error_message`, `failed_var_style_is_error_indicator`,
  `flat_items_build_items_equal_length_invariant`
- All 185 hprof-tui tests pass; clippy clean
- Review fix: Failed var inline label now uses short class + stored error message without
  `local variable:` prefix (`"! Class — error"`)
- Review fix: Failed collection entries now render inline stored error message and use
  `THEME.error_indicator` style
- Validation rerun: `cargo test -p hprof-tui` (187 passed),
  `cargo clippy -p hprof-tui --all-targets -- -D warnings` (clean)

### File List

- crates/hprof-tui/src/app.rs
- crates/hprof-tui/src/views/stack_view.rs
- crates/hprof-tui/src/views/tree_render.rs
- crates/hprof-tui/src/views/favorites_panel.rs
- docs/code-review/codex-story-9.1-code-review.md

## Senior Developer Review (AI)

- Review report: `docs/code-review/codex-story-9.1-code-review.md`
- Outcome: Changes Requested -> Fixed
- Fixed items:
  - HIGH: Failed local-variable label format now matches story intent (`! Class — error`)
  - MEDIUM: Failed collection entry row now includes stored error message and red error style
  - MEDIUM: Git/story traceability mismatch remains contextual (no active diff in this session)

## Change Log

- 2026-03-10 — AI review follow-up: fixed failed-label formatting and failed collection entry
  error styling/message propagation; reran hprof-tui tests and clippy.
