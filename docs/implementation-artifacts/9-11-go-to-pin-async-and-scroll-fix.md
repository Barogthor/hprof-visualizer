# Story 9.11: Go-to-Pin Async Navigation & Scroll-to-Target

Status: done

## Story

As a user,
I want go-to-pin (`g`) to navigate to the pinned object without
freezing the UI, and to scroll the stack view directly to the
target frame and object,
so that I can use pinned items for efficient navigation even on
large collections.

## Bugs Addressed

### N11 — Go-to-pin blocks UI on large collections

**Symptom:** Pressing `g` on a pinned item from a 450k-entry
collection freezes the TUI for minutes. Ctrl+C does not quit.

**Root cause:** `navigate_to_path()` (`app/mod.rs:338-498`) calls
`expand_object_sync()` (line 412) and
`ensure_collection_entry_loaded()` (line 456) synchronously on
the main event-loop thread. Each call triggers
`engine.expand_object()` or `engine.get_page()` which may scan
large mmap segments via `find_instance()`. On a 70 GB dump with
a 15 GB segment, a single lookup can take seconds; chaining
dozens blocks the UI for minutes.

**Source:** [Source: docs/report/large-dump-ux-observations-2026-03-14.md#N11]

### N12 — Go-to-pin does not scroll to target frame/object

**Symptom:** After go-to-pin completes, the stack view shows the
correct thread but the cursor stays on the first item instead of
scrolling to the target frame and nested object.

**Root cause:** In `navigate_to_path()` (lines 488-495), after
the walk completes, `set_cursor(target)` is called. However:
1. `flat_items()` only contains currently rendered items. If the
   walk was partial (collection page not yet loaded), the retry
   in `poll_pages()` (lines 1597-1615) may fail to find the
   target in `flat_items()` and fall back to the first visible
   `RenderCursor::At(_)`.
2. `set_cursor_and_sync()` (`cursor.rs:71-74`) calls
   `list_state.select(idx)` but does not center or scroll the
   viewport — ratatui auto-scroll only ensures the item is
   visible, often at the very bottom.

**Source:** [Source: docs/report/large-dump-ux-observations-2026-03-14.md#N12]

## Acceptance Criteria

1. **AC1 – Non-blocking go-to-pin:**
   Given the user presses `g` on a pinned item whose path
   includes unexpanded objects or unloaded collection pages,
   When the navigation starts,
   Then the UI remains responsive (16 ms frame budget respected)
   and the user can still scroll, press Ctrl+C, or cancel.

2. **AC2 – Progressive visual feedback:**
   Given a go-to-pin navigation is in progress,
   When each walk step resolves,
   Then the cursor moves to that step's row, the viewport
   scrolls to show it (upper third), and the view re-renders
   before continuing to the next step. The user sees the
   navigation unfold step by step (thread → frame → var →
   field → ...).

3. **AC3 – Spinner during async waits:**
   Given the walk defers on an async operation (object
   expansion or page load),
   When the status bar renders,
   Then an animated spinner with "Navigating to pin..." is
   visible until the deferred step completes or is cancelled.

4. **AC4 – Scroll to final target:**
   Given go-to-pin completes all walk steps,
   When the stack view renders,
   Then the cursor is on the exact pinned row (frame, field,
   or collection entry), visible in the upper third of the
   viewport.

5. **AC5 – Partial navigation with deferred completion:**
   Given a collection page or object expansion required by
   the path is not loaded,
   When navigate_to_path encounters it,
   Then it stores the remaining path in `pending_navigation`,
   renders the cursor on the last resolved step with spinner,
   and completes the walk when the resource is ready.

6. **AC6 – Cancel and continue manually:**
   Given a go-to-pin navigation is in progress,
   When the user presses Escape,
   Then the pending navigation is cleared and the cursor stays
   on the last resolved step. The user can continue navigating
   manually from there. Orphaned background expansions are
   kept (not collapsed).

7. **AC7 – No regression on small dumps:**
   Given a pinned item on a small collection (< 100 entries),
   When the user presses `g`,
   Then navigation completes instantly with correct scroll
   positioning (no perceptible delay vs current behaviour).
   All cached steps chain in the same frame.

8. **AC8 – Navigation failure graceful:**
   Given the walk encounters an object or collection page that
   cannot be resolved (engine returns `None`),
   When the async result is polled,
   Then the navigation stops, the status bar displays "Failed
   to navigate — object not resolvable", the spinner is
   cleared, and the cursor stays on the last successfully
   resolved step.

9. **AC9 – Stale context triggers retry:**
   Given a go-to-pin navigation is deferred (waiting for async
   expansion or page load),
   When the awaited resource arrives but the parent context was
   invalidated (evicted by LRU or collapsed by user),
   Then the walk restarts from the beginning with the original
   full path, the status bar shows "Pin context changed,
   retrying...", and the spinner remains active.

## Tasks / Subtasks

- [x] Task 1: Refactor pending_navigation + incremental walk
        (AC: #1, #5, #9)
  - [x] 1.1 Replace `Option<(NavigationPath, u64)>` with
        `Option<PendingNavigation>` struct containing
        `remaining_path: Vec<PathSegment>`,
        `original_path: NavigationPath` (for stale restart),
        `thread_id: u32`, `awaited: AwaitedResource` enum.
        `AwaitedResource` has three variants:
        `ObjectExpansion(u64)`, `CollectionPage(u64)`,
        `Continue` (in-frame step cap reached, resume next tick).
        Stale restarts are handled in `resume_pending_navigation`
        by checking `prereq_expanded` — no dedicated variant needed
  - [x] 1.2 Refactor `navigate_to_path` to accept a
        `&[PathSegment]` slice (remaining steps). After each
        resolved step, drop it from remaining and reposition
        cursor + scroll (progressive render). When called with
        an empty slice, treat as "walk complete" — position
        cursor at current location and clear pending nav
  - [x] 1.3 Replace `expand_object_sync(oid)` with async spawn:
        if not already expanded, spawn background thread, store
        remaining steps in `pending_navigation`, return
        `WalkOutcome::PartialAt`
  - [x] 1.4 Replace `ensure_collection_entry_loaded()` with
        async page request: if page not loaded, spawn
        `engine.get_page()` in background, store remaining path,
        return `WalkOutcome::PartialAt`
  - [x] 1.5 In-frame continuation: after each resolved step, if
        the next step is cached, continue in the same frame.
        Cap at 10 consecutive in-frame steps then yield
  - [x] 1.6 In `poll_pages()`: when awaited collection loads
        and `pending_navigation.is_some()`, resume walk with
        `pending_navigation.remaining_path` (NOT the full
        original path). Use `pending_navigation.thread_id`
        (NOT `thread_list.selected_serial()`). If
        `remaining_path` is empty, treat as walk-complete.
        **Bug fix:** current code at `mod.rs:1598-1612` passes
        the full `nav_path` and reads thread from selection —
        both are wrong
  - [x] 1.7 In `poll_expansions()`: when awaited object
        expansion completes and
        `pending_navigation.awaited == ObjectExpansion(oid)`,
        resume walk with `remaining_path` using
        `pending_navigation.thread_id`. Apply the expansion
        result first, then call walk. If `remaining_path` is
        empty, treat as walk-complete
  - [x] 1.8 Stale context detection (AC9): in poll resume
        paths (1.6 and 1.7), verify parent object_ids still
        in `expansion_phases` before resuming. If invalidated,
        set `awaited = StaleRestart`, show "Pin context
        changed, retrying..." in status bar, restart walk
        with `original_path`
  - [x] 1.9 Handle async failure: if `expand_object` returns
        `None` or `get_page` returns `None`/empty page, clear
        `pending_navigation`, show "Failed to navigate —
        object not resolvable" in status bar, cursor on last
        resolved step (FM2/FM3)

- [x] Task 2: Progressive scroll-to-cursor (AC: #2, #4)
  - [x] 2.1 Add `scroll_to_cursor(items: &[Id], visible_height:
        usize)` method in `cursor.rs`: takes `visible_height`
        as parameter (not a field of `CursorState` — it lives
        in `StackViewState`). Place cursor in upper third via
        `list_state.offset_mut()`. Guard on
        `visible_height == 0`. Clamp offset to
        `items.len().saturating_sub(visible_height)` to
        prevent viewport extending past end of list (FM4)
  - [x] 2.2 Call `scroll_to_cursor()` after each resolved walk
        step (not just at the end) for progressive rendering
  - [x] 2.3 Recalculate `flat_items()` immediately after each
        `set_expansion_done()` / page insertion before calling
        `set_cursor()` — do not defer to next render (PM5)
  - [x] 2.4 In the poll retry path, after final walk completes,
        call `scroll_to_cursor()` one last time for final
        positioning

- [x] Task 3: Spinner + text indicator (AC: #3)
  - [x] 3.1 Add `navigating_to_pin: bool` + `spinner_tick: u8`
        fields to App state
  - [x] 3.2 Set `navigating_to_pin = true` on first
        `WalkOutcome::PartialAt`, clear on final completion or
        cancel
  - [x] 3.3 Increment `spinner_tick` via `wrapping_add(1)` at
        the top of `render()` (before drawing). Reset to 0
        when `navigating_to_pin` is cleared (FM7)
  - [x] 3.4 Render spinner char (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`
        indexed by `spinner_tick % 10`) + "Navigating to
        pin..." in status bar when `navigating_to_pin` is true
  - [x] 3.5 Update help footer: when `navigating_to_pin` is
        true, show "Esc: Cancel navigation" instead of normal
        context shortcuts (same pattern as story 9-7 contextual
        footer)

- [x] Task 4: Cancel pending navigation (AC: #6)
  - [x] 4.1 In the `Escape` handler, if
        `pending_navigation.is_some()`: clear it + clear
        `navigating_to_pin`. Escape takes priority over normal
        collapse/back behaviour during active navigation
  - [x] 4.2 Cursor stays on last resolved step — user can
        continue manually from there
  - [x] 4.3 In `NavigateToSource` handler: if
        `pending_navigation.is_some()`, cancel current nav
        (clear pending + spinner) before starting new walk.
        Only one pending nav at a time (RT-A1)
  - [x] 4.4 Move `poll_expansions()` and `poll_pages()` out
        of `render()` and into `run_loop` so the update order
        is: input handling → poll_expansions → poll_pages →
        render (FM6). Currently polls run inside `render()`
        (`mod.rs:1752-1753`) which means they execute BEFORE
        input handling in the loop
  - [x] 4.5 `poll_expansions()` / `poll_pages()` must check
        `pending_navigation.is_some()` before resuming walk —
        if cleared (cancelled), skip retry silently (PM3)

- [x] Task 5: Tests (AC: #1–#9)
  - [x] 5.1 Unit test: `navigate_to_path` returns
        `WalkOutcome::PartialAt` when expansion is needed
        (not blocking)
  - [x] 5.2 Unit test: after full walk, `cursor_index()` matches
        target position in `flat_items()`
  - [x] 5.3 Unit test: `scroll_to_cursor()` sets offset so
        target is in upper third of viewport. Guard tests:
        `visible_height == 0` returns without panic; offset
        never exceeds `items.len() - visible_height`
  - [x] 5.4 Unit test: Escape during pending navigation clears
        state, cursor stays on last resolved step
  - [x] 5.5 Unit test: small collection (< 100 entries) — walk
        completes in one pass, no `pending_navigation` set,
        all steps chain in same frame, scroll position correct
  - [x] 5.6 Unit test: in-frame continuation cap — after 10
        cached steps, walk yields even if next step is cached
  - [x] 5.7 Unit test: stale context detection (AC9) — if
        expansion is evicted between defer and retry, walk
        restarts from `original_path`, status bar shows "Pin
        context changed, retrying..."
  - [x] 5.8 Unit test: async expansion returns `None` → walk
        terminates, status bar shows failure message, no
        spinner left spinning (FM2/FM3)
  - [x] 5.9 Unit test: spam `g` on different pin during active
        nav → old nav cancelled, new nav starts (RT-A1)
  - [x] 5.10 Unit test: empty `remaining_path` on resume —
        when the deferred segment was the last one, poll
        resume treats empty remaining as walk-complete,
        cursor positioned correctly
  - [x] 5.11 Unit test: poll resume uses
        `pending_navigation.thread_id`, not
        `thread_list.selected_serial()` — verify by switching
        threads manually after deferral, then letting poll
        resume; walk should target the original thread
  - [x] 5.12 Integration test: construct `App` via
        `HprofTestBuilder`, manually insert a `PinnedItem`,
        simulate `NavigateToSource`, verify cursor ends on
        the correct row with correct viewport offset. Note:
        pinning is TUI-layer (`App.pinned`), not engine-layer

## Dev Notes

### Current Architecture

The go-to-pin flow is:
1. `InputEvent::NavigateToSource` → `app/mod.rs:649-678`
2. Extracts `thread_id` + `nav_path` from `PinKey`
3. Calls `navigate_to_path(thread_id, &nav_path)` (line 338)
4. Walk processes segments: Frame → Var → Field → CollectionEntry
5. Each Field/CollectionEntry may call blocking engine methods
6. On completion, `set_cursor(target)` positions the cursor

### Key Files to Modify

| File | Purpose |
|------|---------|
| `crates/hprof-tui/src/app/mod.rs` | `navigate_to_path`, `expand_object_sync`, `ensure_collection_entry_loaded`, `poll_pages`, `poll_expansions`, `run_loop` (move polls out of render), NavigateToSource handler, Escape handler |
| `crates/hprof-tui/src/views/cursor.rs` | Add `scroll_to_cursor()` method using `visible_height` and `list_state.offset` |
| `crates/hprof-tui/src/views/status_bar.rs` | Render "Navigating to pin..." indicator |
| `crates/hprof-tui/src/app/tests.rs` | New tests for async walk, scroll, cancel |

### Existing Async Patterns to Reuse

Object expansion from user input (Right arrow) already uses
async spawning:
- `app/mod.rs`: Right/Enter handler spawns
  `engine.expand_object()` in a background thread
- Result is polled in `poll_expansions()` and applied to
  `stack_state`
- Same pattern applies to collection page loading via
  `poll_pages()`

**Reuse this exact pattern** for navigate_to_path: spawn the
engine call, store remaining walk segments, poll for completion,
resume walk.

### Design Decisions (from elicitation + ADRs)

1. **Incremental walk (not restart):** Each resolved step is
   committed. On retry, only remaining segments are walked.
   Never restart from the beginning.
2. **Sequential deferrals:** Multi-step deferred paths (expand A
   → expand B → load page) are handled sequentially. Each step
   depends on the previous result. No parallel prefetch.
3. **Escape cancels navigation:** When `navigating_to_pin` is
   true, Escape takes priority over its normal behaviour
   (collapse/back). Clears pending navigation, cursor stays on
   the last resolved step — the user can then continue manually
   from there.
4. **Top-third scroll positioning:** After each step (and at
   final completion), the cursor row is placed in the upper
   third of the viewport, leaving 2/3 for content below.
5. **Spinner + text indicator:** Status bar shows an animated
   spinner alongside "Navigating to pin..." during async waits.
   Piggyback on the existing poll tick (no dedicated timer).
   Spinner chars: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, incremented via
   `spinner_tick: u8` each frame.
6. **N12 is large-dump-only:** The scroll bug is confirmed on
   70 GB dumps but not reproducible on small generated test
   dumps. Likely caused by partial walk / deferred page loads
   that only occur when `find_instance` falls back to segment
   scan. Integration tests should validate scroll positioning
   but the full N12 scenario requires manual testing on a large
   dump.
7. **Progressive rendering:** After each resolved walk step,
   reposition the cursor and re-render. The user sees the
   navigation unfold: thread switch → frame scroll → expand →
   drill into field → etc. Steps already in cache chain within
   the same frame (up to 10 consecutive). When an async defer
   is needed, render the current position + spinner, then
   resume on poll. This gives continuous visual feedback and
   lets the user cancel mid-walk to continue manually.
8. **PendingNavigation struct (ADR-1):** Replace the tuple
   `Option<(NavigationPath, u64)>` with a typed struct
   (`PendingNavigation` + `AwaitedResource` enum) for clarity.
9. **In-frame continuation cap (ADR-2):** After a retry
   resolves, continue the walk in the same frame if the next
   step is cached. Cap at 10 consecutive in-frame steps to
   prevent blocking. Single threshold — no conditional.
10. **Direct offset_mut() for scroll (ADR-3):** Use
    `list_state.offset_mut()` directly. No scrollbar widget.
11. **Cancel = keep expansions (ADR-5):** On cancel, orphaned
    async expansions are kept (LRU handles memory). The cursor
    stays where it landed — the user can navigate manually
    from there or re-trigger go-to-pin.
12. **Cancel is Escape-only (ADR-6):** Only Escape and a new
    `g` cancel active navigation. Other user input does NOT
    cancel. Rationale: accidental keystrokes during minutes-
    long walks on 70 GB dumps would be too disruptive.

### pending_navigation Redesign

Current type: `Option<(NavigationPath, u64)>` — stores full path
+ awaited collection ID.

**Proposed change:** Replace with a struct:
```rust
struct PendingNavigation {
    remaining_path: Vec<PathSegment>,
    original_path: NavigationPath,
    thread_id: u32,
    awaited: AwaitedResource,
}

enum AwaitedResource {
    ObjectExpansion(u64),   // object_id
    CollectionPage(u64),    // collection_id
    Continue,               // in-frame step cap reached, resume next tick
}
```

`original_path` is kept for stale-context restarts (AC9).
`remaining_path` holds only unresolved tail segments — poll
resume must use `remaining_path` (NOT the full original path).
`thread_id` is authoritative for resume — poll must use it
instead of `thread_list.selected_serial()` (the user may have
switched threads manually between defer and resume).

When `remaining_path` is empty on resume, treat as walk-complete
(the deferred segment was the last one).

This supports sequential deferred steps: first wait for object
expansion, then on retry encounter an unloaded collection page,
defer again. Each resolved step is dropped from
`remaining_path` — the walk never restarts from scratch
(except on stale context, which uses `original_path`).

### scroll_to_cursor() Design

After `set_cursor_and_sync()`, place target in upper third.
Note: `visible_height` is passed as parameter (it lives in
`StackViewState`, not `CursorState`):
```rust
fn scroll_to_cursor(
    &mut self,
    items: &[Id],
    visible_height: usize,
) {
    if visible_height == 0 { return; }
    if let Some(idx) = self.cursor_index(items) {
        let third = visible_height / 3;
        let offset = idx.saturating_sub(third);
        let max_offset = items.len()
            .saturating_sub(visible_height);
        let offset = offset.min(max_offset);
        *self.list_state.offset_mut() = offset;
    }
}
```

Target appears in the upper third of the viewport, leaving 2/3
for content below. Offset is clamped to
`items.len() - visible_height` to prevent blank rows at the
bottom. ratatui `ListState` exposes `offset_mut()` for manual
scroll control.

### Regression Guard

The synchronous fast path must remain for items that are already
expanded/loaded. Only defer when the engine call would block.
Check expansion_phases and collection_chunks BEFORE spawning
background work. For small dumps where everything resolves from
`instance_offsets` HashMap (O(1)), the walk should complete in
one pass with no async overhead.

### Guardrails

**Stale context (AC9):** Between defer and retry, user may
scroll/collapse or LRU may evict. Verify parent object_ids
still in `expansion_phases` before resuming. If invalidated,
set `awaited = StaleRestart`, show "Pin context changed,
retrying..." in status bar, restart walk with
`original_path`. Keep spinner active.

**In-frame continuation:** Continue walk in same frame while
next step is cached. Cap at 10 consecutive in-frame steps.
Yield only on async call.

**Cancel orphans:** Background thread completes after Escape.
Let expansion apply (LRU manages). Polls must check
`pending_navigation.is_some()` — if cleared, skip silently.

**Spam `g` (RT-A1):** If `pending_navigation.is_some()` when
a new `NavigateToSource` arrives, cancel the current nav
first (clear pending + spinner), then start the new one.
Only one `PendingNavigation` at a time.

**Cancel is Escape-only:** Only Escape and a new `g`
(RT-A1) cancel an active navigation. Other user input
(arrows, scroll, resize) does NOT cancel the walk — the
next walk step will reposition the cursor. Rationale: on
70 GB dumps, walks take minutes; an accidental keystroke
should not kill the navigation.

**Thread switch state (RT-A5):**
`open_stack_for_selected_thread()` creates a fresh
`StackState`, destroying all expansion state of the previous
thread (`mod.rs:182-188`). This means: (a) cross-thread
go-to-pin must re-expand everything on the target thread,
(b) if the user cancels mid-walk, the source thread's
expansion state is already lost. This is a pre-existing
limitation (not in scope). Document in help text that
go-to-pin to a different thread resets expansions.

**Async failure (AC8):** `expand_object` → `None` or
`get_page` → `None`/empty = terminal. Clear nav + spinner.
Show "Failed to navigate — object not resolvable". Never
spin indefinitely.

**Stale flat_items:** Recalculate `flat_items()` immediately
after `set_expansion_done()` / page insertion, before
`set_cursor()`. Do not defer to next render.

**Scroll clamp:**
`offset.min(items.len().saturating_sub(visible_height))`
+ `visible_height == 0` early return.

**Input before polls:** Move polls out of `render()` into
`run_loop`. Update loop order: input handling →
poll_expansions → poll_pages → render. Escape clears state
before polls can resume.

**Spinner overflow:** `wrapping_add(1)`, reset to 0 on clear.

**Timer redraws:** Event loop already polls with 16ms tick
(`event::poll(Duration::from_millis(16))`). When no key is
pressed, poll returns false and the loop re-renders — the
spinner animates at ~60fps. No change needed.

**Empty remaining_path on resume:** When the deferred
segment was the last one in the path, `remaining_path` will
be empty after resume. Poll resume must handle this as
walk-complete (position cursor, clear spinner, clear pending
nav). Do not pass empty slice to `navigate_to_path` as it
currently returns `PartialAt(FrameId(0))` for empty input.

**Poll resume thread_id:** Poll resume must use
`pending_navigation.thread_id`, NOT
`thread_list.selected_serial()`. The user may have switched
threads manually between defer and resume.

**No auto-timeout (out of scope):** Long ops on cloud may
take minutes. Escape covers the need. A future story could
add a configurable timeout.

### expand_object_sync Scope

`expand_object_sync()` is replaced by async spawning ONLY
inside `navigate_to_path`. Other call sites (if any) keep
using it. Do NOT delete `expand_object_sync` — it may be
used elsewhere in the codebase.

### References

- [Source: docs/report/large-dump-ux-observations-2026-03-14.md#N11]
- [Source: docs/report/large-dump-ux-observations-2026-03-14.md#N12]
- [Source: crates/hprof-tui/src/app/mod.rs — navigate_to_path:338-498]
- [Source: crates/hprof-tui/src/app/mod.rs — expand_object_sync:191-206]
- [Source: crates/hprof-tui/src/app/mod.rs — ensure_collection_entry_loaded:240-315]
- [Source: crates/hprof-tui/src/app/mod.rs — poll_pages pending_navigation retry:1597-1615]
- [Source: crates/hprof-tui/src/views/cursor.rs — set_cursor_and_sync:71-74]
- [Source: crates/hprof-tui/src/app/mod.rs — NavigateToSource handler:649-678]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None — all issues resolved via test output and clippy feedback.

### Completion Notes List

- `poll_pages()` return type was changed to `()` during Task 1; several tests and helpers
  needed updating to drop unused return value bindings (clippy `let_unit_value`).
- `poll_navigation_to_completion` helper defined at top-level in `tests.rs` (before any
  `mod` blocks) to be accessible from all submodules via `use super::*`.
- Test 5.3: `open_stack_for_selected_thread` resets `visible_height=0`; `set_visible_height`
  must be called AFTER `Enter` from `ThreadList` focus.
- Test 5.7: `poll_all_expansions` was too aggressive (completed walk entirely). Replaced
  with targeted `while pending_expansions.contains_key(&42)` loop.
- `run_loop` collapsible_if: merged 3 nested if blocks into single `&&` chain.

### File List

- `crates/hprof-tui/src/app/mod.rs`
- `crates/hprof-tui/src/app/tests.rs`
- `crates/hprof-tui/src/views/cursor.rs`
- `crates/hprof-tui/src/views/help_bar.rs`
- `crates/hprof-tui/src/views/status_bar.rs`
- `crates/hprof-tui/src/views/stack_view/state.rs`
- `crates/hprof-tui/src/theme.rs`
