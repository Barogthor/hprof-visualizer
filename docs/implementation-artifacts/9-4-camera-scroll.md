# Story 9.4: Camera Scroll (Vertical View Shift)

Status: done

## Story

As a user,
I want Ctrl+Up/Down to shift the visible window without moving my selection cursor,
so that I can read context above or below my current position while keeping my place.

## Acceptance Criteria

**AC1** — Given the cursor is on any node in the stack frames panel
When the user presses Ctrl+Down
Then the visible window scrolls down by one line without moving the selection cursor

**AC2** — Given the cursor is on any node in the stack frames panel
When the user presses Ctrl+Up
Then the visible window scrolls up by one line without moving the selection cursor

**AC3** — Given the user has scrolled the camera so that the selected node is about to
go off-screen
When the selected node would leave the visible window
Then the camera snaps back **immediately within the same scroll operation**: the viewport
is adjusted so the selected node remains visible at the nearest edge — the cursor does
not move. If the selected node is already at the viewport edge and cannot scroll further
without leaving the screen, the operation is a silent no-op (offset unchanged).

**AC4** — Given the help panel is visible
When it is rendered
Then `Ctrl+↑` / `Ctrl+↓` are listed with label "Scroll view up / down"

**AC5** — Given all existing tests
When `cargo test --all` is run
Then zero failures — no regressions

## Out of Scope

- Camera scroll in the thread list panel or favorites panel — only the stack frames panel
- Horizontal scroll
- Smooth/animated scrolling — one row per key press
- Camera scroll while search mode is active — Ctrl+Up/Down are not intercepted in
  search mode (search currently does not use arrow keys, so no conflict)

## Tasks / Subtasks

### 1. Add `CameraScrollUp` / `CameraScrollDown` to `InputEvent` (AC1–AC2)

- [x] In `crates/hprof-tui/src/input.rs`, add two variants to `InputEvent` after
  `PageDown`:
  ```rust
  /// Scroll the visible window up one line without moving the cursor (stack view only).
  CameraScrollUp,
  /// Scroll the visible window down one line without moving the cursor (stack view only).
  CameraScrollDown,
  ```

- [x] In `from_key()`, add two arms **before** the `(KeyCode::Up, _)` and
  `(KeyCode::Down, _)` arms. Rust matches in order; the specific-modifier arms must
  precede the `_`-modifier catch-alls:
  ```rust
  (KeyCode::Up, KeyModifiers::CONTROL) => Some(InputEvent::CameraScrollUp),
  (KeyCode::Down, KeyModifiers::CONTROL) => Some(InputEvent::CameraScrollDown),
  (KeyCode::Up, _) => Some(InputEvent::Up),       // existing — do not remove
  (KeyCode::Down, _) => Some(InputEvent::Down),   // existing — do not remove
  ```

  **Verify current arm positions before editing:**
  ```
  rg -n "KeyCode::Up\|KeyCode::Down" crates/hprof-tui/src/input.rs
  ```
  If story 9.3 has already been merged, `Right` and `Left` arms will also be present —
  insert the two new Ctrl arms immediately before the `Up`/`Down` arms regardless.

  **Verify arm ordering after editing** (post-implementation check):
  ```
  rg -n "CameraScroll\|KeyCode::Up\|KeyCode::Down" crates/hprof-tui/src/input.rs
  ```
  The line numbers for `CameraScrollUp` and `CameraScrollDown` arms **must be lower**
  than the line numbers for `(KeyCode::Up, _)` and `(KeyCode::Down, _)`. If not, the
  Ctrl events will silently map to `Up`/`Down` — a silent bug with no compiler warning.

  **Verify existing input tests are unaffected:**
  ```
  cargo test -p hprof-tui input
  ```
  All pre-existing tests must pass without modification. The insertion of Ctrl arms
  must not alter the behavior of any existing binding.

