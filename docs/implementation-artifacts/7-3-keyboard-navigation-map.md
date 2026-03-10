# Story 7.3: Keyboard Navigation Map

Status: done

## Story

As a user,
I want a consistent, complete keyboard map across all TUI panels,
so that I can navigate the entire tool without consulting documentation.

## Acceptance Criteria

1. **Help panel via `?`** — Pressing `?` from any panel or focus state toggles a
   bottom help panel. The panel pushes existing panels upward — no modal, no overlay.
   Pressing `?` again closes it. `Esc` is **not** a close trigger (already used for
   navigation / search cancel).

2. **Keymap completeness** — The help panel documents the following keybindings:

   | Key | Action |
   |-----|--------|
   | `q` / `Ctrl+C` | Quit |
   | `Esc` | Go back / cancel search |
   | `Tab` | Cycle panel focus |
   | `↑` / `↓` | Move selection |
   | `PgUp` / `PgDn` | Scroll one page |
   | `Home` / `End` | Jump to first / last item |
   | `Enter` | Expand / confirm |
   | `f` | Pin / unpin favorite *(Story 7.1)* |
   | `F` | Focus favorites panel *(Story 7.1)* |
   | `s` or `/` | Open search (thread list only) |
   | `?` | Toggle help panel |

3. **`s` as search alternative** — Pressing `s` (lowercase) in the thread list panel
   when search is not active activates search, equivalent to `/`.

4. **`Tab` cycles panel focus** — Pressing `Tab` from `Focus::ThreadList` with an
   active `stack_state` moves focus to `Focus::StackFrames`. Pressing `Tab` from
   `Focus::StackFrames` returns focus to `Focus::ThreadList`. If no `stack_state`
   exists, `Tab` is a no-op.
   > Integration note: When Story 7.1 adds `Focus::Favorites`, `Tab` cycling extends
   > to include it. Use `App::cycle_focus()` with a `match` — not `if/else` — so the
   > compiler forces the update.

5. **No-op guarantee** — A key with no binding in the current panel state produces
   no output, no message, and no panic.

6. **`q` exits cleanly from any panel** — `q` in `Focus::ThreadList` or
   `Focus::StackFrames`, search active or not, returns `AppAction::Quit`.

7. **No regressions** — All existing tests pass (`cargo test`). Clippy clean
   (`cargo clippy --all-targets -- -D warnings`).

## Tasks / Subtasks

- [x] Task 1: Extend `InputEvent` and `from_key` in `input.rs` (AC: 1, 3, 4)
  - [x] Add `Tab` variant to `InputEvent`
  - [x] Add `ToggleHelp` variant to `InputEvent`
  - [x] Add `(KeyCode::Tab, _) => Some(InputEvent::Tab)` to `from_key`
  - [x] Add `(KeyCode::Char('?'), KeyModifiers::NONE | KeyModifiers::SHIFT) => Some(InputEvent::ToggleHelp)`
    to `from_key` — placed **above** the generic `SearchChar` catch-all arm, otherwise
    `?` routes to `SearchChar('?')` and the panel never toggles.
    > ⚠️ On most terminals `?` is sent as `Char('?')` with `SHIFT` modifier — the arm
    > must match both `NONE` and `SHIFT`. Placement before the catch-all is mandatory.
  - [x] **No change to `from_key` for `s`** — `s` continues to map to `SearchChar('s')`
    via the existing catch-all. The search-activation alias is handled in
    `handle_thread_list_input` (see Task 2). Do not add a `SearchActivate` arm for `s`
    in `from_key` — it would break `s` typing during active search.
  - [x] Add `InputEvent::ToggleHelp` and `InputEvent::Tab` tests in `mod tests`

- [x] Task 2: Add `show_help` state and handlers to `App` in `app.rs` (AC: 1, 4, 5, 6)
  - [x] Add `show_help: bool` field to `App` struct (initialized `false`)
  - [x] Add `fn cycle_focus(&mut self)` method:
    ```rust
    fn cycle_focus(&mut self) {
        match self.focus {
            Focus::ThreadList => {
                if self.stack_state.is_some() {
                    self.focus = Focus::StackFrames;
                }
            }
            Focus::StackFrames => {
                self.focus = Focus::ThreadList;
                self.refresh_preview_stack();
            }
        }
    }
    ```
  - [x] In `handle_input`: intercept `InputEvent::ToggleHelp` before dispatching to
    panel handlers — toggle `self.show_help` and return `AppAction::Continue`. All
    other events route normally regardless of `show_help` state (the panel is
    non-focusable, navigation continues uninterrupted).
  - [x] In `handle_thread_list_input` non-search branch: add
    `InputEvent::Tab => self.cycle_focus()` arm
  - [x] In `handle_thread_list_input` non-search branch: add
    `InputEvent::SearchChar('s') => self.thread_list.activate_search()` arm
  - [x] In `handle_stack_frames_input`: add `InputEvent::Tab => self.cycle_focus()` arm
    explicitly (do not rely on `_ => {}`)

