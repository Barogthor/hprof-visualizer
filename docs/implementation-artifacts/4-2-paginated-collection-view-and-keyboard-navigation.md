# Story 4.2: Paginated Collection View & Keyboard Navigation

Status: done

## Story

As a user,
I want to browse large collections via expandable
chunk sections (first 100 entries shown eagerly, then
sections of 100 up to 1000, then sections of 1000
beyond), with Page Up/Down for fast tree scrolling and
arrow keys for line-by-line movement,
So that I can efficiently explore collections of any
size without UI freezes, with explicit control over
how much data is loaded.

## Acceptance Criteria

### AC1: Collection expansion — eager first 100

**Given** I expand a collection field with
entry_count > 0 (e.g. `ArrayList (3000 entries)`)
**When** I press Enter on it
**Then** the first 100 entries are loaded via
`get_page(id, 0, 100)` and displayed as child items
in the tree, followed by expandable chunk section
nodes for the remaining entries

### AC2: Chunk sections layout

**Given** a collection with 3000 entries is expanded
**When** I view the tree
**Then** I see:
- entries `[0]` to `[99]` displayed inline (eager)
- collapsed sections: `+ [100...199]`,
  `+ [200...299]`, ... `+ [900...999]`
  (chunks of 100 up to 1000)
- collapsed sections: `+ [1000...1999]`,
  `+ [2000...2999]` (chunks of 1000 beyond)

### AC3: Chunk section expansion

**Given** I navigate to a collapsed section node
(e.g. `+ [1000...1999]`)
**When** I press Enter
**Then** entries for that range are loaded via
`get_page(id, 1000, 1000)` and displayed inline
under the section node, which becomes `- [1000...1999]`

### AC4: Last chunk truncated to actual count

**Given** a collection with 2348 entries
**When** expanded
**Then** the last chunk section is
`+ [2000...2347]` (not `+ [2000...2999]`), and
loading it returns only the 348 remaining entries

### AC5: Small collection — no sections

**Given** a collection with <= 100 entries
**When** expanded
**Then** all entries are shown directly, no chunk
section nodes appear

### AC6: Medium collection — sections of 100 only

**Given** a collection with 500 entries
**When** expanded
**Then** entries `[0]`-`[99]` shown eagerly, followed
by sections `+ [100...199]`, `+ [200...299]`,
`+ [300...399]`, `+ [400...499]`

### AC7: Arrow keys navigate tree entries

**Given** a collection with entries and chunk sections
displayed in the tree
**When** I press Up/Down arrow keys
**Then** the cursor moves one item at a time through
entries, section nodes, and expanded entry fields
(standard tree navigation)

### AC8: Page Up/Down scroll by screen height (FR15)

**Given** I am navigating the stack view tree
**When** I press Page Down or Page Up
**Then** the cursor moves forward/backward by one
screen height worth of items — regardless of whether
I am in a collection or not (general tree scroll)

### AC9: Loading indicator during chunk fetch

**Given** I press Enter on a chunk section node
**When** the `get_page` call is in progress
**Then** the section shows `~ Loading...` and the UI
remains responsive (NFR4: 16ms frame budget)

### AC10: Entry rendering — keys and values

**Given** a chunk is loaded and entries are displayed
**When** I view the entries
**Then** map entries show `[idx] key => value`,
list/array entries show `[idx] value`, and
ObjectRef values are expandable (reusing existing
expand_object flow)

### AC11: Escape collapses collection

**Given** I am browsing entries inside an expanded
collection
**When** I press Escape
**Then** the collection collapses back to its summary
line (`ArrayList (3000 entries)`) and the cursor
returns to the collection field row

### AC12: Unsupported collection type — graceful

**Given** a collection type where `get_page` returns
`None` (e.g. TreeMap)
**When** I press Enter on it
**Then** the object expands via `expand_object` as
before (field-level view, no pagination), with no
crash or error

### AC13: All existing tests pass

**Given** all existing tests (421 tests)
**When** I run `cargo test`
**Then** all tests pass — zero regressions

