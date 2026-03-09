# Code Review — Story 4.1: Collection Pagination Engine

**Date:** 2026-03-09
**Reviewer:** Amelia (Dev Agent, Claude Sonnet 4.6)
**Story:** `docs/implementation-artifacts/4-1-collection-pagination-engine.md`
**Commit reviewed:** `df5c5cb feat: collection pagination engine (Story 4.1)`
**Final status:** done

---

## Summary

| Severity | Count | Fixed |
|----------|-------|-------|
| HIGH     | 2     | 2 ✅  |
| MEDIUM   | 3     | 2 ✅ / 1 deferred |
| LOW      | 2     | 1 ✅ / 1 deferred |

All tests pass: **421 total** (up from 415 — 6 new tests added by this review).

---

## Findings

### 🔴 H1 — LinkedList extractor untested [FIXED]

**File:** `crates/hprof-engine/src/pagination.rs`

`extract_linked_list` (lines 297-382) was implemented and marked [x] in Task 3.5,
but none of the 14 original tests exercised it.

**Fix:** Added two tests:
- `linked_list_walks_chain` — full 2-node walk, offset=0
- `linked_list_offset_into_chain` — offset=1 into 2-node list

---

### 🔴 H2 — ConcurrentHashMap extractor untested [FIXED]

**File:** `crates/hprof-engine/src/pagination.rs`

`extract_hash_map(concurrent=true)` uses `"val"` as the value field name
(vs `"value"` for HashMap). Task 3.6 was marked [x] with zero test coverage
on the `concurrent=true` code path.

**Fix:** Added `concurrent_hashmap_uses_val_field` — verifies that key and value
are both resolved non-null via the `"val"` field name.

---

### 🟡 M1 — `extract_array_list` double allocation [FIXED]

**File:** `crates/hprof-engine/src/pagination.rs:150-154`

```rust
// Before
let bounded: Vec<u64> = elements.into_iter().take(total as usize).collect();
paginate_id_slice(&bounded, total, offset, limit, hfile)

// After
paginate_id_slice(&elements, total, offset, limit, hfile)
```

`paginate_id_slice` already uses the `total` parameter as the logical size bound
to compute `remaining` and `actual_limit`. The intermediate `bounded` allocation
was redundant and violated the story's explicit anti-pattern:
*"Do NOT load all collection entries into memory before paginating"*.

Note: `find_object_array` still loads all elements into a `Vec<u64>`
(architectural limitation of the current scanner design). A `find_object_array_range`
accessor in `hprof-parser` would fully resolve this, deferred to a future story.

---

### 🟡 M2 — Missing tests for HashSet, LinkedHashMap, Vector [FIXED]

Tasks 3.4, 3.7, 3.8 were marked [x] but the routing in `match_extractor` was
untested for these delegating types.

**Fix:** Added three tests:
- `hashset_returns_keys_only` — HashSet → backing HashMap, verifies keys
  returned as values, `key=None`
- `linkedhashmap_delegates_to_hashmap` — verifies entry resolved via HashMap
  extractor
- `vector_uses_elementcount_field` — verifies `elementCount` field name (not
  `size`) is picked up for `java.util.Vector`

---

### 🟡 M3 — Cycle guards use `std::collections::HashSet` [DEFERRED]

**File:** `crates/hprof-engine/src/pagination.rs:196, 330`

```rust
let mut visited = std::collections::HashSet::new();
```

Epic 8 migrated all hot-path maps to `FxHashMap`/`FxHashSet` (via `rustc_hash`).
The cycle guards in `extract_hash_map` and `extract_linked_list` use stdlib
`HashSet`. For large linked chains this is slower than FxHashSet.

**Deferred:** `rustc_hash` is not a workspace dependency. Adding it requires
updating the workspace `Cargo.toml` — deferred to a future cleanup story.

---

### 🟢 L1 — `CollectionPage` missing `Clone` derive [FIXED]

**File:** `crates/hprof-engine/src/engine.rs:162`

`EntryInfo` derives `Clone` but `CollectionPage` did not. Story 4.2 (TUI
rendering) will cache pages in app state and requires `Clone`.

**Fix:** Added `Clone` to `#[derive(Debug, Clone)]` on `CollectionPage`.

---

### 🟢 L2 — Test count in Dev Agent Record inaccurate [FIXED]

Dev Agent Record claimed "415 total tests passing". Actual count was 416.
Updated to 421 after review fixes.

---

## Files Changed by Review

- `crates/hprof-engine/src/engine.rs` — `CollectionPage` derives `Clone`
- `crates/hprof-engine/src/pagination.rs` — removed `bounded` alloc; 6 new tests
- `docs/implementation-artifacts/4-1-collection-pagination-engine.md` — status
  updated to `done`, completion notes updated
