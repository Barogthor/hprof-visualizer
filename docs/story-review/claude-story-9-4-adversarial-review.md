# Adversarial Review — Story 9.4: Camera Scroll

**Date:** 2026-03-11
**Artifact:** `docs/implementation-artifacts/9-4-camera-scroll.md`
**Type:** Implementation story spec

---

## Findings

1. **Tests verify offset but never cursor stability.**
   Every scroll test in task 5 asserts only on `list_state_offset_for_test()`. None assert
   that the cursor position is unchanged after the scroll. The primary AC guarantee —
   "without moving the cursor" — is entirely untested. A broken implementation that shifts
   the cursor AND the offset would pass all tests.

2. **Asymmetric regression test coverage for Ctrl+Down.**
   `ctrl_up_does_not_map_to_up` exists but there is no `ctrl_down_does_not_map_to_down`.
   The arm-ordering bug is equally possible for both directions. Half the regression guard
   is missing.

3. **`ctrl_up_does_not_map_to_up` is logically redundant.**
   If `from_key_maps_ctrl_up_to_camera_scroll_up` passes (result *is* `CameraScrollUp`),
   the result trivially cannot be `Up`. The `assert_ne` test adds zero additional coverage
   and creates maintenance noise without benefit.

4. **Snap-back table in Dev Notes uses ambiguous "offset".**
   The "Snap-back condition" column reads `cursor >= offset + visible_height`, where
   "offset" is undefined — old offset or new_offset. The implementation uses `new_offset`,
   which differs by 1. On a one-row scroll this is precisely the boundary case. A developer
   reading the table to implement a variant could introduce an off-by-one.

5. **Test comment in `scroll_view_down_shifts_offset_without_moving_cursor` explains
   the snap condition backwards.**
   The comment says `"Snap check: selected(2) >= new_offset(1) → no snap"`. The actual
   snap condition is `selected_idx < new_offset`. The comment uses the wrong comparison
   direction. A developer modelling additional tests on this comment will write incorrect
   assertions.

6. **`rg "InputEvent::Camera"` command for search-mode verification proves nothing.**
   The instruction says "confirm they fall through unhandled" but the `rg` command searches
   for `InputEvent::Camera` occurrences that don't exist yet at verification time. It cannot
   verify the *absence* of handling in non-stack-frame dispatch paths. Should instead grep
   the search-mode dispatch branch for `_ => {}` exhaustiveness.

7. **`rg` pipeline for `offset_mut` API verification is fragile.**
   The shell command embeds Python 3 in a `$()` substitution with multi-line string
   escaping. It will fail without Python 3, on different `cargo metadata` output formats,
   or with shell quoting differences. A plain
   `find ~/.cargo/registry/src -name "state.rs" -path "*/ratatui*" | xargs grep "fn offset_mut"`
   achieves the same result with no fragility.

8. **No test for scroll-down no-op when all items fit in the viewport.**
   When `item_count <= visible_height`, `max_offset` is 0 and `scroll_view_down` silently
   does nothing. This is a distinct code path not covered by any test.
   `scroll_view_no_op_when_no_frames` covers empty list; "list shorter than viewport" is a
   common real-world state on short stacks and is unverified.

9. **`preview_stack_state` not addressed.**
   `app.rs` maintains both `self.stack_state` and `self.preview_stack_state`. Camera scroll
   targets only `stack_state`. The story neither handles the preview panel nor explicitly
   excludes it in Out of Scope. Users seeing a desynchronised preview after camera scroll
   may file this as a bug.

10. **ADR on `camera_offset` contains incorrect reasoning.**
    The ADR states "ratatui calls an internal snap during `StatefulWidget::render` that
    adjusts `list_state.offset`". Ratatui does not snap during render — it only adjusts
    offset in `ListState::select()`. The render function reads offset as-is. The stated
    rationale is factually wrong and will mislead anyone who verifies the claim.

11. **No plain `Ctrl+Down` → `Down` regression test.**
    `from_key_plain_up_still_maps_to_up` verifies plain `Up` survives the Ctrl arm
    insertion, but there is no equivalent for plain `Down`. Both arms are touched; only one
    direction is regression-tested.

12. **`scroll_view_up` lacks an upper-bound clamp on `new_offset`.**
    If `list_state.offset()` is ever set above `item_count - visible_height` (future bug or
    test setup), `scroll_view_up` decrements it by 1 without any upper-bound correction.
    `scroll_view_down` correctly clamps via `max_offset`; `scroll_view_up` has no equivalent
    guard, leaving the offset potentially in an invalid range after decrement.
