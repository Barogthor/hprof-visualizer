# Code Review: Story 9.8 — Pinned Item Navigation & Array Expansion

**Date:** 2026-03-12
**Reviewer:** Claude (Amelia / Dev Agent)
**Commits reviewed:** `bac8a35`, `d573ef1`, `cf58f8a`, `969ebba`, `ebfed66`, `7103b07`
**Files reviewed:** `favorites.rs`, `favorites_panel.rs`, `app/mod.rs`, `app/tests.rs`,
`tree_render.rs`, `stack_view/state.rs`

---

## Test Suite & Tooling

| Check | Result |
|-------|--------|
| `cargo test --all` | ✅ 344 passed, 0 failed |
| `cargo clippy --all-targets -- -D warnings` | ✅ Clean |
| `cargo fmt -- --check` | ✅ Clean |

---

## Acceptance Criteria

| AC | Status | Evidence |
|----|--------|----------|
| AC1 – Free navigation between pinned items | ✅ IMPLEMENTED | `move_up/down` + `abs_row()` → `list_state.select` |
| AC2 – Inline expand | ✅ IMPLEMENTED | `Right/Enter` → `local_collapsed.remove` |
| AC3 – Inline collapse | ✅ IMPLEMENTED | `Left` → `local_collapsed.insert` + `clamp_sub_row` |
| AC4 – Array pagination | ✅ IMPLEMENTED | `pending_pinned_pages` + `SNAPSHOT_CHUNK_PAGE_LIMIT` |
| AC5 – Auto-load next batch | ✅ IMPLEMENTED | Sentinel detection on `Down` → thread spawn |

---

## Task Audit

All `[x]` checkboxes verified against actual code. No task marked done without implementation.

Notable verifications:
- `pending_pinned_pages.clear()` in **both** unpin paths (`toggle_pin:468`, `ToggleFavorite:570`) ✅
- Insert-key-before-spawn pattern respected (task 5.3) ✅
- Apply-page-before-remove-key pattern respected (task 5.4) ✅
- `debug_assert_eq!` math in `collect_row_metadata` verified correct ✅
- `collect_object_children_rows` vs `collect_collection_entry_obj_rows` visited-set semantics
  correctly mirror `append_object_children` vs `append_collection_entry_obj` in `tree_render.rs` ✅
- Tests 6.7/6.8 call both `collect_row_metadata` and `render_variable_tree` and compare
  live outputs ✅

---

## Findings

### 🟡 MEDIUM — Fixed

**[M1] `visible_collection_chunks` — unnecessary clone, misleading name**
`favorites_panel.rs:236-243` (deleted)

The function did no filtering despite its name. It converted `&HashMap<u64, CollectionChunks>`
into an owned `HashMap<u64, CollectionChunks>` via full clone, solely to satisfy the borrow
checker. Both `MetadataCollector` and `render_variable_tree` already accept
`&HashMap<u64, CollectionChunks>`, so the clone was avoidable.

Called twice per item per render (once in `collect_row_metadata`, once in the render path),
the function imposed O(2N) unnecessary allocations per frame.

**Fix applied:** Deleted `visible_collection_chunks`. All 6 call sites now pass `collection_chunks`
directly by reference.

---

### 🟢 LOW — Fixed

**[L1] Test 6.14 — assertion too weak**
`favorites_panel.rs:1748`

```rust
// Before
assert!(row_count > 0);
assert!(row_count < 100);

// After
// 1 header + A-row + b-field-row + cyclic-A-row + 1 separator = 5
assert_eq!(row_count, 5);
```

The original bounds `(0, 100)` would pass even if the cyclic-guard were broken and the
traversal collapsed to a trivially small count. The exact value 5 is deterministic for the
A→B/B→A graph in that test and is now asserted explicitly.

---

### 🟢 LOW — Not fixed (no code to change)

**[L2] No test for `Disconnected` path in `pending_pinned_pages` poll**
`app/mod.rs:1718-1724`

The code is correct — warning is added and key is removed on `TryRecvError::Disconnected`.
No automated test covers this path. A future test could drop the sender before polling to
verify the warning appears and the entry is cleaned up.

**[L3] Long lines in `app/tests.rs:542,601`**
Test comments exceed 100 chars. Pre-existing, unrelated to story 9.8.

---

## Story Status

**→ done** (all ACs implemented, no HIGH/MEDIUM issues remaining after fixes)
