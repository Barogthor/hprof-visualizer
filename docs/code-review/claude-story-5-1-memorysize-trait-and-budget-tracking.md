# Code Review — Story 5.1: MemorySize Trait & Budget Tracking

**Date:** 2026-03-10
**Reviewer:** Amelia (Dev Agent, adversarial mode)
**Branch:** `feature/epic-5-memory-management-lru-eviction`
**Story file:** `docs/implementation-artifacts/5-1-memorysize-trait-and-budget-tracking.md`

---

## Git vs Story File List — Discrepancies

All 13 files in the story File List match git changes on the branch vs main.
No undocumented files, no phantom entries. **0 discrepancies.**

---

## Test Suite

All 340 tests pass. No failures, no ignores.

---

## 🔴 CRITICAL ISSUES

### C1 — Task 6 subtask marked [x] but NOT implemented: `expand_object` path has zero counter wiring

**File:** `crates/hprof-engine/src/engine_impl.rs:714-774`
**Task:** `[x] Wire into existing expand/collapse paths (prep for 5.3)`

`expand_object()` resolves fields and enriches them without making a single call
to `self.memory_counter.add()`. `get_local_variables()` and `get_stack_frames()`
are similarly untouched. The Dev Agent Record completion notes for Task 6 never
even mention this subtask — it was silently skipped.

The counter is ONLY updated at construction time
(`engine_impl.rs:290`, `engine_impl.rs:316`). The "wire into existing paths" is
purely declarative (the `memory_counter` field exists on `Engine`) — no behavioral
change was made to the expand/collapse pipeline.

**Impact:** AC2 (`increments by exactly memory_size() when added to cache`) and
AC3 (`decrements by exactly memory_size() on eviction`) are NOT wired in for
objects expanded at runtime. The counter reflects only the initial state, not
ongoing usage. Story 5.3 will build on this supposed wiring — if it was never
done, the integration point is missing.

**Fix:** Either:
- Actually add `self.memory_counter.add(fields.iter().map(|f| f.memory_size()).sum())` in
  `expand_object()` and a corresponding `subtract` when collapsed (if collapse is
  tracked), OR
- Un-check the subtask and document the deferral to 5.3 explicitly

---

## 🟡 HIGH ISSUES

### H1 — Integration tests only verify `memory_used() > 0` — upper bound missing

**Files:**
- `engine_impl.rs:820-831` (`memory_used_positive_after_from_file`)
- `engine_impl.rs:988-1008` (`memory_used_with_populated_fixture`)

The Dev Notes Testing Strategy explicitly states:
> **Integration test:** create Engine from test fixture, assert `memory_used()` > 0
> and **< file_size * 10**

Both integration tests only check the lower bound. The upper bound (`< file_size * 10`)
is absent. This bound would catch catastrophic overcount bugs (e.g. a recursive
`memory_size()` loop or a factor-of-1000 miscalculation).

**Fix:** Add upper bound assertions. For the minimal file test, the file is ~30 bytes,
so `memory_used() < 300` would be a reasonable sanity cap. For the populated fixture
test, compute `bytes.len()` and assert `engine.memory_used() < bytes.len() * 10`.

### H2 — No test verifies the counter equals the exact sum of `memory_size()` calls

**Files:** `engine_impl.rs:372-387` (`initial_memory`), integration tests

AC2 states the counter "increments by **exactly** the value reported by
`memory_size()`". No test constructs a known fixture, calls `index.memory_size()`
and `thread_cache_size` directly, and asserts `engine.memory_used()` equals that
exact value.

The `> 0` tests give no confidence that `initial_memory()` is calling the right
`memory_size()` impls or that the sum is wired correctly. An off-by-one or a
missing map in `PreciseIndex::memory_size()` would go undetected.

**Fix:** Add a test using `HprofTestBuilder` with a controlled fixture, compute
`hfile.index.memory_size()` manually, and assert `engine.memory_used() ==
expected`.

---

## 🟡 MEDIUM ISSUES

### M1 — `FieldInfo::memory_size()` and `VariableInfo::memory_size()` duplicate match logic — DRY violation

**Files:** `engine.rs:232-250` (FieldInfo), `engine.rs:201-211` (VariableInfo)

