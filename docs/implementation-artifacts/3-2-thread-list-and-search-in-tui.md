# Story 3.2: Thread List & Search in TUI

Status: done

## Story

As a user,
I want to see the list of all captured threads in a TUI view, browse them with real-time stack
trace preview, and jump to a specific thread by typing a substring match,
So that I can quickly find the thread I'm investigating among hundreds of threads.

## Acceptance Criteria

1. **Given** the engine has completed indexing
   **When** the TUI launches
   **Then** a list of all threads is displayed with their resolved names (FR9)

2. **Given** the thread list is displayed
   **When** I move the cursor to a thread using arrow keys
   **Then** the stack trace panel updates in real-time to show that thread's frames — no Enter
   required (browse-and-preview). Since stack frames are stubs until Story 3.3, the preview panel
   displays an empty frame list with a placeholder message.

3. **Given** each thread entry in the list
   **When** displayed
   **Then** a colored ANSI dot (using 16-color palette) precedes the thread name to indicate thread
   state (RUNNABLE, WAITING, BLOCKED, etc.), and a legend bar below the list maps dot colors to
   state names. Since hprof `START_THREAD` records carry no thread state, all threads show
   `ThreadState::Unknown` (gray dot) in this story; real state resolution is Story 3.4.

4. **Given** a thread is selected and a filter is active
   **When** the filter changes
   **Then** the previously selected thread remains selected if still visible — selection tracks
   `thread_serial`, not list index

5. **Given** the thread list is displayed with 300+ threads
   **When** I type a substring (e.g., "cluster")
   **Then** the list filters to threads whose names contain the substring, case-insensitively (FR10)

6. **Given** the search matches no threads
   **When** I look at the TUI
   **Then** a clear "no match" indicator is shown (e.g., `No threads match "xyz"`)

7. **Given** I press Enter on a thread
   **When** the stack frames are shown
   **Then** the view transitions to the stack frame panel with full keyboard focus (FR11);
   since frames are stubs in 3.3, the panel shows an empty list with a placeholder message

## Tasks / Subtasks

### Task 1: Extend `ThreadInfo` with `ThreadState` (AC: #3)

- [x] **Red**: Write compile-time test — `ThreadInfo` has a `state: ThreadState` field
- [x] **Red**: Write test — `list_threads()` returns `ThreadState::Unknown` for all threads
- [x] **Green**: Add to `crates/hprof-engine/src/engine.rs`:
  ```rust
  /// Thread execution state, inferred from heap dump object data.
  ///
  /// `Unknown` is returned until Story 3.4 resolves state from the
  /// Thread object's instance dump via the object resolver.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ThreadState {
      Unknown,
      Runnable,
      Waiting,
      Blocked,
  }
  ```
- [x] **Green**: Add `pub state: ThreadState` to `ThreadInfo`
- [x] **Green**: Update `engine_impl.rs` — `list_threads()` and `select_thread()` set
  `state: ThreadState::Unknown`
- [x] **Green**: Update `hprof-engine/src/lib.rs` — re-export `ThreadState`
- [x] Fix any test that constructs `ThreadInfo` directly (add `state: ThreadState::Unknown`)

### Task 2: Add ratatui + crossterm to workspace and `hprof-tui` (AC: #1)

- [x] **Green**: Add to workspace `Cargo.toml` `[workspace.dependencies]`:
  ```toml
  ratatui = "0.29"
  crossterm = "0.28"
  ```
  (Verify latest compatible versions with `cargo search ratatui` before committing)
- [x] **Green**: Add to `crates/hprof-tui/Cargo.toml` `[dependencies]`:
  ```toml
  ratatui = { workspace = true }
  crossterm = { workspace = true }
  ```

### Task 3: Create `theme.rs` — centralized style constants (AC: #3)

File: `crates/hprof-tui/src/theme.rs`

