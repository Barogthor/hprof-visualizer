# Story 3.7: Real Thread Names & Local Variable Resolution

Status: in-progress

## Story

As a user,
I want to see real thread names (not Thread-{serial} placeholders) and local variables on stack frames when viewing a heap dump,
so that the TUI displays accurate thread-level data comparable to VisualVM.

## Acceptance Criteria

1. **Given** a heap dump containing `ROOT_THREAD_OBJ` sub-records
   **When** the thread list is displayed
   **Then** each thread shows its real name extracted from the `java.lang.Thread` instance's `name` field in the heap (not `Thread-{serial}`)

2. **Given** a heap dump without `START_THREAD` records (e.g. jvisualvm)
   **When** `ROOT_THREAD_OBJ` sub-records exist
   **Then** the thread_serial-to-object_id mapping from `ROOT_THREAD_OBJ` is used to resolve thread names even when threads are synthetic

3. **Given** a `ROOT_THREAD_OBJ` whose thread object cannot be found in the heap (missing instance)
   **When** the thread name is resolved
   **Then** the fallback `Thread-{serial}` is used and no error is raised (NFR6)

4. **Given** a heap dump with `GC_ROOT_JAVA_FRAME` sub-records and no `START_THREAD` records
   **When** stack frames are expanded
   **Then** local variables are displayed for each frame (the frame root correlation bug is fixed)

5. **Given** a local variable that is a non-null object reference
   **When** displayed in the TUI
   **Then** it shows the resolved class name of the referenced object (e.g. `local variable: NativeReferenceQueue`) instead of raw `ObjectRef(0x...)`

6. **Given** a local variable whose object instance cannot be found in the heap
   **When** displayed
   **Then** it shows `local variable: Object` as fallback type

7. **Given** the two test dumps `heapdump-visualvm.hprof` (32 ROOT_THREAD_OBJ, 242 ROOT_JAVA_FRAME) and `heapdump-rustrover.hprof` (88 ROOT_THREAD_OBJ, 1123 ROOT_JAVA_FRAME)
   **When** loaded in the TUI
   **Then** threads have real names and stack frames show local variables (manual e2e validation)

## Tasks / Subtasks

- [x] Task 1 — Fix frame root correlation ordering bug (AC: 4)
  - [x] 1.1 In `first_pass.rs`, move the thread synthesis block (currently after the frame root correlation loop) to **before** the correlation loop. The synthesis uses `entry().or_insert()` so real `START_THREAD` entries are preserved.
  - [x] 1.2 Unit test: build an hprof with `STACK_TRACE` (thread_serial=1) + `GC_ROOT_JAVA_FRAME` (thread_serial=1, frame_number=0) but **no `START_THREAD`**. Assert `java_frame_roots` contains the root object ID for frame 0.
  - [x] 1.3 Verify existing tests pass — the reorder must not break dumps that DO have `START_THREAD`.

- [x] Task 2 — Parse `ROOT_THREAD_OBJ` sub-records (AC: 1, 2)
  - [x] 2.1 Add `thread_object_ids: HashMap<u32, u64>` to `PreciseIndex`. Key: thread_serial, Value: heap object_id from `ROOT_THREAD_OBJ`.
  - [x] 2.2 In `extract_heap_object_ids` (`first_pass.rs`), replace the `0x08 => skip_n(...)` arm with parsing: read `object_id` (id_size), `thread_serial` (u32), `stack_trace_serial` (u32). Insert `(thread_serial, object_id)` into a `raw_thread_objects` vec.
  - [x] 2.3 After the main loop (alongside frame root correlation), populate `index.thread_object_ids` from `raw_thread_objects`.
  - [x] 2.4 Also update `HprofThread.object_id` for synthetic threads when a matching `ROOT_THREAD_OBJ` entry exists (so `object_id` is no longer 0).
  - [x] 2.5 Add `add_root_thread_obj(object_id, thread_serial, stack_trace_serial)` to `HprofBuilder` in `test_utils.rs`.
  - [x] 2.6 Unit test: build hprof with `ROOT_THREAD_OBJ` sub-record, assert `thread_object_ids[thread_serial] == object_id`.
  - [x] 2.7 Unit test: synthetic thread gets its `object_id` updated from `ROOT_THREAD_OBJ`.

