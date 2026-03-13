---
title: 'NavigationPath: Replace StackCursor with Composable Path Model'
slug: 'navigation-path-replace-stack-cursor'
created: '2026-03-13'
status: 'pending-review'
stepsCompleted: [1, 2, 3, 4, 5]
implementedCommit: 'abdc293'
implementedDate: '2026-03-13'
tech_stack:
  - Rust
  - ratatui
files_to_modify:
  - crates/hprof-tui/src/views/stack_view/types.rs
  - crates/hprof-tui/src/views/stack_view/state.rs
  - crates/hprof-tui/src/views/stack_view/expansion.rs
  - crates/hprof-tui/src/favorites.rs
  - crates/hprof-tui/src/app/mod.rs
  - crates/hprof-tui/src/app/tests.rs
code_patterns:
  - NavigationPath as composable semantic identity (replaces StackCursor)
  - RenderCursor as thin ratatui rendering wrapper over NavigationPath
  - ExpansionRegistry keyed by NavigationPath for instance-scoped UI state
  - PinKey stores NavigationPath as canonical position
  - Sequential walk of NavigationPath as materialization recipe
  - NavigationPathBuilder for zero-clone path construction
test_patterns:
  - app::tests::favorites_navigate_to_source_*
  - views::stack_view::tests::flat_items_*
  - views::favorites_panel::tests::*
---

# Tech-Spec: NavigationPath — Replace StackCursor with Composable Path Model

**Created:** 2026-03-13

## Overview

### Problem Statement

`StackCursor` (17 variants) conflates two responsibilities: rendering position for ratatui and
semantic identity for navigation/expansion. This conflation causes three concrete bugs:

1. **Wrong labels on nested collection pins** — `field_path: Vec<usize>` in `PinKey` can only
   encode object-field hops. For `var[4].roots[0].mixedObjectArray[k]`, the label builder
   traverses the wrong path and produces `"var[4].roots[k]"` instead of the correct label.
   Root: `emit_collection_entry_obj_children` passes the parent collection's `field_path`
   unchanged to nested collections (state.rs:2363).

2. **Slow `g` navigation on nested collections** — `ensure_cursor_materialized` has no recipe
   for "expand through a collection entry to reach an inner collection." Falls through a
   cascade of fallbacks and calls `flat_items()` three times per navigation.

3. **Shared expansion state across occurrences** — `ExpansionRegistry.object_phases` is keyed
   by `object_id`. Two occurrences of the same object share expand/collapse state.

### Solution

Replace `StackCursor` with two focused types:

- **`NavigationPath`** — composable semantic identity, stable across renders, used for
  expansion keying, `PinKey`, label building, and materialization recipe.
- **`RenderCursor`** — thin ratatui wrapper (8 variants) wrapping a `NavigationPath`
  plus rendering-only state (loading, chunk section, static header, overflow).

`StackCursor` is fully removed. `flat_items()` returns `Vec<RenderCursor>`.

### Scope

**In Scope:**
- Define `PathSegment`, `NavigationPath`, `NavigationPathBuilder` types.
- Define `RenderCursor` (8 variants) replacing `StackCursor` (17 variants).
- Rewrite `flat_items()` and `emit_*` to produce `RenderCursor` with `NavigationPath`.
- Rewrite `ExpansionRegistry` to key UI phase state by `NavigationPath`.
- Replace `PinKey` fields with `nav_path: NavigationPath`; remove `collection_restore_cursor`.
- Rewrite label builders to derive labels from `NavigationPath` segments.
- Implement `navigate_to_path` as sequential segment walk replacing fallback cascade.
- Remove `navigate_stack_cursor_to_pin_key`, `ensure_cursor_materialized`,
  `find_collection_entry_cursor`, `collection_restore_cursors`.
- Add regression tests: nested collection nav, static field nav, instance-scoped expansion.

**Out of Scope:**
- Engine/parser crate changes.
- Persisted storage format changes.
- Broad keybinding redesign outside favorites/source navigation.
- Keying `collection_chunks` by `NavigationPath` (shared data cache — see Known Limitations).
- `PinKey` persistence migration — favorites are not persisted to disk in the current
  implementation. If persistence is added later, a migration strategy will be required.
  This spec makes no backwards-compatibility guarantees for serialized `PinKey` data.

## Context for Development

### Codebase Patterns

- `StackCursor` (17 variants, `types.rs`) conflates rendering + semantic identity — fully replaced.
- `ExpansionRegistry` (`expansion.rs`): `object_phases: HashMap<u64, ExpansionPhase>` → moves
  to `expansion_phases: HashMap<NavigationPath, ExpansionPhase>`. Decoded data maps stay by id.