- [x] Add tests in `input.rs`:
  ```rust
  #[test]
  fn from_key_maps_ctrl_up_to_camera_scroll_up() {
      assert_eq!(
          from_key(key(KeyCode::Up, KeyModifiers::CONTROL)),
          Some(InputEvent::CameraScrollUp)
      );
  }

  #[test]
  fn from_key_maps_ctrl_down_to_camera_scroll_down() {
      assert_eq!(
          from_key(key(KeyCode::Down, KeyModifiers::CONTROL)),
          Some(InputEvent::CameraScrollDown)
      );
  }

  /// Regression: plain Up must NOT be shadowed by the Ctrl arm.
  #[test]
  fn from_key_plain_up_still_maps_to_up() {
      assert_eq!(
          from_key(key(KeyCode::Up, KeyModifiers::NONE)),
          Some(InputEvent::Up)
      );
  }

  /// Regression: Ctrl+Up must NOT resolve to Up (arm ordering guard).
  #[test]
  fn ctrl_up_does_not_map_to_up() {
      assert_ne!(
          from_key(key(KeyCode::Up, KeyModifiers::CONTROL)),
          Some(InputEvent::Up)
      );
  }
  ```

### 2. Add `scroll_view_up()` / `scroll_view_down()` to `StackState` (AC1–AC3)

- [x] In `crates/hprof-tui/src/views/stack_view.rs`, add two methods to `StackState`.
  Place them after `move_page_up()` (~line 898):

  **Verify the `offset_mut` API is available:**
  ```
  rg -n "fn offset_mut\|fn offset\b" \
    $(cargo metadata --format-version 1 --no-deps 2>/dev/null | \
      python3 -c "import sys,json; \
      [print(p['manifest_path']) for p in json.load(sys.stdin)['packages'] \
        if 'ratatui' in p['name']]" 2>/dev/null || \
      find ~/.cargo/registry/src -path "*/ratatui-*/src/widgets/list/state.rs" \
        2>/dev/null | head -1)
  ```
  If `offset_mut` is not found, search for the equivalent: older ratatui versions expose
  `offset` as a `pub` field directly. Adapt accordingly.

  ```rust
  /// Scrolls the visible window up by one line without moving the selection cursor.
  ///
  /// If the cursor would go off the bottom of the viewport after scrolling, the camera
  /// snaps so the cursor is at the last visible row.
  pub fn scroll_view_up(&mut self) {
      if self.visible_height == 0 {
          return;
      }
      let flat = self.flat_items();
      let item_count = flat.len();
      let Some(selected_idx) = flat.iter().position(|c| c == &self.cursor) else {
          return;
      };
      // Clamp current offset to valid range before decrementing, so a stale
      // out-of-bounds offset (possible via test setup or future bugs) is corrected
      // before we subtract 1.
      let max_offset = item_count.saturating_sub(self.visible_height as usize);
      let current_offset = self.list_state.offset().min(max_offset);
      let new_offset = current_offset.saturating_sub(1);
      *self.list_state.offset_mut() = new_offset;
      // Snap back: cursor below viewport after scrolling up.
      // Safety: underflow impossible — snap only fires when
      //   selected_idx >= new_offset + visible_height
      //   which implies selected_idx + 1 >= visible_height + 1 > visible_height,
      //   so selected_idx + 1 - visible_height >= 1 (no usize underflow).
      if selected_idx >= new_offset + self.visible_height as usize {
          *self.list_state.offset_mut() =
              selected_idx + 1 - self.visible_height as usize;
      }
  }

  /// Scrolls the visible window down by one line without moving the selection cursor.
  ///
  /// If the cursor would go off the top of the viewport after scrolling, the camera
  /// snaps so the cursor is at the first visible row.
  pub fn scroll_view_down(&mut self) {
      if self.visible_height == 0 {
          return;
      }
      let flat = self.flat_items();
      let item_count = flat.len();
      if item_count == 0 {
          return;
      }
      let Some(selected_idx) = flat.iter().position(|c| c == &self.cursor) else {
          return;
      };
      let max_offset = item_count.saturating_sub(self.visible_height as usize);
      let new_offset = (self.list_state.offset() + 1).min(max_offset);
      *self.list_state.offset_mut() = new_offset;
      // Snap back: cursor above viewport after scrolling down.
      if selected_idx < new_offset {
          *self.list_state.offset_mut() = selected_idx;
      }
  }
  ```

