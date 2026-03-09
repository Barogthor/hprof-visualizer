# Story 8.1: FxHashMap, Pre-allocation & all_offsets Optimization

Status: done

## Story

As a user,
I want faster heap dump loading through optimized data structures,
so that the indexing phase completes sooner with less memory pressure.

## Acceptance Criteria

### AC1: FxHashMap for integer-keyed maps

**Given** the first pass uses FxHashMap for all integer-keyed maps
**When** I index a heap dump
**Then** indexing time is measurably reduced vs std::HashMap
(verified via Story 8.0 criterion benchmarks).

### AC2: Pre-allocated HashMaps

**Given** the first pass pre-allocates HashMaps based on
`file_size / 80` (instances) and `file_size / 300` (strings)
**When** I index a heap dump
**Then** zero HashMap reallocations occur during heap extraction.

### AC3: Sorted Vec replaces all_offsets HashMap

**Given** `all_offsets` uses a sorted `Vec<(u64, u64)>` instead
of `HashMap<u64, u64>`
**When** `resolve_thread_transitive_offsets` looks up ~600-800
thread-related object offsets
**Then** all lookups succeed via `binary_search_by_key` with
identical results to the previous HashMap-based implementation.

### AC4: ZGC/Shenandoah regression test

**Given** a heap dump with IDs that have common high bits
(ZGC/Shenandoah pattern)
**When** indexed with FxHashMap
**Then** no pathological collision behavior — verified by a
dedicated regression test.

### AC5: All existing tests pass

**Given** all existing tests (359 tests)
**When** I run `cargo test`
**Then** all tests pass with identical results to
pre-optimization behavior.

## Tasks / Subtasks

- [x] Task 1: Add `rustc-hash` dependency (AC: 1)
  - [x] 1.1: Add `rustc-hash = "2"` to workspace
    `[workspace.dependencies]` in root `Cargo.toml`
  - [x] 1.2: Add `rustc-hash = { workspace = true }` to
    `crates/hprof-parser/Cargo.toml` `[dependencies]`

- [x] Task 2: Replace HashMap with FxHashMap in PreciseIndex
  (AC: 1, 5)
  - [x] 2.1: In `precise.rs`, replace
    `use std::collections::HashMap` with
    `use rustc_hash::FxHashMap`
  - [x] 2.2: Replace all 10 field types from
    `HashMap<K, V>` to `FxHashMap<K, V>`:
    - `strings: FxHashMap<u64, HprofString>`
    - `classes: FxHashMap<u32, ClassDef>`
    - `threads: FxHashMap<u32, HprofThread>`
    - `stack_frames: FxHashMap<u64, StackFrame>`
    - `stack_traces: FxHashMap<u32, StackTrace>`
    - `java_frame_roots: FxHashMap<u64, Vec<u64>>`
    - `class_dumps: FxHashMap<u64, ClassDumpInfo>`
    - `thread_object_ids: FxHashMap<u32, u64>`
    - `class_names_by_id: FxHashMap<u64, String>`
    - `instance_offsets: FxHashMap<u64, u64>`
  - [x] 2.3: Update `PreciseIndex::new()` to use
    `FxHashMap::default()` for each field
  - [x] 2.4: Fix all compilation errors across the codebase
    resulting from type changes (engine_impl.rs, resolver.rs,
    first_pass.rs, precise.rs tests)

- [x] Task 3: Pre-allocate maps (AC: 2, 5)
  - [x] 3.1: Add a `data_len: usize` parameter to
    `PreciseIndex::new()` (or create
    `PreciseIndex::with_capacity(data_len: usize)`)
  - [x] 3.2: Pre-allocate:
    - `strings`: `FxHashMap::with_capacity_and_hasher(
      data_len / 300, Default::default())`
    - `classes`: `FxHashMap::with_capacity_and_hasher(
      data_len / 5000, Default::default())`
    - `class_dumps`: same capacity as `classes`
    - `class_names_by_id`: same capacity as `classes`
    - Other maps: keep default (small counts)
  - [x] 3.3: Pass `data.len()` from `run_first_pass` to the
    constructor

