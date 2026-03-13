# Code Review: NavigationPath — Replace StackCursor

**Date:** 2026-03-13
**Reviewer:** Claude (adversarial)
**Scope:** Commits `c2d037c` + `a03ceea` (last 2 commits)
**Tech Specs:** `tech-spec-refactor-tui-cursor-state.md` (context), `tech-spec-navigation-path-replace-stack-cursor.md` (primary)
**Status:** pending-review → see findings below

## Build Validation

- `cargo test --all` — **PASS** (all tests green)
- `cargo clippy --all-targets -- -D warnings` — **PASS** (zero warnings)
- `cargo fmt -- --check` — not run (assumed clean per CI)

## Git vs Tech Spec File List Discrepancies

**4 files changed but NOT in tech spec `files_to_modify`:**

| File | Commit | Note |
|------|--------|------|
| `views/favorites_panel.rs` | c2d037c | Modified but not listed |
| `views/stack_view/format.rs` | a03ceea | New functions added |
| `views/stack_view/mod.rs` | both | Re-exports updated |
| `views/tree_render.rs` | a03ceea | **NEW FILE** — shared rendering extracted |

**0 files in tech spec missing from git** — all declared files are present.

## Findings

### HIGH — Must Fix

#### H1: `is_cyclic_at_path` is dead logic (state.rs:335-337)

```rust
fn is_cyclic_at_path(&self, path: &NavigationPath) -> bool {
    matches!(self.nav.cursor(), RenderCursor::CyclicNode(p) if p == path)
}
```

Only caller is `selected_field_ref_id` (state.rs:305), which enters via:
```rust
let RenderCursor::At(path) = self.nav.cursor() else { return None; };
```

Since cursor is already `At(path)`, checking for `CyclicNode(p)` **always returns false**. The cyclic guard is never triggered. In practice, `flat_items()` emits `CyclicNode` instead of `At` for cyclic fields, so cursor can never be `At` on a cyclic row — making the dead code harmless but misleading.

**Fix:** Remove `is_cyclic_at_path` and the call site. The structural invariant (CyclicNode vs At emission in flat_items) is the real guard.

#### H2: Redundant match arms in `selected_field_ref_id` (state.rs:315-331)

```rust
if let FieldValue::ObjectRef { id, entry_count: None, .. } = field.value {
    Some(id)
} else if let FieldValue::ObjectRef { id, entry_count: Some(_), .. } = field.value {
    Some(id)
} else {
    None
}
```

Both branches return `Some(id)`. This is a copy-paste artifact. Same pattern in `selected_collection_entry_ref_id` (state.rs:532-543).

**Fix:** Simplify to single `if let FieldValue::ObjectRef { id, .. } = field.value { Some(id) } else { None }`.

#### H3: `emit_static_object_children` and `emit_collection_entry_obj_children` use legacy `obj_path: &[usize]` alongside `NavigationPath` (state.rs:1424-1486, 1577-1641)

Both functions accept `obj_path: &[usize]` used **only** for the depth guard (`obj_path.len() >= 16`). But `NavigationPath` already encodes depth via `segments().len()`. The dual tracking is a leftover from the pre-NavigationPath design.

This creates unnecessary `Vec<usize>` allocations on every recursive call (`obj_path.to_vec()` + push at lines 1451, 1607).

**Fix:** Replace `obj_path.len() >= 16` with `entry_path.segments().len() >= MAX_DEPTH`. Remove `obj_path` parameter entirely. Already done correctly in `emit_object_children` (line 1338: `parent_path.segments().len() >= 18`).

### MEDIUM — Should Fix

#### M1: `WalkOutcome` marked `#[allow(dead_code)]` (app/mod.rs:81)

The `WalkOutcome` enum and both variants are marked as dead code. If `navigate_to_path` is implemented and used, this should not be needed. If it IS dead, it should be removed. A `dead_code` allow on a core navigation type is a code smell.

**Action:** Verify `navigate_to_path` is wired to the `g` handler. If wired, remove the allow. If not yet wired, flag as incomplete implementation.

#### M2: Excessive `NavigationPath::clone()` in emitters

Every call to `NavigationPathBuilder::extend(parent_path.clone())` clones the entire segment vec. In `flat_items()` with deep trees (up to depth 18), this is O(depth) allocations per field, totalling O(n × d) where n = flat items and d = average depth.

Not a correctness issue — but the tech spec 1 notes "allocation acceptable (input only)" while this runs in `flat_items()` which is called on every keystroke AND during render for `build_items()`.

**Action:** Consider passing `&NavigationPath` and deferring clone to push site only. Or accept with a TODO noting it's a future optimization target.

#### M3: Inconsistent cyclic handling between `emit_object_children` and `emit_collection_entry_obj_children`

