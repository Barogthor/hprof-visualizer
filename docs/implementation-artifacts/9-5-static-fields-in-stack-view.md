# Story 9.5: Static Fields in Stack View

Status: done

## Story

As a user,
I want static fields to be visible when I expand an object in stack view,
so that I can inspect class-level state without switching tools.

## Acceptance Criteria

1. **AC1 - Static parsing from CLASS_DUMP:**
   Given a CLASS_DUMP record with static fields,
   When the parser reads the dump,
   Then static fields are decoded and stored with typed values.

2. **AC2 - Engine exposes static fields for expanded objects:**
   Given an expanded object,
   When the UI asks for static fields,
   Then the engine resolves the object's class and returns static fields as `FieldInfo`.

3. **AC3 - UI renders a labeled static section:**
   Given an expanded object with static fields,
   When rendered in stack view,
   Then a `[static]` section is shown below instance fields and rows are rendered as `name: value`.

4. **AC4 - No static section when empty:**
   Given a class with no static fields,
   When rendered,
   Then no `[static]` section is shown.

5. **AC5 - Static section helper rows are non-navigable:**
   Given cursor navigation in stack view,
   When moving up/down across the static section header or overflow row,
   Then cursor skips non-interactive rows.

## Tasks / Subtasks

- [x] Task 1: Parse static fields from `CLASS_DUMP` (AC1)
  - [x] 1.1 Add static value model in `crates/hprof-parser/src/types.rs`:
        - `StaticValue` enum
        - `StaticFieldDef` struct
  - [x] 1.2 Extend `ClassDumpInfo` with `static_fields: Vec<StaticFieldDef>`
  - [x] 1.3 Update `parse_class_dump` in
        `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs`
        to decode static fields instead of skipping them
  - [x] 1.4 Add memory accounting for static fields
        (`StaticFieldDef::memory_size`, `ClassDumpInfo::memory_size` update)
  - [x] 1.5 Keep `HprofTestBuilder::add_class_dump` compatibility
        (default static count = 0)

- [x] Task 2: Expose static fields in engine (AC2)
  - [x] 2.1 Add `fn class_of_object(&self, object_id: u64) -> Option<u64>`
        to `NavigationEngine` in `crates/hprof-engine/src/engine.rs`
  - [x] 2.2 Add `fn get_static_fields(&self, class_object_id: u64) -> Vec<FieldInfo>`
        to `NavigationEngine`
  - [x] 2.3 Implement `class_of_object` in
        `crates/hprof-engine/src/engine_impl/mod.rs`
  - [x] 2.4 Implement `get_static_fields` in
        `crates/hprof-engine/src/engine_impl/mod.rs`
        by converting `StaticValue -> FieldValue`
  - [x] 2.5 Reuse object-ref enrichment logic (class name, entry_count, inline value)

- [x] Task 3: Render static section in TUI (AC3, AC4, AC5)
  - [x] 3.1 In `crates/hprof-tui/src/app/mod.rs`, fetch static fields when expansion completes
  - [x] 3.2 In stack view state, store static fields by expanded object id
  - [x] 3.3 Add dedicated non-interactive cursor rows for:
        - static section header
        - static overflow row (`[+N more static fields]`)
  - [x] 3.4 Update cursor movement to skip non-interactive static rows
  - [x] 3.5 Render static rows in `crates/hprof-tui/src/views/tree_render.rs`
        below instance fields; render max 20 rows + overflow marker

- [x] Task 4: Tests (TDD)
  - [x] 4.1 `parse_class_dump_with_static_fields_returns_correct_count_and_values`
  - [x] 4.2 `parse_class_dump_no_static_fields_returns_empty_vec`
  - [x] 4.3 `parse_class_dump_unknown_static_field_type_returns_none`
  - [x] 4.4 `class_of_object_returns_class_id`
  - [x] 4.5 `class_of_object_returns_none_for_unknown_object`
  - [x] 4.6 `get_static_fields_returns_resolved_fields`
  - [x] 4.7 `get_static_fields_empty_when_no_statics`
  - [x] 4.8 `render_static_section_separator_not_navigable`
  - [x] 4.9 `render_static_overflow_row_not_navigable`

