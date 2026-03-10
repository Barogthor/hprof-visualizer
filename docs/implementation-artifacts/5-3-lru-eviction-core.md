# Story 5.3: LRU Eviction Core

Status: done

## Story

As a user,
I want the system to automatically evict the least recently used expanded
subtrees when memory usage approaches the budget,
so that memory stays within bounds during extended exploration sessions.

## Acceptance Criteria

### AC1: Eviction trigger at 80% budget (FR25)

**Given** memory usage reaches 80% of the configured budget
**When** the eviction trigger fires
**Then** the least recently accessed expanded subtree is evicted — all parsed
data under that subtree is freed

### AC2: LRU ordering

**Given** multiple subtrees in the cache
**When** eviction is needed
**Then** the LRU subtree (least recently navigated to) is evicted first

### AC3: Non-blocking eviction

**Given** eviction is running
**When** the user continues navigating
**Then** eviction does not block the UI — the event loop remains responsive

### AC4: Eviction drops usage below target threshold

**Given** memory usage exceeds 80% of the budget
**When** eviction completes
**Then** memory usage is below 60% of the budget, or the object cache is empty
(preventing thrashing when a single object represents a large fraction of the budget)

### AC5: Cache hit avoids re-parse

**Given** an object has already been expanded and is still in the cache
**When** the user navigates to it again
**Then** its fields are returned from cache without re-parsing from mmap —
`memory_used()` does not increase on the second call

## Tasks / Subtasks

- [x] Task 1: Add `lru` dependency (AC: 1, 2)
  - [x] Add `lru = "0.12"` to `crates/hprof-engine/Cargo.toml` `[dependencies]`

- [x] Task 2: Create `ObjectCache` in `cache/lru.rs` (AC: 1, 2, 3)
  - [x] Create `crates/hprof-engine/src/cache/lru.rs`
  - [x] Imports needed:
        `use crate::engine::FieldInfo;`
        `use hprof_api::MemorySize;`
        `use lru::LruCache;`
        `use std::sync::Mutex;`
  - [x] Define `pub struct ObjectCache(Mutex<LruCache<u64, (Vec<FieldInfo>, usize)>>)`
        — tuple stores `(fields, precomputed_memory_size)`
  - [x] `pub fn new() -> Self` — `LruCache::unbounded()` wrapped in `Mutex`
  - [x] `pub fn get(&self, id: u64) -> Option<Vec<FieldInfo>>`
        — `lock().get(&id).map(|(fields, _)| fields.clone())`
        — `.get()` on `LruCache` requires `&mut self`; obtain via
          `self.0.lock().unwrap()` which yields `MutexGuard` (impl `DerefMut`)
        — Promotes the entry to MRU, correctly maintaining LRU order
  - [x] `pub fn insert(&self, id: u64, fields: Vec<FieldInfo>) -> usize`
        — `let mem = compute_fields_size(&fields);`
        — `self.0.lock().unwrap().put(id, (fields, mem));`
        — return `mem` so caller can `memory_counter.add(mem)`
  - [x] `pub fn evict_lru(&self) -> Option<usize>`
        — `self.0.lock().unwrap().pop_lru().map(|(_, (_, size))| size)`
        — returns bytes freed (pre-stored size), or `None` if cache empty
  - [x] `pub fn is_empty(&self) -> bool`
        — `self.0.lock().unwrap().is_empty()`
  - [x] `pub fn len(&self) -> usize`
        — `self.0.lock().unwrap().len()`
        — needed by integration tests to verify eviction actually occurred
  - [x] Define two module-level constants with `pub(crate)` visibility:
        `pub(crate) const EVICTION_TRIGGER: f64 = 0.80;`
        `pub(crate) const EVICTION_TARGET: f64 = 0.60;`
        — `pub(crate)` so `engine_impl.rs` can import them without re-defining
  - [x] Private `fn compute_fields_size(fields: &[FieldInfo]) -> usize`:
        `std::mem::size_of::<Vec<FieldInfo>>()
            + fields.iter().map(|f| f.memory_size()).sum::<usize>()`
        — Intentional approximation: counts `len` slots, not `capacity` slots.
          Consistent with `CollectionPage::memory_size` and the rest of the codebase.
          Do NOT "fix" this to use `capacity` — it would break the pattern.
  - [x] Unit tests (see Testing Strategy section)