- `flat_items()` + `emit_*` in `state.rs` build the rendered list — rewritten to emit
  `RenderCursor`. Currently `emit_collection_children` gates only on `collection_chunks.get(&id)`;
  must also gate on `expansion_phases` to prevent cross-path contamination.
- `PinKey` (`favorites.rs`): `field_path + collection_id + entry_index + collection_restore_cursor`
  — replaced by `thread_id: ThreadId`, `thread_name: String`, `nav_path: NavigationPath`.
- `navigate_stack_cursor_to_pin_key` + `ensure_cursor_materialized` (`app/mod.rs`): fallback
  cascade replaced by `navigate_to_path` sequential walk.
- Label builders (`build_field_path_label`, `build_collection_entry_label`): walk `field_path`
  — replaced by `NavigationPath` segment traversal using loaded field/collection data.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `crates/hprof-tui/src/views/stack_view/types.rs` | Current cursor types — fully replaced |
| `crates/hprof-tui/src/views/stack_view/state.rs` | `flat_items()`, `emit_*`, `CursorState` |
| `crates/hprof-tui/src/views/stack_view/expansion.rs` | `ExpansionRegistry` — phase keying to migrate |
| `crates/hprof-tui/src/favorites.rs` | `PinKey`, label builders, snapshot factory |
| `crates/hprof-tui/src/app/mod.rs` | Navigation, materialization, `poll_pages` |
| `crates/hprof-tui/src/app/tests.rs` | Source navigation regressions |

### Technical Decisions

1. **`NavigationPath` is the single semantic identity — scoped to a single thread's stack.**
   `NavigationPath` does NOT contain a `Thread` segment. The stack view is always scoped to one
   thread (`StackState` is created per thread); thread selection happens at `App` level before
   `StackState` exists. Thread identity lives in `PinKey` (see Decision 4), not in the path.

   Stable IDs where available: `FrameId` from HPROF `STACK_FRAME` serial, `CollectionId`.
   `VarIdx`, `FieldIdx`, `StaticFieldIdx`, `EntryIdx` are positional — the HPROF format has
   no stable per-variable or per-field IDs. This is accepted: variable/field order is fixed
   for a given hprof session. `FrameId` is the HPROF `STACK_FRAME` serial — dev must confirm
   the engine surfaces this field. `flat_items()` currently iterates frames by position with
   `enumerate()` — must switch to using `frame.frame_id` for `FrameId` emission.

   All IDs and indices are newtypes to prevent cross-type mix-ups at call sites:
   ```rust
   struct ThreadId(u32);       // engine uses u32 for thread_serial
   struct FrameId(u64);
   struct CollectionId(u64);
   struct VarIdx(usize);
   struct FieldIdx(usize);
   struct StaticFieldIdx(usize);
   struct EntryIdx(usize);
   struct ChunkOffset(usize);

   enum PathSegment {
       Frame(FrameId),                        // position [0], enforced
       Var(VarIdx),                           // position [1], enforced
       Field(FieldIdx),                       // instance field
       StaticField(StaticFieldIdx),           // static field
       CollectionEntry(CollectionId, EntryIdx),
   }
   struct NavigationPath(Vec<PathSegment>); // private field
   ```
   `ThreadId` is a newtype but NOT a `PathSegment` — used only in `PinKey` and
   `navigate_to_path` for thread selection.
   All newtypes derive `Hash + Eq + Clone + Copy + Debug`. Each newtype exposes a `.0`
   field (tuple struct) for engine API interop — e.g. `child_object_id_at(obj_id, idx.0)`
   where the engine takes `usize`. Conversions from raw engine values are wrapped at
   `PathSegment` construction sites.
   `NavigationPathBuilder::frame_only(frame_id: FrameId)` builds a depth-1 path (frame
   selection without variable). `NavigationPathBuilder::new(frame_id: FrameId, var_idx: VarIdx)`
   builds a depth-2 path (frame + variable). Both start fresh.
   `NavigationPathBuilder::extend(path)` takes ownership of an existing path to extend deeper —
   move semantics, no clones during construction, single allocation per depth level.
   `NavigationPath::parent()` truncates the last segment and returns `Option<Self>`.
   Returns `None` only on a Frame-only path (depth 1 — nothing above the frame).
   On a Frame+Var path (depth 2), returns `Some(Frame-only path)` — collapsing a variable
   row selects the parent frame. On depth 3+, truncates normally.
   `build()` uses `assert!` (not `debug_assert!`) for invariant validation — malformed paths
   are programming errors that must surface in release builds too.

   **Static field subtree encoding:** a field within an object pointed to by a static field is
   `[Frame, Var, StaticField(2), Field(3)]` — `StaticField` vs `Field` at depth 2
   disambiguates the instance vs static subtree. This mirrors the current `StackCursor` split
   between `OnObjectField` and `OnStaticObjectField`.

   **Concrete path trace for Bug 1 fix:**
   `var[4].roots[0].mixedObjectArray[k]` produces:
   ```
   [Frame(fid), Var(4), Field(roots_idx), CollectionEntry(roots_cid, 0),
    Field(mixedObjectArray_idx), CollectionEntry(mixed_cid, k)]
   ```
   vs `var[4].roots[j]`: `[Frame(fid), Var(4), Field(roots_idx), CollectionEntry(roots_cid, j)]`
   — the inner path has two extra segments, no ambiguity possible.

