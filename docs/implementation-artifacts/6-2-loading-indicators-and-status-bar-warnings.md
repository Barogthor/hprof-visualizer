# Story 6.2: Loading Indicators & Status Bar Warnings

Status: review

## Story

As a user,
I want to see loading indicators when operations take more than 1 second and clear
warnings with a persistent status bar indicator when working on corrupted or truncated
files,
so that I always know what the tool is doing and whether the data I'm viewing might be
incomplete.

## Acceptance Criteria

### AC1: Loading indicator after 1 second

**Given** an object expansion or collection page load takes more than 1 second
**When** the operation is in progress
**Then** a loading indicator is displayed in the TUI (FR28)

### AC2: Loading indicator disappears on completion

**Given** the loading indicator is shown
**When** the operation completes
**Then** the indicator disappears and the results are displayed

### AC3: Persistent "incomplete file" status bar indicator

**Given** a truncated or corrupted hprof file was indexed with warnings
**When** the TUI session is active
**Then** the status bar shows "Incomplete file — X% indexed" throughout the session
(FR30)

### AC4: Non-fatal navigation warning displayed without crash

**Given** a non-fatal parsing error occurs during navigation (e.g., unresolved
object reference)
**When** the error is encountered
**Then** a warning is added to the session warning log and navigation continues
(FR29, NFR6)

### AC5: Most recent warning visible with total count

**Given** one or more warnings have been collected during the session
**When** the user looks at the status bar
**Then** the warning count is shown and the most recent warning text is visible

### AC6: Periodic memory log to stderr

**Given** the engine is running with an active memory budget
**When** 20 seconds have elapsed since the last memory log (range 15–30 s)
**Then** an INFO-level line is emitted to stderr:
`[memory] cache 42 MB / 512 MB budget | skeleton 38 MB (non-evictable)`

## Tasks / Subtasks

- [x] Task 1: Extend `NavigationEngine` trait with `indexing_ratio`,
  `is_fully_indexed`, and `skeleton_bytes` (AC: 3, 6)
  - [x] In `crates/hprof-engine/src/engine.rs`, add to `NavigationEngine`:
    ```rust
    /// Returns the percentage of attempted records successfully indexed
    /// (100.0 when the file is complete, < 100.0 for truncated/corrupt files).
    fn indexing_ratio(&self) -> f64;

    /// Returns `true` when every attempted record was successfully indexed.
    ///
    /// Uses integer comparison to avoid floating-point imprecision.
    /// Prefer this over `indexing_ratio() == 100.0`.
    fn is_fully_indexed(&self) -> bool;

    /// Returns the byte size of the non-evictable skeleton — the
    /// `PreciseIndex` held permanently in `HprofFile`.
    fn skeleton_bytes(&self) -> usize;
    ```
  - [x] In `crates/hprof-engine/src/engine_impl.rs`, implement for `Engine`:
    ```rust
    fn indexing_ratio(&self) -> f64 {
        if self.hfile.records_attempted == 0 { return 100.0; }
        self.hfile.records_indexed as f64
            / self.hfile.records_attempted as f64 * 100.0
    }

    fn is_fully_indexed(&self) -> bool {
        self.hfile.records_attempted == 0
            || self.hfile.records_indexed >= self.hfile.records_attempted
    }

    fn skeleton_bytes(&self) -> usize {
        use hprof_api::MemorySize;
        self.hfile.index.memory_size()
    }
    ```
  - [x] Add stub implementations to `DummyEngine` in `engine.rs` tests
    (`indexing_ratio` → `100.0`, `is_fully_indexed` → `true`,
    `skeleton_bytes` → `0`)
  - [x] Unit tests in `engine.rs` test module:
    `indexing_ratio_100_for_complete_file`,
    `indexing_ratio_partial_for_truncated_file`,
    `is_fully_indexed_true_for_complete_file`,
    `is_fully_indexed_false_for_partial_file`,
    `skeleton_bytes_positive_for_real_file`

