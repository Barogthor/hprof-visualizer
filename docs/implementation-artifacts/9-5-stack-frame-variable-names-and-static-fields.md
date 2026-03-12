# Story 9.5: Stack Frame Variable Names & Static Fields

Status: ready-for-dev

## Story

As a user,
I want to see the real local variable name (e.g., `myList`) from debug info instead of a generic
`local variable: ArrayList` label, and to see static fields alongside instance fields,
so that stack frames are as informative as in VisualVM.

## Acceptance Criteria

1. **AC1 — Variable name resolution infrastructure is wired end-to-end:**
   Given a hprof file that contains LOCAL_VARIABLE records (tag `0x25`)
   When the file is parsed
   Then `frame_variable_names` is populated and `get_local_variables` returns `Some(name)`
   for slots that have a matching entry — verified by synthetic unit tests.
   Note: `<unnamed>` remains the expected output on all real dumps tested (jvisualvm has no
   tag `0x25` records). The slot→root index mapping is heuristic; see Dev Notes for details.

2. **AC2 — Fallback label when debug info absent:**
   Given a stack frame where no LOCAL_VARIABLE record exists for a variable (or the dump has
   none at all, which is the common case for jvisualvm dumps)
   When displayed
   Then a fallback label `<unnamed>: ArrayList` is shown — no crash, no silent omission

3. **AC3 — Static fields in labeled section:**
   Given a class instance is expanded
   When its fields are rendered
   Then static fields are displayed in a clearly labeled section (e.g., `[static]`) below
   instance fields

4. **AC4 — No static section when class has no statics:**
   Given a class with no static fields
   When expanded
   Then no `[static]` section header appears

## Tasks / Subtasks

- [ ] Task 1: Parse static fields from CLASS_DUMP (AC3, AC4)
  - [ ] 1.1 Add to `crates/hprof-parser/src/types.rs`:
        ```rust
        #[derive(Debug, Clone, PartialEq)]
        pub enum StaticValue {
            ObjectRef(u64),
            Bool(bool), Byte(i8), Char(char), Short(i16),
            Int(i32), Long(i64), Float(f32), Double(f64),
        }

        #[derive(Debug, Clone, PartialEq)]
        pub struct StaticFieldDef {
            pub name_string_id: u64,
            pub field_type: u8,
            pub value: StaticValue,
        }
        ```
        `Char` stores a decoded `char` (UTF-16 code unit → Rust char, replacement char on
        invalid surrogate) — NOT a raw `u16`. Matches `FieldValue::Char(char)` in the engine.
  - [ ] 1.2 Add `static_fields: Vec<StaticFieldDef>` to `ClassDumpInfo`
        in `crates/hprof-parser/src/types.rs`
  - [ ] 1.3 Update `parse_class_dump` in
        `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs`
        to read + return static fields instead of skipping them.
        Guard: if `value_byte_size(field_type, id_size) == 0` and `field_type != 0`
        (unknown type), log a warning and return `None` for the entire CLASS_DUMP —
        a corrupted static field layout means the cursor is unrecoverable.
  - [ ] 1.4 Add `MemorySize` impl for `StaticFieldDef` in `crates/hprof-parser/src/types.rs`
        (follow the same pattern as `FieldDef::memory_size` — `std::mem::size_of::<Self>()`).
        Update `ClassDumpInfo::memory_size` to add
        `self.static_fields.capacity() * std::mem::size_of::<StaticFieldDef>()`.
  - [ ] 1.5 Update `HprofTestBuilder::add_class_dump` in `test_utils.rs` so existing tests
        still compile (static_fields_count=0 already in the builder — verify and adjust)
  - [ ] 1.6 Verify `ClassDumpInfo` does NOT derive `Default` — if it does, remove it so
        that all construction sites are caught by the compiler when fields are added