## Tasks / Subtasks

- [x] Task 1: Add PageUp/PageDown to input layer
  (AC: 8)
  - [x] 1.1: Add `PageUp` and `PageDown` variants to
    `InputEvent` enum in `input.rs`
  - [x] 1.2: Map `KeyCode::PageUp` and
    `KeyCode::PageDown` to the new variants in the
    crossterm event translation
  - [x] 1.3: Unit test: verify PageUp/PageDown
    translation from crossterm events

- [x] Task 2: Implement Page Up/Down tree scroll
  (AC: 8)
  - [x] 2.1: In `handle_stack_frames_input` and
    `handle_thread_list_input`, handle PageUp/PageDown
    as cursor jump by visible height
  - [x] 2.2: Store `visible_height: u16` in
    `StackState` (set during render from
    `frame.area().height`)
  - [x] 2.3: PageDown: move cursor forward by
    `visible_height` items, clamp to last item
  - [x] 2.4: PageUp: move cursor backward by
    `visible_height` items, clamp to first item
  - [x] 2.5: Unit test: Page Down from item 5 with
    height 20 → item 25; Page Up from item 25 → item 5

- [x] Task 3: Add collection chunk state to
  StackState (AC: 1, 2, 3, 5, 6)
  - [x] 3.1: Add `CollectionChunks` struct:
    ```
    struct CollectionChunks {
        total_count: u64,
        eager_page: Option<CollectionPage>,
        chunk_pages: HashMap<usize, ChunkState>,
    }
    enum ChunkState {
        Collapsed,
        Loading,
        Loaded(CollectionPage),
    }
    ```
  - [x] 3.2: Add `collection_chunks: HashMap<u64,
    CollectionChunks>` to `StackState` — keyed by
    collection object ID
  - [x] 3.3: Implement chunk range computation:
    `compute_chunk_ranges(total_count) ->
    Vec<(usize, usize)>` — returns (offset, limit)
    pairs following the 100/100/1000 rules
  - [x] 3.4: Unit test: chunk ranges for total=50,
    150, 500, 1000, 3000, 2348

- [x] Task 4: Modify collection expansion trigger
  (AC: 1, 5, 12)
  - [x] 4.1: In `handle_stack_frames_input` Enter
    handler, detect when cursor is on an ObjectRef
    with `entry_count.is_some()` and
    `entry_count > 0`
  - [x] 4.2: Call `start_collection_page_load(id, 0,
    100)` to load eager first page — spawn async
    worker via `std::thread` + `mpsc::channel`
  - [x] 4.3: On async completion: create
    `CollectionChunks` with eager_page populated and
    chunk sections computed from `total_count`
  - [x] 4.4: If `get_page` returns `None` (async
    result), fall back to `start_object_expansion(id)`
    for unsupported types (AC12)
  - [x] 4.5: For collections with entry_count=0 or
    entry_count absent, use existing expand_object
    flow unchanged

- [x] Task 5: Handle chunk section expansion
  (AC: 3, 4, 9)
  - [x] 5.1: When Enter is pressed on a chunk section
    node (Collapsed state), start async
    `get_page(id, offset, chunk_size)` and set
    ChunkState to Loading
  - [x] 5.2: Add `pending_pages: HashMap<(u64, usize),
    Receiver<Option<CollectionPage>>>` to `App` —
    keyed by (collection_id, chunk_offset)
  - [x] 5.3: Implement `poll_pages()` in `App` —
    polls channels, updates ChunkState to
    `Loaded(page)` on completion
  - [x] 5.4: On Enter on already-Loaded section,
    toggle collapse (same as object expand/collapse)