- [x] Task 2: Create `crates/hprof-tui/src/warnings.rs` (AC: 4, 5)
  - [x] Add module-level docstring `//!`
  - [x] Define `WarningLog` with a bounded capacity to prevent unbounded growth
    on pathological inputs (e.g., thousands of unresolvable objects):
    ```rust
    /// Maximum number of session warnings retained. Older warnings are kept;
    /// new ones are silently dropped once the cap is reached.
    const MAX_SESSION_WARNINGS: usize = 500;

    /// Accumulates non-fatal session warnings for display in the status bar.
    ///
    /// Capped at [`MAX_SESSION_WARNINGS`] entries.
    #[derive(Debug, Default)]
    pub(crate) struct WarningLog {
        messages: Vec<String>,
    }

    impl WarningLog {
        /// Adds a warning. No-op once [`MAX_SESSION_WARNINGS`] is reached.
        pub(crate) fn add(&mut self, msg: String) {
            if self.messages.len() < MAX_SESSION_WARNINGS {
                self.messages.push(msg);
            }
        }
        pub(crate) fn count(&self) -> usize { self.messages.len() }
        pub(crate) fn last(&self) -> Option<&str> {
            self.messages.last().map(|s| s.as_str())
        }
    }
    ```
  - [x] Expose module in `crates/hprof-tui/src/lib.rs`:
    `pub(crate) mod warnings;`
  - [x] Unit tests: `add_and_count`, `last_returns_most_recent`,
    `last_on_empty_returns_none`,
    `add_drops_message_when_cap_reached`

- [x] Task 3: Extend `StatusBar` with incomplete-file and last-warning fields
  (AC: 3, 5)
  - [x] In `crates/hprof-tui/src/views/status_bar.rs`, add two fields:
    ```rust
    /// Indexing completeness ratio (0.0–100.0). `None` = fully indexed.
    pub file_indexed_pct: Option<f64>,
    /// Most recent session warning text, if any.
    pub last_warning: Option<&'a str>,
    ```
  - [x] Update `Widget::render`:
    - When `file_indexed_pct` is `Some(pct)`: prepend
      `"[!] Incomplete file — {pct:.0}% indexed  |  "` to the line, styled
      with `theme::STATUS_BAR` (same as rest of bar)
    - When `last_warning` is `Some(w)`: show the warning text after the
      count: `"[!] {count} warnings ({w}) — see stderr"` — truncate `w` at
      40 chars with `…` if needed
  - [x] Update the existing `warning_count_nonzero_renders_warning_indicator`
    test to pass `last_warning: None`; add new tests:
    `incomplete_file_shown_in_status_bar`,
    `last_warning_appended_in_status_bar`,
    `last_warning_truncated_at_40_chars`

- [x] Task 4: 1-second loading indicator delay (AC: 1, 2)
  - [ ] In `crates/hprof-tui/src/app.rs`, define the threshold constant and
    private structs near the top of the file:
    ```rust
    /// Delay before showing the loading spinner for expansions/page loads.
    /// Operations completing before this threshold show no spinner.
    const EXPANSION_LOADING_THRESHOLD: Duration = Duration::from_secs(1);

    struct PendingExpansion {
        rx: Receiver<Option<Vec<FieldInfo>>>,
        started: std::time::Instant,
        loading_shown: bool,
    }

    struct PendingPage {
        rx: Receiver<Option<CollectionPage>>,
        started: std::time::Instant,
        loading_shown: bool,
    }
    ```
  - [x] Change field types:
    `pending_expansions: HashMap<u64, PendingExpansion>`
    `pending_pages: HashMap<(u64, usize), PendingPage>`
  - [x] In `start_object_expansion`: store `PendingExpansion { rx, started:
    Instant::now(), loading_shown: false }`, **do NOT** call
    `set_expansion_loading` here
  - [x] In `poll_expansions`, use `iter_mut()` to handle threshold promotion
    and completion in a single pass:
    ```rust
    pub fn poll_expansions(&mut self) {
        let mut done = Vec::new();
        for (&object_id, pe) in self.pending_expansions.iter_mut() {
            match pe.rx.try_recv() {
                Ok(Some(fields)) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_done(object_id, fields);
                    }
                    done.push(object_id);
                }
                Ok(None) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(
                            object_id,
                            "Failed to resolve object".to_string(),
                        );
                    }
                    // Task 5 adds: self.warnings.add(...) here
                    done.push(object_id);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !pe.loading_shown
                        && pe.started.elapsed() >= EXPANSION_LOADING_THRESHOLD
                    {
                        if let Some(s) = &mut self.stack_state {
                            s.set_expansion_loading(object_id);
                        }
                        pe.loading_shown = true;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    if let Some(s) = &mut self.stack_state {
                        s.set_expansion_failed(
                            object_id,
                            "Worker thread disconnected".to_string(),
                        );
                    }
                    // Task 5 adds: self.warnings.add(...) here
                    done.push(object_id);
                }
            }
        }
        for id in done { self.pending_expansions.remove(&id); }
    }
    ```
    Note: `iter_mut()` allows mutating `pe.loading_shown` in place without a
    separate collect-then-mutate pass.
  - [x] Apply the same `iter_mut()` pattern and `EXPANSION_LOADING_THRESHOLD`
    to `poll_pages` for `PendingPage`
  - [x] Tests: `loading_indicator_not_shown_before_1_second` (complete
    immediately → no Loading state set),
    `loading_indicator_shown_if_not_yet_complete_after_1_second` (slow mock
    with unsent channel, check state after injecting a past `started` instant)

