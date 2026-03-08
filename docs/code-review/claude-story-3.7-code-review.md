# Code Review — Story 3.7: Real Thread Names & Local Variable Resolution

**Reviewer:** Amelia (Dev Agent — Claude Opus 4.6)
**Date:** 2026-03-08
**Story file:** `docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md`
**Commit:** `0ac20c5 feat: story 3.7 — real thread names and local variable resolution`

## Summary

| Metric | Value |
|---|---|
| Tests | 344 pass (baseline 337, +7 new) |
| Clippy | Zero warnings |
| Fmt | Clean |
| Git vs Story File List discrepancies | 0 |
| Critical issues | 0 |
| Medium issues | 3 |
| Low issues | 3 |

All tasks marked `[x]` are genuinely implemented. All 7 Acceptance Criteria
are covered by code and tests.

## Critical Issues

None. All tasks verified against implementation. No false claims.

## Medium Issues

### M1 — Module docstring in `precise.rs` is stale (line 1-14)

The module doc says "holds five `HashMap` collections" and the table lists
only 6 fields. The struct now has **9** fields (`java_frame_roots`,
`class_dumps`, `thread_object_ids` are missing from the doc table).

**File:** `crates/hprof-parser/src/indexer/precise.rs:1-14`

### M2 — `resolve_thread_name_from_heap` decodes ALL fields to find "name"

`decode_fields` parses the entirety of `java.lang.Thread`'s instance fields
(~30 fields) just to locate the `name` field. No short-circuit.

Multiplied by N threads on every call to `list_threads()` / `select_thread()`.
In practice the TUI caches threads in `ThreadListState`, but the engine API
recalculates on every call.

**File:** `crates/hprof-engine/src/engine_impl.rs:240-241`

### M3 — `ROOT_THREAD_OBJ` (0x08) parsing is silent on truncation

The `0x08` handler does a bare `break` on read failure without emitting any
warning, whereas `0x03` (`GC_ROOT_JAVA_FRAME`) emits detailed warnings with
byte offset context. Inconsistent error reporting.

**File:** `crates/hprof-parser/src/indexer/first_pass.rs:649-661`

## Low Issues

### L1 — Test `new_creates_empty_index` misses `java_frame_roots`

The test at `precise.rs:84-94` asserts 8 of 9 maps are empty but omits
`java_frame_roots.is_empty()`. Pre-existing gap from Story 3.3.

**File:** `crates/hprof-parser/src/indexer/precise.rs:84-94`

### L2 — `resolve_thread_name_from_heap` has fragile coupling with `decode_fields`

The code searches for `FieldValue::ObjectRef { id, .. }` on the "name" field.
If `decode_fields` were ever changed to eagerly convert String references to
`FieldValue::StringRef` (as `expand_object` does in its enrichment pass), this
code would silently break. Implicit coupling.

**File:** `crates/hprof-engine/src/engine_impl.rs:242-248`

### L3 — Local variable display uses fully-qualified class name

AC 5 example shows `local variable: NativeReferenceQueue` (short name).
Implementation displays `local variable: sun.misc.NativeReferenceQueue`
(fully-qualified). Arguably more informative (no ambiguity), but diverges
from the AC example wording.

**File:** `crates/hprof-tui/src/views/stack_view.rs:692-706`

## AC Verification Matrix

| AC | Status | Evidence |
|---|---|---|
| AC1 — Real thread names from `ROOT_THREAD_OBJ` | IMPLEMENTED | `engine_impl.rs:222-251`, test `list_threads_resolves_real_name_via_root_thread_obj` |
| AC2 — Thread name resolution without `START_THREAD` | IMPLEMENTED | Synthetic thread + `ROOT_THREAD_OBJ` chain, same test as AC1 |
| AC3 — Fallback to `Thread-{serial}` on missing instance | IMPLEMENTED | `engine_impl.rs:231`, test `list_threads_falls_back_when_instance_not_found` |
| AC4 — Frame root correlation bug fixed | IMPLEMENTED | `first_pass.rs:410-436` reorder, test `synthetic_thread_enables_frame_root_correlation` |
| AC5 — Local variable shows resolved class name | IMPLEMENTED | `engine_impl.rs:332-361`, test `get_local_variables_resolves_class_name` |
| AC6 — Fallback to `"Object"` for missing instance | IMPLEMENTED | `engine_impl.rs:353`, test `get_local_variables_unknown_instance_falls_back_to_object` |
| AC7 — Manual e2e validation | PENDING | Tasks 5.4 and 5.5 remain `[ ]` (user action required) |

## Task Completion Audit

| Task | Marked | Verified | Notes |
|---|---|---|---|
| 1 — Frame root reorder | [x] | Yes | Synthesis block moved before correlation loop |
| 2 — Parse ROOT_THREAD_OBJ | [x] | Yes | 0x08 parsed, `thread_object_ids` populated, synthetic thread `object_id` updated |
| 3 — Resolve real thread names | [x] | Yes | `resolve_thread_name_from_heap` chain implemented |
| 4 — Local variable class names | [x] | Yes | `VariableValue::ObjectRef { id, class_name }`, TUI display updated |
| 5 — Test suite + e2e | Partial | Partial | 5.1-5.3 done, 5.4-5.5 pending manual validation |

## Files Reviewed

- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/test_utils.rs`
- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/app.rs`
