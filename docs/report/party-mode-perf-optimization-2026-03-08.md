# Party Mode Session — Performance Optimization Brainstorm

**Date:** 2026-03-08
**Participants:** Winston (Architect), Amelia (Dev), Quinn (QA), Mary (Analyst), John (PM), Sally (UX), Bob (SM)
**Context:** RustRover heap dump loads in ~30s (after previous 2x speedup). Target: 10-15s.

## Current State

- File I/O: mmap (memmap2) — good
- First pass: sequential single-threaded scan of all records
- Heap extraction: inline during first pass, HashMap inserts per object
- Segment filters: BinaryFuse8 built inline during first pass
- Thread cache: rayon parallel post-processing
- String parsing: `from_utf8_lossy()` per string, 135K+ allocations

## Epic 8: First Pass Performance Optimization — Final Design

### Story 8.0: Profiling Infrastructure
- Criterion benchmarks gated behind `HPROF_BENCH_FILE` env var
- `tracing-chrome` behind feature flag `dev-profiling` → `trace.json` → Perfetto UI
- Bench per component: first pass total, string parsing, heap extraction, segment filter build
- `perf stat` for cache miss measurement (Story 8.1 validation)
- No Grafana needed — tracing-chrome + Perfetto = same drill-down UX, zero infra

### Story 8.1: FxHashMap, Pre-allocation & all_offsets Optimization
- **FxHashMap** (rustc-hash v2) for all integer-keyed maps — winner of Algorithm Olympics vs AHashMap, IntMap, BTreeMap
- Pre-allocate with `file_size / 80` for instance data, `file_size / 300` for strings
- Replace `all_offsets: HashMap<u64, u64>` with **sorted `Vec<(u64, u64)>`** + `binary_search_by_key()`
  - HashMap was 120 MB for 5M entries, Vec sorted is ~80 MB, cache-friendly
  - Pre-mortem found: suppressing all_offsets entirely risks slower lookups via segment filters (800 lookups × 2 MB scan each = 1.6 GB)
  - Vec sorted is the safe middle ground: less RAM, O(log n) lookup, compatible with `resolve_thread_transitive_offsets`
- Keep `std::HashMap` for string-keyed maps (FxHash poor on long strings)
- Regression test with ZGC/Shenandoah-style IDs (common high bits) to detect FxHash collision pathology

### Story 8.2: Lazy String References
- `HprofString { id, value: String }` → `HprofStringRef { id, offset: u64, len: u32 }`
- Resolve on-demand via mmap with `from_utf8_lossy`
- **class_names_by_id stays eager** — it's the natural cache for class/method names in UI
- **No LRU cache** for strings — Red Team proved it's over-engineering (20 KB saved for added complexity)
- 4 production call sites to adapt:
  - `first_pass.rs:254` — `s.value.replace('/', ".")` for class names
  - `first_pass.rs:279` — same pattern
  - `first_pass.rs:872` — `s.value.as_str()` for field name lookup
  - `resolver.rs:32` — `s.value.clone()` for field name resolution
- String records (tag 0x01) appear before heap segments → lazy resolve works during first pass
- Estimated gain: -10-15% time, -7 MB RAM

### Story 8.3: Parallel Heap Segment Parsing
- Parallelize by **HEAP_DUMP_SEGMENT** (tag 0x1C) — natural chunk boundaries in hprof format
  - Algorithm Olympics winner vs sub-record index approach (simpler) and DashMap (lock contention)
- **Two sub-passes:**
  1. Sequential: extract CLASS_DUMP (tag 0x20) only + note sub-record offsets — Red Team identified CLASS_DUMP shared state as blocker for pure parallel
  2. Parallel: `heap_record_ranges.par_iter()` with per-worker local Vecs, `class_dumps` as read-only shared state
- **Merge strategy:** Vec `append` (O(1)) + single `sort_unstable` — no HashMap merge contention (Pre-mortem scenario 6)
- **Segment filter coherence:** collect IDs per 64 MiB segment boundary, build BinaryFuse8 after merge
- **Seuil minimum:** 32 MB total heap size to activate parallelism (Pre-mortem scenario 7 — rayon overhead on small dumps)
- Sub-divide segments > 16 MB at sub-record boundaries for finer work-stealing
- Estimated gain: 3-4x on 8 cores

## Elicitation Sessions Summary

### Algorithm Olympics (Method 22)
**Decisions made:**
- FxHashMap > AHashMap > IntMap (identity hash bad on non-uniform IDs) > BTreeMap
- HprofStringRef > Cow (lifetime complications) > Intern pool (overhead) > Hybrid eager/lazy (complexity)
- Parallel by HEAP_DUMP_SEGMENT > sub-record index + par_chunks (more complex) > DashMap (contention)
- Pre-alloc `file_size / 80` > `file_size / 120` (under-estimates) > two-pass count (defeats purpose)