2. **`RenderCursor` wraps `NavigationPath` for ratatui.** 8 variants:
   ```rust
   enum RenderCursor {
       NoFrames,
       At(NavigationPath),
       LoadingNode(NavigationPath),          // expansion in progress
       FailedNode(NavigationPath),           // expansion error — distinct rendering
       CyclicNode(NavigationPath),           // cyclic ref marker — non-expandable
       ChunkSection(NavigationPath, ChunkOffset),
       SectionHeader(NavigationPath),        // static section header
       OverflowRow(NavigationPath),          // [+N more static fields]
   }
   ```
   `FailedNode` and `CyclicNode` need dedicated variants because they have distinct
   rendering (color, icon) and key handling (non-expandable) that `At(NavigationPath)` cannot
   express — the renderer receiving `At(path)` would need extra lookups to disambiguate.
   `OverflowRow` satisfies `flat_items().len() == build_items().len()` invariant.
   List scroll index derived from position in `flat_items()` at render time.
   **Uniqueness:** `CyclicNode(path)` gets the same `NavigationPath` as the field that
   contains the cyclic reference, but the variant itself distinguishes them in `flat_items()`.
   Two consecutive entries with the same path but different variants (`At` + `CyclicNode`) are
   distinct — equality check must include the variant, not just the path.

3. **`ExpansionRegistry` split:**
   - Decoded data (`object_fields`, `object_static_fields`, `collection_chunks`) → keyed by
     `object_id` (shared data cache, memory efficient).
   - `object_errors: HashMap<u64, String>` stays keyed by `object_id`. Two resolution paths:
     (a) **Rendering:** emitters already track the `object_id` they are rendering — they pass
     it to `object_errors.get(&object_id)` directly. (b) **Navigation:** `navigate_to_path`
     walk accumulator tracks `current_object_id` (see Decision 5) — on failure, the walk
     knows which `object_id` failed and can display the error.
   - UI expansion phase → `HashMap<NavigationPath, ExpansionPhase>` (instance-scoped).
     **Clarification:** `RenderCursor::SectionHeader(path)` and `RenderCursor::At(path)` may
     share the same `NavigationPath`, but section headers are never looked up in
     `expansion_phases` — they are non-interactive display rows. No collision.
   - `emit_collection_children` must gate on **both** `expansion_phases.get(&collection_path)
     == Expanded` AND `collection_chunks.get(&id)` — never `collection_chunks` alone. This
     prevents a collection expanded at Path A from auto-rendering at Path B when both reference
     the same `collection_id`.

4. **`PinKey` stores `thread_id: ThreadId`, `thread_name: String`, and `nav_path: NavigationPath`.**
   `ThreadId` is needed for thread selection during `g` navigation. `thread_name` is needed
   for display in the favorites panel (the stack view cannot reverse-lookup a thread name from
   `ThreadId` without an engine call). Remove `field_path`, `collection_restore_cursor`,
   `collection_id`, `entry_index` as separate fields — all encoded in `nav_path`.
   `PinKey` equality uses `thread_id` + `nav_path` (not `thread_name`).