- [x] **Red**: Write compile test — all style constants compile and are of type `ratatui::style::Style`
- [x] **Green**: Create `crates/hprof-tui/src/theme.rs`:
  ```rust
  //! Centralized style constants for the hprof-visualizer TUI.
  //!
  //! All colors use the 16 ANSI base colors only (no 256-color or RGB).
  //! Widgets MUST import styles from here — never hardcode colors or
  //! modifiers elsewhere.
  //!
  //! ## Semantic color vocabulary
  //! - Thread state: green (RUNNABLE), yellow (WAITING), red (BLOCKED),
  //!   dark-gray (UNKNOWN)
  //! - Panel: focused border (bold white), unfocused border (dark-gray)
  //! - Search: active input area (cyan fg)
  //! - Selection: reversed video

  use ratatui::style::{Color, Modifier, Style};

  // --- Thread state dots ---
  pub const STATE_RUNNABLE: Style = Style::new()
      .fg(Color::Green);
  pub const STATE_WAITING: Style = Style::new()
      .fg(Color::Yellow);
  pub const STATE_BLOCKED: Style = Style::new()
      .fg(Color::Red);
  pub const STATE_UNKNOWN: Style = Style::new()
      .fg(Color::DarkGray);

  // --- Selection ---
  pub const SELECTED: Style = Style::new()
      .add_modifier(Modifier::REVERSED);

  // --- Panel borders ---
  pub const BORDER_FOCUSED: Style = Style::new()
      .fg(Color::White)
      .add_modifier(Modifier::BOLD);
  pub const BORDER_UNFOCUSED: Style = Style::new()
      .fg(Color::DarkGray);

  // --- Search input ---
  pub const SEARCH_ACTIVE: Style = Style::new()
      .fg(Color::Cyan);
  pub const SEARCH_HINT: Style = Style::new()
      .fg(Color::DarkGray);

  // --- Status bar ---
  pub const STATUS_BAR: Style = Style::new()
      .fg(Color::White)
      .bg(Color::DarkGray);
  pub const STATUS_WARNING: Style = Style::new()
      .fg(Color::Yellow);

  // --- Legend ---
  pub const LEGEND: Style = Style::new()
      .fg(Color::DarkGray);
  ```

### Task 4: Create `input.rs` — keyboard event abstraction (AC: #2, #4, #5, #7)

File: `crates/hprof-tui/src/input.rs`

- [x] **Red**: Write test — `InputEvent::from_key` maps expected crossterm key events
- [x] **Green**: Create `crates/hprof-tui/src/input.rs`:
  ```rust
  //! Keyboard event abstraction layer.
  //!
  //! Translates raw [`crossterm::event::KeyEvent`] into [`InputEvent`]
  //! variants consumed by [`crate::app::App`]. Centralizing key bindings
  //! here makes remapping straightforward.

  use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

  /// High-level TUI input events.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum InputEvent {
      /// Move selection up one item.
      Up,
      /// Move selection down one item.
      Down,
      /// Jump to first item.
      Home,
      /// Jump to last item.
      End,
      /// Confirm selection / enter sub-panel.
      Enter,
      /// Cancel current action or go back.
      Escape,
      /// Activate search mode (thread list only).
      SearchActivate,
      /// A printable character typed during search.
      SearchChar(char),
      /// Delete last character in search input.
      SearchBackspace,
      /// Quit the application.
      Quit,
  }

  /// Translates a [`KeyEvent`] into an [`InputEvent`], returning `None`
  /// for events that have no TUI binding.
  pub fn from_key(key: KeyEvent) -> Option<InputEvent> {
      match (key.code, key.modifiers) {
          (KeyCode::Char('q'), KeyModifiers::NONE) => Some(InputEvent::Quit),
          (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(InputEvent::Quit),
          (KeyCode::Up, _) => Some(InputEvent::Up),
          (KeyCode::Down, _) => Some(InputEvent::Down),
          (KeyCode::Home, _) => Some(InputEvent::Home),
          (KeyCode::End, _) => Some(InputEvent::End),
          (KeyCode::Enter, _) => Some(InputEvent::Enter),
          (KeyCode::Esc, _) => Some(InputEvent::Escape),
          (KeyCode::Char('/'), KeyModifiers::NONE) => Some(InputEvent::SearchActivate),
          (KeyCode::Backspace, _) => Some(InputEvent::SearchBackspace),
          (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
              Some(InputEvent::SearchChar(c))
          }
          _ => None,
      }
  }
  ```

### Task 5: Create `views/thread_list.rs` — `ThreadListState` + `SearchableList` (AC: #1–#7)

Files: `crates/hprof-tui/src/views/mod.rs`,
       `crates/hprof-tui/src/views/thread_list.rs`

