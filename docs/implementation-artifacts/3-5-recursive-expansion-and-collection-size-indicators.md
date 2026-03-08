# Story 3.5: Recursive Expansion & Collection Size Indicators

Status: done

## Story

As a user,
I want to navigate nested objects by expanding fields recursively and see collection sizes before
expanding them,
So that I can explore the full object graph and know the size of collections before loading them.

## Acceptance Criteria

1. **Given** an expanded object with a nested complex field (`ObjectRef`)
   **When** I press Enter on that field
   **Then** its fields are loaded asynchronously with the same rules as top-level expansion (FR14)

2. **Given** a collection-type object (e.g., HashMap, ArrayList)
   **When** it is displayed in a field list before expansion
   **Then** the entry count is shown as an indicator, e.g. `HashMap (12 entries) [expand →]`
   (FR18)

3. **Given** an object expanded to depth 3 (var ObjectRef → field ObjectRef → nested field)
   **When** I press Enter on the innermost ObjectRef field
   **Then** its fields are displayed at the correct deeper indentation level

4. **Given** an object expanded at depth 1 or deeper
   **When** I press Enter again on an already-expanded ObjectRef field
   **Then** it collapses (toggle behaviour, same as root-level)

5. **Given** an object expanded to any depth with nested expansions
   **When** I press Enter on the root ObjectRef var to collapse it
   **Then** all nested expansion state (fields, phases, errors) for all descendant objects is
   cleaned up recursively

6. **Given** any expansion at any depth that fails
   **When** the error occurs
   **Then** the `! Failed to resolve object` node appears at the correct depth (NFR6)

## Tasks / Subtasks

- [x] Task 1 — Add `class_names_by_id` index to `PreciseIndex` (AC: 2)
  - [x] 1.1 Add `class_names_by_id: HashMap<u64, String>` field to `PreciseIndex` (key =
    `class_object_id`, value = Java simple name decoded from string)
  - [x] 1.2 Populate in `first_pass.rs` when a `LOAD_CLASS` record is parsed: resolve the string
    from `index.strings.get(&class_name_string_id)`, strip JVM binary-name slashes (replace `/`
    with `.`), insert into `class_names_by_id[class_object_id]`
  - [x] 1.3 Re-export `class_names_by_id` through `hprof-parser/src/lib.rs` if needed
  - [x] 1.4 Unit tests: first pass populates `class_names_by_id` correctly for a LOAD_CLASS record
    with a string already in the index; binary name `java/util/HashMap` becomes `java.util.HashMap`

- [x] Task 2 — Enrich `FieldValue::ObjectRef` with class name and collection size (AC: 2)
  - [x] 2.1 Change `FieldValue::ObjectRef(u64)` to
    `FieldValue::ObjectRef { id: u64, class_name: String, entry_count: Option<u64> }` in
    `engine.rs`
  - [x] 2.2 Fix all existing match arms on `FieldValue::ObjectRef` across `resolver.rs`,
    `stack_view.rs`, `app.rs`, and test code (compiler-guided)
  - [x] 2.3 In `engine_impl.rs` `expand_object`: after `decode_fields`, add an enrichment pass.
    For each field where `value == FieldValue::ObjectRef { id, .. }` (raw id from resolver):
    - Call `self.hfile.find_instance(id)` to get the child's `RawInstance`
    - Look up `class_name = index.class_names_by_id.get(&raw.class_object_id)` → fallback
      `"Object"`
    - Detect collection type and entry count (see Task 3)
    - Reconstruct `FieldValue::ObjectRef { id, class_name, entry_count }`
  - [x] 2.4 If `find_instance` returns `None` for a child ObjectRef, keep
    `FieldValue::ObjectRef { id, class_name: "Object".to_string(), entry_count: None }` —
    non-fatal
  - [x] 2.5 Update `resolver.rs` `read_field_value` to return the raw `FieldValue::ObjectRef {
    id, class_name: String::new(), entry_count: None }` (engine_impl enriches afterward)
  - [x] 2.6 Unit tests: `expand_object` on an object with one ObjectRef field to a known class
    returns enriched class name; ObjectRef to unknown ID returns `"Object"` fallback