5. **Materialization = sequential path walk (synchronous, flat):**
   ```rust
   enum WalkOutcome { Success(NavigationPath), PartialAt(NavigationPath) }
   fn navigate_to_path(
       &mut self,
       thread_id: ThreadId,
       path: &NavigationPath,
   ) -> WalkOutcome
   ```
   The walk maintains a `current_object_id: Option<u64>` accumulator. This is the key piece
   of state that links `NavigationPath` segments back to engine `object_id`s:
   - **Thread selection** (before walk): `App` calls `select_thread(thread_id.0)` and opens
     the stack view if the target thread differs from the current one. This happens BEFORE
     walking path segments — the path itself has no `Thread` segment.
   - `Frame(id)` → find frame by `frame_id` serial in current thread's frames. If the frame
     is not the currently expanded frame, expand it. Reset `current_object_id = None`.
   - `Var(idx)` → look up variable at index `idx` in the frame's var list. If the variable
     is an `ObjectRef`, set `current_object_id = Some(var.object_id)`. No expansion needed.
   - `Field(idx)` → if `current_object_id` is `None`, return `PartialAt(last_materialized)` —
     this means the parent was not an `ObjectRef` and the path is stale or invalid.
     Otherwise call `expand_object_sync(current_object_id.unwrap())`. Then resolve
     `current_object_id = child_object_id_at(current_object_id, idx)` from the loaded fields.
     If the resolved field is not an `ObjectRef`, set `current_object_id = None`.
   - `StaticField(idx)` → if `current_object_id` is `None`, return `PartialAt`. Static
     fields are loaded as a side effect of `expand_object_sync` (inside `set_expansion_done`).
     The preceding `Var` or `Field` step must have already expanded the parent object. If
     static fields are not yet available (edge case: object expanded but statics failed),
     return `PartialAt`. Then resolve `current_object_id` from the static field value if
     it's an `ObjectRef`, otherwise set `current_object_id = None`.
   - `CollectionEntry(cid, k)` → mark collection as `Expanded` in `expansion_phases`,
     call `ensure_collection_entry_loaded(cid, k)`; return `PartialAt` if chunk not yet loaded.
     On success, resolve `current_object_id` from the entry value if it's an `ObjectRef`.
   **Critical:** for `CollectionEntry`, `navigate_to_path` must set
   `expansion_phases[collection_path] = Expanded` before returning `Success` — otherwise the
   entry row exists in `collection_chunks` but is invisible in `flat_items()` (not rendered).
   After successful walk, call `flat_items()` once and place cursor. Note: if thread selection
   triggered `StackState::new()` (which calls `flat_items()` internally), the final
   `flat_items()` at end of walk is a second call — this is accepted as the internal call is
   part of `StackState` initialization, not part of the walk itself.
   No fallbacks, no two-phase design.
   **Non-retriable partials:** `PartialAt` returned from a `Field` or `StaticField` step
   means the parent was not an `ObjectRef` or statics failed to load — the path is stale.
   Only `CollectionEntry` partials (unloaded chunk) are retriable via `pending_navigation`.

6. **`pending_navigation: Option<(NavigationPath, CollectionId)>` added to `App`.** Stores
   the full target path AND the specific `CollectionId` being awaited. On
   `WalkOutcome::PartialAt`, cursor lands on last materialized ancestor and target is stored.
   In `poll_pages`, retry only if the loaded `collection_id` exactly matches the stored
   `CollectionId` — not just any collection appearing in the path. This prevents misdirected
   navigation when two rapid `g` presses overwrite `pending_navigation` and both targets share
   a `CollectionId`.

7. **`StackCursor` is fully deleted.** Atomic removal — no coexistence period.

8. **`collection_restore_cursors` map is deleted.** Collapse = `path.parent()`.

## Implementation Plan

### Tasks

**Pass 1 — Types**

