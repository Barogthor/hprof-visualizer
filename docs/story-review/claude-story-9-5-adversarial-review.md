# Adversarial Review — Story 9.5: Stack Frame Variable Names & Static Fields

**Date:** 2026-03-12
**Reviewer:** Claude (adversarial mode)
**Focus:** Validity of code/API proposals against current codebase state

---

## Critical Issues (Blockers)

### 1. `FlatItem` enum does not exist — Task 5.3, 5.4, 5.5 are architecturally wrong

Tasks 5.3, 5.4, and 5.5 propose adding `FlatItem::StaticSection` and
`FlatItem::StaticOverflow(usize)` variants, and instruct the developer to audit all `match
FlatItem` sites. There is **no `FlatItem` enum anywhere in the codebase**.

The TUI uses a `StackCursor`-based architecture. `flat_items()` returns `Vec<StackCursor>`,
where each element represents a navigable cursor position. There is no flat-item rendering
model. The concept of "non-navigable variants" that are skipped by `cursor_up`/`cursor_down`
is a design fiction — `flat_items()` only contains navigable positions by definition.

The developer following Task 5.3 verbatim will waste time searching for a type that does not
exist and will have no valid anchor for the implementation. A correct approach must work within
the `StackCursor` + rendering pipeline architecture.

### 2. Task 5.2 pseudocode is architecturally incorrect

Task 5.2 shows:
```rust
let Some(instance_fields) = self.engine.expand_object(oid) else { return; };
```
and presents it as code to place after a `Cmd::ExpandObject` dispatch.

Three problems:
- **`Cmd::ExpandObject` does not exist.** Object expansion is triggered via
  `RightCmd::StartObj(oid)` → `start_object_expansion(oid)`.
- **`expand_object` is called in a background thread**, not synchronously. The method
  `start_object_expansion` spawns a worker thread and stores a `PendingExpansion { rx, ... }`.
  Results are polled during the tick/update phase, not immediately after the command dispatch.
- **Static field injection belongs in the polling phase**, not the command dispatch phase.
  The pseudocode will never work as written because the fields are not available synchronously.

### 3. `cursor_up` / `cursor_down` references do not match the codebase

Task 5.4 says: "Ensure `cursor_up` / `cursor_down` in `StackState` skip both variants."
The actual navigation API is `CursorNavigator::move_up` and `move_down` operating on
`&[StackCursor]`. There are no methods named `cursor_up` or `cursor_down` in `StackState`.
The framing assumes a skip-list filtering model that doesn't exist.

---

## Serious Issues (Likely Regressions)

### 4. Wrong file path for `engine_impl.rs` — referenced in Tasks 3.2, 4.3, 4.4, and references section

The story repeatedly references `crates/hprof-engine/src/engine_impl.rs`. This file does not
exist. The module was refactored into `crates/hprof-engine/src/engine_impl/mod.rs` (and
`tests.rs`). The line reference `engine_impl.rs:736-786` for `get_local_variables` is doubly
wrong: wrong path and wrong line (actual location: `engine_impl/mod.rs:681`).

### 5. Wrong file path for `stack_view.rs` — referenced in Tasks 5.3, 5.4, 5.5, 6.9, 6.12

The story references `crates/hprof-tui/src/views/stack_view.rs`. This file does not exist.
`stack_view` is a module directory with sub-files: `mod.rs`, `types.rs`, `state.rs`,
`expansion.rs`, `format.rs`, `widget.rs`, `tests.rs`. A developer searching for
`stack_view.rs` will not find it.

### 6. Variable label line references are wrong

The references section says "Variable label render: `tree_render.rs:130,134`". Actual lines
are 141 and 152. Small error, but in a code-dense file this sends the developer to the wrong
location.

---

## Moderate Issues (Tech Debt / Correctness Risks)

### 7. Slot-mapping heuristic is self-acknowledged as wrong, but the story still proposes it