- [x] Task 3 — Resolve real thread names from heap instances (AC: 1, 2, 3)
  - [x] 3.1 In `engine_impl.rs`, update `thread_name()`: if `name_string_id == 0` (synthetic thread), look up `index.thread_object_ids[thread_serial]` → call `self.hfile.find_instance(object_id)` → find the `name` field (ObjectRef) → call `self.resolve_string(name_obj_id)` → return the real name.
  - [x] 3.2 Chain: `ROOT_THREAD_OBJ.object_id` → `find_instance` → `INSTANCE_DUMP` of `java.lang.Thread` → field `name` is an `ObjectRef` to a `java.lang.String` → `resolve_string` (already implemented in Story 3.6) → real name.
  - [x] 3.3 If any step fails (instance not found, field not found, string not resolved), fall back to `Thread-{serial}`.
  - [x] 3.4 Unit test: build hprof with `ROOT_THREAD_OBJ` + `CLASS_DUMP` for Thread class + `INSTANCE_DUMP` with name field pointing to a String + backing char array. Assert `list_threads()` returns the real thread name.
  - [x] 3.5 Unit test: `ROOT_THREAD_OBJ` present but instance not found → fallback to `Thread-{serial}`.

- [x] Task 4 — Resolve class name on local variables (AC: 5, 6)
  - [x] 4.1 In `engine_impl.rs`, update `get_local_variables()`: for each non-null `object_id`, call `self.hfile.find_instance(object_id)` → look up `class_names_by_id[class_object_id]` → set as display name.
  - [x] 4.2 Update `VariableInfo` in `engine.rs`: add `class_name: Option<String>` field. `None` = unresolvable, `Some(name)` = resolved class.
  - [x] 4.3 Update `VariableValue::ObjectRef` to `ObjectRef { id: u64, class_name: String }` (with `"Object"` as fallback).
  - [x] 4.4 Update all match arms on `VariableValue` in engine tests and TUI code (compiler-guided).
  - [x] 4.5 Update TUI display in `stack_view.rs`: show `local variable: {class_name}` instead of `local_{index}: ObjectRef(0x...)`.
  - [x] 4.6 Unit test: `get_local_variables` with instance in heap returns correct class name.
  - [x] 4.7 Unit test: `get_local_variables` with unknown instance ID returns `"Object"` fallback.

- [ ] Task 5 — Full test suite green + e2e validation (AC: 7)
  - [x] 5.1 `cargo test --workspace` — all tests pass
  - [x] 5.2 `cargo clippy --workspace -- -D warnings` — zero warnings
  - [x] 5.3 `cargo fmt --check` — clean
  - [ ] 5.4 Manual e2e: load `heapdump-visualvm.hprof`, verify real thread names and local variables visible
  - [ ] 5.5 Manual e2e: load `heapdump-rustrover.hprof`, verify real thread names and local variables visible

### Review Follow-ups (AI)

- [ ] [AI-Review][High] Complete AC7 manual e2e validation for `heapdump-visualvm.hprof` and
      `heapdump-rustrover.hprof`, then record observed thread names/local variables evidence.
- [ ] [AI-Review][Medium] Reconcile Task 5 completion metadata (`[x]` parent task,
      completion notes, and changelog wording) with pending manual e2e subtasks.
- [ ] [AI-Review][Medium] Add explicit warning emission when `ROOT_THREAD_OBJ` parsing is
      truncated in `crates/hprof-parser/src/indexer/first_pass.rs` (parity with
      `GC_ROOT_JAVA_FRAME` warning behavior).
- [ ] [AI-Review][Low] Harden `resolve_thread_name_from_heap()` with class/shape guards to avoid
      mis-resolving names on malformed heap objects.
- [ ] [AI-Review][Low] Align stack expansion indicators with ASCII-only UX conventions (`>`/`v`)
      if the project enforces `docs/planning-artifacts/ux-design-specification.md` strictly.

## Dev Notes

### Bug: Frame Root Correlation Ordering

In `first_pass.rs`, the current execution order is:
1. Parse sub-records → collect `raw_frame_roots` (242 entries for visualvm dump)
2. Correlate `raw_frame_roots` with `threads` map → but `threads` is **empty** (no `START_THREAD`)
3. Synthesise threads from `STACK_TRACE` → too late, roots already dropped

**Fix:** Swap steps 2 and 3. Move the thread synthesis block (lines ~432-455) before the correlation loop (lines ~408-430). Safe because `entry().or_insert()` preserves real `START_THREAD` entries.

### ROOT_THREAD_OBJ Sub-Record Format (sub-tag 0x08)

