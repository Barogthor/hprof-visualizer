# Code Review: Cyclic Reference Detection in Stack View

**Date:** 2026-03-09
**Reviewer:** Claude (adversarial review)
**Scope:** Diff adding `OnCyclicNode` variant and cycle detection to object expansion

## Summary

The diff adds ancestor-chain cycle detection to both the cursor-list builder (`emit_object_children`) and the rendering function (`build_object_items`). A new `StackCursor::OnCyclicNode` variant marks non-expandable leaf nodes for cyclic references. The approach is sound and the test coverage is good. Several issues found, mostly low severity.

## Findings

| ID | Severity | Validity | Description |
|----|----------|----------|-------------|
| 1 | Low | real | **Dead `_ =>` arm in `app.rs` match.** After adding `StackCursor::OnCyclicNode { .. } \| StackCursor::OnObjectLoadingNode { .. } => return None`, the only remaining variant reaching the `_ =>` wildcard is `StackCursor::NoFrames`. The wildcard is now dead code for all practical purposes. It silently swallows `NoFrames` instead of handling it explicitly, which masks future variant additions that might need real handling. Should be replaced with `StackCursor::NoFrames => return None`. |
| 2 | Medium | real | **Diamond / shared-object false positive.** The `visited` set uses ancestor-chain semantics (insert on entry, remove on backtrack). However, if two sibling fields reference the same object (e.g., `left` and `right` both point to object 500, which is NOT an ancestor), the second sibling will see 500 in `visited` because the first sibling's recursion inserted it and the `visited.remove` only fires after the `Expanded` block ends. This means legitimate shared (non-cyclic) references are incorrectly flagged as cyclic. Example: object A has fields `left -> B(200)` and `right -> B(200)`. When processing `left`, B is inserted into `visited`. B's children are processed, B is removed. Then `right` is processed -- B is NOT in visited, so this specific case works. BUT: if A has fields `left -> B(200)` where B has field `ref -> C(300)`, and A also has field `other -> C(300)`, then C(300) gets inserted during B's traversal, removed when B's branch finishes, so `other -> C(300)` is fine. On closer analysis, the insert/remove pairing IS correct for DFS ancestor tracking. **Downgrading: the ancestor-chain approach is actually correct.** |
| 3 | Low | real | **Unnecessary `.clone()` on `parent_path` in empty-fields branch (line 557).** In `emit_object_children`, the `Expanded` branch with `field_count == 0` now does `field_path: parent_path.clone()` instead of moving `parent_path`. Before this diff, `parent_path` was moved into the `OnObjectLoadingNode`. Now there is `visited.remove(&object_id)` after this block, but `object_id` is `Copy` so `parent_path` could still be moved. The clone is unnecessary because the `if field_count == 0` branch returns early from the `Expanded` block (no further use of `parent_path`). Minor allocation waste. |
| 4 | Low | real | **Duplicated cycle-detection logic.** `emit_object_children` and `build_object_items` implement the same visited-set logic independently. If one is updated (e.g., to handle a new `FieldValue` variant) and the other is not, they will diverge, causing the cursor list and the rendered list to be out of sync. This is an existing pattern in the codebase (both methods already duplicated expansion logic), but the diff deepens it. Consider extracting a shared traversal iterator. |
| 5 | Low | noise | **`self-ref` label is cosmetic only, not a separate code path.** The `self-ref` vs `cyclic` distinction in `build_object_items` is purely a display label. The cursor type is the same `OnCyclicNode` for both. This is fine but worth noting: if product requirements later need different behavior for self-refs vs indirect cycles, the cursor type does not distinguish them. |
| 6 | Low | real | **No test for diamond/shared references.** Tests cover self-ref, indirect A->B->A cycle, acyclic chain, and navigation. But there is no test for the diamond pattern: A has fields pointing to B and C, both of which point to D. D should NOT be marked cyclic (it is shared but not an ancestor). The ancestor-chain logic handles this correctly, but a regression test would catch future breakage. |
| 7 | Low | real | **`HashSet` allocated per variable per frame per render.** In `build_items` and `flat_items`, a new `HashSet` is created for every `ObjectRef` variable. For frames with many variables, this is many small allocations. In practice the sets are tiny (bounded by depth 16), so this is negligible. Noting for completeness. |
| 8 | Low | real | **Trailing comma in format strings.** Lines 847-848 have trailing commas inside `format!` macro args: `format!("... [{}]", short, id, label,)` and `format!("...{}", field.name, marker,)`. These compile fine (Rust allows trailing commas in macro args) but the extra comma after the last positional arg is a style inconsistency. |

## Verdict

The implementation is correct and well-tested. The ancestor-chain visited set with insert/remove is the right approach for DFS cycle detection (I initially suspected a diamond false-positive but the backtracking removes entries properly). The main actionable items are:

1. **ID 1**: Replace `_ =>` with explicit `StackCursor::NoFrames =>` to catch future variants at compile time.
2. **ID 6**: Add a diamond-pattern test to guard the ancestor-chain invariant.
3. **ID 3**: Remove the unnecessary `.clone()`.

No critical or high-severity issues found.
