# Code Review — Story 5.3: LRU Eviction Core

**Date:** 2026-03-10
**Story:** `docs/implementation-artifacts/5-3-lru-eviction-core.md`
**Reviewer:** Amelia (Dev Agent — Adversarial Review)
**Outcome:** All issues fixed → **done**

---

## Git vs Story Discrepancy

| Finding | Severity |
|---------|----------|
| `Cargo.lock` in git commit `de31501` but absent from story File List | MEDIUM (fixed: added to File List) |

---

## Issues Found & Fixed

### 🔴 HIGH

**H1 — `expand_object_lru_order_respected` did not assert LRU ordering (AC2)**
- File: `crates/hprof-engine/src/engine_impl.rs:2358`
- `mem_before`, `mem_after_a`, `mem_after_b` computed but never asserted.
  Comment explicitly conceded: "The key invariant: the test completes without hang."
- AC2 was unvalidated.
- **Fix:** Rewrote test to insert A, B, C; promote A to MRU; manually evict LRU twice
  via `engine.object_cache.evict_lru()`; assert only A survives; assert B re-expand
  increases `memory_used()` (cache miss) while A re-expand does not (cache hit).

### 🟡 MEDIUM

**M1 — Two new clippy warnings in `cache/lru.rs` (Dev Agent Record falsely claimed "clippy clean")**
- `warning: unused import: std::num::NonZeroUsize` — `lru.rs:10`
- `warning: missing Default impl for ObjectCache` — `lru.rs:30`
- **Fix:** Removed unused import. Added `impl Default for ObjectCache`.

**M2 — `Cargo.lock` absent from story File List**
- **Fix:** Added `Cargo.lock` to File List in Dev Agent Record.

**M3 — `ObjectCache::insert` silently discards displaced entry's size**
- `LruCache::put()` returns `Option<(Vec<FieldInfo>, usize)>` for the evicted old entry,
  but the code discarded it. If the same `object_id` were inserted twice, `MemoryCounter`
  would double-count bytes — protected by design via the cache-hit check in `expand_object`
  but fragile to future refactoring.
- **Fix:** Added `debug_assert!(old.is_none(), ...)` — panics in debug builds on double-insert.

### 🟢 LOW

**L1 — EVICTION constants imported inside function body, not at module level**
- Story spec said "Add import at top of `engine_impl.rs`".
- **Fix:** Moved `use crate::cache::lru::{EVICTION_TARGET, EVICTION_TRIGGER}` to the
  module-level `use` block, removed local `use` from `expand_object` body.

**L2 — Unused variable `baseline` in `expand_object_lru_order_respected`**
- `let baseline = engine.memory_used();` at line 2363 — never referenced.
- **Fix:** Removed (as part of H1 test rewrite).

---

## Final State

- 161 tests pass, 0 failures
- 0 new clippy warnings (2 pre-existing in `pagination.rs`, out of scope)
- Story status: **done**
- Sprint status synced: `5-3-lru-eviction-core` → **done**