Both impls duplicate the `match &self.value` arm logic that already exists in
`FieldValue::memory_size()` and `VariableValue::memory_size()`. The correct
idiomatic pattern is:

```rust
// FieldInfo
fn memory_size(&self) -> usize {
    std::mem::size_of::<Self>()
        + self.name.capacity()
        + self.value.memory_size()
        - std::mem::size_of::<FieldValue>()
}

// VariableInfo
fn memory_size(&self) -> usize {
    std::mem::size_of::<Self>()
        + self.value.memory_size()
        - std::mem::size_of::<VariableValue>()
}
```

The duplicated logic means any future change to `FieldValue::memory_size()` (e.g.
a new variant with heap allocations) will NOT be reflected in `FieldInfo::memory_size()`
unless it's updated in both places. This is a classic DRY debt trap for Story 5.3+.

### M2 — `initial_memory()` estimates a `std::HashMap` using `fxhashmap_memory_size`

**File:** `engine_impl.rs:382-386`

```rust
let cache_overhead =
    hprof_api::fxhashmap_memory_size::<u32, ThreadMetadata>(
        thread_cache.capacity(),
    );
```

`thread_cache` is declared as `HashMap<u32, ThreadMetadata>` (stdlib, not FxHashMap).
The helper's docstring explicitly says "Estimates the memory used by an `FxHashMap`".
While both use hashbrown internally, the entry point (`Engine` struct field,
`engine_impl.rs:271`) is a `std::collections::HashMap` —
not a `FxHashMap` — making this naming contract incorrect.

This is also inconsistent with the rest of the codebase which uses FxHashMap for
all data maps. If `thread_cache` should be FxHashMap, change the field type; if
it must be HashMap, document the approximation explicitly.

### M3 — `EntryInfo::memory_size()` lacks heap-path test coverage

**File:** `engine.rs:252-261`, test at `engine.rs:611-622`

The only test for `EntryInfo::memory_size()` exercises `key=None, value=Int(1)` —
both with zero heap allocations. The entire arithmetic (`value.memory_size() -
size_of::<FieldValue>()`) is never verified against a case with actual heap
(an `ObjectRef` key or value with strings).

The `CollectionPage` test (`engine.rs:624-642`) only checks `> size_of::<CollectionPage>()`,
a loose bound that would pass even if `EntryInfo::memory_size()` was wrong.

**Fix:** Add a test with a key and value of type `FieldValue::ObjectRef` carrying
known-length strings, and assert the exact byte count.

---

## 🟢 LOW ISSUES

### L1 — `fxhashmap_memory_size` docstring misleading about control bytes

**File:** `memory_size.rs:25-34`

The comment "8 bytes account for hashbrown control bytes" is inaccurate.
hashbrown uses 1 control byte per slot in its SIMD bitmap, not 8. The `+8` is
more accurately described as bucket alignment/padding overhead per slot. The
formula is intentionally conservative (overestimate), but the explanation
should say "bucket alignment overhead" rather than "control bytes".

### L2 — `PreciseIndex` string capacity test uses loose bound

**File:** `indexer/precise.rs:223-233`

```rust
assert!(total >= 200, "must include string capacity ({total})");
```

A 200-byte string was inserted — but `total >= 200` would also pass if only 1
byte of bucket overhead was counted. A tighter assertion would verify that the
total is within a reasonable range of the expected bucket + string size, giving
better regression protection.

---

## Summary

| Severity | Count | IDs |
|---|---|---|
| 🔴 Critical | 1 | C1 |
| 🟡 High | 2 | H1, H2 |
| 🟡 Medium | 3 | M1, M2, M3 |
| 🟢 Low | 2 | L1, L2 |

**8 issues total.**

The core trait design, `MemoryCounter`, `PreciseIndex` impl, and all parser-type
impls are solid. The critical gap is the false task completion for expand-path
wiring and the absence of a precise integration test for the counter value.

---

## Recommendation

**Story status → `in-progress`** until at minimum C1 and H1-H2 are addressed.

Priority order:
1. **C1** — un-check or actually implement the expand-path wiring
2. **H1/H2** — add upper-bound integration test + exact-value counter test
3. **M1** — fix DRY violation before it bites in 5.3
4. **M2** — align `thread_cache` type with FxHashMap or document the approximation
5. **M3** — add `EntryInfo` heap-path test