- [x] Task 3 — Collection entry count detection (AC: 2)
  - [x] 3.1 In `engine_impl.rs` (or a helper), implement `collection_entry_count(raw:
    &RawInstance, index: &PreciseIndex, id_size: u32) -> Option<u64>`:
    - Look up `class_name` via `class_names_by_id[raw.class_object_id]`
    - If class name ends with one of the known suffixes (case-insensitive): `HashMap`,
      `LinkedHashMap`, `TreeMap`, `ConcurrentHashMap`, `Hashtable`, `ArrayList`, `LinkedList`,
      `Vector`, `ArrayDeque`, `HashSet`, `LinkedHashSet`, `TreeSet`, `CopyOnWriteArrayList` →
      decode fields and find the first `Int` or `Long` field named `size` or `elementCount` →
      return its value as `u64`
    - Otherwise return `None`
  - [x] 3.2 Unit tests for collection detection: HashMap with `size=524288` returns
    `Some(524288)`; a plain object returns `None`; unknown class name returns `None`

- [x] Task 4 — Extend `StackCursor` to support recursive paths (AC: 1, 3, 5)
  - [x] 4.1 Change `OnObjectField` from `{ frame_idx, var_idx, field_idx: usize }` to
    `{ frame_idx: usize, var_idx: usize, field_path: Vec<usize> }` in `stack_view.rs`.
    A `field_path` of length 1 (`[field_idx]`) is equivalent to the old single-level variant.
  - [x] 4.2 Change `OnObjectLoadingNode` from `{ frame_idx, var_idx }` to
    `{ frame_idx: usize, var_idx: usize, field_path: Vec<usize> }`.
    Empty `field_path` (`[]`) = loading node for the root ObjectRef var (existing behaviour).
    Non-empty = loading node for a nested ObjectRef field.
  - [x] 4.3 Fix all pattern matches on `OnObjectField` and `OnObjectLoadingNode` in
    `flat_items`, `build_items`, `selected_frame_id`, `selected_loading_object_id`,
    `sync_list_state`, and `move_down`/`move_up` (compiler-guided)
  - [x] 4.4 Add `selected_field_ref_id() -> Option<u64>` to `StackState`:
    returns the ObjectRef `id` if cursor is `OnObjectField` AND the field at `field_path` has
    value `FieldValue::ObjectRef { id, .. }` and phase is `Collapsed` or `Failed`. Used by App
    to start nested expansion.
  - [x] 4.5 Update existing tests for `OnObjectField` and `OnObjectLoadingNode` to use
    `field_path: vec![n]` syntax; add new tests for depth-2 navigation

- [x] Task 5 — Emit nested items in `flat_items` and `build_items` (AC: 1, 3, 6)
  - [x] 5.1 In `flat_items`: for each `OnObjectField` where the field has
    `FieldValue::ObjectRef { id, .. }`, recurse — emit child cursors based on
    `expansion_state(id)`, with `field_path = [..parent_path, field_idx]`. Use a recursive
    helper `emit_field_children(object_id, parent_path, ...)` to avoid code duplication.
  - [x] 5.2 Guard against infinite recursion: if `field_path.len() >= 16` (unlikely but
    possible from a corrupt file), stop emitting children for that path.
  - [x] 5.3 In `build_items`: indentation = `2 + 2 * field_path.len()` spaces (root fields at
    4 spaces, depth-2 at 6, depth-3 at 8, etc.)
  - [x] 5.4 For `ObjectRef` fields, display format:
    - Collapsed: `{indent}{name}: {ClassName} [expand →]` or
      `{indent}{name}: {ClassName} ({N} entries) [expand →]` for collections
    - Expanded: `{indent}{name}: {ClassName} [▼]`
    - Loading: `{indent}{name}: Object [▼]` + child `{indent+2}~ Loading...`
    - Failed: `{indent}! Failed to resolve object`
  - [x] 5.5 Unit tests: `flat_items` with a depth-2 expansion emits the correct cursor sequence;
    `build_items` produces correct indentation at depth 1 and depth 2