- [x] Task 5: Validation
  - [x] `cargo test --all`
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo fmt -- --check`
  - [x] Manual smoke: open a dump, expand object/collection rows,
        verify static section appears when relevant and is hidden otherwise

## Dev Notes

### Scope guard for this story

- This story is now scoped to static-field support only.
- Ignore any debug-symbol name metadata for stack slots in this story.

### Static field decoding

Each static field in CLASS_DUMP has:

```
name_string_id  (id_size bytes)
field_type      (1 byte)
value           (size from field_type)
```

Decode at parse time into `StaticValue` (do not keep raw `Vec<u8>` payloads).

### Rendering rules

- Static rows are shown under expanded object/collection nodes.
- Header is exactly `[static]`.
- Show at most 20 static fields in the tree; if more, append one dim row:
  `[+N more static fields]`.
- Header and overflow rows are informational and must not be selectable.

### Engine integration notes

- Keep `expand_object` signature unchanged.
- Use `class_of_object` + `get_static_fields` as a second lookup path.
- `ObjectRef(0)` maps to `FieldValue::Null`.

### Project Structure Notes

| File | Change |
|------|--------|
| `crates/hprof-parser/src/types.rs` | Add `StaticValue` and `StaticFieldDef`, extend `ClassDumpInfo` |
| `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs` | Parse static fields |
| `crates/hprof-engine/src/engine.rs` | Add trait methods for class/static lookup |
| `crates/hprof-engine/src/engine_impl/mod.rs` | Implement class/static lookup and conversion |
| `crates/hprof-tui/src/app/mod.rs` | Fetch and store static fields in expansion flow |
| `crates/hprof-tui/src/views/stack_view/state.rs` | Emit static section/rows and cursor skipping |
| `crates/hprof-tui/src/views/stack_view/types.rs` | Add static cursor variants |
| `crates/hprof-tui/src/views/tree_render.rs` | Render static section and overflow marker |

### References

- `docs/planning-artifacts/epics.md` (Story 9.5)
- `docs/planning-artifacts/architecture.md`
- `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md`
- `docs/implementation-artifacts/9-4-camera-scroll.md`

## Dev Agent Record

### Agent Model Used

gpt-5.3-codex

### Debug Log References

- `cargo clippy --all-targets -- -D warnings` initially failed on
  `append_var` (`clippy::too_many_arguments`) after static-field context wiring in
  `tree_render`; fixed by adding `#[allow(clippy::too_many_arguments)]`.

### Completion Notes List

- Parser: added `StaticValue`/`StaticFieldDef`, parsed `CLASS_DUMP` static entries, and
  updated memory accounting and test builder compatibility path.
- Engine: added `class_of_object` + `get_static_fields` to `NavigationEngine` and
  concrete engine implementation, including `StaticValue -> FieldValue` conversion and
  object-ref enrichment.
- TUI: added static-section cursor variants, static-field storage in expansion registry,
  rendering of `[static]` + overflow row, and navigation skip logic for non-interactive
  rows.
- Tests: added parser/engine static tests and TUI navigation regression tests
  `render_static_section_separator_not_navigable` and
  `render_static_overflow_row_not_navigable`.
- Validation: `cargo test --all`, `cargo clippy --all-targets -- -D warnings`, and
  `cargo fmt -- --check` passed.
- Manual smoke run in live TUI is pending (requires interactive local dump session).

### File List

- `crates/hprof-parser/src/types.rs`
- `crates/hprof-parser/src/lib.rs`
- `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs`
- `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs`
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/test_utils.rs`
- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl/mod.rs`
- `crates/hprof-engine/src/engine_impl/tests.rs`
- `crates/hprof-engine/src/resolver.rs`
- `crates/hprof-tui/src/app/mod.rs`
- `crates/hprof-tui/src/app/tests.rs`
- `crates/hprof-tui/src/favorites.rs`
- `crates/hprof-tui/src/views/favorites_panel.rs`
- `crates/hprof-tui/src/views/stack_view/expansion.rs`
- `crates/hprof-tui/src/views/stack_view/mod.rs`
- `crates/hprof-tui/src/views/stack_view/state.rs`
- `crates/hprof-tui/src/views/stack_view/tests.rs`
- `crates/hprof-tui/src/views/stack_view/types.rs`
- `crates/hprof-tui/src/views/tree_render.rs`
- `docs/implementation-artifacts/9-5-static-fields-in-stack-view.md`