- [x] Task 3: Expose `ObjectCache` from `cache/mod.rs` (AC: 1, 2)
  - [x] Add `pub mod lru;` to `crates/hprof-engine/src/cache/mod.rs`
  - [x] Add `pub use lru::ObjectCache;`

- [x] Task 4: Add `object_cache` field to `Engine` (AC: 1, 2, 3)
  - [x] Add `object_cache: crate::cache::ObjectCache` to the `Engine` struct
        in `engine_impl.rs` alongside `thread_cache` and `memory_counter`
        — NO `pub` modifier: the field is private; tests in `mod tests` of
          the same file access it directly via Rust module scoping rules
  - [x] In `Engine::from_file` (~line 289 in struct init):
        add `object_cache: crate::cache::ObjectCache::new()`
  - [x] In `Engine::from_file_with_progress` (~line 316 in struct init):
        add `object_cache: crate::cache::ObjectCache::new()`

- [x] Task 5: Wire `expand_object` with cache + counter + eviction (AC: 1, 2, 3)
  - [x] Extract current `expand_object` body (the field decode + enrichment pass)
        into a private helper: `fn decode_object_fields(&self, object_id: u64) -> Option<Vec<FieldInfo>>`
        — exact same logic as the current `expand_object` body
  - [x] Add import at top of `engine_impl.rs`:
        `use crate::cache::lru::{EVICTION_TRIGGER, EVICTION_TARGET};`
        — Do NOT redefine these constants locally; they must stay in `cache/lru.rs`
          as the single source of truth
  - [x] Rewrite `expand_object` in `impl NavigationEngine for Engine` as:
        ```rust
        fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>> {
            // Cache hit: return clone, entry promoted to MRU
            if let Some(fields) = self.object_cache.get(object_id) {
                return Some(fields);
            }
            // Cache miss: decode from mmap
            let fields = self.decode_object_fields(object_id)?;
            // Insert into cache, track memory
            let mem = self.object_cache.insert(object_id, fields.clone());
            self.memory_counter.add(mem);
            // Evict LRU entries: trigger at 80%, target 60% (hysteresis)
            // Prevents thrashing when a single object is ≥ 20% of the budget
            while self.memory_counter.usage_ratio() >= EVICTION_TRIGGER {
                if let Some(freed) = self.object_cache.evict_lru() {
                    // Saturating subtract: guards against underflow if freed
                    // somehow exceeds current() — keeps counter sane on any bug
                    let current = self.memory_counter.current();
                    self.memory_counter.subtract(freed.min(current));
                    if self.memory_counter.usage_ratio() < EVICTION_TARGET {
                        break;
                    }
                } else {
                    break; // cache empty, nothing more to evict
                }
            }
            Some(fields)
        }
        ```
  - [x] Verify `decode_object_fields` covers the full enrichment pass
        (class name, entry_count, inline_value for ObjectRef children)
  - [x] Confirm existing `expand_object` tests still pass (they test decode logic,
        which is now in `decode_object_fields`)

- [x] Task 6: Integration tests (AC: 1, 2, 3)
  - [x] Test `expand_object_cached_does_not_double_count_memory`:
        call `expand_object(id)` twice on the same object,
        verify `engine.memory_used()` after the second call equals
        `engine.memory_used()` after the first call
  - [x] Test `expand_object_with_tiny_budget_evicts`:
        use `EngineConfig { budget_bytes: Some(1) }` (budget = 1 byte),
        after two distinct `expand_object` calls,
        verify `engine.memory_used() > 0` (something is tracked) and
        the cache is not empty (last expand kept)
  - [x] Test `expand_object_lru_order_respected`:
        insert A, B, C then call `expand_object(A_id)` again (promotes A to MRU);
        apply budget pressure via a 4th `expand_object` on a new object D;
        verify `engine.object_cache.len()` decreased;
        verify a subsequent `expand_object(A_id)` does NOT increase `memory_used()`
        (cache hit — A was MRU), while `expand_object(B_id)` DOES increase it
        (cache miss — B was LRU and was evicted first)
  - [x] Test `expand_object_ac4_usage_below_target_after_eviction`:
        configure budget so two expanded objects together exceed 80%;
        after both are expanded, verify:
        `engine.memory_used() as f64 / engine.memory_budget() as f64 < 0.60`
        OR `engine.object_cache.is_empty()` (satisfies AC4)
        — use `memory_used()`/`memory_budget()` (public trait methods),
          NOT `engine.memory_counter` (private field)
  - [x] Place integration tests in a `mod lru_eviction_tests` inside
        the existing `#[cfg(test)] mod tests` in `engine_impl.rs`
  - [x] Reuse the existing test builder helpers already present in the
        `expand_object_tests` mod (see ~line 1530 in engine_impl.rs)