### 3. Add handlers in `app.rs` (AC1–AC3)

- [x] In `crates/hprof-tui/src/app.rs`, inside `handle_stack_frames_input()`,
  add two new arms after `InputEvent::PageUp` (~line 448) and before
  `InputEvent::Enter`:
  ```rust
  InputEvent::CameraScrollUp => {
      if let Some(s) = &mut self.stack_state {
          s.scroll_view_up();
      }
  }
  InputEvent::CameraScrollDown => {
      if let Some(s) = &mut self.stack_state {
          s.scroll_view_down();
      }
  }
  ```

  **Verify routing is focus-gated** (camera scroll must only fire in stack frames panel):
  ```
  rg -n "handle_stack_frames_input\|Focus::StackFrames" \
    crates/hprof-tui/src/app.rs
  ```

  **Verify `CameraScrollUp/Down` are no-ops outside the stack frames panel:**
  ```
  rg -n "CameraScroll\|Focus::Favorites\|handle_favorites\|handle_thread" \
    crates/hprof-tui/src/app.rs
  ```
  Confirm that when focus is on the thread list or favorites panel, `CameraScrollUp` and
  `CameraScrollDown` fall through to a `_ => {}` arm and do not trigger any scroll
  action. If the favorites panel handler has its own scroll logic (story 7.1), ensure
  neither variant is accidentally matched there.

  **Verify `CameraScroll` is inert in search mode:**
  ```
  rg -n "SearchMode\|handle_search\|InputEvent::Camera" \
    crates/hprof-tui/src/app.rs
  ```
  In search mode, `CameraScrollUp/Down` must not reach `handle_stack_frames_input`.
  Confirm they fall through unhandled (silent no-op during search).

### 4. Update `help_bar.rs` (AC4)

- [x] Before editing, check the current entry count and story 9.3 merge status:
  ```
  rg -n "ENTRY_COUNT\|CameraScroll\|Right.*Expand\|Unexpand" \
    crates/hprof-tui/src/views/help_bar.rs \
    crates/hprof-tui/src/input.rs
  ```

  **Case A — story 9.3 NOT yet merged** (`ENTRY_COUNT = 11`):
  - Add 2 entries → new `ENTRY_COUNT = 13`
  - New `required_height()` = `2 + 1 + div_ceil(13,2) + 1 = 11`
  - `build_rows()` length = `1 + 7 + 1 = 9`

  **Case B — story 9.3 already merged** (`ENTRY_COUNT = 13`, entries include `→` and `←`):
  - Add 2 entries → new `ENTRY_COUNT = 15`
  - New `required_height()` = `2 + 1 + div_ceil(15,2) + 1 = 12`
  - `build_rows()` length = `1 + 8 + 1 = 10`

  Insert the two new entries **after** the existing `PgUp / PgDn` entry (row position is
  not critical for correctness, but grouping scroll-related shortcuts together is good UX):
  ```rust
  ("Ctrl+\u{2191}", "Scroll view up"),
  ("Ctrl+\u{2193}", "Scroll view down"),
  ```
  Unicode: `\u{2191}` = `↑`, `\u{2193}` = `↓`.

- [x] Update `ENTRY_COUNT` to reflect the new total (13 or 15, per Case A/B above).

- [x] Fix tests in `help_bar.rs`:
  - `required_height_returns_ten_for_eleven_entries` (or its story 9.3 successor):
    rename and update assertion to reflect new height.
  - `build_rows_produces_correct_line_count`: update `rows.len()` assertion.
  - `entry_count_constant_matches_entries_slice`: no change needed — auto-verifies.

### 5. Tests in `stack_view.rs` (AC1–AC3)

Use the existing `page_down_jumps_by_visible_height` test as constructor reference.

