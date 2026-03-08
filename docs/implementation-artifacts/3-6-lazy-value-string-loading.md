# Story 3.6: Lazy Value String Loading

Status: done

## Story

As a user,
I want String object values to be loaded lazily only when I navigate to them,
So that memory is not consumed by string content I never inspect.

## Acceptance Criteria

1. **Given** a String object field in an expanded object list
   **When** it has not yet been navigated to
   **Then** it is displayed as `{name}: String = "..."` — a typed placeholder with no value loaded
   (FR19)

2. **Given** a String field displayed as a placeholder
   **When** I press Enter on it
   **Then** the string value is loaded asynchronously from the hprof file, and the display updates
   to `{name}: String = "actual value"` (truncated to 80 chars with `..` if longer)
   (FR19)

3. **Given** a String field whose backing primitive array cannot be located in the file
   **When** lazy loading is attempted
   **Then** the field updates to `{name}: String = <unresolved>`, a non-fatal warning is emitted
   to the warning list (visible in the status bar count), and navigation continues
   (NFR6)

4. **Given** a String field is in Loading state
   **When** I press Enter again or move the cursor
   **Then** no additional load is started — the pending load completes normally

5. **Given** a loaded or failed String field
   **When** the parent object is collapsed (recursive collapse)
   **Then** the string load state (phase, value, error) is cleared from memory

## Tasks / Subtasks

- [x] Task 1 — Add `FieldValue::StringRef` variant to engine (AC: 1, 2, 3)
  - [x] 1.1 In `engine.rs`, add `StringRef { id: u64 }` to `FieldValue` enum:
    ```rust
    /// Non-null reference to a `java.lang.String` object.
    /// Value is loaded lazily via [`NavigationEngine::resolve_string`].
    StringRef { id: u64 },
    ```
  - [x] 1.2 Update all existing `match` arms on `FieldValue` in `engine.rs` tests to include
    a `StringRef { .. }` arm (compiler-guided).
  - [x] 1.3 Add `resolve_string` method to `NavigationEngine` trait:
    ```rust
    /// Resolves the content of a `java.lang.String` object from the hprof file.
    ///
    /// Returns `Some(value)` if the String's backing primitive array is found and
    /// decoded, `None` if the object or its backing array cannot be located.
    fn resolve_string(&self, object_id: u64) -> Option<String>;
    ```
  - [x] 1.4 Add a stub `resolve_string` impl to `DummyEngine` in `engine.rs` tests returning
    `None`.
  - [x] 1.5 Unit test in `engine.rs`: `StringRef { id: 1 }` variant exists and is distinct from
    `ObjectRef { id: 1, .. }`.

- [x] Task 2 — Add `HprofFile::find_prim_array` in parser (AC: 2, 3)
  - [x] 2.1 In `hprof_file.rs`, add `pub fn find_prim_array(&self, array_id: u64) -> Option<(u8, Vec<u8>)>`
  - [x] 2.2 Implemented with `scan_for_prim_array` helper using segment-filter narrowing loop.
  - [x] 2.3 No re-export needed — method on already-exported `HprofFile`.
  - [x] 2.4 Unit tests in `hprof_file.rs` (behind `test-utils` feature):
    - `find_prim_array` on known char-array ID returns `(5, bytes)`
    - `find_prim_array` on known byte-array ID returns `(8, bytes)`
    - `find_prim_array` on unknown ID returns `None`

- [x] Task 3 — Implement `resolve_string` in engine (AC: 2, 3)
  - [x] 3.1 Implemented `resolve_string` in `engine_impl.rs`
  - [x] 3.2 Added `decode_prim_array_as_string(elem_type: u8, bytes: &[u8]) -> String` free function
  - [x] 3.3 Unit tests for `resolve_string` (char[], byte[], absent array, no value field)
  - [x] 3.4 Unit tests for `decode_prim_array_as_string` (UTF-16, Latin-1, surrogate, unknown type)

- [x] Task 4 — Update enrichment pass to produce StringRef (AC: 1)
  - [x] 4.1 `expand_object` enrichment: `java.lang.String` ObjectRef → `StringRef { id }`
  - [x] 4.2 Unit test: expanding object with String field produces `FieldValue::StringRef { id }`

