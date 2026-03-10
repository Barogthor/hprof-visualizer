# Story 5.1: MemorySize Trait & Budget Tracking

Status: done

## Story

As a developer,
I want a `MemorySize` trait implemented by all parsed structures
that reports their estimated heap footprint, and a global counter
that tracks total memory usage,
so that the system always knows how much memory is consumed by
parsed data and can make eviction decisions.

## Acceptance Criteria

### AC1: MemorySize trait definition

**Given** a parsed domain type (HprofThread, StackFrame, ClassDef,
ClassDumpInfo, StackTrace, RawInstance, ThreadMetadata,
FieldInfo, FieldValue, EntryInfo, CollectionPage, etc.)
**When** I call `.memory_size()` on it
**Then** it returns `std::mem::size_of::<Self>()` for the static
part plus manual counting of heap allocations:
- `Vec<T>`: `capacity() * size_of::<T>()`
- `String`: `capacity()`
- `FxHashMap<K,V>`: bucket overhead + per-entry sizes
- Nested types: recursive `.memory_size()` calls

### AC2: Global memory counter increments on parse

**Given** an object is parsed and added to a cache
**When** the global counter is updated
**Then** it increments by exactly the value reported by
`memory_size()`

### AC3: Global memory counter decrements on eviction

**Given** an object is evicted from the cache
**When** the global counter is updated
**Then** it decrements by exactly the value reported by
`memory_size()`

### AC4: Unit tests verify coherence

**Given** key domain structs with known allocations
**When** unit tests run
**Then** they verify coherence between reported `memory_size()`
and actual allocations for all key types — specifically:
- Fixed-size structs return exactly `size_of::<Self>()`
- Vec-bearing structs account for capacity
- String-bearing structs account for string capacity
- Nested structs recurse correctly
- FxHashMap-bearing structs include bucket + entry overhead

### AC5: PreciseIndex total size

**Given** a `PreciseIndex` with populated maps
**When** I call `.memory_size()` on it
**Then** it returns the sum of all 10 FxHashMap sizes plus the
static struct size — this is the dominant memory consumer at
indexing time

## Tasks / Subtasks

- [x] Task 1: Define `MemorySize` trait in hprof-api (AC: 1)
  - [x] Create `crates/hprof-api/src/memory_size.rs`
  - [x] Define trait: `fn memory_size(&self) -> usize`
  - [x] Add helper function for FxHashMap sizing
  - [x] Register module in `crates/hprof-api/src/lib.rs`
  - [x] Re-export from `hprof-api` public API

- [x] Task 2: Implement MemorySize for hprof-parser types (AC: 1, 4)
  - [x] `HprofThread` — fixed size only (no heap allocs)
  - [x] `StackFrame` — fixed size only
  - [x] `ClassDef` — fixed size only
  - [x] `StackTrace` — static + `frame_ids: Vec<u64>`
  - [x] `ClassDumpInfo` — static + `instance_fields: Vec<FieldDef>`
  - [x] `FieldDef` — fixed size only
  - [x] `RawInstance` — static + `data: Vec<u8>`
  - [x] `HprofStringRef` — fixed size only (lazy ref, no string)
  - [x] Write unit tests for each impl

- [x] Task 3: Implement MemorySize for PreciseIndex (AC: 1, 4, 5)
  - [x] Implement for `PreciseIndex` — sum all 10 FxHashMaps
  - [x] Helper: `fxhashmap_memory_size<K,V>()` using
        `capacity() * (size_of::<K>() + size_of::<V>() + 8)`
        for bucket overhead
  - [x] Special case: `class_names_by_id` — values are Strings,
        recurse into each
  - [x] Special case: `java_frame_roots` — values are `Vec<u64>`,
        recurse into each
  - [x] Special case: `stack_traces` — values contain `Vec<u64>`
  - [x] Special case: `class_dumps` — values contain `Vec<FieldDef>`
  - [x] Write unit test: build a small index, verify total > sum
        of static sizes

- [x] Task 4: Implement MemorySize for hprof-engine types (AC: 1, 4)
  - [x] `ThreadMetadata` — static + `name: String`
  - [x] `ThreadInfo` — static + `name: String`
  - [x] `FrameInfo` — static + 3 Strings
  - [x] `FieldInfo` — static + `name: String` + `value: FieldValue`
  - [x] `FieldValue` (enum) — match on variant, account for
        Strings in `ObjectRef`
  - [x] `VariableInfo` — static + `VariableValue`
  - [x] `VariableValue` (enum) — match on variant
  - [x] `EntryInfo` — static + key/value FieldValues
  - [x] `CollectionPage` — static + `entries: Vec<EntryInfo>`
  - [x] Write unit tests for each impl

