# Story 5.4: Transparent Re-Parse & Multi-Cycle Stability

Status: review

## Story

As a user,
I want evicted data to be transparently re-parsed on demand when I navigate back,
with identical results, and the system to remain stable across many eviction cycles,
so that I can freely explore large heap dumps without worrying about data loss or
degradation.

## Acceptance Criteria

### AC1: Re-parse on demand (FR26)

**Given** a subtree that was previously evicted from the LRU cache
**When** the user navigates back to it (calls `expand_object(id)`)
**Then** the data is re-parsed from mmap on demand — the same
`decode_object_fields` path as the initial parse

### AC2: Byte-accurate re-parse (NFR8)

**Given** a subtree is evicted and then re-parsed
**When** the values are displayed
**Then** they are identical to the original parse — field names, types, and
values are byte-accurate with no data corruption

### AC3: Multi-cycle stability (NFR5)

**Given** the memory budget is very small (e.g., 1 byte) relative to the data
being explored and every expand triggers immediate eviction
**When** the user navigates to the same objects repeatedly (≥ 50 cycles)
**Then** the system remains stable — `expand_object` always returns `Some`,
the `MemoryCounter` does not underflow to `usize::MAX`, and no panic occurs

## Tasks / Subtasks

- [x] Task 1: Add `PartialEq` to `FieldInfo` (AC: 2)
  - [x] In `crates/hprof-engine/src/engine.rs`, add `PartialEq` to the
        `#[derive(Debug, Clone)]` of `struct FieldInfo`
  - [x] `FieldValue` already derives `PartialEq` — no change needed there
  - [x] This is the only production code change in this story

- [x] Task 2: Add AC2 test — byte-accurate re-parse after eviction (AC: 1, 2)
  - [x] In `mod lru_eviction_tests` inside `#[cfg(test)] mod tests` in
        `engine_impl.rs`, add:
        ```rust
        #[test]
        fn re_parse_after_eviction_produces_identical_fields() {
            // Budget = 1 → every expand triggers immediate full eviction
            // A is evicted as soon as it is inserted (it becomes the sole
            // LRU entry and the eviction loop drains the cache).
            let engine = engine_two_objects(1);
            let fields_first =
                engine.expand_object(0xAAA).unwrap();
            // Expand B to confirm eviction and internal state remain sane
            engine.expand_object(0xBBB).unwrap();
            // Re-expand A: must be a cache miss → re-parse from mmap
            let fields_second =
                engine.expand_object(0xAAA).unwrap();
            assert_eq!(
                fields_first, fields_second,
                "re-parse must produce byte-identical fields (AC2 / NFR8)"
            );
        }
        ```

- [x] Task 3: Add AC3 test — multi-cycle stability (AC: 3)
  - [x] In `mod lru_eviction_tests`, add:
        ```rust
        #[test]
        fn multi_cycle_no_panic_no_counter_overflow() {
            // Budget = 1 → each expand evicts all cached data.
            // 50 cycles of alternating A/B expansion must not panic
            // and must not overflow the MemoryCounter.
            let engine = engine_two_objects(1);
            for _ in 0..50 {
                let r_a = engine.expand_object(0xAAA);
                assert!(
                    r_a.is_some(),
                    "A must always return Some across all cycles"
                );
                let r_b = engine.expand_object(0xBBB);
                assert!(
                    r_b.is_some(),
                    "B must always return Some across all cycles"
                );
            }
            // usize::MAX / 2 is a conservative sentinel: real usage is
            // at most a few KB; any value above this indicates underflow.
            assert!(
                engine.memory_used() < usize::MAX / 2,
                "MemoryCounter must not underflow to usize::MAX"
            );
        }
        ```

## Dev Notes

### Why AC1 requires no production code change

`expand_object` was rewritten in Story 5.3 as a cache-checked wrapper around
`decode_object_fields`. On a cache miss — whether the object was never expanded
or was evicted — the code path is identical: `decode_object_fields` re-parses
from the mmap-backed `HprofFile`. The transparent re-parse guarantee is
structural, not additional logic.

[Source: crates/hprof-engine/src/engine_impl.rs — `expand_object` (~line 787)]

### Why `PartialEq` on `FieldInfo` is justified

`FieldValue` already derives `PartialEq` (engine.rs ~line 108). Adding it to
`FieldInfo` is consistent, minimal, and the only way to write a readable AC2
assertion (`assert_eq!(fields_first, fields_second)`). Without it, the
comparison would require either a manual field-by-field loop or a `Debug`
string comparison — both are less clear.