- [x] Task 6: Render collection entries and chunks
  in tree (AC: 2, 4, 5, 6, 10)
  - [x] 6.1: In `build_object_items()`, detect when
    object has `CollectionChunks` and switch to
    collection rendering mode
  - [x] 6.2: Render eager entries (from eager_page):
    map entries as `[idx] key => value`, list/array
    as `[idx] value`
  - [x] 6.3: Render chunk section nodes:
    `+ [offset...end]` (collapsed),
    `- [offset...end]` (loaded), `~ Loading...`
    (loading)
  - [x] 6.4: Render loaded chunk entries inline under
    expanded section node
  - [x] 6.5: ObjectRef values in entries show
    `+ ClassName` and are expandable via existing
    expand_object flow
  - [x] 6.6: Omit chunk sections entirely when
    total_count <= 100 (AC5)

- [x] Task 7: Handle Escape to collapse collection
  (AC: 11)
  - [x] 7.1: When cursor is inside a collection
    (entries or chunk sections) and Escape is pressed,
    remove the `CollectionChunks` for that ID
  - [x] 7.2: Collapse the object back to its summary
    line
  - [x] 7.3: Move cursor back to the collection field
    row

- [x] Task 8: Update StubEngine for TUI tests
  (AC: 13)
  - [x] 8.1: Update `StubEngine::get_page` in
    `app.rs` to return test `CollectionPage` data
    for stub collection ID (888) — return entries
    matching the requested offset/limit
  - [x] 8.2: Verify existing `app.rs` tests still
    pass with updated stub

- [x] Task 9: Write unit/integration tests
  (AC: 1-13)
  - [x] 9.1: Test initial collection expansion
    triggers `get_page(id, 0, 100)` not
    `expand_object`
  - [x] 9.2: Test chunk layout: 3000 entries →
    100 eager + 9 sections of 100 + 2 sections
    of 1000
  - [x] 9.3: Test chunk layout: 50 entries → all
    inline, no sections
  - [x] 9.4: Test chunk layout: 150 entries →
    100 eager + 1 section `[100...149]`
  - [x] 9.5: Test chunk section Enter → loads
    correct offset/limit
  - [x] 9.6: Test last chunk truncated: 2348 →
    section `[2000...2347]` loads limit=348
  - [x] 9.7: Test loading indicator for chunk
  - [x] 9.8: Test Escape collapses entire collection
  - [x] 9.9: Test unsupported type falls back to
    expand_object
  - [x] 9.10: Test entry rendering: map vs list format
  - [x] 9.11: Test Page Up/Down scrolls tree by
    visible height (not collection-specific)
  - [x] 9.12: Test re-Enter on loaded chunk toggles
    collapse

- [x] Task 10: Run full test suite — verify zero
  regressions (AC: 13)

## Dev Notes

### Architecture Compliance

- **Crate boundary:** All TUI changes stay in
  `hprof-tui`. Page loading calls
  `NavigationEngine::get_page()` via the trait — no
  direct import from `hprof-parser` or `pagination.rs`.
- **Trait boundary:** TUI only sees
  `NavigationEngine` trait. The `get_page` method and
  `CollectionPage`/`EntryInfo` types are already
  defined in `engine.rs`.
- **No `unwrap()` in production code** — use `?` or
  explicit `match`.
- **Async pattern:** Follow the existing
  `pending_expansions` / `poll_expansions` pattern
  exactly. Spawn `std::thread` worker, use
  `mpsc::channel`, poll in render loop.

### Key Code Locations

- **NavigationEngine trait + types:**
  `crates/hprof-engine/src/engine.rs`
  - `EntryInfo` (line ~149): `index: usize`,
    `key: Option<FieldValue>`, `value: FieldValue`
  - `CollectionPage` (line ~162): `entries`,
    `total_count`, `offset`, `has_more` — derives
    `Debug, Clone`
  - `get_page` (line ~182): trait method signature
  - `FieldValue` enum (line ~107): reuse for rendering
    entry values

- **TUI App state machine:**
  `crates/hprof-tui/src/app.rs`
  - `App` struct (line ~30): owns `engine`,
    `stack_state`, `pending_expansions`,
    `pending_strings`
  - `start_object_expansion()` (line ~321-338):
    reference pattern for async page loading
  - `poll_expansions()` (line ~401-429): reference
    pattern for poll_pages()
  - `StubEngine` (line ~576-672): test double, must
    update `get_page`
  - `handle_stack_frames_input()` (line ~194-315):
    where Enter/Escape/Up/Down are handled