Task 3.2 proposes `frame_variable_names.get(&(frame_id, i as u32))` where `i` is the
enumerate index of the GC_ROOT_JAVA_FRAME list. The Dev Notes section then explicitly warns
that this mapping "is wrong in the general case" because `long`/`double` occupy 2 slots and
constructor params shift accordingly.

The story thus proposes code it knows will produce incorrect results most of the time, and
paper-covers it with "`<unnamed>` is acceptable." However, this means the feature branded as
"AC1 — Variable name resolution" is intentionally broken for most JVMs. The AC1 label creates
false confidence. It should be labelled clearly as "best-effort heuristic, not spec-compliant."

### 8. `VariableInfo` docstring is not updated to mention the `name` field

Task 3.1 adds `pub name: Option<String>` to `VariableInfo`. The existing docstring
(engine.rs:97-100) says "Variables are numbered by their 0-based position in the root list"
with no mention of variable names. Per CLAUDE.md, every public field change on a documented
struct should update the docstring. The story does not mention updating it.

### 9. `ClassDumpInfo::memory_size` update has a precision issue

Task 1.4 says to add `self.static_fields.capacity() * std::mem::size_of::<StaticFieldDef>()`.
`StaticValue` contains a `f64` and a `char`, which could have enum discriminant padding.
`size_of::<StaticFieldDef>()` on its own is correct, but the story neglects to document this
dependency on the enum's memory layout. If `StaticValue` is later modified, `memory_size`
silently drifts. A note or a compile-time assertion would be appropriate.

### 10. Guard condition for unknown static field type is incomplete

Task 1.3 says: guard with `if value_byte_size(field_type, id_size) == 0 && field_type != 0`.
Looking at the existing constant pool skip loop (hprof_primitives.rs:125-133), it uses
`value_byte_size` without such a guard and simply calls `skip_n(cursor, 0)` — which is a
no-op and leaves `field_type` unread as the next byte. For static fields, the proposed guard
returns `None` for the entire CLASS_DUMP on an unknown type. This is more conservative, but
the guard expression is subtly wrong: `field_type != 0` is always true when
`value_byte_size == 0` for truly unknown types (type 0 is not a valid HPROF type code).
The condition should simply be `if value_byte_size(field_type, id_size) == 0`.

### 11. Test 6.14 does not specify which crate it lives in

Test 6.14 (`local_variable_names_wired_end_to_end`) is described as an "integration test"
but no target file is specified. The existing integration tests for the parser use
`test_utils.rs`/`HprofTestBuilder` within the `hprof-parser` crate. For this test to call
`engine.get_local_variables`, it also needs the `hprof-engine` crate — making it a cross-crate
integration test. The story should specify whether this test lives in `hprof-engine`'s
`engine_impl/tests.rs` or as a separate integration test crate, and clarify the dependency path.

### 12. Task 5.5 "no `_ =>` wildcards" audit scope is ambiguous

Task 5.5 says "Audit all `match` on `FlatItem` in the TUI crate." Given that `FlatItem` does
not exist (see finding #1), this task is vacuous as written. If the intent is to audit all
`match StackCursor` sites (the actual type in scope), that is a far larger change that will
touch many files and tests. The scope must be clarified before implementation begins.

---

## Summary

| Severity | Count | Description |
|----------|-------|-------------|
| Critical (blocker) | 3 | FlatItem nonexistent, Task 5.2 wrong model, cursor skip API wrong |
| Serious | 3 | Two wrong file paths (engine_impl, stack_view), wrong line refs |
| Moderate | 6 | Slot heuristic label, docstring gap, memory_size precision, guard condition, test location, wildcard audit scope |

The TUI-layer tasks (5.x, 6.9, 6.12) are **not implementable as written** without resolving
finding #1 first. All other tasks (parser, engine) are sound and can proceed immediately.
**Recommend halting Task 5 until the rendering architecture for static fields is redesigned
in terms of `StackCursor` and the stack_view module structure.**