- [x] Task 6 — App input handling for nested expansion (AC: 1, 4)
  - [x] 6.1 In `handle_stack_frames_input` Enter arm, extend the `Cmd` enum with:
    - `StartNestedObj(u64)` — expand a nested ObjectRef field
    - `CollapseNestedObj(u64)` — collapse an already-expanded nested ObjectRef field
  - [x] 6.2 In the cmd collection: when `cursor == OnObjectField { field_path, .. }`, call
    `stack_state.selected_field_ref_id()` and check `expansion_state(id)`:
    - `Collapsed` / `Failed` → `Cmd::StartNestedObj(id)` → `start_object_expansion(id)` (same
      method as before — no change needed to the method itself)
    - `Expanded` → `Cmd::CollapseNestedObj(id)` → `stack_state.collapse_object_recursive(id)`
    - `Loading` → no-op
  - [x] 6.3 Esc on `OnObjectLoadingNode { field_path: non-empty, .. }`: cancel the nested
    expansion (same logic as existing root-level cancel)
  - [x] 6.4 Unit tests: Enter on a nested ObjectRef field starts expansion; Enter again collapses
    it; Esc on a nested loading node cancels and stays in StackFrames focus

- [x] Task 7 — Recursive collapse cleanup (AC: 5)
  - [x] 7.1 Add `collapse_object_recursive(&mut self, object_id: u64)` to `StackState`.
    Implementation: call `self.object_fields.get(&object_id).cloned()` to get the field list,
    then for each field that is an `ObjectRef { id, .. }` call
    `collapse_object_recursive(id)` (depth-first). Finally call `collapse_object(object_id)` to
    remove the object itself.
  - [x] 7.2 Also guard against cycles: maintain a `visited: HashSet<u64>` across the recursion.
  - [x] 7.3 Update `handle_stack_frames_input` Enter collapse-root arm (`Cmd::CollapseObj`) to
    call `collapse_object_recursive` instead of `collapse_object`.
  - [x] 7.4 Unit tests: collapsing root object that has a nested expanded child removes both from
    `object_phases`/`object_fields`; cycle guard doesn't infinite-loop

- [x] Task 8 — Update `StackState.toggle_expand` collapse path (AC: 5)
  - [x] 8.1 When collapsing a frame (toggle_expand collapse arm), for each var in the frame that
    has an expanded ObjectRef, call `collapse_object_recursive(object_id)` to clean up expansion
    state. This ensures no orphaned expansion state when a frame is collapsed.
  - [x] 8.2 Unit test: expand frame → expand nested object → collapse frame → assert
    `object_phases` is empty

- [x] Task 9 — Display format update for `ObjectRef` in vars (AC: 2)
  - [ ] 9.1 In `build_items`, update the root-var display (currently shows `"Object [expand →]"`
    for collapsed ObjectRef vars). Change to use the enriched class name from fields once the
    object has been expanded at least once (from `object_fields`). For a collapsed var that has
    never been expanded, we don't have the class name yet — keep `"Object [expand →]"` as the
    pre-expansion label. After expansion, the var row shows `ClassName [▼]`.
    **DEFERRED to Story 3.6**: showing the class of the root ObjectRef var requires an additional
    engine query (the root var carries only `object_id`, not class info). The `object_fields` map
    holds the object's *fields*, not the object's own class name. Deferred per Task 9.2 note.
  - [x] 9.2 NOTE: collection size at root-var level requires a separate engine query not covered
    in this story — defer to Story 3.6 or a future enhancement. The collection size indicator
    (AC2) applies to ObjectRef FIELDS of expanded objects (level 2+), not to root vars.

- [x] Task 10 — Full test suite green
  - [x] 10.1 Run `cargo test --workspace` — all tests must pass
  - [x] 10.2 Run `cargo clippy --workspace -- -D warnings` — zero warnings
  - [x] 10.3 Run `cargo fmt --check` — clean

## Dev Notes

### Field Path Design

`field_path: Vec<usize>` encodes navigation depth from the root ObjectRef var:
- `field_path = []` (empty) — loading/error node for the root var (existing behaviour)
- `field_path = [2]` — field at index 2 of the root var's expanded object (depth 1)
- `field_path = [2, 1]` — field at index 1 of field 2's expanded object (depth 2)

Given an `OnObjectField { frame_idx, var_idx, field_path }`, to find the object_id that owns
the field at `field_path[-1]`:
- Start from the root var: get `object_id_A` from var value (`VariableValue::ObjectRef(id)`)
- Walk `field_path[0..len-1]`: for each step `i`, get `fields = object_fields[current_id]`,
  extract `fields[field_path[i]].value` as `ObjectRef { id, .. }` → `current_id = id`