- **Stack view rendering:**
  `crates/hprof-tui/src/views/stack_view.rs`
  - `StackState` struct: manages frame/var/field tree
  - `StackCursor` enum: 7 cursor variants
  - `ExpansionPhase` enum (line ~19): Collapsed |
    Loading | Expanded | Failed
  - `build_object_items()` (line ~813): renders
    expanded object fields — insert collection
    rendering here
  - `format_object_ref_collapsed()` (line ~639):
    shows `ClassName (N entries)` — reuse for
    collapsed state
  - `emit_object_children()` (line ~564): cycle
    detection in tree traversal

- **Input translation:**
  `crates/hprof-tui/src/input.rs`
  - `InputEvent` enum: currently has Up, Down, Home,
    End, Enter, Escape, SearchActivate, SearchChar,
    SearchBackspace, Quit
  - Missing: `PageUp`, `PageDown` — must add
  - Crossterm key mapping (line ~36-55): add
    `KeyCode::PageUp` and `KeyCode::PageDown`

- **Theme constants:**
  `crates/hprof-tui/src/theme.rs`
  - `SEARCH_HINT`: dim gray style — reuse for
    loading indicator and chunk section labels

### Chunk Size Rules

**Chunking logic (`compute_chunk_ranges`):**

| total_count | Eager (auto-loaded) | Sections |
|---|---|---|
| <= 100 | all entries | none |
| 101-1000 | first 100 | chunks of 100 |
| 1001+ | first 100 | chunks of 100 (100-999) + chunks of 1000 (1000+) |

Last section truncated to actual total: e.g. 2348
entries → last section is `[2000...2347]` with
limit=348.

**Examples:**
- `total=50` → 50 entries inline, no sections
- `total=150` → 100 eager + `[100...149]`
- `total=500` → 100 eager + `[100...199]`,
  `[200...299]`, `[300...399]`, `[400...499]`
- `total=1000` → 100 eager + 9 sections of 100
- `total=3000` → 100 eager + 9 sections of 100
  + `[1000...1999]`, `[2000...2999]`
- `total=2348` → 100 eager + 9 sections of 100
  + `[1000...1999]`, `[2000...2347]`

### Collection Entry Display

**Entry rendering rules (from `CollectionPage`):**
- **Map entries** (`key.is_some()`):
  `[idx] key_display => value_display`
- **List/array entries** (`key.is_none()`):
  `[idx] value_display`
- **Value display** reuses existing
  `format_field_value()` logic from `stack_view.rs`
- **ObjectRef keys** in map entries: display as
  `ClassName@0xID` inline (no inline expansion —
  user can Enter to expand the entry)
- **ObjectRef values** in entries are expandable:
  show `+ ClassName` toggle, Enter triggers
  `start_object_expansion(entry.value.id)`

### Navigation Model

**Page Up/Down = tree scroll (FR15):**
Page Up/Down move the cursor by one screen height
worth of tree items. This is uniform across the
entire stack view — thread list, frames, variables,
object fields, collection entries, chunk sections.
No special handling for collections.

**Collection-specific navigation:**
- **Enter on collection ObjectRef** (with
  entry_count): load eager page + show chunk sections
- **Enter on chunk section** (Collapsed): load that
  chunk's entries
- **Enter on chunk section** (Loaded): toggle
  collapse
- **Enter on ObjectRef entry value**: expand via
  existing `start_object_expansion()` flow
- **Escape inside collection**: collapse entire
  collection back to summary line
- **Up/Down arrows**: standard tree cursor movement

**Chunk sections are standard tree nodes:**
They behave like expandable objects — Collapsed,
Loading, Loaded states. No special navigation mode.
The cursor treats them as any other tree item.

### Async Page Loading

**Pattern (mirrors `pending_expansions`):**
1. User triggers chunk load (Enter on collection or
   Enter on chunk section)