Add two `#[cfg(test)]` accessors to `StackState` (place near other test helpers, or
after `set_visible_height`):
```rust
#[cfg(test)]
pub fn list_state_offset_for_test(&self) -> usize {
    self.list_state.offset()
}

#[cfg(test)]
pub fn set_list_state_offset_for_test(&mut self, offset: usize) {
    *self.list_state.offset_mut() = offset;
}
```
The setter makes test preconditions hermetic — no need to chain scroll calls to
reach a specific offset, which could itself trigger snap-back and invalidate the setup.
If equivalent accessors already exist from previous stories, reuse them.

**Verify `#[cfg(test)]` gates are present after writing:**
```
rg -n "list_state_offset_for_test\|set_list_state_offset_for_test" \
  crates/hprof-tui/src/views/stack_view.rs
```
The line immediately preceding each `pub fn` must be `#[cfg(test)]`. Without this gate
the methods are compiled into the release binary, widening the public API unnecessarily.
`cargo clippy` does not catch a missing gate — this must be verified manually.

- [x] `scroll_view_down_shifts_offset_without_moving_cursor`:
  ```rust
  #[test]
  fn scroll_view_down_shifts_offset_without_moving_cursor() {
      let frames = (0..5).map(make_frame).collect();
      let mut state = StackState::new_with_frames(frames);
      // Move cursor to frame 2 (flat index 2).
      state.move_down(); // frame 1
      state.move_down(); // frame 2
      state.set_visible_height(3); // window shows items [offset, offset+3)
      state.set_list_state_offset_for_test(0);
      state.scroll_view_down();
      // Offset moved from 0 to 1.
      // Snap check: selected(2) is NOT less than new_offset(1) → no snap.
      assert_eq!(state.list_state_offset_for_test(), 1);
      // Cursor must not have moved — still on frame 2.
      assert_eq!(state.selected_frame_id(), Some(2));
  }
  ```

- [x] `scroll_view_up_shifts_offset_without_moving_cursor`:
  ```rust
  #[test]
  fn scroll_view_up_shifts_offset_without_moving_cursor() {
      let frames = (0..5).map(make_frame).collect();
      let mut state = StackState::new_with_frames(frames);
      state.move_down(); // cursor frame 1
      state.set_visible_height(3);
      // Set offset to 1 directly — no scroll call that might trigger snap.
      state.set_list_state_offset_for_test(1);
      state.scroll_view_up();
      // Offset back to 0.
      // Snap check: selected(1) < 0+3=3 → NOT below viewport top → no snap.
      assert_eq!(state.list_state_offset_for_test(), 0);
      // Cursor must not have moved — still on frame 1.
      assert_eq!(state.selected_frame_id(), Some(1));
  }
  ```

- [x] `scroll_view_down_snaps_when_cursor_at_top`:
  ```rust
  #[test]
  fn scroll_view_down_snaps_back_when_cursor_would_leave_viewport() {
      let frames = (0..5).map(make_frame).collect();
      let mut state = StackState::new_with_frames(frames);
      // Cursor at frame 0 (flat index 0), visible_height=3, offset=0.
      state.set_visible_height(3);
      state.scroll_view_down();
      // new_offset = 1. selected_idx(0) < new_offset(1) → snap → offset = 0.
      assert_eq!(state.list_state_offset_for_test(), 0);
      // Cursor must not have moved — still on frame 0.
      assert_eq!(state.selected_frame_id(), Some(0));
  }
  ```

- [x] `scroll_view_up_snaps_when_cursor_at_bottom_of_viewport`:
  ```rust
  #[test]
  fn scroll_view_up_snaps_when_cursor_at_bottom_of_viewport() {
      let frames = (0..5).map(make_frame).collect();
      let mut state = StackState::new_with_frames(frames);
      // Move cursor to frame 4 (flat index 4, last item).
      state.move_down(); // 1
      state.move_down(); // 2
      state.move_down(); // 3
      state.move_down(); // 4
      state.set_visible_height(2);
      // Set offset to 3: viewport = [3, 5), cursor(4) is at the bottom edge.
      state.set_list_state_offset_for_test(3);
      state.scroll_view_up();
      // new_offset = 2, viewport = [2, 4).
      // selected(4) >= new_offset(2) + height(2) = 4 → snap fires → offset = 4+1-2 = 3.
      assert_eq!(state.list_state_offset_for_test(), 3);
      // Cursor must not have moved — still on frame 4.
      assert_eq!(state.selected_frame_id(), Some(4));
  }
  ```