```
thread_object_id: id_size bytes
thread_serial: u32
stack_trace_serial: u32
```

Total: `id_size + 8` bytes. Currently skipped at `first_pass.rs:630`:
```rust
0x08 => skip_n(&mut cursor, id_size as usize + 8),
```

Note: the current skip reads `id_size + 8` but the actual format is `id_size + 4 + 4`. The skip size is correct (same total) but we now need to parse the individual fields.

### Thread Name Resolution Chain

```
ROOT_THREAD_OBJ.object_id
  → find_instance(object_id)     // existing method
  → INSTANCE_DUMP { class_object_id, fields_bytes }
  → class_names_by_id[class_object_id] == "java.lang.Thread"
  → decode_fields(raw, index, id_size)
  → find field named "name" (type ObjectRef)
  → resolve_string(name_field.id)  // Story 3.6 method
  → real thread name string
```

`find_instance` and `resolve_string` are already implemented. The new code just chains them.

### VariableInfo Class Name Resolution

Currently `get_local_variables` returns bare `ObjectRef(id)`. To resolve the class name:

```rust
// In get_local_variables, for each non-null object_id:
let class_name = self.hfile.find_instance(object_id)
    .and_then(|raw| self.hfile.index.class_names_by_id
        .get(&raw.class_object_id)
        .cloned())
    .unwrap_or_else(|| "Object".to_string());
```

This reuses existing `find_instance` which already does segment-filter narrowing.

### TUI Display Changes

Current display: `local_0: ObjectRef(0xD0184978)`
Target display: `local variable: NativeReferenceQueue`

In `stack_view.rs`, the `VariableValue::ObjectRef` match arm should render:
```
"local variable: {class_name}"
```

The `index` field on `VariableInfo` is still useful for ordering but not for display.

### HprofBuilder: add_root_thread_obj

Mirror `add_gc_root_java_frame` pattern in `test_utils.rs`:

```rust
pub fn add_root_thread_obj(
    &mut self,
    object_id: u64,
    thread_serial: u32,
    stack_trace_serial: u32,
) -> &mut Self {
    let mut sub = vec![0x08u8]; // ROOT_THREAD_OBJ sub-tag
    write_id(&mut sub, object_id, self.id_size);
    sub.write_u32::<BigEndian>(thread_serial).unwrap();
    sub.write_u32::<BigEndian>(stack_trace_serial).unwrap();
    self.wrap_in_heap_dump_segment(sub);
    self
}
```

### Project Structure Notes

Files to change:
```
crates/hprof-parser/src/
├── indexer/
│   ├── first_pass.rs     Task 1 (reorder) + Task 2 (parse 0x08)
│   └── precise.rs        Task 2 (thread_object_ids field)
├── test_utils.rs          Task 2 (add_root_thread_obj builder)
└── types.rs               No change (HprofThread already has object_id)

crates/hprof-engine/src/
├── engine.rs              Task 4 (VariableValue::ObjectRef class_name)
└── engine_impl.rs         Task 3 (thread_name) + Task 4 (get_local_variables)

crates/hprof-tui/src/
├── views/stack_view.rs    Task 4 (display class name)
└── app.rs                 Task 4 (update VariableValue match arms)
```

### Previous Story Intelligence (3.6)

Patterns to follow:
- `StackState` mutations through methods only — no direct field access from `App`
- `flat_items()` / `build_items()` rebuilt every call
- Async pattern: `thread::spawn` + `mpsc::channel`, polled in `render()`
- `Cmd` local enum in `handle_stack_frames_input`
- Theme constants from `theme.rs` — no inline colors
- `App<E: NavigationEngine>` generic — `StubEngine` for tests
- `unwrap()`/`expect()` forbidden outside tests; use `?` or fallback
- 337 tests pass after Story 3.6 baseline

### References