- [ ] Task 2: Parse LOCAL_VARIABLE records (tag `0x25`) (AC1, AC2)
  - [ ] 2.1 Add `LocalVariableRecord` to `RecordTag` enum
        in `crates/hprof-parser/src/tags.rs` with value `0x25`
  - [ ] 2.2 Parse tag `0x25` in `crates/hprof-parser/src/indexer/first_pass/record_scan.rs`
        (top-level records are processed there, not in heap_extraction.rs which handles
        heap sub-records only). Record body layout per HPROF 1.0.2 binary spec:
        ```
        frame_id          (id_size bytes)
        slot              (u32)
        name_string_id    (id_size bytes)
        signature_str_id  (id_size bytes)  ← type descriptor, ignored for now
        length            (u32)            ← scope length in bytecodes, ignored
        line_number       (i32)            ← ignored
        ```
        Parse with `Option` return (same pattern as `parse_class_dump`): if any read fails,
        log a warning and skip this record — do NOT abort the entire parse pass.
  - [ ] 2.3 Add `frame_variable_names: FxHashMap<(u64, u32), u64>`
        (composite key `(frame_id, slot)` → `name_string_id`) to `PreciseIndex`
        in `crates/hprof-parser/src/indexer/precise.rs`.
        Rationale: flat composite-key map is more cache-friendly than a nested
        `FxHashMap<u64, FxHashMap<u32, u64>>` and avoids per-frame inner-map allocation.
        With ~169 frames in the jvisualvm dump this difference is negligible, but the flat
        structure is simpler to reason about and has no correctness downside.
  - [ ] 2.4 Wire parsing results into `PreciseIndex::frame_variable_names` in first_pass
  - [ ] 2.5 Add `add_local_variable_record(frame_id, slot, name_string_id)` to
        `HprofTestBuilder` in `test_utils.rs` so tests 6.3/6.4 can use the builder
        rather than constructing raw bytes manually

- [ ] Task 3: Add `name: Option<String>` to `VariableInfo` (AC1, AC2)
  - [ ] 3.1 Add field `pub name: Option<String>` to `VariableInfo`
        in `crates/hprof-engine/src/engine.rs`
  - [ ] 3.2 Update `get_local_variables` in `engine_impl.rs`:
        for each GC_ROOT_JAVA_FRAME root at enumerate index `i`,
        look up `self.hfile.index.frame_variable_names.get(&(frame_id, i as u32))`
        and resolve the string id → `Some(name)` or `None`
  - [ ] 3.3 Update the stub `get_local_variables` in the test `MinimalEngine`
        in `engine.rs` to include `name: None`
  - [ ] 3.4 Update all existing construction sites: grep for `VariableInfo {`,
        `ClassDumpInfo {`, and `ClassDumpEntry {` — each needs the new field.
        Verify `VariableInfo` and `ClassDumpInfo` do NOT derive `Default` so the compiler
        catches every missed site.

- [ ] Task 4: Expose class identity and static fields via engine trait (AC3, AC4)
  - [ ] 4.1 Add `fn class_of_object(&self, object_id: u64) -> Option<u64>` to
        `NavigationEngine` trait in `engine.rs` — returns the `class_object_id` from
        `INSTANCE_DUMP[object_id]`. Stub in `MinimalEngine` returns `None`.
        Note: this does a second O(1) FxHashMap lookup after `expand_object` — acceptable
        at user-action frequency. Do NOT change the `expand_object` signature.
  - [ ] 4.2 Add `fn get_static_fields(&self, class_object_id: u64) -> Vec<FieldInfo>`
        to `NavigationEngine` trait in `engine.rs`
        (reuses `FieldInfo { name: String, value: FieldValue }` — no new type needed).
        Stub in `MinimalEngine` returns `vec![]`.
  - [ ] 4.3 Implement `class_of_object` in `engine_impl.rs`: read `class_object_id` from
        `index.instance_dumps[object_id]` (or equivalent lookup path for RawInstance).
  - [ ] 4.4 Implement `get_static_fields` in `engine_impl.rs`: iterate only
        `class_dumps[class_object_id].static_fields` — do NOT walk the `super_class_id`
        chain. Static fields belong to the declaring class; superclass statics are not shown
        (consistent with VisualVM behaviour). For each `StaticFieldDef`, resolve the name
        string and convert `StaticValue` → `FieldValue`:
        - `StaticValue` is defined in `hprof-parser` and imported here via the existing
          `hprof-parser` dependency — do NOT redefine it in `hprof-engine`.
        - `Char` conversion: `StaticValue::Char(c) → FieldValue::Char(c)` — direct.
        - `ObjectRef(id)` → resolve class name via `class_names_by_id` (same as instance fields).
        - Do NOT duplicate decoding logic — extract or reuse existing helpers.