- The last element `field_path[-1]` is the index of the field within `current_id`'s fields

### `collect_object_refs_recursive` helper

To find all descendant object_ids reachable from a given object (for recursive collapse):
```rust
fn collect_descendants(root_id: u64, fields: &HashMap<u64, Vec<FieldInfo>>,
                       visited: &mut HashSet<u64>, out: &mut Vec<u64>) {
    if !visited.insert(root_id) { return; }
    if let Some(field_list) = fields.get(&root_id) {
        for f in field_list {
            if let FieldValue::ObjectRef { id, .. } = f.value {
                collect_descendants(id, fields, visited, out);
            }
        }
    }
    out.push(root_id);
}
```
Call this, then for each id in `out`: remove from `object_phases`, `object_fields`,
`object_errors`.

### Class Name Resolution Chain

```
object_id  (from FieldValue::ObjectRef)
  │ find_instance(object_id) → RawInstance { class_object_id, ... }
  │ index.class_names_by_id[class_object_id]
  └→ "java.util.HashMap"
```
`class_names_by_id` is populated during first pass from `LOAD_CLASS` records:
```
ClassDef { class_object_id, class_name_string_id, ... }
  │ index.strings[class_name_string_id].value → "java/util/HashMap"
  │ replace '/' with '.'
  └→ class_names_by_id[class_object_id] = "java.util.HashMap"
```
Simple name extraction (for display): split by `.` and take the last segment.

### Enrichment is Async-Safe

The enrichment pass in `expand_object` runs inside the worker thread (spawned in
`start_object_expansion`). Each `find_instance` call within the enrichment pass may perform
file I/O. This is acceptable — the worker thread is off the main event loop.

### FieldValue::ObjectRef Variant Change Impact

The existing pattern `FieldValue::ObjectRef(u64)` appears in:
- `resolver.rs:read_field_value` — change to struct variant with empty class_name/None count
- `stack_view.rs:format_field_value` — use `{ id, .. }` (existing display → update for class name)
- `stack_view.rs:build_items` — update display format
- `stack_view.rs:flat_items` — update expansion check
- `stack_view.rs:selected_object_id` — update match (root var is `VariableValue::ObjectRef` which
  is unchanged)
- `app.rs:tests` — update StubEngine.expand_object to return enriched fields
- `engine.rs:tests` — update field_value_variants_exist test

Note: `VariableValue::ObjectRef(u64)` is unchanged — it lives in a different enum for local vars.

### Collection Field Name Heuristics

Look for int/long fields named `size` first, then `elementCount`, then `count`. Only check
fields on the immediate class (not super) — the `size` field in java.util.HashMap is declared
in HashMap itself.

Use simple suffix matching on the short class name (after last `.`):
```rust
const COLLECTION_CLASS_SUFFIXES: &[&str] = &[
    "HashMap", "LinkedHashMap", "TreeMap", "ConcurrentHashMap", "Hashtable",
    "ArrayList", "LinkedList", "Vector", "ArrayDeque",
    "HashSet", "LinkedHashSet", "TreeSet", "CopyOnWriteArrayList",
    "PriorityQueue",
];
```

### Module Structure — Files to Change

```
crates/hprof-parser/src/
└── indexer/
    ├── precise.rs          add class_names_by_id field
    └── first_pass.rs       populate class_names_by_id from LOAD_CLASS records

crates/hprof-engine/src/
├── engine.rs               FieldValue::ObjectRef: (u64) → { id, class_name, entry_count }
├── engine_impl.rs          add enrichment pass after decode_fields; collection_entry_count helper
└── resolver.rs             read_field_value returns raw ObjectRef with empty class_name/None count

crates/hprof-tui/src/views/
└── stack_view.rs           StackCursor variants, flat_items, build_items, collapse_object_recursive,
                            selected_field_ref_id, indentation scaling

crates/hprof-tui/src/
└── app.rs                  handle_stack_frames_input: StartNestedObj/CollapseNestedObj cmd
```

### Previous Story Intelligence (3.4)