- [x] Task 3: Create `views/help_bar.rs` (AC: 1, 2)
  - [x] New file `crates/hprof-tui/src/views/help_bar.rs`
  - [x] Module docstring required (`//!`)
  - [x] Define `pub struct HelpBar;` (stateless widget, all content is static)
  - [x] Implement `ratatui::widgets::Widget for HelpBar`
  - [x] Render as a `Block` filling its area with title `" Keyboard Shortcuts "`
    styled with `THEME.border_focused`
  - [x] Content: two-column layout (Key | Action) using `Paragraph` with aligned
    text — two entries per line.
  - [x] Favorites entries annotation: comment in source
    `// TODO(7.1): remove "(Story 7.1)" annotations`
  - [x] No `Color::*` literals — use `THEME` exclusively
  - [x] `pub fn required_height() -> u16` — formula `2 + 1 + ENTRY_COUNT.div_ceil(2) + 1 = 10`
    with `const ENTRY_COUNT: u16 = 11`

- [x] Task 4: Wire help panel into render loop in `app.rs` (AC: 1, 2)
  - [x] Import `crate::views::help_bar::{self, HelpBar}` in `app.rs`
  - [x] In `App::render`, the actual layout at line ~814 (post-7.1 working tree) is:
    ```rust
    // EXISTING line ~814 — variable is `main_area`, not `content_area`
    let [main_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    // ... followed by 7.1 favorites logic using main_area ...
    ```
    Step 1 — rename the existing `main_area` binding to `content_area` at line ~814
    (and update the subsequent `areas(main_area)` call on line ~826 to `areas(content_area)`).

    Step 2 — insert immediately after the renamed destructuring:
    ```rust
    let (main_area, help_area) = if self.show_help {
        let [m, h] = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(help_bar::required_height()),
        ])
        .areas(content_area);   // use .areas(), not .split() — consistent with codebase
        (m, Some(h))
    } else {
        (content_area, None)
    };
    ```
    All subsequent code that used `content_area` for panel layout now uses `main_area`.

    Step 3 — at the end of the render method, before the closing brace, add:
    ```rust
    if let Some(area) = help_area {
        frame.render_widget(HelpBar, area);
    }
    ```
    > ⚠️ Status bar (`status_area`) stays last. Help bar occupies the slot between
    > main panels and status bar.
    > ⚠️ Story 7.1 has already added favorites panel logic in this render method
    > (`show_favorites`, `pinned`, `fav_area`). Do not remove or reorder that logic —
    > it operates on `main_area` (the new binding) and is unaffected by this change.

- [x] Task 5: Export `help_bar` from `views/mod.rs` (AC: 1)
  - [x] Add `pub mod help_bar;` to `crates/hprof-tui/src/views/mod.rs`

- [x] Task 6: Tests (AC: 1, 3, 4, 5, 6)
  - [x] `input.rs` tests:
    - `from_key` maps `?` → `InputEvent::ToggleHelp`
    - `from_key` maps `Tab` → `InputEvent::Tab`
    - `from_key` maps `s` → `InputEvent::SearchChar('s')` (existing behavior preserved)
  - [x] `app.rs` tests (using existing `StubEngine`):
    - `handle_input(ToggleHelp)` sets `show_help = true`
    - Second `handle_input(ToggleHelp)` sets `show_help = false`
    - When `show_help = true`, `handle_input(Up)` still routes to panel (selection moves)
    - When `show_help = true`, `handle_input(Quit)` returns `AppAction::Quit`
    - `handle_input(Tab)` from `Focus::ThreadList` with no `stack_state` → focus unchanged
    - `handle_input(Tab)` from `Focus::ThreadList` with `stack_state = Some(...)` →
      `Focus::StackFrames`
    - `handle_input(Tab)` from `Focus::StackFrames` → `Focus::ThreadList`
    - `handle_input(SearchChar('s'))` from `Focus::ThreadList` non-search mode →
      search activated
    - `handle_input(Quit)` from `Focus::ThreadList` (search active) → `AppAction::Quit`
    - `handle_input(Quit)` from `Focus::StackFrames` → `AppAction::Quit`