- [x] Task 5: Global memory counter (AC: 2, 3)
  - [x] Create `crates/hprof-engine/src/cache/budget.rs`
  - [x] `MemoryCounter` struct with `AtomicUsize` for thread-safe
        tracking
  - [x] `fn add(&self, bytes: usize)` — atomic increment
  - [x] `fn subtract(&self, bytes: usize)` — atomic decrement
  - [x] `fn current(&self) -> usize` — atomic load
  - [x] `fn reset(&self)` — for testing
  - [x] Write unit tests: add/subtract/current/concurrent access

- [x] Task 6: Integrate counter with Engine (AC: 2, 3)
  - [x] Add `memory_counter: Arc<MemoryCounter>` to `Engine`
  - [x] On `Engine::from_file()`: compute and record
        `PreciseIndex.memory_size()` + thread_cache size
  - [x] Expose `fn memory_used(&self) -> usize` on
        `NavigationEngine` trait
  - [ ] Wire into existing expand/collapse paths (deferred to 5.3 — requires LRU cache before counter wiring is meaningful)
  - [x] Write integration test: create Engine from test fixture,
        verify `memory_used() > 0`

## Dev Notes

### Architecture Decisions

- **LRU algorithm**: Use `lru` crate directly, no trait
  abstraction for eviction policy (Party Mode decision 2026-03-10
  — YAGNI, trivial to extract later if needed)
- **MemorySize trait lives in hprof-api** — the shared
  cross-cutting traits crate, alongside `ParseProgressObserver`.
  This avoids orphan rule issues: both `hprof-parser` and
  `hprof-engine` depend on `hprof-api` and can implement the
  trait for their own types directly.
- **Counter is atomic** — `AtomicUsize` with `Ordering::Relaxed`
  is sufficient (no strict ordering needed between threads,
  approximate budget tracking is fine)
- **Mmap is NOT counted** in the memory budget — it's OS-managed
  virtual memory, not heap. Only parsed/decoded data counts.
- **PreciseIndex is tracked but NOT evictable** — it stays in
  RAM permanently. Only expanded objects/pages will be evictable
  (Story 5.3).

### FxHashMap Memory Estimation

FxHashMap uses Robin Hood hashing with power-of-2 capacity.
Approximate formula per map:
```
capacity * (size_of::<K>() + size_of::<V>() + 8_ctrl_bytes)
```
Where `capacity` = `raw_table().capacity()` (not `.len()`).
The `hashbrown` crate (backing FxHashMap) exposes capacity
via `.capacity()` method.

### Key Struct Inventory

**Fixed-size (trivial impl — return `size_of`):**
- `HprofThread` (32B), `StackFrame` (40B), `ClassDef` (24B),
  `FieldDef` (12B), `HprofStringRef` (20B)

**Vec-bearing (static + capacity * element_size):**
- `StackTrace` (Vec<u64>), `ClassDumpInfo` (Vec<FieldDef>),
  `RawInstance` (Vec<u8>), `CollectionPage` (Vec<EntryInfo>)

**String-bearing (static + string capacity):**
- `ThreadMetadata`, `ThreadInfo`, `FrameInfo` (3 strings),
  `FieldInfo`, `FieldValue::ObjectRef` (class_name +
  inline_value)

**Map-bearing (dominant consumer):**
- `PreciseIndex` — 10 FxHashMaps, largest is `strings`
  (~135k entries on 41 MB dump)

### Anti-Patterns to Avoid

- Do NOT use `std::mem::size_of_val()` — it only returns
  stack size for smart pointers, not heap contents
- Do NOT count `Arc<HprofFile>` contents in the counter —
  the mmap and index are counted separately at construction
- Do NOT use `.len()` for Vec/HashMap — use `.capacity()` to
  account for allocated-but-unused slots
- Do NOT make MemorySize a derive macro — manual impls are
  needed for accuracy with enums and special types

### Previous Epic Learnings (Epic 4 Retro)

- Async pattern `pending_X` / `poll_X` is proven reusable
  (expansions, strings, pages) — 5.3 can follow same pattern
  for eviction
- Zero Round 2 reviews streak — maintain by being thorough
  in TDD
- Manual validation on real dump before done — non-negotiable
  team agreement (3 consecutive retros)
- All extractors must have tests at delivery (no test-free code)

### Testing Strategy

- **Unit tests per impl**: Create struct with known allocations,
  assert `memory_size()` returns expected value
- **PreciseIndex test**: Build small index with a few entries
  per map, verify total is reasonable (> static, < absurd)