2. Spawn `std::thread` worker calling
   `engine.get_page(id, offset, limit)`
3. Result sent via `mpsc::channel`
4. `App::poll_pages()` called every frame in
   `render()`, picks up results
5. On completion: update `ChunkState` to
   `Loaded(page)` or fall back to expand_object
   if `None`

**Key for pending_pages:** `(collection_id,
chunk_offset)` — allows multiple chunks of the
same collection to load concurrently if needed.

**NFR4 compliance:** The async pattern ensures the
event loop never blocks. `get_page` runs in a
background thread. `poll_pages()` is non-blocking
(`try_recv`). 16ms frame budget maintained.

### Performance Considerations

- Eager page = 100 entries — fast to render
- Each chunk = 100 or 1000 entries — bounded memory
- Multiple chunks can be expanded simultaneously
  (user choice) — acceptable memory cost
- `CollectionPage` derives `Clone` — cheap for
  100-1000 entry pages
- ObjectRef values within entries resolve on-demand
  (user must Enter to expand)
- For 500K+ collection: ~500 chunk section nodes
  rendered (labels only, no entry data until
  expanded) — negligible render cost

### Anti-Patterns — DO NOT

- Do NOT load all entries on collection expansion —
  only the first 100 (eager)
- Do NOT block the event loop for `get_page` calls —
  always async
- Do NOT modify `hprof-engine` code — this story is
  TUI-only (engine API is ready from Story 4.1)
- Do NOT modify `pagination.rs` — it's tested and
  reviewed
- Do NOT add `entry_count` field detection logic to
  TUI — reuse the `entry_count` already present in
  `FieldValue::ObjectRef`
- Do NOT break existing object expansion flow — it
  must still work for non-collection objects and
  unsupported collection types
- Do NOT make Page Up/Down collection-specific —
  they are general tree scroll (FR15)

### Previous Story Intelligence

**From Story 4.1 (Collection Pagination Engine):**
- `CollectionPage` derives `Clone` (code review
  fix L1) — safe to cache in TUI state
- `get_page(id, offset, limit)` accepts any limit —
  works with 100 or 1000
- `get_page` returns `None` for unsupported types
  (TreeMap, TreeSet, Hashtable, ArrayDeque,
  PriorityQueue) — TUI must fall back to
  `expand_object` for these
- `EntryInfo.key` is `Some` for maps, `None` for
  lists/arrays — use this to decide rendering format
- Cycle guards in engine extractors (HashMap chains,
  LinkedList) — TUI does NOT need its own cycle guard
  for pagination
- 421 tests passing post-review, zero regressions

**From Story 4.1 code review (M3 deferred):**
- Cycle guards in pagination use stdlib `HashSet`
  (not FxHashSet) — no impact on TUI

**From Epic 8 (performance):**
- `start_object_expansion` spawns `std::thread` —
  same pattern for page loading (no need for
  rayon/tokio)

**From cyclic reference fix:**
- `expand_object` cycle detection is engine-internal
  — TUI's `emit_object_children()` has its own cycle
  guard for nested expansion display. Page entries
  that are ObjectRef can still be expanded normally.

### Git Intelligence

Recent commits:
- `fa82afd` — review fixes for Story 4.1 (missing
  tests, double alloc, CollectionPage Clone)
- `df5c5cb` — Story 4.1 implementation (pagination
  engine, extractors, 14 tests)
- `73d7d79` — non-recursive collapse for nested
  objects (relevant: collapse logic exists)

Code patterns: feature work on `feature/epic-4-*`
branches, squash merge per epic.

### Project Structure Notes

- Modified files (TUI only):
  - `crates/hprof-tui/src/input.rs` (add PageUp,
    PageDown)
  - `crates/hprof-tui/src/app.rs` (pending_pages,
    poll_pages, start_collection_page_load,
    handle PageUp/PageDown as tree scroll,
    StubEngine update)
  - `crates/hprof-tui/src/views/stack_view.rs`
    (CollectionChunks, ChunkState, chunk rendering
    in build_object_items, compute_chunk_ranges,
    cursor support for collection entries and
    chunk section nodes)