Patterns established in 3.4 to continue:
- `StackState` mutations always go through methods — no direct field access from `App`
- `flat_items()` rebuilds the full flat list on every call (no caching)
- `build_items()` produces `Vec<ListItem<'static>>`
- Async pattern: `thread::spawn` + `mpsc::channel`, `App.poll_expansions()` called each frame
- The local enum `Cmd` inside `handle_stack_frames_input` to separate immutable read from mutable
  write (avoids borrow conflicts)
- `collapse_object` removes from all three maps; the new `collapse_object_recursive` wraps it
- Theme constants from `theme.rs` — no inline colors
- `App<E: NavigationEngine>` — engine generic
- Error handling: non-fatal issues → `None` or warning string, never panic
- 289 tests pass after Story 3.4 baseline

### References

- [Source: docs/planning-artifacts/epics.md#Story 3.5]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture — Navigation Engine Trait]
- [Source: docs/planning-artifacts/ux-design-specification.md — State 3 mockup with collection sizes]
- [Source: crates/hprof-parser/src/indexer/precise.rs — PreciseIndex fields]
- [Source: crates/hprof-parser/src/types.rs — ClassDef (class_object_id, class_name_string_id)]
- [Source: crates/hprof-engine/src/engine.rs — FieldValue enum]
- [Source: crates/hprof-engine/src/resolver.rs — decode_fields, read_field_value]
- [Source: crates/hprof-engine/src/engine_impl.rs — expand_object, find_instance usage]
- [Source: crates/hprof-tui/src/views/stack_view.rs — StackCursor, StackState, flat_items,
  build_items, 289 baseline test count]
- [Source: crates/hprof-tui/src/app.rs — Cmd enum pattern, start_object_expansion, poll_expansions]
- [Source: docs/implementation-artifacts/3-4-object-resolution-and-single-level-expansion.md —
  async architecture, BinaryFuse8 false positive notes, field_path design foundation]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- **Task 1**: Added `class_names_by_id: HashMap<u64, String>` to `PreciseIndex`. Populated in
  `first_pass.rs` when tag `0x02` (LOAD_CLASS) is parsed — resolves string from existing index,
  replaces `/` with `.`. Works for both `(Ok, true)` and `(Ok, false)` match arms.
- **Task 2+3**: Changed `FieldValue::ObjectRef(u64)` to struct variant `{ id, class_name,
  entry_count }`. `resolver.rs` returns raw (empty class_name, None count); `engine_impl.rs`
  enriches after `decode_fields` via `find_instance` + `class_names_by_id` lookup. Implemented
  `collection_entry_count` free function with super-class field skipping to find `size` /
  `elementCount` / `count` fields in known collection types.
- **Task 4**: `OnObjectField.field_idx` → `field_path: Vec<usize>`. `OnObjectLoadingNode`
  gains `field_path` (empty = root, non-empty = nested). Added `selected_field_ref_id()` and
  `resolve_object_at_path()` helpers.
- **Task 5**: Recursive `emit_object_children` and `build_object_items` helpers in `StackState`.
  Depth guard at 16. Indentation formula: `2 + 2 * (depth + 1)` spaces.
- **Task 6**: Extended `Cmd` enum with `StartNestedObj`/`CollapseNestedObj`. Reuses
  `start_object_expansion` (no change needed). `CollapseObj` and `CollapseNestedObj` both call
  `collapse_object_recursive`.
- **Task 7**: `collect_descendants` free function (depth-first post-order with cycle guard).
  `collapse_object_recursive` delegates to it.
- **Task 8**: `toggle_expand` collapse arm now calls `collapse_object_recursive` for each
  ObjectRef var in the frame. Cursor reset extended to `OnObjectField` and `OnObjectLoadingNode`.
- **Task 9**: Root-var display keeps `"Object [expand →]"` / `"Object [▼]"` (class name at
  root-var level deferred to Story 3.6 per story note). Field-level display uses enriched
  class name with optional entry count.
- **Tests**: 304 total (baseline 289, +15 new tests). All pass. Clippy clean. fmt clean.

### File List

- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/resolver.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/app.rs`
- `docs/implementation-artifacts/sprint-status.yaml`
- `docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md`

## Change Log

- 2026-03-07: Story 3.5 implemented — recursive expansion, collection size indicators,
  `class_names_by_id` index, `FieldValue::ObjectRef` enrichment, `field_path`-based
  `StackCursor`, `collapse_object_recursive`, 15 new tests (304 total).