- [x] Task 7: Validate and finalize (AC: 7)
  - [x] Run `cargo test` — zero failures
  - [x] Run `cargo clippy --all-targets -- -D warnings` — zero warnings
  - [x] Run `cargo fmt` — no changes

## Dev Notes

### Current `input.rs` state

> ⚠️ **Story 7.1 has already modified `input.rs` (uncommitted).** Read the actual
> file before editing — the state below reflects what exists after 7.1's changes.

`from_key` currently maps (post-7.1 working tree state):
- `q` / `Ctrl+C` → `Quit`
- `↑`/`↓`/`Home`/`End`/`PgUp`/`PgDn` → navigation variants
- `Enter` / `Esc` → `Enter` / `Escape`
- `/` → `SearchActivate`
- `Backspace` → `SearchBackspace`
- `f` (`NONE`) → `ToggleFavorite` ← added by Story 7.1
- `F` (`SHIFT`) → `FocusFavorites` ← added by Story 7.1
- `(Char(c), NONE|SHIFT)` catch-all → `SearchChar(c)` ← `s` hits this

To add: `Tab` → `Tab`, `?` → `ToggleHelp`.

The `?` arm must be inserted **before** the `SearchChar` catch-all, alongside the
existing `f`/`F` arms added by Story 7.1.

`s` outside search activates search — handled in `handle_thread_list_input`
non-search branch by matching `InputEvent::SearchChar('s')` before `_ => {}`.

### `?` key routing caveat

`?` on most terminals sends `Char('?')` with `SHIFT` modifier (`?` = Shift+`/`).
The arm must match both modifiers and be placed above the `SearchChar` catch-all:
```rust
(KeyCode::Char('?'), KeyModifiers::NONE | KeyModifiers::SHIFT) => Some(InputEvent::ToggleHelp),
```

### Help panel design

The help panel is a non-focusable bottom section — not a floating overlay. It occupies
`required_height()` lines between the main panels and the status bar. The main panels
shrink via `Constraint::Min(0)` above it; the status bar (`Length(1)`) is always last.

`?` is the only toggle. `Esc` is intentionally excluded — it is already bound to
"go back / cancel search" and must not be overloaded.

Navigation continues normally while the help panel is visible — there is no event
interception, no modal guard.

### `s` key — expected friction

`s` outside search activates search rather than typing `s` into the filter. A user
wanting to filter "scheduler" must press `s` (activates search) then type `s` again.
This is a one-keystroke friction, intentional per the AC, consistent with `vim`-style
modal key bindings. Document in manual testing: expect this behavior.

### `Tab` — `stack_state` behavior

`Tab` from `StackFrames` → `ThreadList` leaves `stack_state` intact. The stack
remains visible in preview mode. This differs from `Esc` which destroys `stack_state`
and collapses the view.

`Tab` during active search (search bar open in thread list) is a **silent no-op** —
it falls through to `_ => {}` in the search-active branch. This is intentional: the
user must close search (`Esc`) before cycling focus. Document this in manual testing.

### `cycle_focus()` extensibility

Use `match` with explicit arms — not `if/else`:
```rust
match self.focus {
    Focus::ThreadList => { ... }
    Focus::StackFrames => { ... }
    // Story 7.1 will add: Focus::Favorites => { ... }
}
```
When Story 7.1 adds `Focus::Favorites`, the compiler forces the update.

### Testing — `StubEngine` et `is_search_active`

`StubEngine` already exists in `app.rs` under `#[cfg(test)]` (line ~914). Use it
directly — do not create a new mock.

`ThreadListState::is_search_active()` is `pub` (thread_list.rs line 166). Use it
directly to assert search activation in the `SearchChar('s')` test.

### Merge conflict — Story 7.1 déjà actif

> ⚠️ **Action immédiate requise avant de commencer.** `input.rs` et `stack_view.rs`
> ont des modifications non commitées de Story 7.1 dans le working tree.