- [ ] Task 5: Update TUI rendering (AC1, AC2, AC3, AC4)
  - [ ] 5.1 In `crates/hprof-tui/src/views/tree_render.rs`, update variable label logic:
        - If `name.is_some()` → `"{name}: {label}"` (e.g. `myList: ArrayList`)
        - If `name.is_none()` → `"<unnamed>: {label}"` (replacing current `"local variable: …"`)
        - Failed nodes keep current behavior (no prefix)
  - [ ] 5.2 In `crates/hprof-tui/src/app.rs`, after expanding an object (`Cmd::ExpandObject`):
        ```rust
        let Some(instance_fields) = self.engine.expand_object(oid) else {
            // object not found or not an INSTANCE_DUMP — no fields, no static section
            return;
        };
        let static_fields = self.engine
            .class_of_object(oid)
            .map(|cid| self.engine.get_static_fields(cid))
            .unwrap_or_default();
        // inject instance_fields + FlatItem::StaticSection + static_fields (if non-empty)
        ```
        Only inject `FlatItem::StaticSection` when `static_fields` is non-empty (AC4).
        If `expand_object` returns `None`, skip static fields entirely — no separator.
  - [ ] 5.3 Add two new non-navigable variants to the `FlatItem` enum in `stack_view.rs`:
        ```rust
        FlatItem::StaticSection,           // renders as "  [static]"
        FlatItem::StaticOverflow(usize),    // renders as "  [+N more static fields]"
        ```
        Render `StaticSection` as `  [static]` (dim style).
        Render `StaticOverflow(n)` as `  [+{n} more static fields]` (dim style).
        If `static_fields.len() > 20`, inject only the first 20 `FieldInfo` items, then
        append one `FlatItem::StaticOverflow(total - 20)`. Defer full pagination to a
        future polish story. Both variants must be skipped by `cursor_up`/`cursor_down`.
  - [ ] 5.4 Ensure `cursor_up` / `cursor_down` in `StackState` skip both
        `FlatItem::StaticSection` and `FlatItem::StaticOverflow(_)`
        — same pattern as frame header items are already skipped
  - [ ] 5.5 Audit all `match` on `FlatItem` in the TUI crate: **no `_ =>` wildcards allowed**.
        Every arm must be explicit so the compiler catches both `StaticSection` and
        `StaticOverflow(_)` at every call site.
        Fix any existing wildcards encountered during this story.

- [ ] Task 6: Tests (TDD — write tests first)
  - [ ] 6.1 `parse_class_dump_with_static_fields_returns_correct_count_and_values` — parser
        unit test: assert `static_fields.len()`, name_string_ids, and decoded `StaticValue`
        variants (at minimum one `ObjectRef` and one primitive) match the encoded input
  - [ ] 6.2 `parse_class_dump_no_static_fields_returns_empty_vec` — edge case
  - [ ] 6.3 `get_local_variables_with_name_resolves_string` — engine unit test
  - [ ] 6.4 `get_local_variables_no_debug_info_returns_none_name` — engine unit test
  - [ ] 6.5 `get_static_fields_returns_resolved_fields` — engine unit test
  - [ ] 6.6 `get_static_fields_empty_when_no_statics` — engine unit test
  - [ ] 6.7 `render_var_with_name_shows_name_colon_label` — tree_render unit test
  - [ ] 6.8 `render_var_no_name_shows_unnamed_colon_label` — tree_render unit test
  - [ ] 6.9 `render_static_section_separator_not_navigable` — stack_view unit test
        (assert cursor cannot land on `FlatItem::StaticSection` — cursor_up/down skips it)
  - [ ] 6.10 `class_of_object_returns_class_id` — engine unit test (object with known class)
  - [ ] 6.11 `class_of_object_returns_none_for_unknown_object` — engine unit test
  - [ ] 6.12 `render_static_overflow_row_not_navigable` — stack_view unit test
        (assert cursor skips `FlatItem::StaticOverflow` and row renders `[+N more ...]`)
  - [ ] 6.13 `parse_class_dump_unknown_static_field_type_returns_none` — parser unit test:
        encode a CLASS_DUMP with a static field of type `0xFF` (unknown); assert
        `parse_class_dump` returns `None` and does not panic
  - [ ] 6.14 `local_variable_names_wired_end_to_end` — integration test using
        `HprofTestBuilder` to build a complete file with a STACK_FRAME, a LOCAL_VARIABLE
        record (tag `0x25`), and a GC_ROOT_JAVA_FRAME; parse via `HprofFile::open_bytes`;
        assert `engine.get_local_variables(frame_id)[0].name == Some("myVar".to_string())`.
        This validates the full pipeline: record_scan → PreciseIndex → engine.