- No new files expected — all changes fit in existing
  modules
- No changes to `hprof-engine` or `hprof-parser`

### References

- [Source: docs/planning-artifacts/epics.md#Epic 4,
  Story 4.2]
- [Source: docs/planning-artifacts/architecture.md#
  Frontend Architecture]
- [Source: docs/planning-artifacts/
  ux-design-specification.md#Journey 3]
- [Source: docs/code-review/claude-story-4.1-
  collection-pagination-engine.md]
- [Source: crates/hprof-engine/src/engine.rs#
  NavigationEngine trait]
- [Source: crates/hprof-tui/src/app.rs#
  start_object_expansion, poll_expansions]
- [Source: crates/hprof-tui/src/views/stack_view.rs#
  build_object_items]
- [Source: crates/hprof-tui/src/input.rs#InputEvent]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

- Borrow checker fix: `poll_pages` collected fallback
  IDs in `Vec<u64>` to avoid mutable borrow conflict
- Added `Send + Sync + 'static` bound on `render()`
  for `poll_pages` thread spawning
- Changed existing StubEngine child ObjectRef to
  `entry_count: None` to prevent collection mode in
  existing tests
- Fixed navigation count in tests: 101 downs needed
  (1 field node + 100 entries), not 100
- Fixed 6 clippy `collapsible_if` warnings with
  `let` chains

### Completion Notes List

- All 10 tasks implemented with TDD (red-green-refactor)
- 452 tests passing, 0 failures (445 original + 7 from CR)
- Clippy clean (0 warnings), cargo fmt applied
- StubEngine uses collection_id 888 (Int entries) and
  889 (ObjectRef entries for AC10 tests)
- New cursor variants: `OnChunkSection`,
  `OnCollectionEntry`, `OnCollectionEntryObjField`
- Async page loading follows existing
  `pending_expansions` / `poll_expansions` pattern

### Change Log

- 2026-03-09: Story 4.2 implemented — paginated
  collection view with chunk sections, Page Up/Down
  tree scrolling, async chunk loading, Escape collapse
- 2026-03-09: Code review fixes (Claude Sonnet 4.6):
  - H1 (AC10): ObjectRef entry values now expandable
    via new `OnCollectionEntryObjField` cursor;
    `format_entry_line` shows `+`/`-` toggle;
    Enter on `OnCollectionEntry` / `OnCollectionEntryObjField`
    dispatches `StartEntryObj`/`CollapseEntryObj`
  - M1: Added 4 unit tests for `ThreadListState::page_down/page_up`
  - M2: `resync_cursor_after_collapse` and `toggle_expand`
    now handle `OnChunkSection`, `OnCollectionEntry`,
    `OnCollectionEntryObjField` cursors
  - M3: Added test `escape_from_chunk_section_collapses_collection`

### File List

- `crates/hprof-tui/src/input.rs` — added PageUp,
  PageDown variants and key mappings
- `crates/hprof-tui/src/views/thread_list.rs` — added
  page_down(n)/page_up(n) methods + 4 tests
- `crates/hprof-tui/src/views/stack_view.rs` — added
  CollectionChunks, ChunkState, compute_chunk_ranges,
  OnChunkSection/OnCollectionEntry/OnCollectionEntryObjField
  cursor variants, collection rendering,
  emit_collection_children,
  emit_collection_entry_obj_children,
  build_collection_entry_obj_items,
  selected_collection_entry_ref_id,
  selected_collection_entry_obj_field_ref_id,
  CollectionChunks::find_entry, visible_height, page
  up/down
- `crates/hprof-tui/src/app.rs` — added
  pending_pages, poll_pages,
  start_collection_page_load, collection Enter/Escape
  handling, PageUp/PageDown handling,
  StartEntryObj/CollapseEntryObj commands,
  StubEngine get_page (collection 888 + 889)