Ne pas travailler sur `input.rs` ou `app.rs` en isolation. Coordonner avec 7.1 :
- Option A : baser la branche 7.3 sur la branche 7.1 et rebase au besoin
- Option B : implémenter 7.3 après que 7.1 soit mergée

Les conflits garantis : `input.rs` (ajout d'arms), `app.rs` (Focus enum, render loop).

### Dependency on Story 7.1

Story 7.3 documents `f` / `F` in the help panel but does not implement them. The
`HelpBar` widget contains:
```rust
// TODO(7.1): remove "(Story 7.1)" annotations once favorites are implemented
```
Removing this comment and the annotations is a task for Story 7.1.

### Key files

```
crates/hprof-tui/src/
├── input.rs          ← add Tab, ToggleHelp variants + from_key arms
├── app.rs            ← add show_help field, cycle_focus(), layout change
└── views/
    ├── mod.rs        ← add `pub mod help_bar;`
    └── help_bar.rs   ← NEW: HelpBar widget + required_height() + ENTRY_COUNT
```

### References

- Current `input.rs`: `crates/hprof-tui/src/input.rs`
- App state machine: `crates/hprof-tui/src/app.rs` — `Focus` enum line 55,
  `handle_input` line 133, `handle_thread_list_input` line 143,
  `handle_stack_frames_input` line 241
- UX keymap spec: `docs/planning-artifacts/ux-design-specification.md` §Keyboard
  Shortcut Vocabulary
- Epic spec: `docs/planning-artifacts/epics.md` — Epic 7, Story 7.3
- Theme: `crates/hprof-tui/src/theme.rs` — `THEME` constant
- Story 7.2 (done): `docs/implementation-artifacts/7-2-theme-system.md`
- Story 7.1 (in-progress): `docs/implementation-artifacts/7-1-favorites-panel.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Implemented Task 1: `InputEvent::Tab` and `InputEvent::ToggleHelp` variants added to
  `input.rs`. `ToggleHelp` arm placed above `SearchChar` catch-all so `?` (SHIFT modifier)
  is correctly intercepted.
- Implemented Task 2: `show_help: bool` field and `cycle_focus()` method added to `App`.
  `handle_input` intercepts `ToggleHelp` globally. `Tab` and `SearchChar('s')` arms added
  to non-search branch of `handle_thread_list_input`; `Tab` added to
  `handle_stack_frames_input`.
- Implemented Task 3: Stateless `HelpBar` widget created in `views/help_bar.rs` with
  `required_height() = 10` (ENTRY_COUNT=11). Two-column layout, 11 entries, key labels
  styled with `THEME.null_value`.
- Implemented Task 4: Render loop updated — `main_area` renamed to `content_area`, help
  panel slot carved out below main panels and above status bar.
- Implemented Task 5: `pub mod help_bar;` added to `views/mod.rs`.
- Task 6: 13 new tests added (3 in `help_bar`, 10 in `app`). All 178 hprof-tui tests pass.
- Task 7: `cargo test` ✓, `cargo clippy --all-targets -- -D warnings` ✓, `cargo fmt` ✓.
- Code review follow-up (2026-03-10): fixed `Tab` handling in thread-list search-active mode and in favorites panel so focus cycling is consistent with the keymap.
- Added tests for `Tab` in search-active thread list and `Tab` from favorites focus.

### File List

- `crates/hprof-tui/src/input.rs`
- `crates/hprof-tui/src/app.rs`
- `crates/hprof-tui/src/views/mod.rs`
- `crates/hprof-tui/src/views/help_bar.rs` (NEW)
- `docs/implementation-artifacts/7-3-keyboard-navigation-map.md`
- `docs/implementation-artifacts/sprint-status.yaml`
- `docs/story-review/codex-story-7-3-keyboard-navigation-map.md`

## Senior Developer Review (AI)

### Reviewer

Codex

### Date

2026-03-10

### Outcome

Changes requested, then fixed in same review pass.

### Findings addressed

- Fixed: `Tab` no-op in `Focus::ThreadList` when search is active.
- Fixed: `Tab` no-op in `Focus::Favorites`.
- Added regression tests for both scenarios.

### Validation

- `cargo test` passed
- `cargo clippy --all-targets -- -D warnings` passed
- `cargo fmt --check` passed

## Change Log

- 2026-03-10: Senior AI review completed; fixed Tab focus cycling gaps (search-active thread list and favorites), added tests, set status to done.
