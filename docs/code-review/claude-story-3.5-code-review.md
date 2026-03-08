# Code Review — Story 3.5: Recursive Expansion & Collection Size Indicators

**Reviewer:** Claude (Amelia, Dev Agent)
**Date:** 2026-03-07
**Branch:** feature/epic-3-thread-centric-navigation
**Story file:** `docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md`

---

## Summary

- **Git vs Story File List:** ✅ No discrepancies — all 7 source files modified match the story File List exactly.
- **Test suite:** ✅ 304/304 pass (baseline 289 + 15 new, as claimed).
- **Issues found:** 1 High, 3 Medium, 2 Low.

---

## AC Validation

| AC | Status | Evidence |
|----|--------|----------|
| AC1 — Recursive expansion via Enter on nested ObjectRef | ✅ IMPLEMENTED | `OnObjectField` + `StartNestedObj` cmd → `start_object_expansion` |
| AC2 — Collection size indicator before expansion | ✅ IMPLEMENTED | `collection_entry_count` + `FieldValue::ObjectRef { entry_count }` + `format_object_ref_collapsed` |
| AC3 — Correct deeper indentation | ✅ IMPLEMENTED | `2 + 2 * (parent_path.len() + 1)` in `build_object_items` |
| AC4 — Toggle collapse nested field | ✅ IMPLEMENTED | `CollapseNestedObj(id)` → `collapse_object_recursive` |
| AC5 — Recursive cleanup on root collapse | ✅ IMPLEMENTED | `collect_descendants` + `collapse_object_recursive` |
| AC6 — Error node at correct depth | ✅ IMPLEMENTED | `ExpansionPhase::Failed` arm in `build_object_items` emits `{indent}! {msg}` |

---

## 🔴 HIGH Issues

### H1 — Test `build_items_depth1_field_has_4_space_indent` doesn't verify indentation content

**File:** `crates/hprof-tui/src/views/stack_view.rs:1189`

Task 5.5 claims: *"build_items produces correct indentation at depth 1 and depth 2"*. The test only asserts `items.len() == 3` — it never inspects the text content of `items[2]` to confirm the 4-space prefix. No depth-2 `build_items` indentation test exists at all. The core correctness claim for AC 5.3 (indentation formula `2 + 2 * depth`) is unverified.

**Fix:** Extract `items[2]` text via `ListItem` span content and assert it starts with `"    "` (4 spaces). Add a depth-2 variant asserting 6-space indent.

---

## 🟡 MEDIUM Issues

### M1 — Negative `size` field cast to `u64` gives absurd entry count

**File:** `crates/hprof-engine/src/engine_impl.rs:94` (Int) and `:103` (Long)

```rust
return Some(v as u64); // v: i32, wraps on negative
```

If a collection's `size` field is `-1` (uninitialized or corrupted heap), the display becomes:
`HashMap (18446744073709551615 entries) [expand →]`

**Fix:**
```rust
if v >= 0 { return Some(v as u64); }
```

### M2 — AC4 collapse of a *nested* expanded field not tested in `app.rs`

**File:** `crates/hprof-tui/src/app.rs`

Task 6.4 claims: *"Enter on a nested ObjectRef field starts expansion; Enter again collapses it"*. The existing test `enter_on_nested_object_field_starts_expansion` only validates start. There is no test that, after the nested object is expanded, presses Enter on the field again and asserts `expansion_state(999) == Collapsed`.

### M3 — AC6 failure node format and depth not tested

**File:** `crates/hprof-tui/src/views/stack_view.rs`

No test verifies that `build_object_items` for `ExpansionPhase::Failed` emits a row starting with the correct indentation followed by `! Failed to resolve object`. The failure path is exercised by `set_expansion_failed_changes_phase_to_failed` but the rendered output is never inspected.

---

## 🟢 LOW Issues

### L1 — Docstring for `selected_field_ref_id` implies phase checking inside the function

**File:** `crates/hprof-tui/src/views/stack_view.rs:204`

The doc says *"field is an ObjectRef in Collapsed or Failed phase or Expanded phase (for collapse)"* — but the function does not check phase at all; `App` does. This is misleading for future maintainers.

**Fix:** Remove the phase mention from the docstring; document that the caller is responsible for phase checks.

### L2 — `diag.txt` untracked debug artifact not in File List

`diag.txt` appears in `git status --porcelain` as untracked (`??`) but is not mentioned in the story File List or Change Log. Should be deleted or added to `.gitignore`.

---

## Task Completion Audit

All 10 tasks marked `[x]`. Evidence found for each:

| Task | Evidence |
|------|----------|
| 1 — `class_names_by_id` in `PreciseIndex` | `precise.rs:49`, `first_pass.rs:248` |
| 2 — `FieldValue::ObjectRef` struct variant | `engine.rs:112-116` |
| 3 — `collection_entry_count` | `engine_impl.rs:46-118` |
| 4 — `field_path`-based `StackCursor` | `stack_view.rs:43-55` |
| 5 — Recursive `emit_object_children` / `build_object_items` | `stack_view.rs:390-443`, `583-685` |
| 6 — `StartNestedObj` / `CollapseNestedObj` cmds | `app.rs:227-288` |
| 7 — `collapse_object_recursive` | `stack_view.rs:277-284` |
| 8 — `toggle_expand` calls `collapse_object_recursive` | `stack_view.rs:306-308` |
| 9 — Root-var display deferred to 3.6 (per story note) | `stack_view.rs:540-550` |
| 10 — Test suite green | 304/304 pass |

---

## Verdict

Story is **close to done** but two test gaps (H1, M2, M3) prevent a clean sign-off. No functional regressions found. The `collection_entry_count` sign bug (M1) is a latent defect with corrupted/uninitialized heaps.

**Recommended status:** `in-progress` (address H1 + M1 + M2 minimum before `done`).