## Dev Notes

### Architecture Decisions

- **LRU unit / "subtree" clarification**: The architecture says "unit of eviction:
  the full expanded subtree of a navigated object". In this implementation, a
  "subtree" = the `Vec<FieldInfo>` returned by one `expand_object(id)` call.
  Each object in the user's navigation path is cached independently by `object_id`.
  The cache is intentionally flat — there is no parent/child hierarchy to track.
  This is the correct interpretation: each `expand_object` call is atomic and
  independently cacheable. Do NOT implement a tree-structured cache.
  [Source: docs/planning-artifacts/architecture.md#Cache & Eviction: LRU by Subtree]
- **`lru` crate**: Industry-standard Rust LRU cache (`lru = "0.12"`).
  `LruCache::unbounded()` — no count-based capacity; memory managed via budget.
  `get(&mut self)` promotes to MRU. `pop_lru()` evicts LRU.
  Wrap in `Mutex<>` for interior mutability (all `NavigationEngine` methods take
  `&self`).
- **Precompute size on insert**: Store `(Vec<FieldInfo>, usize)`.
  The `usize` is the immutable source of truth computed once on insert via
  `compute_fields_size`. Reusing it on eviction avoids both recomputing over a
  dropping field list and any risk of divergence if `memory_size()` were to change
  between insert and evict.
- **Eviction hystérésis (ADR-2)**: Two constants in `cache/lru.rs`:
  `EVICTION_TRIGGER = 0.80` (trigger eviction) and `EVICTION_TARGET = 0.60`
  (stop evicting). Without the lower target, an object whose size ≥ 20% of the
  budget would be inserted, trigger eviction of itself, re-inserted on next access,
  causing thrashing. The 60% target gives a comfortable margin.
- **`ObjectCache` is ignorant of `MemoryCounter`** (ADR-5): `ObjectCache` only
  manages the LRU data structure. `Engine` orchestrates the eviction loop and
  updates the counter. This avoids a circular dependency (cache → budget, both in
  the `cache` module) and keeps each type with one responsibility.
- **Baseline memory may exceed `EVICTION_TARGET`** (FM-2): `MemoryCounter` tracks
  ALL parsed memory including the structural baseline (`hfile` index + `thread_cache`)
  set at construction. This baseline is NOT in `ObjectCache` and cannot be evicted.
  If baseline alone exceeds `EVICTION_TARGET` (e.g., a 1 GB index on a 2 GB budget),
  the eviction loop will empty the object cache on every `expand_object` call and
  break on empty cache. This is **expected behavior** — the system operates above
  the soft target but remains functional (re-parses from mmap on each access).
- **Self-eviction for large objects** (FM-5): If a single expanded object exceeds
  `(1 - EVICTION_TARGET) * budget` bytes (40% with TARGET=0.60), the eviction loop
  will evict it immediately after each insert. The caller still receives the fields
  (we return the pre-clone original), but subsequent navigations to that object
  will always re-parse from mmap. This is correct behavior for an undersized budget.
- **`subtract` symmetry** (FM-3): `MemoryCounter::subtract` uses unchecked
  `fetch_sub` on `AtomicUsize`. Underflow wraps to `usize::MAX`, causing
  `usage_ratio()` to explode and the eviction loop to drain the entire cache.
  The only safe source for the bytes to subtract is the value returned by
  `evict_lru()` — which is the size stored at insert time. Never call `subtract`
  with any other value.
- **Counter ordering (add before evict)**: `memory_counter.add(mem)` is called
  before the eviction loop. A transient over-budget state is acceptable
  (`Ordering::Relaxed` is already used in `MemoryCounter` — approximate tracking).
  The eviction loop immediately corrects any overshoot.
- **Non-blocking (AC3)**: AC3 is satisfied by architecture, not by benchmark.
  The TUI event loop is single-threaded — eviction runs inline in `expand_object`
  with no concurrent callers. Mutex is held for one `pop_lru` call per loop
  iteration then released. Dropping `Vec<FieldInfo>` involves no I/O or syscalls.
  Do NOT add benchmarks or timeouts for this story — the guarantee is structural.
- **Eviction protection — what is safe by design**:
  - `thread_cache`, `PreciseIndex`, `segment_filters` — Catégorie 1 (structural),
    physically outside `ObjectCache`, never evictable.
  - `get_stack_frames`, `list_threads`, `get_local_variables` — not cached at all,
    always re-parsed from the index. Cannot be evicted.
  - The object currently displayed in the TUI — protected de facto: each TUI render
    calls `expand_object(selected_id)`, promoting it to MRU on every keypress.
  - Previously visited objects in the navigation path — evictable if budget is tight,
    but transparently re-parsed on demand (Story 5.4).
- **Known technical debt — pinning for Story 7.1**: Story 7.1 introduces a
  "favorites panel" where users pin values. Pinned objects must survive eviction —
  otherwise they silently disappear from the favorites panel. Story 5.3 does NOT
  implement pinning (YAGNI). When Story 7.1 is implemented, `ObjectCache` will
  need `pin(id)` / `unpin(id)` methods, and `evict_lru` must skip pinned entries.
  Minimal implementation: a `FxHashSet<u64>` of pinned ids checked in the eviction
  loop before `pop_lru`.
- **`get_page` not cached**: Collection pages are out of scope for 5.3.
  Story 5.4 deals with transparent re-parse stability. YAGNI.
  Consequence: `memory_used()` reflects only the `expand_object` cache, not
  temporary `Vec<EntryInfo>` allocations from `get_page`. Do not interpret
  a stable `memory_used()` during collection pagination as "memory is controlled"
  — those allocations exist but are not tracked until Story 5.4.
- **Clone on cache hit**: Current `NavigationEngine::expand_object` returns
  `Option<Vec<FieldInfo>>` (owned). Cloning from the cache is correct.
  For most objects (tens of fields) the clone cost is negligible.
  `Arc<Vec<FieldInfo>>` would give O(1) clone but requires changing the trait
  return type — a breaking change across the TUI. Candidat Epic 8 if profiling
  identifies this as a hotspot.
- **Carryover from Story 5.1 (Task 6)**: This story completes the wiring of
  `Engine::memory_counter` into `expand_object()` (add) and the eviction path
  (subtract). The counter field, `add/subtract/usage_ratio` methods, and
  `memory_used()` trait method already exist — only the call sites were missing.
  [Source: docs/implementation-artifacts/5-2-memory-budget-auto-calculation-and-override.md#Previous Story Intelligence]

### Key Code Locations

- `crates/hprof-engine/Cargo.toml` — add `lru = "0.12"` to `[dependencies]`
- `crates/hprof-engine/src/cache/mod.rs` — add `pub mod lru; pub use lru::ObjectCache;`
- `crates/hprof-engine/src/cache/lru.rs` — NEW FILE to create
- `crates/hprof-engine/src/engine_impl.rs`:
  - `Engine` struct (~line 264) — add `object_cache` field
  - `Engine::from_file` (~line 281) — init `object_cache`
  - `Engine::from_file_with_progress` (~line 305) — init `object_cache`
  - `impl NavigationEngine for Engine` (~line 591) — rewrite `expand_object` (~line 705)
  - Existing `mod expand_object_tests` (~line 1530) — test helpers to reuse
- `crates/hprof-engine/src/engine.rs` — `FieldInfo::memory_size()` impl (~line 220)
- `crates/hprof-engine/src/cache/budget.rs` — `MemoryCounter::add/subtract/usage_ratio`

### Previous Story Intelligence (5.2)

- `MemoryCounter::new(budget)`, `budget()`, `usage_ratio()`, `add()`, `subtract()`,
  `reset()` are all available and tested in `cache/budget.rs`
- `Engine` holds `memory_counter: Arc<MemoryCounter>` — use
  `self.memory_counter.add(n)` / `self.memory_counter.subtract(n)` /
  `self.memory_counter.usage_ratio()`
- `EngineConfig { budget_bytes: Option<u64> }` — use
  `EngineConfig { budget_bytes: Some(n) }` in tests for deterministic budgets
  (avoids calling `sysinfo` in tests)
- All existing tests already use `EngineConfig::default()` — no changes needed
- `StubEngine` in `hprof-tui/src/app.rs` does NOT need changes (no new trait methods)
- `FxHashMap` is used throughout; `LruCache` uses std hasher internally — acceptable,
  do not try to swap it out
- Commit style: `feat: Story N.M — short description` (no co-author lines)

### Anti-Patterns to Avoid

- **Do NOT use `Arc<Vec<FieldInfo>>`** for cache values — YAGNI, plain clone is
  correct and consistent with the existing API contract
- **Do NOT cache `get_page`** — out of scope; Story 5.4 handles re-parse
- **Do NOT use `RwLock`** — `LruCache::get` takes `&mut self`, so `Mutex` is required
- **Do NOT recompute size on eviction** — size is stored in the tuple at insert time;
  the fields are about to be dropped, computing over them is wasteful and fragile
- **Do NOT add count-based capacity to `LruCache`** — use `unbounded()`, capacity
  is managed via the memory budget
- **Do NOT call `effective_budget()` repeatedly** — it invokes `sysinfo`; the budget
  is already resolved and stored in `MemoryCounter` at engine construction
- **Do NOT evict from `list_threads` / `get_stack_frames`** — the thread cache is
  built once at construction, is static, and is not part of LRU scope
- **Do NOT hold the Mutex across `decode_object_fields`** — lock only for cache
  operations (get, insert, pop_lru), release immediately after. Calling any
  `self.*` method while the ObjectCache Mutex is held risks a future deadlock
  if that method also tries to acquire the same Mutex.
- **Do NOT add `pub` to `object_cache` field on `Engine`** — it is private by
  design. Tests access it directly via `mod tests` in the same file (Rust module
  scoping). Exposing it would leak ObjectCache as a public API surface.
- **Do NOT redefine `EVICTION_TRIGGER`/`EVICTION_TARGET` in `engine_impl.rs`** —
  import them from `crate::cache::lru`; two definitions = two sources of truth
- **Do NOT call `memory_counter.subtract(n)` with any value other than what
  `evict_lru()` returns** — the stored size is the only consistent pair for the
  corresponding `add`. Any other value risks `AtomicUsize` underflow wrap-around.

### Testing Strategy

**Unit tests in `cache/lru.rs`** (place in `#[cfg(test)] mod tests`):
- `cache_miss_returns_none` — empty cache, `get(42)` → `None`
- `cache_hit_returns_same_fields` — `insert` then `get` → same field names/values
- `cache_hit_promotes_to_mru` — insert A then B, call `get(A)` (promotes A),
  call `evict_lru()` twice → B evicted before A
- `evict_lru_on_empty_returns_none` — `evict_lru()` on empty cache → `None`
- `insert_returns_nonzero_size_for_nonempty_fields` — insert a `FieldInfo` with
  a non-empty `name` String → returned size > 0
- `evict_lru_returns_precomputed_size` — `insert` returns size N,
  `evict_lru()` returns `Some(N)` (same value)
- `is_empty_true_on_new_cache` — `ObjectCache::new().is_empty()` → `true`
- `len_reflects_inserted_entries` — insert 2 distinct ids, `len()` → `2`;
  evict one, `len()` → `1`

**Integration tests in `engine_impl.rs`** (in new `mod lru_eviction_tests`
inside the existing `#[cfg(test)] mod tests`):
- Reuse existing test-builder helpers from `mod expand_object_tests` (~line 1530)
  to construct minimal hprof fixtures with instance dumps
- `expand_object_cached_does_not_double_count_memory` — call `expand_object(id)`
  twice, assert `memory_used()` after second call == after first call
- `expand_object_with_tiny_budget_triggers_eviction` — use
  `EngineConfig { budget_bytes: Some(1) }`, call `expand_object` on two distinct
  objects, verify `memory_counter.usage_ratio() < 0.8` after both calls (eviction
  kept ratio in check), and `memory_used() > 0` (last entry is still tracked)
- `eviction_loop_terminates_when_cache_empty` — use a budget so small that
  baseline memory alone exceeds `EVICTION_TARGET`; call `expand_object` and
  verify it returns `Some(fields)` without hanging (loop terminates on empty cache)

### Project Structure Notes

New files:
```
crates/hprof-engine/src/cache/
└── lru.rs    # ObjectCache — LruCache<u64, (Vec<FieldInfo>, usize)>
```

Modified files:
```
crates/hprof-engine/Cargo.toml           # + lru = "0.12"
crates/hprof-engine/src/cache/mod.rs     # + pub mod lru; pub use lru::ObjectCache
crates/hprof-engine/src/engine_impl.rs   # Engine struct + from_file* + expand_object
```

### References

- [Source: docs/planning-artifacts/epics.md#Story 5.3]
- [Source: docs/planning-artifacts/architecture.md#Cache & Eviction: LRU by Subtree]
- [Source: docs/implementation-artifacts/5-2-memory-budget-auto-calculation-and-override.md]
- [Source: crates/hprof-engine/src/cache/budget.rs — MemoryCounter]
- [Source: crates/hprof-engine/src/engine_impl.rs — Engine struct (~264),
  expand_object (~705), expand_object_tests (~1530)]
- [Source: crates/hprof-engine/src/engine.rs — FieldInfo::memory_size (~220)]
- [Source: crates/hprof-engine/src/lib.rs — EngineConfig]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation, no debugging required.

### Completion Notes List

- Task 1: Added `lru = "0.12"` dependency to hprof-engine
- Task 2: Created `ObjectCache` in `cache/lru.rs` — `Mutex<LruCache<u64, (Vec<FieldInfo>, usize)>>` with `new`, `get`, `insert`, `evict_lru`, `is_empty`, `len` methods + `EVICTION_TRIGGER`/`EVICTION_TARGET` constants + `compute_fields_size` helper + 8 unit tests
- Task 3: Exposed `ObjectCache` from `cache/mod.rs` via `pub mod lru` and `pub use`
- Task 4: Added private `object_cache` field to `Engine` struct, initialized in both `from_file` and `from_file_with_progress`
- Task 5: Extracted `decode_object_fields` private helper from `expand_object`, rewrote `expand_object` with cache-hit path, memory tracking via `MemoryCounter`, and hysteresis eviction loop (trigger 80%, target 60%). Constants imported from `cache::lru`, not redefined. Mutex never held across `decode_object_fields`.
- Task 6: Added 5 integration tests in `mod lru_eviction_tests`: cache-hit no double-count (AC5), tiny-budget eviction (AC1), LRU order respected (AC2), AC4 usage below target or cache empty, eviction loop terminates on empty cache (FM-2)
- All 117 tests pass, 0 regressions, clippy clean (no new warnings)

### Change Log

- 2026-03-10: Story 5.3 implementation complete — LRU eviction core with ObjectCache, hysteresis eviction loop, 13 new tests (8 unit + 5 integration)
- 2026-03-10: Code review fixes — remove unused `NonZeroUsize` import, add `Default` impl for `ObjectCache`, add `debug_assert` in `insert` against re-insertion drift, move EVICTION constants import to module level, rewrite `expand_object_lru_order_respected` to assert actual LRU eviction order (AC2). 161 tests pass, 0 new clippy warnings.

### File List

New:
- `crates/hprof-engine/src/cache/lru.rs`

Modified:
- `Cargo.lock`
- `crates/hprof-engine/Cargo.toml`
- `crates/hprof-engine/src/cache/mod.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `docs/implementation-artifacts/sprint-status.yaml`
- `docs/implementation-artifacts/5-3-lru-eviction-core.md`