- [x] Task 1: Define ID/index newtypes, `PathSegment`, and `NavigationPath`
  - File: `crates/hprof-tui/src/views/stack_view/types.rs`
  - Action: Add 8 newtypes: `ThreadId(u32)`, `FrameId(u64)`, `CollectionId(u64)`,
    `VarIdx(usize)`, `FieldIdx(usize)`, `StaticFieldIdx(usize)`, `EntryIdx(usize)`,
    `ChunkOffset(usize)` — each `#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]`.
    `ThreadId` uses `u32` (the engine's `thread_serial` type). `ThreadId` is NOT a
    `PathSegment` variant — it lives in `PinKey` and `navigate_to_path` only.
    Add `PathSegment` enum with 5 variants: `Frame(FrameId)`, `Var(VarIdx)`,
    `Field(FieldIdx)`, `StaticField(StaticFieldIdx)`, `CollectionEntry(CollectionId, EntryIdx)`.
    Add `NavigationPath(Vec<PathSegment>)` with private field,
    `#[derive(Hash, PartialEq, Eq, Clone, Debug)]`. Add
    `NavigationPath::parent() -> Option<Self>`.
  - Notes: No public tuple constructor on `NavigationPath`. `parent()` returns `None` at
    minimum depth (1 segment: Frame-only). On Frame+Var (depth 2), returns Frame-only path.
    Raw engine values are wrapped into newtypes at `PathSegment` construction sites.
    `flat_items()` must use `frame.frame_id` (not positional index) when emitting `Frame`
    segments.

- [x] Task 2: Define `NavigationPathBuilder`
  - File: `crates/hprof-tui/src/views/stack_view/types.rs`
  - Action: Add `NavigationPathBuilder` with
    `frame_only(frame_id: FrameId)` (depth-1 path),
    `new(frame_id: FrameId, var_idx: VarIdx)` (depth-2 path),
    `extend(path: NavigationPath)`, `field(FieldIdx)`, `static_field(StaticFieldIdx)`,
    `collection_entry(CollectionId, EntryIdx)`, `build() -> NavigationPath`. All chaining
    methods take `mut self` (move semantics). `build()` calls
    `assert!(invariants_hold(...))` — NOT `debug_assert!`.
  - Notes: `invariants_hold` checks: `Frame` at [0]; if len ≥ 2 then `Var` at [1]; no
    `Frame`/`Var` at positions 2+. A depth-1 path (Frame-only) is valid. Use `assert!` —
    invariant violations are programming errors that must panic in release builds. Do not
    implement `std::convert::From<NavigationPath>` for the builder — `extend()` avoids the
    clippy `should_implement_trait` lint.

- [x] Task 3: Define `RenderCursor`
  - File: `crates/hprof-tui/src/views/stack_view/types.rs`
  - Action: Add `RenderCursor` enum with 8 variants: `NoFrames`, `At(NavigationPath)`,
    `LoadingNode(NavigationPath)`, `FailedNode(NavigationPath)`,
    `CyclicNode(NavigationPath)`, `ChunkSection(NavigationPath, ChunkOffset)`,
    `SectionHeader(NavigationPath)`, `OverflowRow(NavigationPath)`.
    **Do NOT remove `StackCursor` in this task** — deletion is deferred to Task 9 so that
    the codebase compiles throughout Pass 2. Tasks 5–8 add new types and rewrite internals
    while `StackCursor` still exists.
  - Notes: Keep `ExpansionPhase`, `ChunkState`, `CollectionChunks` — unchanged.

- [x] Task 4: Write `NavigationPath` unit tests
  - File: `crates/hprof-tui/src/views/stack_view/tests.rs`
  - Action: Add tests verifying: (a) two builders producing the same logical path are `Eq`
    and produce the same hash; (b) `invariants_hold` rejects `Frame` at position 2+ and
    `Var` at position 2+; (c) `parent()` returns `None` on Frame-only (depth 1), returns
    Frame-only on Frame+Var (depth 2), truncates correctly at depth 3+;
    (d) `frame_only` builds a valid depth-1 path.
  - Notes: Run `cargo test` after this task — compile errors expected until Pass 2.

**Pass 2 — Renderer**

- [x] Task 5: Rewrite `ExpansionRegistry` phase keying
  - File: `crates/hprof-tui/src/views/stack_view/expansion.rs`
  - Action: Replace `object_phases: HashMap<u64, ExpansionPhase>` with
    `expansion_phases: HashMap<NavigationPath, ExpansionPhase>`. Update all methods:
    `expansion_state`, `set_expansion_loading`, `set_expansion_done`,
    `set_expansion_failed`, `cancel_expansion`, `collapse_object` to take
    `path: &NavigationPath` instead of `object_id: u64`.
    Keep `object_fields`, `object_static_fields`, `object_errors`, `collection_chunks`
    keyed by `u64`. `object_errors` stays keyed by `object_id` — the renderer resolves
    the object_id from emitter context (the emitter knows which object_id it is rendering).
    Delete `object_phases` field and `collection_restore_cursors` field.
  - Notes: `collection_restore_cursors` is deleted — collapse now uses `path.parent()`.

- [x] Task 6: Rewrite `emit_object_children` and related emitters
  - File: `crates/hprof-tui/src/views/stack_view/state.rs`
  - Action: Rewrite `emit_object_children`, `emit_collection_children` (gate wrapper),
    `emit_collection_children_inner`, `emit_collection_entry_cursor`,
    `emit_collection_entry_obj_children`, `emit_collection_entry_static_rows` to accept and
    produce `NavigationPath` via `NavigationPathBuilder::extend(parent_path)` at each level.
    `emit_collection_children` must gate on `expansion_phases.get(&collection_path) ==
    Expanded` AND `collection_chunks.get(&id)` — never on `collection_chunks` alone.
    Nested collection entries must carry a distinct `NavigationPath` (not inherit parent's).
    **Important:** call sites in `app/mod.rs` that insert into `collection_chunks` (e.g.
    `expand_collection`, `poll_pages`) must also insert `Expanded` into `expansion_phases`
    for the current path — otherwise `flat_items()` will not render the entries.
  - Notes: Key correctness check — `var[4].roots[0].mixedObjectArray[k]` must produce a
    path that differs from `var[4].roots[j]` at segment level. **Tasks 6 and 7 form an
    atomic change** — emitters and `flat_items()` return type must be updated together.
    Recommended strategy: rewrite one emitter at a time, keeping both `StackCursor` and
    `RenderCursor` return paths temporarily (conversion helper or dual emit). Each emitter
    rewrite is one commit that compiles. Final commit switches `flat_items()` return type
    to `Vec<RenderCursor>` and removes conversion helpers. This avoids a single 500+ line
    unreviewable diff.

- [x] Task 7: Rewrite `flat_items()` and `CursorState` (atomic with Task 6)
  - File: `crates/hprof-tui/src/views/stack_view/state.rs`
  - Action: Change `flat_items() -> Vec<RenderCursor>`. Update `CursorState<StackCursor>`
    to `CursorState<RenderCursor>`. Update all callers of `flat_items()` in `state.rs`
    and `app/mod.rs`.
  - Notes: `StackState::new()` currently creates `StackCursor::OnFrame(0)` as the initial
    cursor — replace with `RenderCursor::At(NavigationPathBuilder::frame_only(first_frame_id).build())`.
    `build_items()` is the ratatui `ListItem` builder that produces the visible rows
    for the stack view widget (parallel to `flat_items()` which produces the semantic cursor
    list). Both must have the same length so scroll index → cursor mapping is correct. The
    `flat_items_build_items_equal_length_invariant` test asserts this invariant.

- [x] Task 8: Update sentinel test for nested collection paths
  - File: `crates/hprof-tui/src/views/stack_view/tests.rs`
  - Action: Update `flat_items_include_nested_collection_entries_for_multidimensional_arrays`
    to assert exact `NavigationPath` values for inner collection entries, not just cursor
    presence. Inner entries must have a path distinct from outer collection entries.
  - Notes: This is the sentinel regression test for the original nested collection path bug.

- [x] Task 9: Delete `StackCursor` and fix compile errors
  - File: `crates/hprof-tui/src/views/stack_view/types.rs` and all callers
  - Action: Remove `StackCursor` enum. Run `cargo build` and fix all resulting compile
    errors across `state.rs`, `app/mod.rs`, `favorites.rs`, `format.rs`, tests.
  - Notes: Do not stub or alias — fix each call site directly.

**Pass 3 — Favorites and Labels**

- [x] Task 10: Migrate `PinKey` to `NavigationPath`
  - File: `crates/hprof-tui/src/favorites.rs`
  - Action: Replace `PinKey` variants' fields (`field_path`, `collection_id`,
    `entry_index`, `collection_restore_cursor`) with three fields:
    `thread_id: ThreadId`, `thread_name: String`, `nav_path: NavigationPath`.
    `thread_id` is needed for thread selection during `g` navigation. `thread_name` is
    needed for display in the favorites panel. Simplify variants — `Frame`, `Var`, `Field`,
    `CollectionEntry`, `CollectionEntryField` may collapse into fewer variants if the path
    encodes all distinctions.
  - Notes: `PinKey` equality uses `thread_id + nav_path` (not `thread_name`). Update all
    `matches!` assertions in tests that compare `PinKey` fields directly.

- [x] Task 11: Rewrite label builders
  - File: `crates/hprof-tui/src/favorites.rs`
  - Action: Replace `build_field_path_label`, `build_collection_entry_label`,
    `build_collection_entry_obj_field_label` with a single `build_label_from_path` that
    walks `NavigationPath` segments. For each `Field(idx)` / `StaticField(idx)`, look up
    field name from `object_fields` / `object_static_fields`. For each
    `CollectionEntry(cid, entry_idx)`, append `[entry_idx]`. For `Var(idx)`, emit
    `"var[{idx}]"`.
  - Notes: Instance field names are available at pin time because the object must be
    expanded to reach a field row. For `StaticField` rows, static fields are loaded lazily —
    `snapshot_from_cursor` (Task 12) must only create a `StaticField` pin when the parent
    object's static section is in `Expanded` phase. If static fields are not loaded,
    `build_label_from_path` must fall back to `"static[{idx}]"` rather than panicking.

- [x] Task 12: Update snapshot factory
  - File: `crates/hprof-tui/src/favorites.rs`
  - Action: Update `snapshot_from_cursor` (or equivalent factory) to derive
    `NavigationPath` from the current `RenderCursor` at pin time using
    `NavigationPathBuilder`. Store in `PinKey.nav_path`.
  - Notes: `RenderCursor::At(path)` → `path.clone()` for the pin.
    `RenderCursor::LoadingNode` / non-`At` variants → pin is a no-op (not pinnable).

**Pass 4 — Navigation**

- [x] Task 13: Implement `navigate_to_path`
  - File: `crates/hprof-tui/src/app/mod.rs`
  - Action: Add `fn navigate_to_path(&mut self, thread_id: ThreadId, path: &NavigationPath) -> WalkOutcome`.
    Add `enum WalkOutcome { Success(NavigationPath), PartialAt(NavigationPath) }`.
    Thread selection (via `select_thread(thread_id.0)`) happens first, before walking
    segments. Walk maintains `current_object_id: Option<u64>` accumulator (see Decision 5).
    Walk segments sequentially: `Frame` → find by serial, expand if needed;
    `Var` → resolve object_id from var; `Field(idx)` → `expand_object_sync(current_id)`,
    then `current_id = child_object_id_at(...)`;
    `StaticField(idx)` → resolve from already-loaded statics (parent object must be
    expanded by a prior step);
    `CollectionEntry(cid, k)` → mark `Expanded` + `ensure_collection_entry_loaded(cid, k)`,
    return `PartialAt` if chunk not loaded.
    After successful walk, call `flat_items()` once and place `RenderCursor::At(path)`.
  - Notes: No fallbacks. No `find_collection_entry_cursor`. If thread selection triggered
    `StackState::new()` (internal `flat_items()` call), the final `flat_items()` is a second
    call — accepted as part of `StackState` initialization.

- [x] Task 14: Add `pending_navigation` and retry logic
  - File: `crates/hprof-tui/src/app/mod.rs`
  - Action: Add `pending_navigation: Option<(NavigationPath, CollectionId)>` to `App`. On
    `WalkOutcome::PartialAt(last)`, set cursor to `RenderCursor::At(last)` and store
    target in `pending_navigation`. In `poll_pages`, after a chunk loads successfully,
    check if loaded `collection_id` exactly matches the stored `CollectionId` in
    `pending_navigation` — if yes, retry `navigate_to_path` and clear `pending_navigation`.
  - Notes: Store `(NavigationPath, CollectionId)` — the exact collection being awaited —
    not just the target path. Match against the specific `CollectionId`, not any collection
    appearing anywhere in the path segments. Clear `pending_navigation` after retry regardless
    of `WalkOutcome`.

- [x] Task 15: Wire `g` to `navigate_to_path` and delete legacy functions
  - File: `crates/hprof-tui/src/app/mod.rs`
  - Action: Replace `navigate_stack_cursor_to_pin_key` call in `g` handler with
    `navigate_to_path(pin.thread_id, &pin.nav_path)`. Delete `navigate_stack_cursor_to_pin_key`,
    `ensure_cursor_materialized`, `find_collection_entry_cursor`,
    `find_collection_entry_obj_field_cursor`, `remap_cursor_frame`.
    `remap_cursor_frame` remaps a cursor to a new frame index after thread/frame selection
    changes — this remapping is no longer needed because `navigate_to_path` selects the
    correct frame by `FrameId` directly. Verify with `cargo build` that it has no callers
    before deletion.
  - Notes: Delete atomically — do not stub. Compile errors reveal any remaining callers.

**Pass 5 — Tests and Cleanup**

- [x] Task 16: Add regression tests — nested collection pin + navigation
  - File: `crates/hprof-tui/src/app/tests.rs`
  - Action: Add test `navigate_to_path_nested_collection_lands_on_exact_entry`: set up
    state with `var[0].roots` (collection) → `roots[0]` (object) → `mixedObjectArray`
    (collection) → entry k. Pin entry k. Call `navigate_to_path`. Assert cursor is
    `RenderCursor::At(path)` where path matches the pinned `NavigationPath`.
  - Notes: Use existing test helpers (`make_frame`, `make_var_object_ref`) as models.

- [x] Task 17: Add regression tests — static field pin + navigation
  - File: `crates/hprof-tui/src/app/tests.rs`
  - Action: Add test `navigate_to_path_static_field_lands_on_exact_row`: set up object
    with static fields, pin a static field, call `navigate_to_path`, assert exact cursor.

- [x] Task 18: Add regression tests — instance-scoped expansion
  - File: `crates/hprof-tui/src/views/stack_view/tests.rs`
  - Action: Add test `expansion_at_path_a_does_not_affect_path_b`: same `object_id`
    reachable via two paths. Expand at Path A. Assert Path B `expansion_phases` is
    `Collapsed`. Also test same `collection_id` at two paths — expanding at Path A must
    not render entries at Path B.

- [x] Task 19: Add regression test — `WalkOutcome::PartialAt` and retry
  - File: `crates/hprof-tui/src/app/tests.rs`
  - Action: Add test `navigate_to_path_partial_on_unloaded_chunk`: set up collection with
    `ChunkState::Collapsed` for the target entry's chunk. Assert `WalkOutcome::PartialAt`
    is returned and cursor is on last materialized ancestor.

- [x] Task 20: Final cleanup and validation
  - File: all modified files
  - Action: Delete `collection_restore_cursors` map if any reference remains. Remove
    `field_path` reconstruction helpers. Run:
    `cargo test --all`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.
  - Notes: Zero warnings required. Zero dead code allowed.

### Acceptance Criteria

- [x] **AC1 — Correct labels on nested collection pins**
  - Given `var[4].roots[0].mixedObjectArray[k]` is pinned in the favorites panel,
  - When the pin label is displayed,
  - Then it reads `"var[4].<roots_field_name>[0].<mixedObjectArray_field_name>[k]"`,
    not `"var[4].<roots_field_name>[k]"`.

- [x] **AC2 — Exact source navigation on nested collections**
  - Given a pin at `var[4].roots[0].mixedObjectArray[k]`,
  - When the user presses `g`,
  - Then focus lands on exactly that entry row, not a parent or neighbor fallback.

- [x] **AC3 — Instance-scoped expansion and collapse**
  - Given two occurrences of the same `object_id` visible in the tree simultaneously,
  - When one occurrence is expanded,
  - Then the other occurrence remains collapsed.
  - And given an expanded node at path P,
  - When the user collapses it,
  - Then the cursor moves to `P.parent()` without affecting other expanded paths.

- [x] **AC4 — Static object field pin and navigation**
  - Given a static field on an expanded object is pinned,
  - When the user presses `g`,
  - Then focus lands on the exact `StaticField` row.

- [x] **AC5 — Navigation walk partial result and async retry**
  - Given a pin whose target entry is in an unloaded collection chunk,
  - When `navigate_to_path` is called,
  - Then `WalkOutcome::PartialAt` is returned and cursor lands on the last successfully
    materialized ancestor, not the frame root.
  - And when the correct chunk finishes loading (its `collection_id` matches the stored
    `CollectionId` in `pending_navigation`),
  - Then `navigate_to_path` is retried automatically and cursor lands on the exact target.
  - And when a different collection's chunk loads concurrently,
  - Then no spurious retry occurs and `pending_navigation` is unchanged.

- [x] **AC6 — Regression safety**
  - Given all existing favorites and stack view tests,
  - When the full migration is complete,
  - Then `cargo test --all` passes with zero failures, `navigate_to_path` calls
    `flat_items()` exactly once after a successful walk (additional internal calls from
    `StackState::new()` during thread switch are accepted but are not part of the walk),
    and existing pin+navigation for `Var`, `Field`, `Frame`, and `CollectionEntryField`
    cases continue to work correctly.

- [x] **AC7 — NavigationPath builder invariants**
  - Given any `NavigationPath` produced by `NavigationPathBuilder`,
  - When segments are inspected,
  - Then `Frame` is at index 0, `Var` at index 1, and no `Frame`/`Var` appears at
    index 2 or beyond.
  - And given two `NavigationPathBuilder` calls producing the same logical path,
  - Then the resulting `NavigationPath` values compare equal under `Eq` and produce
    identical hashes.

## Additional Context

### Dependencies

- Stories 9.8 and 9.9 must be merged — this spec builds on the `PinKey` and snapshot
  factory introduced there.
- No engine or parser crate changes required.
- No external library additions required.

### Testing Strategy

- **Unit tests (Pass 1):** `NavigationPath` equality, builder invariants, `parent()` — in
  `views/stack_view/tests.rs`.
- **Integration tests (Pass 5):** `navigate_to_path` end-to-end with real `App` state —
  in `app/tests.rs`. Use existing `make_frame`, `make_var_object_ref` helpers.
- **Sentinel regression (Pass 2):** Update
  `flat_items_include_nested_collection_entries_for_multidimensional_arrays` to assert
  exact `NavigationPath` values.
- **Full validation after each pass:**
  ```
  cargo test --all
  cargo clippy --all-targets -- -D warnings
  cargo fmt -- --check
  ```

### Notes

- **Known limitation: `FrameId` ambiguity for recursive methods.** If the same method
  appears multiple times in a thread's call stack (recursion), two distinct stack frames
  share the same HPROF `STACK_FRAME` serial (`FrameId`). `navigate_to_path` walks frames
  top-to-bottom and stops at the first match — this means it always navigates to the
  topmost occurrence. Recursive frame pinning is considered low-value and no mitigation is
  planned. If needed later, a positional disambiguator (occurrence index) can be added to
  `Frame(FrameId)` without breaking the path model.
- **Known limitation:** `collection_chunks` remains keyed by `collection_id` (shared data
  cache). If the same `collection_id` is reachable via two distinct `NavigationPath`s,
  chunk data loaded at one path is reused at the other. This is correct — `expansion_phases`
  per path is the sole render gate. Keying `collection_chunks` by path is out of scope.
- **Pre-mortem mitigations applied:** (1) `StackCursor` deleted atomically — no coexistence
  drift. (2) `field_path` removed from `PinKey` in same pass as `NavigationPath`
  introduction — no dual encoding. (3) `pending_navigation` retry checks `collection_id`
  before retrying — no misdirected navigation. (4) Sentinel test on nested collection
  `NavigationPath` distinctness added in Pass 2.
- **`collection_restore_cursors` deleted:** Collapse now uses `NavigationPath.parent()` —
  no compensatory storage needed.