[Source: crates/hprof-engine/src/engine.rs — `FieldInfo` (~line 141)]

### MemoryCounter underflow protection

`MemoryCounter::subtract` uses `fetch_sub` on `AtomicUsize`. If the value to
subtract ever exceeds the current counter, it wraps to `usize::MAX`. The
multi-cycle test guards against this regression. The correct invariant is:
`evict_lru()` returns the size stored at insert time — the only value safe to
pass to `subtract`. This is already enforced by the Story 5.3 implementation.

[Source: crates/hprof-engine/src/cache/budget.rs — `MemoryCounter::subtract`]
[Source: crates/hprof-engine/src/engine_impl.rs — eviction loop (~line 806)]

### Budget = 1 byte test rationale

With `budget_bytes: Some(1)`, `usage_ratio()` exceeds `EVICTION_TRIGGER` (0.80)
immediately after any `add()` call. The eviction loop drains the cache on every
`expand_object` call. This is the worst-case scenario for re-parse stability:
every navigation forces a full re-parse cycle with no caching benefit.

[Source: docs/implementation-artifacts/5-3-lru-eviction-core.md#Dev Notes]

### No changes to `StubEngine` in `hprof-tui`

`StubEngine` does not implement `expand_object` in a way that touches the
cache. No TUI changes are required.

### Test builder helpers

Reuse `engine_two_objects(budget)` already defined in `mod lru_eviction_tests`
(~line 2219 in engine_impl.rs). No new helpers needed.

[Source: crates/hprof-engine/src/engine_impl.rs — `engine_two_objects` (~line 2219)]

### Previous Story Intelligence (5.3)

- `ObjectCache::new()`, `get(id)`, `insert(id, fields)`, `evict_lru()`,
  `is_empty()`, `len()` — all available in `crates/hprof-engine/src/cache/lru.rs`
- `EVICTION_TRIGGER = 0.80`, `EVICTION_TARGET = 0.60` — constants in `cache/lru.rs`,
  imported in `engine_impl.rs` at module level
- `Engine::expand_object` — cache hit → return clone; cache miss → re-parse via
  `decode_object_fields` → insert → run eviction loop
- `Engine::memory_used()` / `Engine::memory_budget()` — public trait methods
- `EngineConfig { budget_bytes: Some(n) }` — use for deterministic budgets in tests
- 161 tests pass after Story 5.3 code review; 0 clippy warnings
- Commit style: `feat: Story N.M — short description` (no co-author lines)

### Project Structure Notes

- Aligned with `crates/hprof-engine/src/cache/` module structure
- Existing `mod lru_eviction_tests` location: `engine_impl.rs` ~line 2210

Modified files:
```
crates/hprof-engine/src/engine.rs        # +PartialEq on FieldInfo
crates/hprof-engine/src/engine_impl.rs   # +2 tests in mod lru_eviction_tests
```

No new files.

### References

- [Source: docs/planning-artifacts/epics.md#Story 5.4]
- [Source: docs/planning-artifacts/architecture.md#Cache & Eviction: LRU by Subtree]
- [Source: docs/implementation-artifacts/5-3-lru-eviction-core.md]
- [Source: crates/hprof-engine/src/engine_impl.rs — `expand_object` (~787),
  `lru_eviction_tests` (~2210)]
- [Source: crates/hprof-engine/src/engine.rs — `FieldInfo` (~141), `FieldValue` (~108)]
- [Source: crates/hprof-engine/src/cache/lru.rs — `ObjectCache`, eviction constants]
- [Source: crates/hprof-engine/src/cache/budget.rs — `MemoryCounter::subtract`]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Task 1: Added `PartialEq` to `#[derive(Debug, Clone, PartialEq)]` on `struct FieldInfo`
  in `engine.rs:141`. No other production code changes needed — `expand_object` already
  transparently re-parses on cache miss (structural guarantee from Story 5.3).
- Task 2: `re_parse_after_eviction_produces_identical_fields` — budget=1 forces full eviction
  after every insert; assert_eq!(fields_first, fields_second) confirms byte-accurate re-parse.
- Task 3: `multi_cycle_no_panic_no_counter_overflow` — 50 A/B alternating cycles confirm
  no panic and `memory_used() < usize::MAX / 2` guards against `AtomicUsize` underflow.
- 163 tests pass (161 pre-existing + 2 new); 0 clippy warnings.

### File List

- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `docs/implementation-artifacts/5-4-transparent-re-parse-and-multi-cycle-stability.md`
- `docs/implementation-artifacts/sprint-status.yaml`