- [x] Task 5 — TUI: StringRef display and loading state (AC: 1, 2, 3, 4, 5)
  - [x] 5.1 Added `StringPhase` enum to `stack_view.rs`
  - [x] 5.2 Added `string_phases`, `string_values`, `string_errors` to `StackState`
  - [x] 5.3 Added public methods: `string_phase`, `start_string_loading`, `set_string_loaded`, `set_string_failed`
  - [x] 5.4 Updated `build_object_items` for `StringRef` display (Unloaded/Loaded/Failed)
  - [x] 5.5 `emit_object_children` skips ObjectRef recursion for StringRef (leaf node)
  - [x] 5.6 Added `selected_field_string_id()` to `StackState`
  - [x] 5.7 `collapse_object_recursive` clears string state for StringRef fields in descendants
  - [x] 5.8 Unit tests for all StringPhase state transitions and build_items display

- [x] Task 6 — App: async string loading (AC: 2, 3, 4)
  - [x] 6.1 Added `pending_strings: HashMap<u64, Receiver<Option<String>>>` to `App`
  - [x] 6.2 Added `start_string_loading` — spawns thread, registers receiver, no-op if already pending
  - [x] 6.3 Added `poll_strings` — drains completed receivers, emits warnings on `None`
  - [x] 6.4 Extended `Cmd` with `LoadString(u64)`; `OnObjectField` arm checks `selected_field_string_id()` first
  - [x] 6.5 `render()` calls `poll_strings()` alongside `poll_expansions()`
  - [x] 6.6 Unit tests: Loading state, Loaded after poll, no-op on Loading, Failed + warning on None

- [x] Task 7 — Full test suite green
  - [x] 7.1 `cargo test --workspace` — 337 tests, 0 failed
  - [x] 7.2 `cargo clippy --workspace -- -D warnings` — zero warnings
  - [x] 7.3 `cargo fmt --check` — clean

- [x] Task 8 — Code review fixes (Claude + Codex review, 2026-03-07)
  - [x] 8.1 [HIGH] AC3: `render()` now passes `warning_count + app_warnings.len()` to `StatusBar`
  - [x] 8.2 [HIGH] AC5: `CollapseObj`/`CollapseNestedObj` cancel in-flight `pending_strings` via
    new `StackState::string_ids_in_subtree()` before calling `collapse_object_recursive`
  - [x] 8.3 [MEDIUM] AC2: UTF-16 decoding uses `String::from_utf16_lossy` — surrogate pairs now correct
  - [x] 8.4 [MEDIUM] Failed `StringRef` fields styled with `theme::STATUS_WARNING` (yellow)
    instead of `theme::SEARCH_HINT` (dark gray)
  - [x] 8.5 [MEDIUM] `StringPhase::Loading` renders `String = "~"` instead of `"..."` for
    visual distinction from `Unloaded`
  - [x] 8.6 [MEDIUM] Test added: cursor movement while `StringRef` is Loading does not spawn
    a new load (`moving_cursor_while_string_ref_loading_does_not_start_new_load`)
  - [x] 8.7 Test added: `string_ids_in_subtree` collects StringRef IDs from descendants

## Dev Notes

### java.lang.String Internal Structure

Two layouts depending on JVM version:

**Java 8 (char[]):**
```
String instance fields:
  value: ObjectRef → char[] (PRIMITIVE_ARRAY_DUMP, elem_type=5)
  hash: int
```
Each char is a UTF-16 code unit stored big-endian as 2 bytes.

**Java 9+ compact strings:**
```
String instance fields:
  value: ObjectRef → byte[] (PRIMITIVE_ARRAY_DUMP, elem_type=8)
  coder: byte (0=LATIN1, 1=UTF16)
  hash: int
```
LATIN1: each byte is an ISO-8859-1 character (cast `u8 → char`).
UTF16: byte pairs are big-endian UTF-16 code units.

**Decoding rule for Task 3:**
- `resolve_string` does NOT read `coder` — it delegates to `decode_prim_array_as_string`
  which handles both byte types. For elem_type=8 (byte), assume LATIN1 (covers the most
  common case without needing `coder`). Edge case: UTF-16 encoded as byte[] will appear
  as garbled Latin-1, but this is acceptable for MVP — tracking `coder` adds complexity.

### `find_prim_array` Implementation Pattern

Mirror `find_instance` exactly, replacing the inner scan helper:

```rust
fn scan_for_prim_array(data: &[u8], target_id: u64, id_size: u32)
    -> Option<(u8, Vec<u8>)>
{
    let mut cursor = Cursor::new(data);
    loop {
        let sub_tag = cursor.read_u8().ok()?;
        if sub_tag == 0x23 {
            let arr_id = read_id(&mut cursor, id_size).ok()?;
            let _stack_serial = cursor.read_u32::<BigEndian>().ok()?;
            let num_elements = cursor.read_u32::<BigEndian>().ok()? as usize;
            let elem_type = cursor.read_u8().ok()?;
            let elem_size = value_byte_size(elem_type, id_size);
            if elem_size == 0 { return None; }
            let byte_count = num_elements.checked_mul(elem_size)?;
            let pos = cursor.position() as usize;
            if pos + byte_count > data.len() { return None; }
            if arr_id == target_id {
                return Some((elem_type, data[pos..pos + byte_count].to_vec()));
            }
            cursor.set_position((pos + byte_count) as u64);
        } else {
            if !skip_sub_record(&mut cursor, sub_tag, id_size) { return None; }
        }
    }
}
```

Note: `value_byte_size` is `pub(crate)` in `first_pass.rs` and already imported inside
`skip_sub_record` in `hprof_file.rs` via
`use crate::indexer::first_pass::{parse_class_dump, value_byte_size};`. Use the same import
in `scan_for_prim_array` — no new exports needed.

### StringRef in flat_items / build_items

StringRef fields are **terminal leaf nodes** — they do not have child cursors for a loading
node. The loading state is shown inline on the field row itself. This differs from ObjectRef
expansion which uses `OnObjectLoadingNode` as a separate cursor entry.

The `emit_object_children` helper in `stack_view.rs` must skip ObjectRef recursion for
StringRef: check `if let FieldValue::StringRef { .. } = field.value { /* emit as leaf */ }`.

### selected_field_ref_id vs selected_field_string_id

- `selected_field_ref_id()` (existing, Story 3.5): returns the ObjectRef id at cursor if
  phase is Collapsed/Failed. Must NOT return for StringRef fields.
- `selected_field_string_id()` (new, Task 5.6): returns the StringRef id at cursor if
  string phase is Unloaded/Failed.

To avoid returning an id for StringRef from `selected_field_ref_id`, add a guard:
```rust
if let FieldValue::ObjectRef { id, .. } = &field.value {  // explicit, not `..`
    return Some(*id);
}
```
This already excludes StringRef since the match is explicit.

### Async Pattern Consistency

The string loading pattern mirrors object expansion exactly:
- `pending_strings: HashMap<u64, Receiver<Option<String>>>` (parallel to `pending_expansions`)
- `start_string_loading` spawns a thread, inserts receiver
- `poll_strings` drains completed receivers

Because string resolution is typically fast (a single segment scan), the async overhead is
low. However, correctness and UI responsiveness (NFR4) require the async approach — a blocking
`resolve_string` call in the event loop would violate the 16ms frame budget on large files.

### Display Truncation

```rust
const MAX_STRING_DISPLAY: usize = 80;

fn truncate_string_display(s: &str) -> String {
    if s.chars().count() <= MAX_STRING_DISPLAY {
        s.to_string()
    } else {
        format!("{}..", &s[..s.char_indices().nth(MAX_STRING_DISPLAY).unwrap().0])
    }
}
```

### Collapse Cleanup for String State

When `collapse_object_recursive(root_id)` clears an object's state, it must also clear
string state for any StringRef fields in that object:

```rust
// After collecting all descendant IDs:
for &desc_id in &descendants {
    if let Some(fields) = self.object_fields.get(&desc_id) {
        for f in fields {
            if let FieldValue::StringRef { id } = f.value {
                self.string_phases.remove(&id);
                self.string_values.remove(&id);
                self.string_errors.remove(&id);
            }
        }
    }
}
// Then remove object state:
for &desc_id in &descendants {
    self.object_phases.remove(&desc_id);
    self.object_fields.remove(&desc_id);
    self.object_errors.remove(&desc_id);
}
```

### Module Structure — Files to Change