- [x] Task 5: Collect navigation warnings into `WarningLog` (AC: 4, 5)
  - [x] In `crates/hprof-tui/src/app.rs`, replace `app_warnings: Vec<String>`
    with `warnings: WarningLog` (from `crate::warnings::WarningLog`)
  - [x] In `poll_expansions`, when `Ok(None)` (object not resolvable):
    ```rust
    self.warnings.add(format!("Object 0x{object_id:X} could not be resolved"));
    ```
  - [x] In `poll_expansions`, when `Disconnected`:
    ```rust
    self.warnings.add(format!("Worker disconnected for object 0x{object_id:X}"));
    ```
  - [x] Update `App::render` / `StatusBar` construction — extract `last_warning`
    into a local variable first to avoid borrow conflicts with other `self`
    accesses:
    ```rust
    let last_warning: Option<String> = self.warnings.last().map(str::to_string);
    // Use is_fully_indexed() (integer comparison) rather than
    // indexing_ratio() == 100.0 to avoid floating-point imprecision.
    let file_indexed_pct = if self.engine.is_fully_indexed() {
        None
    } else {
        Some(self.engine.indexing_ratio())
    };
    StatusBar {
        // ...
        warning_count: self.warning_count + self.warnings.count(),
        last_warning: last_warning.as_deref(),
        file_indexed_pct,
    }
    ```
  - [x] Update all existing `App` tests that reference `app_warnings` field
  - [x] Tests: `failed_expansion_adds_warning_to_log`,
    `disconnected_expansion_adds_warning_to_log`

- [x] Task 6: Periodic memory log to stderr — `dev-profiling` only (AC: 6)
  - [x] In `crates/hprof-tui/src/lib.rs`, add a `mem_log!` macro alongside
    the existing pattern (feature already declared in `hprof-tui/Cargo.toml` —
    no `Cargo.toml` change needed):
    ```rust
    #[cfg(feature = "dev-profiling")]
    macro_rules! mem_log {
        ($($arg:tt)*) => { eprintln!($($arg)*) };
    }
    #[cfg(not(feature = "dev-profiling"))]
    macro_rules! mem_log {
        ($($arg:tt)*) => {};
    }
    ```
  - [x] In `crates/hprof-tui/src/app.rs`, add field:
    `last_memory_log: std::time::Instant`
  - [x] Initialize in `App::new`: `last_memory_log: std::time::Instant::now()`
  - [x] Extract a pure formatting function (testable without stderr capture):
    ```rust
    pub(crate) fn format_memory_log(
        cache_bytes: usize,
        budget_bytes: u64,
        skeleton_bytes: usize,
    ) -> String {
        let cache_mb = cache_bytes / (1024 * 1024);
        let budget_mb = budget_bytes / 1_048_576;
        let skeleton_mb = skeleton_bytes / (1024 * 1024);
        format!(
            "[memory] cache {cache_mb} MB / {budget_mb} MB budget \
             | skeleton {skeleton_mb} MB (non-evictable)"
        )
    }
    ```
  - [x] In `App::render`, after polling, emit via `mem_log!`:
    ```rust
    if self.last_memory_log.elapsed() >= Duration::from_secs(20) {
        mem_log!(
            "{}",
            format_memory_log(
                self.engine.memory_used(),
                self.engine.memory_budget(),
                self.engine.skeleton_bytes(),
            )
        );
        self.last_memory_log = std::time::Instant::now();
    }
    ```
  - [x] Tests: test `format_memory_log` directly — no stderr capture needed:
    `format_memory_log_produces_correct_output`,
    `format_memory_log_rounds_down_to_mb`