- [x] `scroll_view_no_op_when_no_frames`:
  ```rust
  #[test]
  fn scroll_view_no_op_when_no_frames() {
      let mut state = StackState::new_with_frames(vec![]);
      state.set_visible_height(5);
      // Should not panic.
      state.scroll_view_up();
      state.scroll_view_down();
      assert_eq!(state.list_state_offset_for_test(), 0);
  }
  ```

- [x] `scroll_view_no_op_when_visible_height_zero`:
  ```rust
  #[test]
  fn scroll_view_no_op_when_visible_height_zero() {
      let frames = (0..5).map(make_frame).collect();
      let mut state = StackState::new_with_frames(frames);
      state.move_down();
      state.move_down(); // cursor frame 2
      // visible_height stays at 0 (default, not yet rendered).
      state.set_list_state_offset_for_test(0);
      // Both directions must be no-ops — no panic, no offset or cursor change.
      state.scroll_view_up();
      state.scroll_view_down();
      assert_eq!(state.list_state_offset_for_test(), 0);
      assert_eq!(state.selected_frame_id(), Some(2));
  }
  ```

- [x] `scroll_view_down_no_op_when_list_fits_in_viewport`:
  ```rust
  #[test]
  fn scroll_view_down_no_op_when_list_fits_in_viewport() {
      let frames = (0..3).map(make_frame).collect();
      let mut state = StackState::new_with_frames(frames);
      state.move_down(); // cursor frame 1
      // visible_height > item_count: all 3 items visible, max_offset = 0.
      state.set_visible_height(10);
      state.scroll_view_down();
      // max_offset = 3.saturating_sub(10) = 0. new_offset = (0+1).min(0) = 0. No change.
      assert_eq!(state.list_state_offset_for_test(), 0);
      assert_eq!(state.selected_frame_id(), Some(1));
  }
  ```

### 6. Run validation

- [x] `cargo test --all` — zero failures
- [x] `cargo clippy --all-targets -- -D warnings` — zero warnings

## Dev Notes

### Why Ctrl+Up/Down — not a custom key

Ctrl+Up/Down is the de-facto standard for "scroll without moving cursor" in editors
(VS Code, Vim `Ctrl+E`/`Ctrl+Y`, IntelliJ). It also avoids conflicts with all existing
bindings.

### Input arm ordering is critical

`(KeyCode::Up, _)` currently catches ALL modifiers. The new Ctrl arms must be placed
**before** the `_` arms or the Ctrl events will silently resolve to `Up`/`Down`. This
is a common Rust pattern — more specific patterns must precede wildcards.

### `list_state.offset_mut()` — ratatui 0.30 API

`ListState::offset_mut(&mut self) -> &mut usize` is available since ratatui 0.24.1 and
is present in 0.30.0. The offset is the index of the top-most visible item. Ratatui's
`List` widget uses it during render to determine which items to display.

When `select()` is called (by move_up, move_down, sync_list_state etc.), ratatui
auto-adjusts offset to keep the selection visible. Camera scroll bypasses `select()`,
so we manually control offset and must implement the snap-back ourselves.

**ADR: Why not a separate `camera_offset` field in `StackState`**

A natural alternative is to store the camera offset in a dedicated field and merge it
into `list_state` at render time. This does not work cleanly: ratatui's `List` render
reads `list_state.offset` as-is, but any subsequent call to `list_state.select()` (which
happens in `sync_list_state` on every cursor move) resets `offset` to keep the selected
item visible, overwriting the camera field's contribution. There is no render hook to
re-apply the camera offset after `select()`. `offset_mut()` is the API ratatui explicitly
provides for direct, persistent offset control — use it directly.

**ADR: Why snap-back lives in `StackState`, not `App`**

`App` does not have access to `flat_index()` (private to `StackState`). Exposing it
to move snap-back logic into `App` would enlarge the public API surface without any
benefit — `StackState` already holds both the offset and the cursor index, so the
invariant ("cursor stays visible after a camera scroll") is naturally enforced there.