- [ ] Task 7: Validation
  - [ ] `cargo test --all`
  - [ ] `cargo clippy --all-targets -- -D warnings`
  - [ ] `cargo fmt -- --check`
  - [ ] Manual smoke: open `assets/heapdump-visualvm.hprof`, expand a frame with objects,
        confirm `<unnamed>: ClassName` labels and that static fields appear for relevant classes
  - [ ] AC1 validation: if a debug-enabled dump (e.g., via `-agentlib:hprof` or `jmap -dump`)
        is available, verify real variable names display. If not available, document explicitly
        in Completion Notes: "AC1 validated by synthetic unit tests only — no debug dump asset"

## Dev Notes

### Implementation order dependency

Stories 9.3, 9.4, and 9.5 all touch the same TUI files (`FlatItem`, `StackState`, `app.rs`,
`stack_view.rs`, `tree_render.rs`). Implement in sequence **9.3 → 9.4 → 9.5** to avoid merge
conflicts. If parallel development is unavoidable, coordinate on which sections of these files
each story owns before starting.

---

### Architecture Decisions Summary (ADRs)

| ADR | Question | Decision |
|-----|----------|---------|
| ADR-1 | Where to decode `StaticValue`? | At parse time in `hprof_primitives.rs` — consistent with existing approach, avoids `Vec<u8>` |
| ADR-2 | How to expose `class_object_id`? | `fn class_of_object()` on the trait — see dedicated section below |
| ADR-3 | `[static]` separator representation | `FlatItem::StaticSection` dedicated variant — clear semantics, compiler-guided exhaustiveness |
| ADR-4 | Store `frame_variable_names` | Direct `FxHashMap` without `Option` — empty map = 40 bytes, negligible |

---

### Tag 0x25 is a top-level record — sequential parsing only

Tag `0x25` (LOCAL_VARIABLE) is a top-level HPROF record, not a heap sub-record. It is
processed in the **sequential** first-pass, not in the parallel `heap_extraction.rs` phase
introduced in Epic 8. There are no ordering or race conditions.

**Before implementing Task 2.2**, verify the actual tag dispatch location by checking
`crates/hprof-parser/src/indexer/first_pass/mod.rs` for the top-level `match tag` block.
The new arm for `RecordTag::LocalVariableRecord` must be added there (or in the helper it
delegates to). `record_scan.rs` likely contains the helper parsing functions, but the
dispatch arm lives in `mod.rs`. Add the arm in whichever file owns the top-level tag match.

`frame_variable_names` is populated in the first-pass and stored in `PreciseIndex` like
all other top-level index maps (`stack_frames`, `threads`, etc.).

---

### Key architecture insight: jvisualvm dumps have NO tag 0x25 records

The test asset (`assets/heapdump-visualvm.hprof`) is a jvisualvm dump. Known tag inventory:
`0x01 STRING`, `0x02 LOAD_CLASS`, `0x04 STACK_FRAME`, `0x05 STACK_TRACE`, `0x1C HEAP_DUMP_SEGMENT`.
**Tag 0x25 (LOCAL_VARIABLE) is absent** — so AC2 (fallback `<unnamed>`) is the common path.
AC1 (real names) applies to debug-enabled dumps (e.g., from `-agentlib:hprof`) only.

### Static field value decoding

Each `StaticFieldDef` in the CLASS_DUMP body contains:
```
name_string_id  (id_size bytes)
field_type      (1 byte)        ← same type codes as instance fields
value           (variable size) ← value_byte_size(field_type, id_size)
```

**Memory safety:** Do NOT store the raw value as `Vec<u8>`. Instead decode it immediately during
`parse_class_dump` into the compact `StaticValue` enum (see Task 1.1). This avoids per-field
heap allocation across thousands of classes in large dumps.

In `get_static_fields()`, convert `StaticValue` → `FieldValue` using a simple match.
For `StaticValue::ObjectRef(id)` resolve class name via `class_names_by_id` (same pattern as
instance field object refs). Do NOT duplicate `decode_field_value` logic — extract or reuse.

### StaticSection representation — decided

`FlatItem::StaticSection` and `FlatItem::StaticOverflow(usize)` are the chosen representation
(ADR-3). Sentinel `FieldInfo` was considered and rejected as a hack. No further decision needed.

### Variable slot ↔ GC_ROOT_JAVA_FRAME index

The HPROF spec says: a GC_ROOT_JAVA_FRAME entry contains `(object_id, thread_serial, frame_serial)`.
Multiple roots for the same `frame_id` accumulate in `java_frame_roots[frame_id]` as a Vec,
in encounter order. The LOCAL_VARIABLE record's `slot` field identifies which slot (0-based).