## Dev Notes

### What already exists (do not re-implement)

- `ExpansionPhase::Loading` and `set_expansion_loading()` in
  `crates/hprof-tui/src/views/stack_view.rs:528` — renders a loading node
- `App.pending_expansions: HashMap<u64, Receiver<...>>` and
  `App.pending_pages` — Task 4 changes their value types
- `App.app_warnings: Vec<String>` — Task 5 replaces with `WarningLog`
- `App.warning_count: usize` — stays, counts engine-level warnings
- `StatusBar.warning_count: usize` — already renders `[!] N warnings — see stderr`
- `engine.warnings()` → non-fatal parse warnings from indexing (already wired)
- `HprofFile.records_attempted` / `records_indexed` — already populated from
  `IndexResult`; Task 1 exposes them via trait
- `hfile.index.memory_size()` from `MemorySize for PreciseIndex`
  (`crates/hprof-parser/src/indexer/precise.rs:117`) — already implemented

### Loading indicator delay: behaviour without `set_expansion_loading`

When `set_expansion_loading` is never called (< 1 s), the object phase remains
`Collapsed` or absent. `set_expansion_done` inserts `Expanded` directly —
no intermediate `Loading` state needed. The UI skips the spinner for fast ops,
which is the intended UX.
[Source: crates/hprof-tui/src/views/stack_view.rs:534 — `set_expansion_done`]

### Field borrow splitting in `poll_expansions`

`poll_expansions` uses `self.pending_expansions.iter_mut()` while also
accessing `self.stack_state` inside the loop body. This compiles correctly
because Rust's borrow checker tracks borrows at the field level for direct
field access — `self.pending_expansions` and `self.stack_state` are disjoint
fields and can be borrowed simultaneously. This would **not** compile if
`self.stack_state` were accessed via a method call on `self` (the compiler
cannot prove the method leaves `pending_expansions` untouched). Always use
direct field access (`self.stack_state`) inside this loop, never a `self`
method.

### `DummyEngine` in `engine.rs`

The `DummyEngine` struct in `engine.rs` tests (line ~307) must implement all
trait methods. Add the two new methods with trivial returns to keep tests
compiling.
[Source: crates/hprof-engine/src/engine.rs:307]

### `StatusBar` lifetime `'a` and borrow conflicts

`StatusBar<'a>` already uses lifetime `'a` for `selected: Option<&'a
ThreadInfo>`. `last_warning: Option<&'a str>` uses the same lifetime.

**Critical:** do NOT pass `self.warnings.last()` directly into `StatusBar` —
`App::render` takes `&mut self`, and an immutable borrow of `self.warnings`
alongside other accesses to `self.engine` or `self.thread_list` will fail to
compile. Always extract into a local `Option<String>` first (owned copy), then
pass `.as_deref()`:
```rust
let last_warning: Option<String> = self.warnings.last().map(str::to_string);
// ... other self accesses ...
StatusBar { last_warning: last_warning.as_deref(), ... }
```

### Warning text truncation

40-character limit with `…` suffix prevents the status bar from overflowing.
Use a helper (not a separate function — inline is fine):
```rust
let display = if w.len() > 40 {
    format!("{}…", &w[..40])
} else {
    w.to_string()
};
```
Be mindful of char boundaries — use `w.chars().take(40).collect::<String>()`.