**ADR: Why `CameraScrollUp/Down` variants, not `InputEvent::ScrollBy(i32)`**

A generic `ScrollBy(i32)` variant could theoretically unify PageUp/PageDown and camera
scroll. The spec defines exactly one scroll amount (1 line). Named variants are more
documentable, more testable, and consistent with every other binding in `InputEvent`.
YAGNI applies — generalize only when a second scroll amount is actually needed.

### Snap-back semantics

| Scroll direction | new_offset        | Snap-back condition                   | Snap-back action                        |
|-----------------|-------------------|---------------------------------------|-----------------------------------------|
| Up (offset - 1) | old_offset - 1    | cursor >= **new_offset** + height     | new_offset = cursor + 1 - height        |
| Down (offset + 1)| old_offset + 1   | cursor < **new_offset**               | new_offset = cursor                     |

"Snap" means the camera is moved the minimum amount to keep the cursor visible —
it does NOT move the cursor. `new_offset` is the offset **after** the one-row scroll,
before snap correction. Using `old_offset` in the condition would produce an off-by-one.

### Watch out: `visible_height = 0` on first frame

`visible_height` is initialized to 0 and set during the first render call. If the user
triggers Ctrl+Up/Down before the first render (theoretically impossible via real input,
but possible in tests), both methods must be no-ops.

Both `scroll_view_up` and `scroll_view_down` guard against this with
`if self.visible_height == 0 { return; }` as their first line. This prevents:
- underflow in `selected_idx + 1 - self.visible_height as usize` (scroll_view_up snap)
- an offset of 1 persisting invisibly when `item_count > 1` (scroll_view_down)

### Cross-story dependency: help_bar.rs

Story 9.3 also modifies `help_bar.rs` (adds 2 entries for `→` and `←`). Check
`ENTRY_COUNT` at implementation time to avoid clobbering 9.3's changes. The developer
must count the current entries before updating `ENTRY_COUNT`.

### `scroll_view_down` max-offset clamping

```rust
let max_offset = item_count.saturating_sub(self.visible_height as usize);
let new_offset = (self.list_state.offset() + 1).min(max_offset);
```
The upper bound is `item_count - visible_height`, **not** `item_count - 1`. Clamping to
`item_count - 1` would allow the last item to scroll to the top of the viewport, leaving
`visible_height - 1` empty lines below — a degraded UX with no benefit.

`item_count - visible_height` ensures the last item stays at or below the bottom edge
of the viewport. When `item_count <= visible_height`, `saturating_sub` returns 0, making
scroll-down a no-op (the whole list fits in the viewport).

Note: if `item_count = 0`, the early return fires before this line.

### Single `flat_items()` pass in both scroll methods

Both `scroll_view_up` and `scroll_view_down` call `self.flat_items()` exactly once,
storing the result in a local `flat` binding. `flat_items()` traverses the entire
expanded tree to build a `Vec<StackCursor>` — calling it twice per scroll event would
double the traversal cost on large trees (200+ items). The `selected_idx` is derived
from `flat.iter().position(...)` on the same binding, avoiding the redundant call that
`flat_index()` would introduce.

### Snap-back underflow safety invariant

The snap-back in `scroll_view_up` computes `selected_idx + 1 - visible_height as usize`.
This subtraction is safe because the snap condition `selected_idx >= new_offset +
visible_height` implies `selected_idx + 1 >= visible_height + 1`, so
`selected_idx + 1 - visible_height >= 1`. The comment in the implementation documents
this invariant explicitly to prevent future maintainers from adding a redundant
`saturating_sub` or accidentally breaking the condition.

### Project Structure

| File | Change | Tasks |
|------|--------|-------|
| `crates/hprof-tui/src/input.rs` | Add `CameraScrollUp`, `CameraScrollDown`; Ctrl+Up/Down arms before `_` arms; tests | 1 |
| `crates/hprof-tui/src/views/stack_view.rs` | Add `scroll_view_up()`, `scroll_view_down()`; test accessor; tests | 2, 5 |
| `crates/hprof-tui/src/app.rs` | Add `CameraScrollUp`/`CameraScrollDown` arms in `handle_stack_frames_input` | 3 |
| `crates/hprof-tui/src/views/help_bar.rs` | Add 2 entries, update `ENTRY_COUNT`, fix tests | 4 |