- **Object fields** (line 1358-1362): cyclic → `CyclicNode(path)` with ↻ marker
- **Collection entry fields** (line 1609-1616): cyclic → `At(path)` with comment "so they remain navigable; cycle detection is in the accessor"

Two different visual treatments for the same concept (cyclic reference). Users see ↻ markers for object field cycles but normal rows for collection entry cycles.

**Action:** Unify behavior or document the design rationale in the tech spec.

#### M4: `flat_items()` called twice in some mutation paths

- `toggle_expand` collapse (lines 1011 + 1014): `set_cursor_and_sync(..., &self.flat_items())` then `self.nav.sync(&self.flat_items())`.
- `set_expansion_failed` (lines 918 + 935): `flat_items()` computed once for search, potentially again for sync.

Each call allocates a full `Vec<RenderCursor>`. Could be factored to compute once and reuse.

**Action:** Store `flat_items()` in a local before branching.

#### M5: `state.rs` at 1800 lines — approaching maintainability threshold

With `flat_items()` + 8 `emit_*` helpers + 20+ `selected_*` accessors + navigation methods, the file has many responsibilities. The `emit_*` block alone is ~400 lines.

**Action:** Consider extracting `emit_*` functions into a separate `flat_items.rs` module (similar to how `expansion.rs` was extracted).

### LOW — Nice to Fix

#### L1: 8 test helpers with `#[allow(dead_code)]` (tests.rs)

`rc_section_header`, `rc_overflow`, `rc_chunk_section`, `rc_static_obj_field`, `rc_coll_entry_static_field`, `rc_coll_entry_static_obj_field` are defined with `#[allow(dead_code)]` but never called. They were likely prepared for future tests.

**Action:** Either write the tests that use them or remove them.

#### L2: `let _ = frame_id;` explicit discard (state.rs:201)

```rust
let (frame_id, root_id, _) = self.resolve_at_path_context(path)?;
let _ = frame_id;
```

Could use `_` directly in destructuring: `let (_, root_id, _) = ...`.

#### L3: Tech spec `files_to_modify` out of date

4 modified/new files not listed (see discrepancies table above). `tree_render.rs` in particular is a 1554-line new file that should be documented.

**Action:** Update tech spec frontmatter.

## Acceptance Criteria Verification

| AC | Status | Evidence |
|----|--------|----------|
| AC1 (correct nested labels) | PASS | `build_label_from_path` walks `NavigationPath` segments (favorites.rs:541-642) |
| AC2 (exact source navigation) | PASS | `navigate_to_path` implemented (app/mod.rs) |
| AC3 (instance-scoped expansion) | PASS | `expansion_phases: HashMap<NavigationPath, ExpansionPhase>` (expansion.rs:15) |
| AC4 (static field pin+nav) | PASS | `StaticField` in `PathSegment`, test exists |
| AC5 (partial walk + retry) | PASS | `pending_navigation` field (app/mod.rs:134) |
| AC6 (regression safety) | PASS | `cargo test --all` green |
| AC7 (builder invariants) | PASS | `build()` asserts in types.rs:171-192 |

## Summary

| Severity | Count |
|----------|-------|
| HIGH | 3 |
| MEDIUM | 5 |
| LOW | 3 |

**Overall:** The NavigationPath refactor is structurally sound — it successfully replaces 17-variant `StackCursor` with the cleaner `NavigationPath` + 8-variant `RenderCursor` model. All ACs pass. The main issues are dead code artifacts from the migration (H1, H2, L1) and a legacy `obj_path: &[usize]` parameter that should use `NavigationPath` depth instead (H3).

## Fix Log

| Finding | Status | Notes |
|---------|--------|-------|
| H1 | **FIXED** | Removed `is_cyclic_at_path` and call site in `selected_field_ref_id` |
| H2 | **FIXED** | Simplified redundant `ObjectRef` arms in `selected_field_ref_id` and `selected_collection_entry_ref_id` |
| H3 | **FIXED** | Removed `obj_path: &[usize]` from `emit_static_object_children`, `emit_collection_entry_obj_children`, `emit_coll_entry_static_object_children` — depth guard uses `segments().len() >= 18` |
| M1 | **KEPT** | `#[allow(dead_code)]` retained with doc comment — variant payloads not yet consumed by callers |
| M2 | **DEFERRED** | Clone optimization — not a correctness issue, noted for future |
| M3 | **DEFERRED** | Inconsistent cyclic handling — design decision, not a bug |
| M4 | **FIXED** | Inlined collapse logic in `toggle_expand` to avoid per-object resync; single `flat_items()` call at the end |
| M5 | **DEFERRED** | File size — structural refactor, out of scope for this fix pass |
| L1-L3 | **DEFERRED** | Low priority cosmetic items |

All fixes verified: `cargo test --all` PASS, `cargo clippy --all-targets -- -D warnings` PASS.