- [Source: docs/report/party-mode-story-3.7-discovery.md — diagnostic scan results + root cause analysis]
- [Source: docs/planning-artifacts/epics.md#Story 3.3 — AC for local variables (FR12)]
- [Source: docs/planning-artifacts/epics.md#Story 3.2 — AC for thread names (FR9)]
- [Source: crates/hprof-parser/src/indexer/first_pass.rs:408-455 — frame root correlation + thread synthesis]
- [Source: crates/hprof-parser/src/indexer/first_pass.rs:630 — 0x08 skip_n to replace]
- [Source: crates/hprof-parser/src/indexer/precise.rs — PreciseIndex struct]
- [Source: crates/hprof-engine/src/engine.rs — VariableInfo, VariableValue, ThreadInfo]
- [Source: crates/hprof-engine/src/engine_impl.rs:218-225 — thread_name() fallback]
- [Source: crates/hprof-engine/src/engine_impl.rs:306-322 — get_local_variables()]
- [Source: crates/hprof-engine/src/engine_impl.rs:367 — resolve_string() from Story 3.6]
- [Source: crates/hprof-parser/src/test_utils.rs:264 — add_gc_root_java_frame pattern to mirror]
- [Source: assets/screenshot-01.png — VisualVM reference showing "local variable: Type#N" format]

## Dev Agent Record

### Agent Model Used
Claude Opus 4.6

### Debug Log References
None

### Completion Notes List
- Task 1: Swapped thread synthesis block before frame root correlation loop in `first_pass.rs`. Added test `synthetic_thread_enables_frame_root_correlation`.
- Task 2: Parsed ROOT_THREAD_OBJ (0x08) sub-records instead of skipping. Added `thread_object_ids` to `PreciseIndex`, `raw_thread_objects` collection in `extract_heap_object_ids`, post-loop population of `thread_object_ids` and synthetic thread `object_id` update. Added `add_root_thread_obj` to `HprofTestBuilder`. Tests: `root_thread_obj_populates_thread_object_ids`, `root_thread_obj_updates_synthetic_thread_object_id`.
- Task 3: Added `resolve_thread_name_from_heap()` method implementing the chain: `thread_object_ids[serial]` → `find_instance` → decode fields → find "name" ObjectRef → `resolve_string`. Updated `thread_name()` to try heap resolution before falling back to `Thread-{serial}`. Tests: `list_threads_resolves_real_name_via_root_thread_obj`, `list_threads_falls_back_when_instance_not_found`.
- Task 4: Changed `VariableValue::ObjectRef(u64)` to `ObjectRef { id: u64, class_name: String }`. Updated `get_local_variables()` to resolve class name via `find_instance` + `class_names_by_id` with "Object" fallback. Updated all match arms in engine.rs, engine_impl.rs, app.rs, stack_view.rs (compiler-guided). Updated TUI display to show `local variable: {class_name}` format. Tests: `get_local_variables_resolves_class_name`, `get_local_variables_unknown_instance_falls_back_to_object`.
- Task 5: 344 tests pass (baseline 337, +7 new). Clippy zero warnings. Fmt clean. Manual e2e pending user validation.

### Change Log
- 2026-03-08: Story 3.7 implementation — tasks 1-4 complete, task 5 partial (5.1-5.3 done, 5.4-5.5 manual e2e pending).
- 2026-03-08: Codex code review completed — changes requested; story moved to in-progress and AI follow-ups added.

### File List
- `crates/hprof-parser/src/indexer/first_pass.rs` — reordered thread synthesis before frame root correlation; parsed ROOT_THREAD_OBJ; populated thread_object_ids; added 3 tests
- `crates/hprof-parser/src/indexer/precise.rs` — added `thread_object_ids` field to PreciseIndex
- `crates/hprof-parser/src/test_utils.rs` — added `add_root_thread_obj` builder method
- `crates/hprof-engine/src/engine.rs` — changed `VariableValue::ObjectRef` from tuple to struct variant with `class_name`; updated tests
- `crates/hprof-engine/src/engine_impl.rs` — added `resolve_thread_name_from_heap`; updated `thread_name()` and `get_local_variables()`; added 4 tests
- `crates/hprof-tui/src/views/stack_view.rs` — updated all `VariableValue::ObjectRef` match arms; updated display to show `local variable: {class_name}`
- `crates/hprof-tui/src/app.rs` — updated `make_obj_var` test helper for new ObjectRef struct
- `docs/implementation-artifacts/sprint-status.yaml` — status: review → in-progress (code review outcome)
- `docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md` — task checkboxes, dev record
- `docs/code-review/codex-story-3.7-code-review.md` — Story 3.7 code review report (changes requested)

## Senior Developer Review (AI)

### Review Date

2026-03-08

### Reviewer

Codex

### Outcome

Changes Requested.

### Notes

- AC1-AC6 are implemented and covered by automated tests.
- AC7 is still pending manual e2e validation on both required dumps.
- Validation run passed: `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`,
  and `cargo fmt --check`.