### References

- [Source: docs/planning-artifacts/epics.md#Story 9.4] — ACs and description
- [Source: crates/hprof-tui/src/input.rs:11] — `InputEvent` enum
- [Source: crates/hprof-tui/src/input.rs:48] — `from_key()` match arms
- [Source: crates/hprof-tui/src/views/stack_view.rs:181] — `list_state: ListState` field
- [Source: crates/hprof-tui/src/views/stack_view.rs:189] — `visible_height: u16` field
- [Source: crates/hprof-tui/src/views/stack_view.rs:877] — `move_page_down()` / `move_page_up()`
- [Source: crates/hprof-tui/src/views/stack_view.rs:1168] — `sync_list_state()`
- [Source: crates/hprof-tui/src/app.rs:440] — `PageDown`/`PageUp` handler arms (insert after)
- [Source: crates/hprof-tui/src/app.rs:894] — `set_visible_height()` call site
- [Source: crates/hprof-tui/src/views/help_bar.rs:17] — `ENTRY_COUNT` constant
- [Source: crates/hprof-tui/src/views/help_bar.rs:20] — `ENTRIES` constant
- [Source: docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md] —
  previous story: also modifies `input.rs` and `help_bar.rs`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- `scroll_view_no_op_when_visible_height_zero`: story spec assumed visible_height defaults
  to 0, but `CursorState::new()` initialises it to 1. Fixed test to explicitly call
  `set_visible_height(0)`.

### Completion Notes List

- Task 1: Added `CameraScrollUp`/`CameraScrollDown` variants to `InputEvent`; inserted
  Ctrl+Up/Down arms before wildcard arms in `from_key()`; 4 tests added and passing.
- Task 2: Added `visible_height()`/`list_state()` getters to `CursorState`; added
  `scroll_view_up()`/`scroll_view_down()` to `StackState` with snap-back logic; 2
  `#[cfg(test)]` accessors added.
- Task 3: Added `CameraScrollUp`/`CameraScrollDown` arms in `handle_stack_frames_input()`.
- Task 4: Case B (9.3 merged) — added 2 entries, ENTRY_COUNT 13→15, updated 2 tests.
- Task 5: 7 scroll_view tests added to stack_view/tests.rs; all pass.
- Task 6: `cargo test --all` 252 passed; `cargo clippy --all-targets -- -D warnings` clean.
- Task 7 (post-review hardening): clamped stale offsets in `scroll_view_down()` to avoid
  overflow risk, added app-level CameraScroll routing/no-op tests, cleaned help footer labels,
  and aligned story traceability notes with workspace git state.

### File List

- `crates/hprof-tui/src/input.rs`
- `crates/hprof-tui/src/views/cursor.rs`
- `crates/hprof-tui/src/views/stack_view/state.rs`
- `crates/hprof-tui/src/views/stack_view/tests.rs`
- `crates/hprof-tui/src/app/mod.rs`
- `crates/hprof-tui/src/app/tests.rs`
- `crates/hprof-tui/src/views/help_bar.rs`
- `docs/code-review/codex-story-9-4-adversarial-review-2026-03-12.md`
- `docs/implementation-artifacts/sprint-status.yaml`
- `docs/implementation-artifacts/9-4-camera-scroll.md`

### Git Reality Notes (Review Context)

- Workspace had unrelated local changes during this code review:
  - `crates/hprof-engine/src/pagination/tests.rs`
  - `tools/hprof-redact-custom/src/main/java/io/hprofvisualizer/redact/PathOnlyTransformer.java`
  - `docs/code-review/claude-story-9-4-adversarial-review.md`
  - `docs/implementation-artifacts/9-5-stack-frame-variable-names-and-static-fields.md`
- These files are not part of story 9.4 implementation scope.