**Important:** slot numbers may have gaps (e.g., slot 0 = `this`, slot 2 = first param if it's
`long`/`double`). Do NOT assume dense slot → index mapping. Use the slot directly as the key.

In `get_local_variables()`, the current code iterates `java_frame_roots[frame_id]` by enumerate
index `i`. Correct lookup syntax (flat composite key):
```rust
self.hfile.index.frame_variable_names.get(&(frame_id, i as u32))
```
This is a best-effort approximation — for AC2, `None` is always safe.

**Risk — slot mapping is heuristic and will often fail:** JVM slot layout means `long`/`double`
occupy 2 slots, `this` is always slot 0, and constructor params shift accordingly. The mapping
`i as u32` (enumerate index → slot) is wrong in the general case. `<unnamed>` is the expected
output for most variables even when tag `0x25` data is present.

Rule: **prefer `None` over a wrong name.** Only emit `Some(name)` when
`frame_variable_names.get(&(frame_id, i as u32))` returns a hit. Never guess or infer.
AC1 is intentionally best-effort; AC2 is the primary user-visible behaviour.

### Existing tests that reference `VariableInfo`

Search for `VariableInfo {` in all crates — each construction site needs `name: None` added.
The compiler will catch all missed sites.

Tests in `tree_render.rs` that assert `"local variable:"` will need updating to `"<unnamed>:"`.
Specifically `failed_var_label_uses_short_class_without_local_variable_prefix` should remain
passing (failed nodes never show prefix — behavior unchanged).

### ADR-2: Exposing `class_object_id` to the engine — decision record

**Decision: `fn class_of_object(&self, object_id: u64) -> Option<u64>` added to the trait.**

`expand_object` internally reads `INSTANCE_DUMP[oid]` which contains `class_object_id` + field
data bytes. The trait does not currently surface `class_object_id`. Three options were evaluated:

| Option | Description | Rejected because |
|--------|-------------|-----------------|
| A ✅ | `class_of_object` — new separate method | — selected |
| B | Change `expand_object` → `Option<(instance_fields, static_fields)>` | Breaks ~12 call sites, tight coupling |
| C | `expand_object` → `Option<ExpandedObject { ... }>` | Same churn as B; defer if ≥3 fields needed |

**Rationale:** The second O(1) FxHashMap lookup at user-action frequency is negligible.
Breaking all `expand_object` call sites (trait + impl + ~8 tests) in an already large story
is a concrete regression risk. If `ExpandedObject` becomes useful (≥3 fields), migrate in Epic 10.

**Do NOT change the `expand_object` signature.**

### Project Structure Notes

| File | Change |
|------|--------|
| `crates/hprof-parser/src/types.rs` | Add `StaticFieldDef`, extend `ClassDumpInfo` |
| `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs` | Parse static fields instead of skipping |
| `crates/hprof-parser/src/indexer/first_pass/record_scan.rs` | Parse tag 0x25 |
| `crates/hprof-parser/src/indexer/precise.rs` | Add `frame_variable_names` map |
| `crates/hprof-engine/src/engine.rs` | Add `name` to `VariableInfo`, add trait methods |
| `crates/hprof-engine/src/engine_impl.rs` | Implement name resolution + static fields |
| `crates/hprof-tui/src/views/tree_render.rs` | Update variable label format |
| `crates/hprof-tui/src/views/stack_view.rs` | Add `FlatItem::StaticSection` + `FlatItem::StaticOverflow(usize)`, render both |
| `crates/hprof-tui/src/app.rs` | Fetch static fields on expand, inject separator |

### References

- `parse_class_dump` static skip: `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs:135-146`
- `ClassDumpInfo` struct: `crates/hprof-parser/src/types.rs:93-99`
- `VariableInfo` struct: `crates/hprof-engine/src/engine.rs:97-107`
- `get_local_variables` impl: `crates/hprof-engine/src/engine_impl.rs:736-786`
- Variable label render: `crates/hprof-tui/src/views/tree_render.rs:130,134`
- Epic 9 story spec: `docs/planning-artifacts/epics.md` (Story 9.5, FR44, FR45)
- Architecture: `docs/planning-artifacts/architecture.md`
- Previous stories (expand/camera): `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md`,
  `docs/implementation-artifacts/9-4-camera-scroll.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List