### Stderr capture in tests

To test `eprintln!` output, use `gag` crate or redirect with pipes. Alternatively,
extract the log-formatting logic into a pure function
`fn format_memory_log(cache: usize, budget: u64, skeleton: usize) -> String`
and test that function directly (simpler, no stderr capture needed).
Prefer the pure-function approach.

### Source references

- [Source: crates/hprof-engine/src/engine.rs — `NavigationEngine` trait]
- [Source: crates/hprof-engine/src/engine_impl.rs — `Engine::memory_used`,
  `Engine::memory_budget`, `Engine::initial_memory`]
- [Source: crates/hprof-parser/src/hprof_file.rs — `records_attempted`,
  `records_indexed`, `index.memory_size()`]
- [Source: crates/hprof-tui/src/app.rs — `App`, `poll_expansions`,
  `start_object_expansion`, `render`]
- [Source: crates/hprof-tui/src/views/status_bar.rs — `StatusBar`]
- [Source: crates/hprof-tui/src/views/stack_view.rs — `set_expansion_loading`,
  `set_expansion_done`]
- [Source: docs/planning-artifacts/architecture.md#Project Structure —
  `warnings.rs`]
- [Source: docs/planning-artifacts/epics.md#Story 6.2]

### Project structure

Files to create or modify:
```
crates/hprof-engine/src/engine.rs          # +indexing_ratio, +skeleton_bytes in trait
crates/hprof-engine/src/engine_impl.rs     # implement new trait methods
crates/hprof-tui/src/warnings.rs           # NEW — WarningLog
crates/hprof-tui/src/lib.rs                # +mod warnings
crates/hprof-tui/src/views/status_bar.rs   # +file_indexed_pct, +last_warning
crates/hprof-tui/src/app.rs                # pending type changes, memory log,
                                           #   WarningLog, StatusBar wiring
```

### Commit style

`feat: Story 6.2 — loading indicators and status bar warnings`
(no co-author lines per CLAUDE.md)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Task 1: Added `indexing_ratio`, `is_fully_indexed`, `skeleton_bytes` to `NavigationEngine` trait. Implemented for `Engine` (via `hfile.records_attempted/indexed` and `hfile.index.memory_size()`). Added stubs to `DummyEngine` and `StubEngine`. 8 new tests.
- Task 2: Created `warnings.rs` with bounded `WarningLog` (cap 500). 4 tests.
- Task 3: Extended `StatusBar<'a>` with `file_indexed_pct: Option<f64>` and `last_warning: Option<&'a str>`. Updated render to show incomplete-file prefix and truncated last warning. 5 tests (1 updated + 4 new).
- Task 4: Introduced `PendingExpansion`/`PendingPage` structs with `started: Instant` and `loading_shown: bool`. Deferred loading spinner to 1-second threshold via `iter_mut()` in `poll_expansions` and `poll_pages`. Updated 4 existing tests + 2 new tests.
- Task 5: Replaced `app_warnings: Vec<String>` with `warnings: WarningLog`. Wired `warnings.add(...)` in `poll_expansions` for `None` and `Disconnected` cases. Updated `StatusBar` construction in `render`. 2 new tests.
- Task 6: Added `mem_log!` macro (dev-profiling gated) to `lib.rs`. Added `last_memory_log: Instant` field and `format_memory_log` pure function. Periodic emission in `render` every 20s. 2 tests.
- All 560 tests pass, 0 regressions, 0 clippy errors.

### File List

- crates/hprof-engine/src/engine.rs
- crates/hprof-engine/src/engine_impl.rs
- crates/hprof-tui/src/warnings.rs (NEW)
- crates/hprof-tui/src/lib.rs
- crates/hprof-tui/src/views/status_bar.rs
- crates/hprof-tui/src/app.rs
- docs/implementation-artifacts/sprint-status.yaml
- docs/implementation-artifacts/6-2-loading-indicators-and-status-bar-warnings.md