```
crates/hprof-parser/src/
└── hprof_file.rs          find_prim_array + scan_for_prim_array + tests

crates/hprof-engine/src/
├── engine.rs              FieldValue::StringRef variant + resolve_string trait method
├── engine_impl.rs         resolve_string impl + decode_prim_array_as_string + enrichment
└── resolver.rs            no change needed (enrichment handles StringRef downstream)

crates/hprof-tui/src/views/
└── stack_view.rs          StringPhase enum + StackState string maps + display + cleanup

crates/hprof-tui/src/
└── app.rs                 pending_strings + start_string_loading + poll_strings +
                           Cmd::LoadString + render integration
```

### Previous Story Intelligence (3.5)

Established patterns to continue:
- `StackState` mutations go through methods — no direct field access from `App`
- `flat_items()` / `build_items()` rebuild every call (no caching)
- Async pattern: `thread::spawn` + `mpsc::channel`, polled in `render()`
- `Cmd` local enum in `handle_stack_frames_input` to separate read from write
- `collapse_object` removes from all three maps; wrap cleanup in a method
- Theme constants from `theme.rs` — no inline colors
- `App<E: NavigationEngine>` generic — `StubEngine` for tests, add `resolve_string` stub
- `unwrap()`/`expect()` forbidden outside tests; use `?` in production code
- 309 tests pass after Story 3.5 baseline

### References

- [Source: docs/planning-artifacts/epics.md#Story 3.6]
- [Source: docs/planning-artifacts/ux-design-specification.md — String display format line 747]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture — NavigationEngine Trait]
- [Source: crates/hprof-engine/src/engine.rs — FieldValue enum, NavigationEngine trait]
- [Source: crates/hprof-engine/src/engine_impl.rs — expand_object enrichment pass, async pattern]
- [Source: crates/hprof-engine/src/resolver.rs — decode_fields, read_field_value]
- [Source: crates/hprof-parser/src/hprof_file.rs — find_instance, scan_for_instance,
  skip_sub_record pattern]
- [Source: crates/hprof-parser/src/indexer/first_pass.rs — 0x23 prim array indexed in seg
  filter, value_byte_size]
- [Source: crates/hprof-parser/src/test_utils.rs — add_prim_array(id, serial, n, type, data)]
- [Source: crates/hprof-tui/src/views/stack_view.rs — StringPhase analogous to ExpansionPhase,
  flat_items / build_items / collect_descendants / collapse_object_recursive]
- [Source: crates/hprof-tui/src/app.rs — pending_expansions / start_object_expansion /
  poll_expansions pattern to replicate for strings]
- [Source: docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md
  — field_path design, async pattern, Cmd enum, 309 baseline test count]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None — no blocking issues encountered.

### Completion Notes List

- Task 1: `FieldValue::StringRef { id: u64 }` added to engine.rs; `resolve_string` trait method added; DummyEngine stub returns None.
- Task 2: `find_prim_array` + `scan_for_prim_array` in hprof_file.rs mirrors find_instance pattern exactly. No new exports needed.
- Task 3: `decode_prim_array_as_string` handles elem_type 5 (UTF-16BE) and 8 (Latin-1); resolve_string walks instance fields to find "value" ObjectRef then calls find_prim_array.
- Task 4: Enrichment pass in `expand_object` now checks `class_name == "java.lang.String"` and replaces ObjectRef with StringRef, skipping collection entry count logic.
- Task 5: StringPhase enum + 3 new HashMaps in StackState; format_field_value updated with new signature accepting string_phase_info; build_object_items renders Unloaded as `"..."`, Loaded as `"value"` (truncated to 80 chars + `..`), Failed as `<unresolved>` in warning style. collapse_object_recursive clears string state.
- Task 6: App gains `pending_strings` + `app_warnings`; start_string_loading spawns thread; poll_strings drains receivers; Cmd::LoadString handled before field_ref_id check; poll_strings called in render().
- Task 7: 335 tests pass, clippy clean, fmt clean.

### File List

- crates/hprof-engine/src/engine.rs
- crates/hprof-engine/src/engine_impl.rs
- crates/hprof-parser/src/hprof_file.rs
- crates/hprof-tui/src/views/stack_view.rs
- crates/hprof-tui/src/app.rs
- docs/implementation-artifacts/3-6-lazy-value-string-loading.md
- docs/implementation-artifacts/sprint-status.yaml
- docs/code-review/claude-story-3.6-code-review.md
- docs/code-review/compiled-story-3.6-code-review.md