### Performance Profiler Panel (Method 24)
**Key discovery:** `all_offsets: HashMap<u64, u64>` is the real bottleneck
- Stores offset for EVERY instance/primitive array (5M+ entries for RustRover dump)
- Only used to extract ~200 thread-related offsets → 0.004% utilization ratio
- HashMap insert dominates at 60-80% of per-iteration cost (cache misses ~100-200ns each)
- ~120 MB RAM + ~1-2s time just for this temporary map
- Profiling breakdown (estimated for 30s total):
  - all_offsets HashMap: ~1-2s + 120 MB RAM
  - String allocations (135K+): ~2-4s
  - Segment filter build: ~1-2s
  - Heap sub-record parsing (sequential): ~15-20s
  - HashMap inserts (permanent maps): ~0.5-1s
  - Thread cache build (rayon): ~2-3s

### Pre-mortem Analysis (Method 34)
**7 failure scenarios identified with preventions:**
1. **Second pass via segment filters slower than all_offsets** (HIGH prob) → Vec sorted instead of suppression
2. **FxHash collisions on ZGC/Shenandoah IDs** (LOW) → regression test with pathological patterns
3. **Blast radius lazy strings** (LOW) → 4 call sites, manageable
4. **Lazy string resolve during first pass** (MEDIUM) → strings appear before heap segments, ordering safe
5. **Segment filters incoherent in parallel** (MEDIUM) → merge ID lists per 64 MiB segment before BinaryFuse8 build
6. **Merge cost annuls parallel gain** (MEDIUM) → Vec concat instead of HashMap merge
7. **Rayon overhead on small dumps** (CERTAIN) → 32 MB minimum threshold

### Red Team vs Blue Team (Method 17)
**Adjustments from adversarial analysis:**
- Vec sorted confirmed as right tradeoff (Red: sort cost ~200ms but cache-friendly vs HashMap cache misses)
- Inline resolution during scan rejected (Blue: impossible in single pass — transitive refs may be forward)
- **LRU cache for strings REMOVED** — over-engineering, class_names_by_id is the natural cache
- CLASS_DUMP sequential pre-pass added to Story 8.3 (Red identified shared mutable state blocker)
- Sub-divide segments > 16 MB confirmed for load balancing
- 32 MB threshold confirmed for parallelism activation

## Deferred Pistes

### Piste 1: Lazy Two-Phase Loading (DEFERRED)
- Phase 1: scan only top-level records, skip heap segments → UI interactive in ~2-3s
- Phase 2: parse heap in background
- **User feedback:** Degraded view while waiting is a blocker. Could revisit as hybrid with piste 4 for very large dumps (>2 GB).

### Piste 5: read() chunks vs mmap on WSL2 (NOT PRIORITIZED)
- WSL2 mmap on `/mnt/` paths has degraded performance (9P filesystem)
- Quick benchmark: `cp dump.hprof ~/dump.hprof` and compare times
- Hybrid possible: BufReader for first pass, mmap for interactive
- **Status:** To validate with benchmark

### Piste 6: Eliminate Superfluous Work (COMBINABLE with 8.3)
- Defer segment filter build to post-pass instead of inline
- Each micro-op removed from hot loop × 5-10M iterations = significant

## Dev Profiling Tooling (Party Mode Discussion)

User initially asked about "Grafana-style" tools — clarified as dev profiling, not end-user feature.

**Recommended toolkit:**
| Tool | Purpose | Setup |
|---|---|---|
| `cargo flamegraph` | CPU hotspots | `cargo install flamegraph` (needs perf on WSL2) |
| criterion benchmarks | Reproducible before/after comparison | Story 8.0 |
| `tracing-chrome` + Perfetto | Timeline visualization of phases | Feature flag `dev-profiling` in Story 8.0 |
| `perf stat` | Cache misses, branch misses, IPC | CLI, no setup |
| DHAT (valgrind) | Allocation profiling | `cargo install dhat` |

**Decision:** No Grafana needed. `tracing-chrome` export → Perfetto UI gives same interactive drill-down experience with zero infrastructure.

## Target

30s → 10-15s for RustRover heap dump (~50-60% reduction).

## Epics 4-7 Impact on Epic 8

**Pending analysis:** Epics 4-7 may modify engine internals (pagination, LRU, config). Epic 8 touches first_pass and precise index — mostly orthogonal but should be validated before implementation order decision.