- [x] Task 4: Replace all_offsets HashMap with sorted Vec
  (AC: 3, 5)
  - [x] 4.1: In `run_first_pass`, change line 115:
    `let mut all_offsets: HashMap<u64, u64> = HashMap::new();`
    to `let mut all_offsets: Vec<(u64, u64)> = Vec::with_capacity(data.len() / 80);`
  - [x] 4.2: In `extract_heap_object_ids`, change parameter
    type from `&mut HashMap<u64, u64>` to
    `&mut Vec<(u64, u64)>`
  - [x] 4.3: Replace `all_offsets.insert(obj_id, offset)`
    with `all_offsets.push((obj_id, offset))`
    at lines 764 and 801
  - [x] 4.4: After all `extract_heap_object_ids` calls
    complete (after the main while loop, before thread cache
    build), add:
    `all_offsets.sort_unstable_by_key(|&(id, _)| id);`
  - [x] 4.5: Replace all `all_offsets.get(&id)` lookups with
    binary search helper:
    ```rust
    fn lookup_offset(
        sorted: &[(u64, u64)],
        id: u64,
    ) -> Option<u64> {
        sorted
            .binary_search_by_key(&id, |&(k, _)| k)
            .ok()
            .map(|i| sorted[i].1)
    }
    ```
  - [x] 4.6: Update `resolve_thread_transitive_offsets`
    parameter from `&HashMap<u64, u64>` to `&[(u64, u64)]`
    and use `lookup_offset` for all lookups (lines 943, 959)
  - [x] 4.7: Update the cross-reference loop (line 506-510)
    to use `lookup_offset` instead of `all_offsets.get`

- [x] Task 5: Add ZGC/Shenandoah regression test (AC: 4)
  - [x] 5.1: Add a test in `first_pass.rs::tests` that
    creates IDs with common high bits
    (e.g., `0xFFFF_0000_0000_0001`, `0xFFFF_0000_0000_0002`,
    etc.) and verifies FxHashMap handles them without
    pathological collision
  - [x] 5.2: Verify all such IDs are retrievable after
    insertion (round-trip correctness)

- [x] Task 6: Verify all tests pass + benchmarks (AC: 5)
  - [x] 6.1: `cargo test` — all 363 tests pass (4 new)
  - [x] 6.2: `cargo clippy` — no warnings
  - [x] 6.3: `cargo fmt -- --check` — clean
  - [x] 6.4: If `HPROF_BENCH_FILE` is set, run
    `cargo bench --bench first_pass` and compare to
    pre-optimization baseline

## Dev Notes

### Architecture Compliance

- **Crate boundary:** All changes in `hprof-parser`. The engine
  crate (`hprof-engine`) consumes `PreciseIndex` fields but does
  not need `rustc-hash` as a direct dependency — it accesses
  maps via standard trait methods (`.get()`, `.values()`,
  `.iter()`, `.contains_key()`) which are identical between
  `HashMap` and `FxHashMap`.
- **Dependency direction:**
  `hprof-cli` -> `hprof-engine` -> `hprof-parser` (unchanged).
- **No `println!`** — only tracing macros (if needed), gated
  behind `dev-profiling` feature flag.
- **Error propagation:** Use `?` operator, never `unwrap()` in
  production code. The `lookup_offset` helper returns `Option`,
  callers use `if let Some(...)`.

### all_offsets: Why Vec Instead of Suppression