- **MemoryCounter test**: Concurrent add/subtract with threads,
  verify final consistency
- **Integration test**: `Engine::from_file()` on test fixture,
  assert `memory_used()` > 0 and < file_size * 10

### Project Structure Notes

New files to create:
```
crates/hprof-api/src/
└── memory_size.rs   # MemorySize trait definition + helpers

crates/hprof-engine/src/cache/
├── mod.rs           # pub mod budget;
└── budget.rs        # MemoryCounter (AtomicUsize wrapper)
```

- Trait in `hprof-api` — impls in each crate for its own types
- `hprof-parser/src/types.rs` — impl MemorySize for parser types
- `hprof-parser/src/indexer/precise.rs` — impl for PreciseIndex
- `hprof-engine/src/engine.rs` — impl for engine API types
- `hprof-engine/src/engine_impl.rs` — impl for ThreadMetadata

### References

- [Source: docs/planning-artifacts/epics.md#Epic 5, Story 5.1]
- [Source: docs/planning-artifacts/architecture.md — cache module
  structure, MemorySize trait conventions]
- [Source: docs/implementation-artifacts/epic-4-retro-2026-03-10.md
  — action items, team agreements]
- [Source: crates/hprof-parser/src/types.rs — all parsed structs]
- [Source: crates/hprof-parser/src/indexer/precise.rs —
  PreciseIndex definition]
- [Source: crates/hprof-engine/src/engine_impl.rs — Engine,
  ThreadMetadata, expand_object]
- [Source: crates/hprof-engine/src/engine.rs — FieldInfo,
  FieldValue, EntryInfo, CollectionPage, NavigationEngine trait]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation, no debug issues.

### Completion Notes List

- Task 1: Created `MemorySize` trait + `fxhashmap_memory_size` helper in `hprof-api`. 4 unit tests.
- Task 2: Implemented `MemorySize` for all 8 hprof-parser types (ClassDef, HprofThread, StackFrame, StackTrace, FieldDef, ClassDumpInfo, RawInstance, HprofStringRef). Fixed-size types return `size_of::<Self>()`, Vec/String types add capacity. 8 unit tests.
- Task 3: Implemented `MemorySize` for `PreciseIndex` — sums all 10 FxHashMaps with special-case recursion for `class_names_by_id` (String values), `java_frame_roots` (Vec values), `stack_traces` (Vec<u64> in values), `class_dumps` (Vec<FieldDef> in values). 3 unit tests.
- Task 4: Implemented `MemorySize` for all 9 engine types (ThreadInfo, FrameInfo, FieldValue, FieldInfo, VariableValue, VariableInfo, EntryInfo, CollectionPage, ThreadMetadata). 10 unit tests.
- Task 5: Created `MemoryCounter` with `AtomicUsize` (Ordering::Relaxed). Methods: add/subtract/current/reset. 6 unit tests including concurrent thread safety test.
- Task 6: Integrated counter into Engine construction — `initial_memory()` computes PreciseIndex + thread_cache size. Added `memory_used()` to NavigationEngine trait. 2 integration tests.

### File List

- `crates/hprof-api/src/memory_size.rs` (new)
- `crates/hprof-api/src/lib.rs` (modified)
- `crates/hprof-parser/src/types.rs` (modified)
- `crates/hprof-parser/src/strings.rs` (modified)
- `crates/hprof-parser/src/indexer/precise.rs` (modified)
- `crates/hprof-engine/Cargo.toml` (modified — rustc-hash dependency)
- `crates/hprof-engine/src/cache/mod.rs` (new)
- `crates/hprof-engine/src/cache/budget.rs` (new)
- `crates/hprof-engine/src/engine.rs` (modified)
- `crates/hprof-engine/src/engine_impl.rs` (modified)
- `crates/hprof-engine/src/lib.rs` (modified)
- `crates/hprof-tui/src/app.rs` (modified — StubEngine)
- `docs/implementation-artifacts/sprint-status.yaml` (modified)
- `docs/implementation-artifacts/5-1-memorysize-trait-and-budget-tracking.md` (modified)

### Change Log

- 2026-03-10: Story 5.1 implemented — MemorySize trait, impls for all domain types, MemoryCounter, Engine integration
- 2026-03-10: Code review fixes (Amelia CR) — DRY fix for FieldInfo/VariableInfo memory_size (M1);
  thread_cache changed to FxHashMap + rustc-hash dep added (M2);
  EntryInfo heap-path tests added (M3); integration test upper bounds added (H1);
  exact-value counter test added (H2); Task 6 expand-path subtask un-checked as
  deferred to 5.3 (C1)