- [x] **Red**: Write test — `ThreadListState::new` with 3 threads selects first thread
- [x] **Red**: Write test — `move_down()` on 3-thread list moves to second thread
- [x] **Red**: Write test — `move_up()` at top does nothing
- [x] **Red**: Write test — `move_up()` from last item moves to second-to-last
- [x] **Red**: Write test — `apply_filter("worker")` on ["main","worker-1","worker-2"] keeps both
  workers, preserves selection on "worker-1" if it was selected
- [x] **Red**: Write test — `apply_filter("xyz")` yields empty filtered list
- [x] **Red**: Write test — `apply_filter("")` restores full list
- [x] **Red**: Write test — selection tracks `thread_serial` not index: select "worker-1", apply
  filter "worker", selection is still "worker-1"
- [x] **Red**: Write test — `selected_serial()` returns `None` when filtered list is empty
- [x] **Green**: Create `crates/hprof-tui/src/views/mod.rs`:
  ```rust
  //! TUI view modules: thread list, stack trace preview, status bar.
  pub mod thread_list;
  pub mod stack_view;
  pub mod status_bar;
  ```
- [x] **Green**: Create `crates/hprof-tui/src/views/thread_list.rs`:

  `ThreadListState` struct:
  ```rust
  pub struct ThreadListState {
      /// Full unfiltered thread list, sorted by thread_serial.
      threads: Vec<ThreadInfo>,
      /// Active case-insensitive substring filter. Empty = no filter.
      filter: String,
      /// Thread serials in display order after filtering.
      filtered_serials: Vec<u32>,
      /// Serial of the currently highlighted thread.
      selected_serial: Option<u32>,
      /// ratatui list state (owns the visual scroll offset).
      list_state: ListState,
      /// Whether the search input box is focused.
      pub search_active: bool,
  }
  ```

  Key methods:
  - `new(threads: Vec<ThreadInfo>) -> Self` — builds filtered_serials = all serials, selects
    first
  - `apply_filter(&mut self, query: &str)` — rebuilds `filtered_serials` from `threads` whose
    name contains `query` case-insensitively; if previously selected serial is still in the
    filtered list, keeps it; otherwise selects first visible thread
  - `move_down(&mut self)` / `move_up(&mut self)` — move within `filtered_serials`, wrapping at
    bounds
  - `move_home(&mut self)` / `move_end(&mut self)`
  - `selected_serial(&self) -> Option<u32>` — returns `selected_serial`

  `SearchableList` widget that renders a ratatui `List` with:
  - Panel `Block` (border + title "Threads (N)" where N is filtered count)
  - Border style from `theme::BORDER_FOCUSED` / `BORDER_UNFOCUSED` based on focus
  - Search input row at top: `/ <query>_` when active, `Press / to search` (gray) when inactive
  - List items: `o ` (dot colored by thread state from `theme`) + thread name
  - Selected item uses `theme::SELECTED`
  - Legend row at bottom: `o Running  o Waiting  o Blocked  o Unknown` with matching colors
  - "No threads match ..." message when filtered list is empty (AC #6)

  Implement `StatefulWidget` for `SearchableList` to pair with `ThreadListState`.

- [x] Add `//!` module docstring to `thread_list.rs`

### Task 6: Create `views/stack_view.rs` — preview panel stub (AC: #2, #7)

File: `crates/hprof-tui/src/views/stack_view.rs`

- [x] **Green**: Create stub panel that renders:
  - `Block` with title "Stack Frames" and border (focused/unfocused)
  - When no thread selected: empty with hint "Select a thread"
  - When thread selected but frames empty (stub): placeholder "No frames (Story 3.3)"
  - Focused state receives keyboard input in future stories
- [x] Add `//!` module docstring

### Task 7: Create `views/status_bar.rs` — file info + thread state (AC: #3)

File: `crates/hprof-tui/src/views/status_bar.rs`

- [x] **Green**: Create status bar widget rendering one line:
  `<filename>  |  <N> threads  |  <thread-name>  <STATE>  |  [q]uit  [/]search  [Esc]back`
  - Thread state text (e.g., `UNKNOWN`) shown for selected thread — full state name (UX spec)
  - Style from `theme::STATUS_BAR`
- [x] Add `//!` module docstring

### Task 8: Create `app.rs` — TUI application loop (AC: #1–#7)

File: `crates/hprof-tui/src/app.rs`

- [x] **Red**: Write test — `App::new(engine)` builds without panic when engine has 0 threads
- [x] **Red**: Write test — `App::new(engine)` builds without panic when engine has 3 threads
- [x] **Red**: Write test — `handle_input(InputEvent::Down)` in `Focus::ThreadList` updates
  selection
- [x] **Red**: Write test — `handle_input(InputEvent::SearchActivate)` sets
  `thread_list.search_active = true`
- [x] **Red**: Write test — `handle_input(InputEvent::SearchChar('x'))` appends to filter query
- [x] **Red**: Write test — `handle_input(InputEvent::Escape)` in search mode clears filter and
  deactivates search
- [x] **Red**: Write test — `handle_input(InputEvent::Enter)` in `Focus::ThreadList` transitions
  to `Focus::StackFrames`
- [x] **Red**: Write test — `handle_input(InputEvent::Escape)` in `Focus::StackFrames` returns
  to `Focus::ThreadList`
- [x] **Red**: Write test — `handle_input(InputEvent::Quit)` returns `AppAction::Quit`
- [x] **Green**: Create `crates/hprof-tui/src/app.rs`:

  ```rust
  //! TUI application loop and top-level state machine.
  //!
  //! `App` owns the navigation engine and all UI state. `run_tui` sets up
  //! the terminal and drives the 16ms event loop (60 fps target, NFR4).

  pub enum Focus {
      ThreadList,
      StackFrames,
  }

  pub enum AppAction {
      Continue,
      Quit,
  }

  pub struct App<E: NavigationEngine> {
      engine: E,
      thread_list: ThreadListState,
      focus: Focus,
      filename: String,
  }

  impl<E: NavigationEngine> App<E> {
      pub fn new(engine: E, filename: String) -> Self { ... }
      /// Process one input event; returns `AppAction`.
      pub fn handle_input(&mut self, event: InputEvent) -> AppAction { ... }
      /// Render current state to the ratatui frame.
      pub fn render(&mut self, frame: &mut Frame) { ... }
  }
  ```

  `render` layout:
  - `Layout::horizontal` splits terminal width: ~30% left (thread list), ~70% right (stack view)
  - Status bar: 1-line `Rect` at the bottom (subtract 1 row from main area)
  - `SearchableList` rendered into left panel with `focus == Focus::ThreadList`
  - `StackView` rendered into right panel with `focus == Focus::StackFrames`
  - `StatusBar` rendered into bottom row

  Event loop in `run_tui`:
  ```rust
  pub fn run_tui<E: NavigationEngine>(engine: E, filename: String) -> io::Result<()> {
      // enable_raw_mode + EnterAlternateScreen
      // terminal.draw(|f| app.render(f))
      // poll(Duration::from_millis(16)) for key events
      // on AppAction::Quit → cleanup + return
  }
  ```
  Always restore terminal (disable_raw_mode + LeaveAlternateScreen) even on error via a
  cleanup guard or explicit cleanup in the error path.

### Task 9: Update `hprof-tui/src/lib.rs`

- [x] Add module declarations:
  ```rust
  pub mod app;
  pub mod input;
  pub mod theme;
  pub mod views;
  ```
- [x] Re-export `run_tui` from `app`:
  ```rust
  pub use app::run_tui;
  ```
- [x] Update `//!` crate docstring to mention the new TUI entry point

### Task 10: Wire `Engine::from_file_with_progress` + launch TUI in `hprof-cli` (AC: #1)

- [x] **Green**: Add to `crates/hprof-engine/src/engine_impl.rs`:
  ```rust
  /// Opens `path`, runs the first-pass indexer with progress callbacks, and
  /// returns a ready-to-use engine.
  ///
  /// `progress_fn(bytes)` — called every ~4 MiB during the scan phase.
  /// `filter_progress_fn(done, total)` — called after each segment filter
  /// built.
  ///
  /// ## Errors
  /// See [`Engine::from_file`].
  pub fn from_file_with_progress(
      path: &Path,
      _config: &EngineConfig,
      progress_fn: impl FnMut(u64),
      filter_progress_fn: impl FnMut(usize, usize),
  ) -> Result<Self, HprofError> {
      let hfile = HprofFile::from_path_with_progress(
          path, progress_fn, filter_progress_fn,
      )?;
      Ok(Self { hfile })
  }
  ```
- [x] **Red**: Write test — `Engine::from_file_with_progress` on valid file calls progress
  callback at least once
- [x] **Green**: Update `hprof-engine/src/lib.rs` — re-export `Engine` already done; verify
  no changes needed
- [x] **Green**: Update `crates/hprof-cli/src/main.rs` — replace
  `open_hprof_file_with_progress` + separate indexing with:
  ```rust
  use hprof_engine::{Engine, EngineConfig};
  use hprof_tui::run_tui;

  // In run():
  let config = EngineConfig::default();
  let engine = Engine::from_file_with_progress(
      &path,
      &config,
      |bytes| reporter.on_bytes_processed(bytes),
      |done, total| filter_reporter.on_segment_built(done, total),
  )
  .map_err(CliError::OpenFailed)?;
  filter_reporter.finish();
  // Print summary using engine's index data or keep IndexSummary approach
  // (see Dev Notes for resolution)
  run_tui(engine, path.display().to_string())
      .map_err(CliError::TuiFailed)?;
  ```
  Add `TuiFailed(std::io::Error)` variant to `CliError`.
- [x] Update `CliError::Display` for the new variant

### Task 11: Verify all checks pass

- [x] `cargo test -p hprof-engine` — all engine tests pass (including updated ThreadInfo)
- [x] `cargo test -p hprof-tui` — all TUI unit tests pass
- [x] `cargo test -p hprof-cli` — all CLI tests pass
- [x] `cargo test --workspace`
- [x] `cargo clippy --workspace -- -D warnings`
- [x] `cargo fmt -- --check`
- [x] Manual smoke test: `cargo run -- <some.hprof>` — TUI launches, thread list displays, search
  works, q quits

## Dev Notes

### `ThreadState` and hprof Format Limitation

The hprof `START_THREAD` record (tag `0x06`) does NOT carry a thread execution state field.
`HprofThread` only has: `thread_serial`, `object_id`, `stack_trace_serial`, `name_string_id`,
`group_name_string_id`, `group_parent_name_string_id`.

Real thread state (RUNNABLE/WAITING/BLOCKED) is stored in the Thread object's instance data in
the heap dump segment — accessible only via object resolution (Story 3.4). Until then, all
threads show `ThreadState::Unknown` with a gray dot. This is correct and expected behavior per
the story AC which acknowledges the limitation.

Add `ThreadState` to `engine.rs` alongside `ThreadInfo`. Update all direct `ThreadInfo`
constructions in tests to include `state: ThreadState::Unknown`.

### Updated `ThreadInfo` struct

```rust
/// Minimal information about a Java thread found in the heap dump.
#[derive(Debug)]
pub struct ThreadInfo {
    /// The serial number assigned to this thread in the `START_THREAD` record.
    pub thread_serial: u32,
    /// Thread name resolved from structural strings, or `"<unknown:{id}>"` if
    /// the string record is missing.
    pub name: String,
    /// Execution state. `Unknown` until Story 3.4 resolves it via object
    /// resolution.
    pub state: ThreadState,
}
```

### `IndexSummary` and `Engine::from_file_with_progress`

The legacy `open_hprof_file_with_progress` (in `hprof-engine/src/lib.rs`) returns `IndexSummary`
for the progress summary displayed after indexing. When switching to
`Engine::from_file_with_progress`, the CLI needs summary data too.

**Solution**: Keep `open_hprof_file_with_progress` as-is (it is still used by the existing CLI
test `run_succeeds_for_valid_hprof_header_file`). The new `Engine::from_file_with_progress` is
an additional method. After calling it, the CLI can derive a lightweight summary from
`engine.index_summary()` or skip the summary printout for TUI mode. The simplest approach: in
TUI mode skip the summary line (it'll be replaced by the full TUI view anyway). Keep the
`reporter.finish(&summary)` line ONLY for headless/test mode — or simply remove it from the
`run()` function since the TUI is now the primary UI.

For the CLI test `run_succeeds_for_valid_hprof_header_file`: it will need updating since `run()`
now launches a TUI which requires a real terminal. Adjust the test to either:
- Test only the `parse_hprof_path` part (already covered)
- Mock/stub the TUI call (not ideal)
- Accept that the integration test now requires a different fixture approach

Recommended: remove or restructure `run_succeeds_for_valid_hprof_header_file` since the full
`run()` now has a terminal dependency. Keep the unit tests for `parse_hprof_path` and add a
test for `Engine::from_file_with_progress` at the engine level.

### ratatui `StatefulWidget` Pattern

`SearchableList` should implement `StatefulWidget<State = ThreadListState>` so ratatui manages
the scroll offset:

```rust
impl StatefulWidget for SearchableList {
    type State = ThreadListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        // Build ListItems from state.visible_threads()
        // Render Block, search area, List, legend
    }
}
```

The `ListState` inside `ThreadListState` drives ratatui's scroll offset via `list_state.select(index)`.

### Layout Calculation

```
┌─────────────────────────────────────────────────────────────────┐
│ ┌──── Threads (N) ────┐ ┌──── Stack Frames ──────────────────┐ │
│ │ / <query>_          │ │                                     │ │
│ │ o main              │ │ (empty — Story 3.3)                 │ │
│ │ > o worker-1        │ │                                     │ │  <- selected
│ │ o worker-2          │ │                                     │ │
│ │                     │ │                                     │ │
│ │ o Run o Wt o Blk    │ │                                     │ │  <- legend
│ └─────────────────────┘ └─────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│ heap.hprof  |  3 threads  |  worker-1  UNKNOWN  |  [q]quit     │
└─────────────────────────────────────────────────────────────────┘
```

Use `Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])` for the main
area, and `Layout::vertical([Constraint::Min(0), Constraint::Length(1)])` for the terminal to
carve out the status bar.

### Terminal Cleanup

Always restore the terminal on exit — even if the TUI panics. Use a cleanup pattern:

```rust
pub fn run_tui<E: NavigationEngine>(engine: E, filename: String) -> io::Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, engine, filename);

    // Always restore, even if run_loop errored
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    result
}
```

### `App::render` — thread preview without Enter

Browse-and-preview (AC #2) is implemented by passing `state.selected_serial()` to the stack view
on every render. Since `get_stack_frames()` is a stub returning `vec![]`, the preview panel will
always show the placeholder message. No extra work needed for AC #2 in this story.

### Selection stability across filter changes (AC #4)

In `ThreadListState::apply_filter`:
1. Rebuild `filtered_serials` from all threads matching the new query
2. If `selected_serial` is `Some(s)` and `s` is in `filtered_serials` → keep it, update
   `list_state` to its new position
3. If `selected_serial` is `Some(s)` but `s` is NOT in `filtered_serials` → set
   `selected_serial` to `filtered_serials.first().copied()`
4. If `filtered_serials` is empty → set `selected_serial = None`

### Module Structure

New files in this story:

```
crates/hprof-tui/src/
├── lib.rs             # updated: new pub mods + run_tui re-export
├── app.rs             # NEW: App<E> struct + run_tui() entry point
├── input.rs           # NEW: InputEvent + from_key()
├── theme.rs           # NEW: style constants (16 ANSI colors)
├── views/
│   ├── mod.rs         # NEW: sub-module declarations
│   ├── thread_list.rs # NEW: ThreadListState + SearchableList widget
│   ├── stack_view.rs  # NEW: StackView stub panel
│   └── status_bar.rs  # NEW: StatusBar widget
└── progress.rs        # unchanged
```

```
crates/hprof-engine/src/
├── engine.rs          # updated: ThreadState enum + ThreadInfo.state field
└── engine_impl.rs     # updated: from_file_with_progress() + state=Unknown in list/select
```

```
crates/hprof-cli/src/
└── main.rs            # updated: use Engine::from_file_with_progress + run_tui
```

```
Cargo.toml (workspace)  # updated: ratatui + crossterm in workspace.dependencies
crates/hprof-tui/Cargo.toml  # updated: ratatui + crossterm dependencies
```

### `hprof-cli` Test Update

`run_succeeds_for_valid_hprof_header_file` in `main.rs` currently calls `run()` which will now
try to launch a TUI. This test must be removed or replaced with engine-level tests. Replace with:
- Keep `parse_hprof_path` tests (unchanged)
- Keep `run_returns_metadata_failed_for_missing_path` if it can be adapted (it will now fail at
  `Engine::from_file_with_progress`, which is fine — the error type changes to `OpenFailed`)
- Remove the `run_succeeds_for_valid_hprof_header_file` test — its coverage is now provided by
  `Engine::from_file_with_progress` test in `hprof-engine`

### Previous Story Intelligence (3.1)

- `Engine` is in `crates/hprof-engine/src/engine_impl.rs`, wraps `HprofFile`
- `HprofFile::from_path_with_progress(path, progress_fn, filter_fn)` already exists
  (used internally by `open_hprof_file_with_progress`) — use this in
  `Engine::from_file_with_progress`
- All 160 workspace tests pass from Story 3.1
- `ThreadInfo` currently has only `thread_serial: u32` and `name: String` — adding `state` is
  additive but breaks direct struct construction in tests — fix all such constructions

### Git Intelligence (recent commits)

```
9deea66 feat: NavigationEngine trait and Engine factory (Story 3.1)
f32720f feat: add ETA to segment filter progress bar
0199f92 Fix: freeze scan bar during filter-build phase
```

Pattern: new modules in `hprof-tui` follow the same `pub mod <name>` + re-export pattern
established in `hprof-engine`.

### References

- [Source: docs/planning-artifacts/epics.md#Story 3.2]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/ux-design-specification.md#Thread State Indicators]
- [Source: docs/planning-artifacts/ux-design-specification.md#Color Palette]
- [Source: docs/planning-artifacts/ux-design-specification.md#SearchableList]
- [Source: docs/planning-artifacts/ux-design-specification.md#Layout]
- [Source: crates/hprof-engine/src/engine.rs]
- [Source: crates/hprof-engine/src/engine_impl.rs]
- [Source: crates/hprof-tui/src/lib.rs]
- [Source: crates/hprof-cli/src/main.rs]
- [Source: docs/implementation-artifacts/3-1-navigation-engine-trait-and-engine-factory.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- Fixed orphan rule violation: added `#[derive(Clone)]` to `ThreadInfo` in `hprof-engine`
  instead of implementing `Clone` in `hprof-tui` tests.
- Upgraded ratatui to 0.30.0 / crossterm to 0.29.0 (latest at implementation date).
- `EngineConfig::default()` → `EngineConfig` in CLI (clippy unit struct lint).
- Collapsed nested `if` in `run_loop` to satisfy clippy::collapsible_if.

### Code Review Fixes (AI)

**H1 Fixed** — `list_threads()` no longer called in `render()` every frame. `thread_count`
captured once in `App::new()` and stored as a field. Eliminates 60 Hz `Vec<ThreadInfo>` allocation.

**H2 Fixed** — Removed duplicate `App::search_query` field. `ThreadListState::filter()` is now
the single source of truth for the search query. `handle_thread_list_input` builds the updated
query from `thread_list.filter()` on each character event.

**M1 Fixed** — Added `TerminalGuard` struct implementing `Drop` (disable_raw_mode +
LeaveAlternateScreen + cursor::Show). Guard declared after `terminal` in `run_tui`, so it
drops first (LIFO). Terminal cleanup now runs even on panic.

**M2 Fixed** — `App::render` now uses `StatusBar` widget from `views/status_bar.rs` instead
of inlining status bar logic. Dead code eliminated.

**M3 Fixed** — Naturally resolved by M2: `state_label` now lives only in `status_bar.rs`
(promoted to `pub(crate)`). No duplication remains.

**M4 Fixed** — `search_active` made private on `ThreadListState`. Added `activate_search()`,
`deactivate_search()`, `is_search_active()` methods. All callers updated.

**L2 Fixed** — Added 3 tests to `status_bar.rs`: `state_label` exhaustiveness, empty label
guard, integration of name + state rendering.

### Completion Notes List

- Task 1: Added `ThreadState` enum + `state: ThreadState` field to `ThreadInfo`. All
  `list_threads()`/`select_thread()` calls set `state: ThreadState::Unknown`. Re-exported from
  `hprof-engine/src/lib.rs`. Added `#[derive(Clone)]` to `ThreadInfo`.
- Task 2: ratatui 0.30 + crossterm 0.29 added to workspace and `hprof-tui`.
- Task 3: `theme.rs` created with 12 ANSI-16 style constants, compile test passes.
- Task 4: `input.rs` created with `InputEvent` enum + `from_key()`, 8 tests cover all bindings.
- Task 5: `views/thread_list.rs` + `views/mod.rs` — `ThreadListState` with filter/selection
  stability (tracks serial not index), `SearchableList` `StatefulWidget`. 9 unit tests.
- Task 6: `views/stack_view.rs` — stub panel with "No frames (Story 3.3)" placeholder.
- Task 7: `views/status_bar.rs` — one-line status bar widget.
- Task 8: `app.rs` — `App<E: NavigationEngine>` state machine + `run_tui` event loop. 9 unit
  tests via `StubEngine`. Terminal cleanup pattern applied.
- Task 9: `hprof-tui/src/lib.rs` updated — new `pub mod` declarations + `pub use app::run_tui`.
- Task 10: `Engine::from_file_with_progress` added to `engine_impl.rs`. CLI `run()` updated to
  use it and launch TUI. `CliError::TuiFailed` variant added. `run_succeeds_for_valid_hprof_header_file`
  removed (TUI now requires a real terminal; coverage provided by engine-level test).
- Task 11: 190 workspace tests pass. clippy clean. fmt clean.

### File List

- `Cargo.toml` — added ratatui 0.30, crossterm 0.29 to workspace.dependencies
- `crates/hprof-engine/src/engine.rs` — `ThreadState` enum, `ThreadInfo.state`, `Clone` derive
- `crates/hprof-engine/src/engine_impl.rs` — `from_file_with_progress`, state=Unknown in list/select
- `crates/hprof-engine/src/lib.rs` — re-export `ThreadState`
- `crates/hprof-tui/Cargo.toml` — ratatui + crossterm dependencies
- `crates/hprof-tui/src/lib.rs` — new module declarations + run_tui re-export
- `crates/hprof-tui/src/app.rs` — NEW: App<E>, Focus, AppAction, run_tui, run_loop
- `crates/hprof-tui/src/input.rs` — NEW: InputEvent, from_key()
- `crates/hprof-tui/src/theme.rs` — NEW: 12 ANSI-16 style constants
- `crates/hprof-tui/src/views/mod.rs` — NEW: sub-module declarations
- `crates/hprof-tui/src/views/thread_list.rs` — NEW: ThreadListState, SearchableList
- `crates/hprof-tui/src/views/stack_view.rs` — NEW: StackView stub
- `crates/hprof-tui/src/views/status_bar.rs` — NEW: StatusBar widget
- `crates/hprof-cli/src/main.rs` — Engine::from_file_with_progress + run_tui wiring
- `docs/implementation-artifacts/3-2-thread-list-and-search-in-tui.md` — story updated
- `docs/implementation-artifacts/sprint-status.yaml` — 3-2 → review

### Code Review Fixes (AI) - Round 2 (2026-03-07)

- **H1 Fixed** — Implemented browse-and-preview (AC #2): stack preview now updates while moving
  in thread list without pressing Enter. Added `preview_stack_state` and refresh logic on
  selection/filter navigation in `app.rs`.
- **M1 Fixed** — `TerminalGuard` is now created before `Terminal::new`; cleanup is guaranteed if
  terminal initialization fails after raw mode + alternate screen activation.
- Added tests:
  - `app_new_initializes_stack_preview_for_selected_thread`
  - `moving_thread_selection_updates_stack_preview_without_enter`

## Senior Developer Review (AI)

### Review Date

2026-03-07

### Reviewer

Codex (Amelia / Dev Agent execution)

### Outcome

Approved after Round 2 fixes.

### Notes

- Story 3.2 AC #2 (browse-and-preview without Enter) is now satisfied.
- Reliability gap on terminal initialization cleanup is closed.
- Workspace validation passed after fixes (`cargo test --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo fmt -- --check`).

## Change Log

- 2026-03-07 — Applied Round 2 review fixes for Story 3.2:
  - Browse-and-preview implementation in `crates/hprof-tui/src/app.rs`
  - Early terminal cleanup guard in `crates/hprof-tui/src/app.rs`
  - Added regression tests for preview behavior.