The party-mode pre-mortem analysis (Scenario #1) identified that
completely suppressing `all_offsets` and falling back to segment
filter lookups would be **slower**: ~800 thread-related lookups
x ~2 MB scan each = 1.6 GB of I/O. The sorted Vec is the safe
middle ground:

| Approach | RAM | Lookup | Cache Behavior |
|----------|-----|--------|----------------|
| HashMap (current) | ~120 MB | O(1) amortized | Poor (random) |
| Sorted Vec | ~80 MB | O(log n) | Excellent (sequential) |
| Suppressed (filters) | 0 MB | O(segment_size) | Terrible |

[Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Pre-mortem Analysis]

### FxHashMap: Why Not AHashMap or IntMap

Algorithm Olympics (Method 22) compared 4 hasher options:

- **FxHashMap** (winner): `rustc-hash` v2, multiply-xor hash,
  fastest for small integer keys. Used by rustc itself.
- AHashMap: Good general-purpose but slower for pure integer
  keys (AES-NI overhead).
- IntMap: Identity hash — catastrophic on non-uniform IDs
  (ZGC/Shenandoah use tagged pointers with common high bits).
- BTreeMap: O(log n) — no advantage over sorted Vec for the
  all_offsets use case, higher constant factor.

**IMPORTANT:** Keep `std::HashMap` for any string-keyed maps
(FxHash is poor on long strings). Currently no string-keyed
maps exist in PreciseIndex — all keys are `u64` or `u32`.

[Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Algorithm Olympics]

### Pre-allocation Strategy

| Map | Estimate Formula | Rationale |
|-----|-----------------|-----------|
| `strings` | `data_len / 300` | ~135K strings in 41 MB dump |
| `classes` / `class_dumps` / `class_names_by_id` | `data_len / 5000` | ~7K classes in 41 MB dump |
| `all_offsets` Vec | `data_len / 80` | ~5M instances in large dumps |
| Others (`threads`, `stack_frames`, `stack_traces`) | default | Typically < 200 entries |

Over-estimating capacity by 20-30% is acceptable — unused
capacity costs far less than rehashing millions of entries.

### Sorted Vec Implementation Details

1. **Build phase:** `all_offsets.push((obj_id, offset))` during
   heap extraction — O(1) per insert, no hash computation.
2. **Sort phase:** `all_offsets.sort_unstable_by_key(|&(id, _)| id)`
   after ALL `extract_heap_object_ids` calls complete.
   `sort_unstable` is preferred (no allocation, ~200ms for 5M
   entries).
3. **Lookup phase:** `binary_search_by_key(&id, |&(k, _)| k)`
   returns `Result<usize, usize>` — O(log n) per lookup.
4. **Dedup is NOT needed:** Each object ID appears exactly once
   in hprof format (INSTANCE_DUMP + PRIMITIVE_ARRAY only).
   If duplicate IDs exist in a corrupted dump, `binary_search`
   will find one of them, which is correct behavior.

### Key Code Locations

| File | What Changes |
|------|-------------|
| `Cargo.toml` (workspace root) | Add `rustc-hash = "2"` to `[workspace.dependencies]` |
| `crates/hprof-parser/Cargo.toml` | Add `rustc-hash = { workspace = true }` |
| `crates/hprof-parser/src/indexer/precise.rs` | Replace all `HashMap` with `FxHashMap`, add pre-allocation |
| `crates/hprof-parser/src/indexer/first_pass.rs` | Replace `all_offsets` HashMap with Vec, add sort + binary_search, update `extract_heap_object_ids` and `resolve_thread_transitive_offsets` signatures |
| `crates/hprof-parser/src/indexer/mod.rs` | May need to re-export `FxHashMap` if engine uses the type directly |

### Engine Crate Impact (Compile Fix)

Two files in `hprof-engine` access `PreciseIndex` fields:

- `crates/hprof-engine/src/engine_impl.rs` — reads
  `index.threads`, `index.stack_traces`, `index.stack_frames`,
  `index.strings`, `index.class_names_by_id`,
  `index.instance_offsets`, `index.java_frame_roots`
- `crates/hprof-engine/src/resolver.rs` — reads
  `index.strings`, `index.class_dumps`

These use `.get()`, `.values()`, `.iter()`, `.contains_key()` —
all of which are identical between `HashMap` and `FxHashMap`.
**No code changes needed in engine** — only type inference will
change. However, `hprof-engine` may need `rustc-hash` as a
dependency if it declares any variable with explicit
`FxHashMap<K, V>` type annotation. Check compilation and add
dependency if needed.

### Previous Story Intelligence (Story 8.0)

**Learnings from 8.0:**
- `run_first_pass` was made `pub` and re-exported from
  `lib.rs` for benchmark access.
- `SegmentFilterBuilder`, `SegmentFilter`, `IndexResult` were
  made public.
- Feature flag convention: `dep:` syntax for optional
  dependencies.
- `#[cfg(feature = "dev-profiling")]` tracing spans are already
  in place — do NOT disturb them.
- `Default` derive was added to `SegmentFilterBuilder` for
  clippy compliance.
- 359 tests at end of Story 8.0 (baseline).

**Files modified in 8.0 that overlap with 8.1:**
- `crates/hprof-parser/src/indexer/first_pass.rs` (tracing
  spans added — preserve them)
- `crates/hprof-parser/src/indexer/mod.rs` (visibility changes)
- `crates/hprof-parser/Cargo.toml` (criterion + tracing deps)

### Anti-Patterns to Avoid

- Do NOT replace the `HashSet<u64>` in `extract_obj_refs`
  (line 869) with `FxHashSet` — it has < 10 entries per call
  (class hierarchy depth) and is not a bottleneck.
- Do NOT sort `all_offsets` inside `extract_heap_object_ids` —
  sort ONCE after ALL heap segments are processed.
- Do NOT dedup `all_offsets` — IDs are unique per hprof spec.
- Do NOT add `rustc-hash` to `hprof-engine` or `hprof-cli`
  unless compilation requires it.
- Do NOT change the `HprofString` struct — that is Story 8.2.
- Do NOT modify segment filter logic — that stays in 8.3.
- Do NOT disturb existing `#[cfg(feature = "dev-profiling")]`
  tracing spans.
- Do NOT use `FxHashMap` for any string-keyed maps (none
  currently exist, but guard against future additions).

### Testing Strategy

1. **All 359 existing tests** must pass unchanged — the
   FxHashMap and sorted Vec are drop-in replacements with
   identical semantics.
2. **New test: ZGC/Shenandoah regression** — create IDs with
   common high 32 bits (e.g., `0xFFFF_0000_xxxx_xxxx`),
   insert into FxHashMap, verify all retrievable.
3. **New test: sorted Vec binary search correctness** — build
   a Vec of (id, offset) pairs, sort, verify `lookup_offset`
   finds all inserted entries and returns `None` for missing.
4. **Benchmark validation** (manual, if `HPROF_BENCH_FILE`
   set): Run `cargo bench --bench first_pass` before and after
   to measure improvement.

### Project Structure Notes

- All new code stays within existing file structure — no new
  files or modules needed.
- `lookup_offset` helper can be a private function in
  `first_pass.rs` (used only there).
- Workspace dependency pattern follows existing convention
  (workspace-level declaration + per-crate `{ workspace = true }`).

### References

- [Source: docs/planning-artifacts/epics.md#Story 8.1]
- [Source: docs/planning-artifacts/architecture.md#Data Architecture]
- [Source: docs/planning-artifacts/architecture.md#Implementation Patterns]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Story 8.1]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Algorithm Olympics]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Pre-mortem Analysis]
- [Source: docs/implementation-artifacts/8-0-profiling-infrastructure.md#Dev Notes]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None required.

### Completion Notes List

- Replaced all 10 `HashMap` fields in `PreciseIndex` with `FxHashMap` (rustc-hash v2.1.1)
- Added `PreciseIndex::with_capacity(data_len)` for pre-allocation based on file size heuristics
- Replaced `all_offsets: HashMap<u64,u64>` with `Vec<(u64,u64)>` + sort + `binary_search_by_key`
- Added `lookup_offset()` private helper in `first_pass.rs`
- No changes needed in `hprof-engine` — FxHashMap API is identical to HashMap for `.get()`, `.values()`, `.iter()`, `.contains_key()`
- Added 4 new tests: ZGC/Shenandoah integration (50 high-bit IDs through run_first_pass), sorted Vec lookup correctness (1000 entries), missing ID returns None, empty Vec returns None
- All 363 tests pass, clippy clean, fmt clean
- Code review fixes: capped pre-allocation to avoid multi-GB reservations on very large dumps, replaced standalone ZGC test with integration test through `run_first_pass`, documented `sub_record_start - 1` offset semantics

### Benchmark Results (criterion, 10 samples)

| Dump | Size | Before (8.0) | After (8.1) | Delta |
|------|------|-------------|-------------|-------|
| visualvm | 41 MB | 63.2 ms | 42.6 ms | **-32%** |
| rustrover | ~1.4 GB | 3.01 s | 1.74 s | **-42%** |

### End-to-End (full workflow, rustrover dump)

| Metric | Before (8.0) | After (8.1) | After review fixes | Delta |
|--------|-------------|-------------|-------------------|-------|
| Total load time | ~29-30 s | ~24.3 s | ~24.3 s | **-19%** |
| Peak RSS (preparation) | — | ~500 MB | ~430 MB | **-14%** |
| RSS after load | — | ~350 MB | ~250 MB | **-29%** |

### Change Log

- 2026-03-08: Story 8.1 implemented — FxHashMap, pre-allocation, sorted Vec for all_offsets
- 2026-03-08: Code review fixes (Claude + Codex) — capped pre-alloc, ZGC integration test, fmt fix, offset comment

### File List

- `Cargo.lock` — updated (auto-generated)
- `Cargo.toml` (workspace root) — added `rustc-hash = "2"`
- `crates/hprof-cli/src/main.rs` — reformatted (cosmetic)
- `crates/hprof-parser/Cargo.toml` — added `rustc-hash = { workspace = true }`
- `crates/hprof-parser/benches/first_pass.rs` — path resolution fix, criterion config (10 samples, 10s)
- `crates/hprof-parser/src/indexer/first_pass.rs` — sorted Vec for all_offsets, `lookup_offset()`, capped pre-alloc, offset comment, 4 new tests
- `crates/hprof-parser/src/indexer/precise.rs` — FxHashMap fields, `with_capacity()` with caps
- `crates/hprof-parser/src/indexer/segment.rs` — reformatted (cosmetic)
- `docs/implementation-artifacts/sprint-status.yaml` — status updated
- `docs/implementation-artifacts/8-1-fxhashmap-pre-allocation-and-all-offsets-optimization.md` — story file updated
