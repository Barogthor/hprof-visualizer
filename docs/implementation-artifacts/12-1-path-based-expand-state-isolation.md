# Story 12.1: Path-Based Expand State Isolation

Status: review

## Story

As a user,
I want expanding a node to only affect that specific occurrence in
the tree, not all nodes with the same object ID,
so that navigating complex object graphs with shared references does
not produce confusing auto-expand behavior.

## Acceptance Criteria

1. **Given** object A (id=0x1234) appears at two different tree
   paths: `/thread-1/frame-0/local-0/field-x` and
   `/thread-1/frame-0/local-0/field-y/field-z`
   **When** the user expands the occurrence at path field-x
   **Then** only that occurrence expands — the occurrence at
   field-y/field-z remains collapsed

2. **Given** an enum class with a static field that references itself
   (e.g., `MyEnum.VALUE1` has a static field `VALUE1` pointing to the
   same object)
   **When** the user collapses the static field value
   **Then** the parent instance remains expanded — no cross-path
   collapse

3. **Given** a pinned item in the favorites panel referencing the
   same object as a node in the stack view
   **When** the user expands the pinned item
   **Then** the stack view node is unaffected

4. **Given** the current `object_phases` map
   **When** refactored
   **Then** `expansion_phases` (path-keyed) becomes the sole source
   of truth for expansion state; `object_phases` (object_id-keyed)
   is removed. Data caches (`object_fields`, `object_static_fields`,
   `object_errors`, `collection_chunks`) remain `object_id`-keyed
   as shared content caches.
   **Note:** This narrows epics.md AC #4 which says "keys use a
   path-based composite key instead of plain object_id" for ALL maps.
   Occam's Razor analysis (see Dev Notes) determined that only phase
   state needs path-keying — data caches are content caches where
   object_id keys are correct. Approved by Project Lead during story
   elicitation session 2026-03-15

5. **Given** all existing tests
   **When** `cargo test` is run
   **Then** zero failures — the refactoring is transparent to tests
   that use single-path scenarios

## Tasks / Subtasks

### Occam's Razor Design Principle

Only `expansion_phases` (UI visibility state) needs path-keying.
Data caches (`object_fields`, `object_static_fields`,
`object_errors`, `collection_chunks`) remain `object_id`-keyed —
an object's decoded fields are identical regardless of tree path.
This is **shared content caching**, not shared state.

### Implementation Order (MANDATORY)

Tasks MUST be executed in this order — intermediate steps will not
compile otherwise:

1. **Task 4 first** — add `NavigationPath` to `PendingExpansion`
   and change `pending_expansions` key. This makes paths available
   at all call sites before Tasks 1-2 change signatures.
2. **Task 1 + Task 2 together** — remove `object_phases` and update
   `StackState` methods atomically. These are tightly coupled.
3. **Task 3** — update renderer to use `expansion_phases`.
4. **Task 5** — verify favorites (no changes expected).
5. **Task 6 + Task 7** — tests and manual validation.

- [x] Task 1: Make `expansion_phases` the sole source of truth
  (AC: #4)
  - [x] 1.1 Remove `object_phases: HashMap<u64, ExpansionPhase>`
    from `ExpansionRegistry`. `expansion_phases:
    HashMap<NavigationPath, ExpansionPhase>` (already exists, line
    15 in expansion.rs) becomes the only phase map
  - [x] 1.2 Update `set_expansion_done()` — insert phase into
    `expansion_phases` by path; insert fields into `object_fields`
    by `object_id` (data cache stays object_id-keyed)
  - [x] 1.3 Replace `collapse_object_by_id()` with
    `collapse_at_path(&NavigationPath)`:
    - Remove path + descendant paths from `expansion_phases` via
      `retain` (prefix match on segments)
    - Check if any remaining expanded path references the same
      `object_id`; if not, remove from data caches
      (`object_fields`, `object_static_fields`, `object_errors`,
      `collection_chunks`)
    - If LRU eviction is active, let it handle orphaned cache
      entries instead of eager cleanup
  - [x] 1.4 Update `expansion_state()` to look up
    `expansion_phases.get(path)` only

- [x] Task 2: Update `StackState` methods (AC: #1, #2)
  - [x] 2.1 `set_expansion_loading()` — accept `&NavigationPath`,
    insert into `expansion_phases`. DEPENDS ON Task 4.1 (path must
    be available in `PendingExpansion` before this signature changes).
    Note: this method is also called with `cid` (collection_id) at
    app/mod.rs ~line 1846 during collection page loading — that call
    site needs a path too, or needs a separate loading indicator
  - [x] 2.2 `set_expansion_done_at_path()` — insert phase by path
    into `expansion_phases`, insert fields by `object_id` into
    `object_fields`
  - [x] 2.3 Remove `set_expansion_done(object_id, fields)` (the
    object_id-only shortcut). DEPENDS ON Task 4.3 (all callers must
    be migrated to `set_expansion_done_at_path` first). Also remove
    or update `expand_object_sync` (app/mod.rs line 219, marked
    `#[allow(dead_code)]`) which calls this overload
  - [x] 2.4 `set_static_fields()` — keeps `object_id` param (data
    cache, unchanged)
  - [x] 2.5 `set_expansion_failed()` — accept `&NavigationPath` for
    phase, keep `object_id` for `object_errors` (data cache)
  - [x] 2.6 `cancel_expansion()` / `collapse_object()` — accept
    `&NavigationPath`, delegate to `collapse_at_path`
  - [x] 2.7 `collapse_object_recursive()` — uses
    `collapse_at_path` for path-prefix removal. Data cache cleanup
    deferred to LRU eviction.
  - [x] 2.8 `expansion_state()` — removed the `object_id` overload.
    All ~20 callers migrated to `expansion_state_for_path(&path)`.

- [x] Task 3: Update `object_ref_state` in renderer (AC: #1)
  - [x] 3.1 Add `path: Option<&NavigationPath>` parameter to
    `object_ref_state()`. Phase lookup uses `ctx.phase_for()` which
    checks `expansion_phases` by path first, falls back to
    `object_phases` by object_id
  - [x] 3.2 Thread `NavigationPath` through the render call chain:
    `render_variable_tree` → `append_var` →
    `append_fields_expanded` → `render_single_field` →
    `append_collection_items` → static fields. Each level extends
    path by one segment.
  - [x] 3.3 `RenderCtx` changes:
    - Kept `object_phases` for snapshot mode
    - Added `expansion_phases: Option<&HashMap<NavigationPath,
      ExpansionPhase>>` for live stack view mode
    - Added `phase_for(object_id, path)` helper method
  - [x] 3.4 Updated `object_ref_state()` phase lookup logic with
    dual-path approach via `ctx.phase_for()`
  - [x] 3.5 `visited: HashSet<u64>` stays keyed by `object_id`
    (cycle guard)
  - [x] 3.6 `TreeRoot::Subtree` passes `None` for
    `expansion_phases`

- [x] Task 4: Update `App` async expansion pipeline (AC: #1, #2)
  - [x] 4.1 Change `pending_expansions: HashMap<u64, _>` key to
    `NavigationPath`
  - [x] 4.2 `start_object_expansion()` — capture cursor
    `NavigationPath`. Stored in `PendingExpansion` struct
  - [x] 4.3 `poll_expansions()` — calls
    `set_expansion_done_at_path(path, object_id, fields)`
  - [x] 4.4 Updated `pending_expansions.remove(&oid)` sites to use
    `retain(|_, pe| pe.object_id != oid)`
  - [x] 4.5 Updated `contains_key` guard to check by path
  - [x] 4.6 `set_static_fields` keeps `object_id` param

- [x] Task 5: Favorites panel verification (AC: #3)
  - [x] 5.1 `PinnedSnapshot` maps stay `object_id`-keyed — verified
  - [x] 5.2 `snapshot_from_cursor()` clones by `object_id` — verified
  - [x] 5.3 `local_collapsed: HashSet<u64>` unaffected — verified
  - [x] 5.4 Verified by code analysis; expansion_phases is None in
    snapshot mode, favorites uses object_phases fallback

- [x] Task 6: Write tests (AC: #1, #2, #3, #5)
  - [x] 6.1 Test: phase_isolation_expand_a_b_stays_collapsed
  - [x] 6.2 Test: data_sharing_same_object_shares_cache
  - [x] 6.3 Test: collapse_static_field_parent_stays_expanded
  - [x] 6.4 Test: collapse_at_path_removes_only_that_path
  - [x] 6.5 Test: collapse_last_path_phase_collapsed_data_deferred
  - [x] 6.6 Test: stale_path_expansion_does_not_panic
  - [x] 6.7 All 909 tests pass (regression verified)

- [ ] Task 7: Manual validation on real dump (AC: #1, #2)
  - [ ] 7.1 Open `assets/heapdump-visualvm.hprof`, find an object
    referenced from two different frames/locals, expand one, verify
    the other stays collapsed

## Dev Notes

### Root Cause

The bug originates from Epic 4 (pagination), documented in
`docs/implementation-artifacts/epic-4-retro-2026-03-10.md` line 189:
> "Shared expansion state (path-based keys) — accepted deferral by
> Project Lead"

`ExpansionRegistry` (expansion.rs) uses `object_id: u64` as keys for
`object_fields`, `object_static_fields`, `object_errors`, and
`object_phases`. When the same Java object appears at multiple tree
paths (common with enums, singletons, shared collections), all
occurrences share state.

### Existing Partial Solution

`expansion_phases: HashMap<NavigationPath, ExpansionPhase>` already
exists alongside `object_phases` (lines 14-20 in expansion.rs). It
was added as a parallel path-keyed map but never became the sole
source of truth. The comment on line 18 says: "Kept in sync with
expansion_phases. When multiple paths point to the same object, the
last-written phase wins (acceptable for visual styling)."

This story completes the migration: make `expansion_phases` the sole
phase map and remove `object_phases`. Data caches stay
`object_id`-keyed.

### Occam's Razor: Only Phase State Needs Path-Keying

The bug is that the **visual state** (expanded/collapsed) is shared
across paths. The **content** (decoded fields, static fields, errors,
collection chunks) is inherently shared — an object's fields are
identical regardless of tree path. Separating these two concerns:

- `expansion_phases` → path-keyed (UI state, per-occurrence)
- `object_fields`, `object_static_fields`, `object_errors`,
  `collection_chunks` → `object_id`-keyed (data cache, shared)

This reduces the refactoring from 5 maps to 1 map, eliminates the
need for a generic `RenderCtx<K>`, preserves the favorites snapshot
system unchanged, and removes the "path doesn't contain object_id"
conversion problem entirely.

### Key Architectural Insight

`NavigationPath` is already a first-class, hashable, clonable type
with builder validation (types.rs lines 81-193). It encodes
`Frame → Var → Field* → StaticField* → CollectionEntry*`. This is
the natural key for path-isolated expansion state.

### ADR: Use `NavigationPath` as Direct Key (Option A)

Three keying strategies were evaluated for `expansion_phases`:
- **A) `NavigationPath` direct** — full path as HashMap key
- **B) `u64` hash** — hash of path as key (compact)
- **C) `NavigationPath` + cached hash** — pre-computed hash for O(1)
  lookup

**Decision: Option A.** Rationale:
- Option B eliminated — cannot do prefix match for
  `collapse_at_path` (dealbreaker for recursive collapse)
- Option C is premature optimization — with the simplified design,
  only 1 map (`expansion_phases`) uses path keys. ~5000 phase
  lookups per render (500 objects × 10 fields). Hashing a Vec of
  5-10 segments ≈ 50 ns each → ~250 μs = 1.5% of 16 ms frame
  budget. Acceptable.
- If profiling ever shows a hotspot, migrating from A to C is a
  localized change in `types.rs` (add `cached_hash: u64` field,
  custom `Hash` impl) with zero impact on the rest of the codebase

### Rendering: Thread Path for Phase Lookup Only

The renderer must thread a `NavigationPath` through the recursive
call chain so that `object_ref_state()` can look up the expansion
phase by path. However, all data map lookups (`object_fields`,
`object_static_fields`, `collection_chunks`) remain by `object_id`
— their signatures are unchanged.

```
render_variable_tree(root, path)
  └─ append_var(var, path)          // path = Frame + Var
       └─ render_single_field(f, path)
            └─ object_ref_state(id, path)  // phase by path
            └─ append_fields_expanded(oid, child_path)
                 └─ render_single_field(f, child_path)
                      └─ ...
```

Each level extends the path by one segment. The data lookups inside
`append_fields_expanded` still use `ctx.object_fields.get(&object_id)`
— unchanged. Only `object_ref_state` switches from
`ctx.object_phases.get(&object_id)` to
`ctx.expansion_phases.get(path)`.

`RenderCtx` gains one field (`expansion_phases`) — no generic needed.
Recommended order: bottom-up from `object_ref_state` →
`render_single_field` → `append_fields_expanded` → `append_var` →
`render_variable_tree`.

### Async Pipeline Impact

`App.pending_expansions` is keyed by `u64` (object_id). The async
flow is:
1. `start_object_expansion(object_id)` (app/mod.rs ~line 1719) —
   spawns background thread
2. `poll_expansions()` (~line 1950) — checks receivers, calls
   `set_expansion_done(object_id, fields)` (~line 1983)

After refactoring, the key becomes `NavigationPath` captured at step
1. This also enables expanding the same object at two different paths
concurrently (currently impossible — second request is deduped by
`contains_key(&object_id)`).

### Favorites Panel — Unchanged

With the simplified design, data caches remain `object_id`-keyed.
`PinnedSnapshot` clones these caches as-is — no conversion needed.
`local_collapsed: HashSet<u64>` remains valid. `snapshot_from_cursor()`
is completely unaffected by this story.

### Anti-Patterns to Avoid

- **DO NOT** convert data caches (`object_fields`,
  `object_static_fields`, `object_errors`, `collection_chunks`) to
  path-keyed maps. They are content caches — an object's fields are
  identical regardless of tree path. Path-keying them adds
  complexity with zero benefit.
- **DO NOT** make `RenderCtx` generic. With the simplified design,
  only `expansion_phases` is path-keyed — add it as one extra field.
  All other map fields stay `&HashMap<u64, _>`.
- **DO NOT** break the existing `NavigationPathBuilder` invariants
  (Frame at [0], Var at [1], no Frame/Var at [2+]).
- **DO NOT** change `visited: HashSet<u64>` in
  `append_fields_expanded` to `HashSet<NavigationPath>`. This is a
  **cycle guard** that detects when the same object_id is visited
  twice in a single render pass. If keyed by path, enum
  self-references would not be detected → stack overflow.

### Performance Note: Path Clones in Renderer

The renderer builds a `NavigationPath` at each recursion level via
`NavigationPathBuilder::extend(path.clone()).field(...).build()`.
With 500 expanded objects × 10 fields = 5000 clones per render.

**Expected cost:** ~500 μs = 3% of 16 ms frame budget. Acceptable.

**If profiling shows a hotspot**, mitigation: use a single
stack-local `Vec<PathSegment>` that is extended/truncated at each
recursion level (one allocation, reused). Implement
`Borrow<[PathSegment]> for NavigationPath` for zero-clone lookups.

**DO NOT pre-optimize** — measure first after implementation.

### Project Structure Notes

- All expansion state lives in TUI crate — engine crate is unaffected
- `NavigationEngine::expand_object(object_id)` signature stays the
  same — the engine resolves fields by heap object_id regardless of
  tree path
- The path-keying is purely a UI state concern

### References

- [Source: docs/planning-artifacts/epics.md#Epic 12] — Story
  definition, acceptance criteria, technical notes
- [Source: docs/implementation-artifacts/epic-4-retro-2026-03-10.md
  line 189] — Original deferral decision
- [Source: crates/hprof-tui/src/views/stack_view/expansion.rs
  lines 13-90] — `ExpansionRegistry` with dual map architecture
- [Source: crates/hprof-tui/src/views/stack_view/types.rs
  lines 59-193] — `PathSegment`, `NavigationPath`, builder
- [Source: crates/hprof-tui/src/views/stack_view/state.rs
  lines 849-960] — `StackState` expand/collapse methods
- [Source: crates/hprof-tui/src/views/tree_render/mod.rs]
  — `RenderCtx` struct with `object_phases` field,
  `render_variable_tree()`
- [Source: crates/hprof-tui/src/views/tree_render/helpers.rs]
  — `object_ref_state()`, `get_phase()`
- [Source: crates/hprof-tui/src/app/mod.rs lines 48-52] —
  `PendingExpansion` struct
- [Source: crates/hprof-tui/src/app/mod.rs line 134] —
  `pending_expansions: HashMap<u64, PendingExpansion>`
- [Source: crates/hprof-tui/src/app/mod.rs line 219] —
  `expand_object_sync` (dead code, calls `set_expansion_done`)
- [Source: crates/hprof-tui/src/app/mod.rs ~line 1719] —
  `start_object_expansion()`
- [Source: crates/hprof-tui/src/app/mod.rs ~line 1950] —
  `poll_expansions()`
- [Source: crates/hprof-tui/src/favorites.rs lines 202-254] —
  `snapshot_from_cursor()`, snapshot isolation

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — no debugging sessions required.

### Completion Notes List

- Removed `object_phases: HashMap<u64, ExpansionPhase>` — sole
  source of truth is now `expansion_phases: HashMap<NavigationPath,
  ExpansionPhase>`
- Added `derive_object_phases()` compatibility bridge for renderer
  snapshot mode (favorites panel)
- All ~20 callers of `expansion_state(oid)` migrated to
  `expansion_state_for_path(&path)`
- `pending_expansions` key changed from `u64` to `NavigationPath`;
  collapse operations use `retain` to find by `object_id`
- Renderer threads `Option<NavigationPath>` through entire call
  chain; `RenderCtx.phase_for()` centralizes dual-lookup logic
- `toggle_expand()` now clears expansion_phases for the collapsed
  frame via `retain` on Frame segment prefix
- 6 new path-isolation tests added in `path_isolation_tests` module
- Task 7 (manual validation) deferred — requires interactive TUI

### File List

- `crates/hprof-tui/src/views/stack_view/expansion.rs` — removed
  `object_phases`, added `collapse_at_path`,
  `collapse_all_for_object`, `derive_object_phases`
- `crates/hprof-tui/src/views/stack_view/state.rs` — all
  expand/collapse/loading/failed methods path-based, removed
  `expansion_state(oid)`, migrated 4 internal emit callers,
  `toggle_expand` clears expansion_phases
- `crates/hprof-tui/src/views/tree_render/mod.rs` — `RenderCtx`
  gains `expansion_phases` + `phase_for()`, `TreeRoot::Frame`
  gains `frame_id`, path threaded through render chain
- `crates/hprof-tui/src/views/tree_render/helpers.rs` —
  `object_ref_state` accepts `Option<&NavigationPath>`,
  uses `ctx.phase_for()`
- `crates/hprof-tui/src/views/tree_render/expansion.rs` —
  `append_fields_expanded`, `render_single_field`,
  `append_static_items` thread path
- `crates/hprof-tui/src/views/tree_render/variable.rs` —
  `append_var` threads path
- `crates/hprof-tui/src/views/tree_render/collection.rs` —
  `append_collection_items` threads path with entry paths
- `crates/hprof-tui/src/app/mod.rs` — `PendingExpansion` gains
  `object_id`+`path`, `pending_expansions` keyed by
  `NavigationPath`, `start_object_expansion` accepts path,
  `poll_expansions` uses path, ~16 `expansion_state` callers
  migrated, `PendingNavigation.prereq_expanded` → `Vec<NavigationPath>`
- `crates/hprof-tui/src/views/favorites_panel/mod.rs` —
  `render_variable_tree` calls updated (frame_id, expansion_phases)
- `crates/hprof-tui/src/favorites.rs` — test updates for
  path-based API
- `crates/hprof-tui/src/views/stack_view/tests.rs` — all test
  callers migrated + 6 new path_isolation_tests
- `crates/hprof-tui/src/app/tests.rs` — all test callers migrated
- `crates/hprof-tui/src/views/tree_render/tests.rs` — updated
  for new render_variable_tree signature
- `crates/hprof-tui/src/views/favorites_panel/tests.rs` — updated
  for new render_variable_tree signature
