# Hprof-Visualizer: Parsing & Preparation Phases

## Overview

This document describes the complete loading pipeline from file open to TUI display,
identifies current limitations, and documents applied optimizations.

---

## Pipeline Summary

```
File open
  |
  v
[Phase 0] CLI Setup & Progress Reporters
  |
  v
[Phase 1] Header Parsing (mmap + version/id_size/timestamp)
  |
  v
[Phase 2] First-Pass Sequential Scan + Inline Filter Build
  |  - Indexes: strings, classes, stack_frames, stack_traces, threads
  |  - Collects: heap segment offsets, GC roots, ROOT_THREAD_OBJ
  |  - Synthesizes threads from STACK_TRACE if no START_THREAD
  |  - Correlates frame roots
  |  - Builds BinaryFuse8 filters inline per 64 MiB segment
  |  - Records instance offsets for thread-related objects
  |
  v
[Phase 3] Thread Name & State Resolution (parallel, offset-based)
  |
  v
[Phase 4] Engine + TUI Construction (thread list, app state)
  |
  v
TUI Display
```

---

## Phase Details

### Phase 0: CLI Setup

**File:** `crates/hprof-cli/src/main.rs`

- Validates file existence, obtains file size
- Creates 2 progress reporters:
  - `ProgressReporter` -- byte-level scan progress (unified scan + filter)
  - `NameProgressReporter` -- thread name resolution spinner

### Phase 1: Header Parsing

**File:** `crates/hprof-parser/src/hprof_file.rs`

- Memory-maps file (read-only)
- Parses: version string, `id_size` (4 or 8), timestamp (u64)
- Records `records_start` byte offset

### Phase 2: First-Pass Sequential Indexing + Inline Filters

**File:** `crates/hprof-parser/src/indexer/first_pass.rs`

This is the largest phase, scanning every byte of the file sequentially.
Segment filters are built inline as each 64 MiB chunk completes.

#### 2a. Main Loop

Progress reported every ~4 MiB or every 1 second. For each record:

| Tag    | Record Type        | Action                                                |
|--------|--------------------|-------------------------------------------------------|
| `0x01` | STRING_IN_UTF8     | Cache string ID -> HprofString (from_utf8_lossy)      |
| `0x02` | LOAD_CLASS         | Cache class serial -> ClassDef (binary name cached)   |
| `0x04` | STACK_FRAME        | Cache frame ID -> StackFrame                          |
| `0x05` | STACK_TRACE        | Cache trace serial -> StackTrace + thread association  |
| `0x06` | START_THREAD       | Cache thread serial -> HprofThread (with name ID)     |
| `0x0C`/`0x1C` | HEAP_DUMP/SEGMENT | Sub-record scan: inline filters, GC roots, ROOT_THREAD_OBJ, instance offsets |

Non-fatal errors: warned (up to 100), scan continues.

#### 2b. Inline Segment Filter Construction

**File:** `crates/hprof-parser/src/indexer/segment.rs`

`SegmentFilterBuilder` builds filters incrementally during the scan:
- Tracks `current_segment` index and a `current_ids: Vec<u64>` buffer
- When `add()` detects a new segment index, finalizes the previous segment's
  filter (sort + dedup + BinaryFuse8) and frees the ID vector immediately
- `finish()` called after the main loop to finalize the last segment
- Peak memory: one segment's worth (~200K IDs × 8 bytes ≈ 1.6 MB)
  instead of all segments combined

#### 2c. Instance Offset Recording

During the heap sub-record scan, a temporary `HashMap<u64, u64>` records
`(object_id, file_offset)` for every INSTANCE_DUMP (0x21),
OBJECT_ARRAY_DUMP (0x22), and PRIMITIVE_ARRAY_DUMP (0x23) sub-record.

After the main loop, this map is cross-referenced with `thread_object_ids`
to store only thread-related offsets in `PreciseIndex.instance_offsets`.
The temporary map is then dropped.

Trade-off: ~48 MB transient memory for a 1 GB dump (freed after
cross-reference).

#### 2d. Post-Loop Synthesis & Correlation

Runs after the main loop completes:

1. **Thread synthesis** (lines 410-436): If no START_THREAD records exist
   (e.g., jvisualvm dumps), create synthetic `HprofThread` from each
   STACK_TRACE with `thread_serial > 0` and `name_string_id = 0`.

2. **Thread object ID population** (lines 438-450): From ROOT_THREAD_OBJ
   sub-records, map `thread_serial -> object_id`. Update synthetic threads
   if their `object_id` was 0.

3. **Frame root correlation** (lines 452-469): From GC_ROOT_JAVA_FRAME,
   chain `thread_serial -> stack_trace -> frame_ids[frame_number] -> frame_id`
   to build `java_frame_roots[frame_id] = [object_id, ...]`.

### Phase 3: Thread Name & State Resolution

**File:** `crates/hprof-engine/src/engine_impl.rs` (`build_thread_cache`)

Called after the file is fully indexed. Parallelized with `rayon::par_iter`
over all threads. For each thread, resolves name and state concurrently.

#### Name Resolution (3-level fallback)

1. **Direct string lookup**: If `name_string_id != 0`, use `strings[id].value`
2. **Heap-based resolution**: Via ROOT_THREAD_OBJ object_id, traverse
   `Thread instance -> "name" field -> String instance -> "value" char[]/byte[]`
3. **Fallback**: `Thread-{serial}`

#### State Resolution

Reads `threadStatus` int field from the `java.lang.Thread` instance and
maps it to `ThreadState` (Runnable, Waiting, Blocked, TimedWaiting, etc.).

#### Offset-Based Reads

Two helper methods enable O(1) direct seeks instead of O(segment_size)
linear scans:
- `read_instance_at_offset(offset)` -- seeks to recorded offset, reads
  INSTANCE_DUMP header and data directly
- `read_prim_array_at_offset(offset)` -- same for PRIMITIVE_ARRAY_DUMP
- Falls back to `find_instance`/`find_prim_array` if offset unavailable

Progress: `AtomicUsize` counter with spinner ("Resolving thread names… N/M").

Results cached in `HashMap<u32, ThreadMetadata>` -- built once, O(1)
thereafter.

### Phase 4: Engine + TUI Construction

**Files:** `crates/hprof-engine/src/engine_impl.rs`, `crates/hprof-tui/src/app.rs`

- `Engine` wraps `HprofFile` + `thread_cache` (names + states)
- `App::new()` calls `engine.list_threads()` to build the full thread list
- `ThreadListState` stores full list + filter state

---

## Current Caching & Optimizations

| Data              | Storage                         | Lookup  | Built When     |
|-------------------|---------------------------------|---------|----------------|
| String records    | `PreciseIndex.strings`          | O(1)    | Phase 2a       |
| Stack frames      | `PreciseIndex.stack_frames`     | O(1)    | Phase 2a       |
| Stack traces      | `PreciseIndex.stack_traces`     | O(1)    | Phase 2a       |
| Class names       | `PreciseIndex.class_names_by_id`| O(1)    | Phase 2a       |
| Thread object IDs | `PreciseIndex.thread_object_ids`| O(1)    | Phase 2d       |
| Instance offsets  | `PreciseIndex.instance_offsets` | O(1)    | Phase 2c       |
| Segment filters   | BinaryFuse8 per 64 MiB chunk   | O(1)    | Phase 2b       |
| Thread metadata   | `Engine.thread_cache`           | O(1)    | Phase 3        |

---

## Known Issues

### 1. Thread Search Not Working

**Location**: `crates/hprof-tui/src/views/thread_list.rs`

**Mechanism**: User presses `/`, types filter text. `apply_filter()` does
case-insensitive substring match on thread names against `filtered_serials`.

**Potential issues to investigate**:
- Filter matching against the correct name source (cached names vs display names)
- Selection stability logic after filter application
- Key event routing when search mode is active

---

## Benchmarks (Story 3.8)

Measured on release builds after inline filters + offset-based resolution
+ rayon parallelism:

| Dump                  | Size   | Load Time | Peak RSS |
|-----------------------|--------|-----------|----------|
| heapdump-visualvm     | 41 MB  | 222 ms    | 66 MB    |
| heapdump-rustrover    | 1.1 GB | 8.4 s     | ~1.5 GB  |

Improvement over pre-3.8 baseline (~60s for 1.1 GB): **~7x faster**.

The 1.1 GB RSS is dominated by the read-only mmap of the heap dump file.

---

## Optimization History

### Applied (Story 3.8)

1. **Inline segment filter construction**: Merged former Phase 3 into
   Phase 2. Filters built incrementally as each 64 MiB segment completes
   during the scan. Peak memory reduced from all-segments to one-segment.

2. **Thread state resolution**: `threadStatus` field decoded from heap
   Thread instance during name resolution. No longer hardcoded as Unknown.

3. **Offset-based thread resolution**: Instance offsets recorded during
   first pass, enabling O(1) direct seeks instead of O(segment_size)
   linear scans via `find_instance`.

4. **Rayon parallelism**: `build_thread_cache` uses `par_iter()` over
   threads with `AtomicUsize` progress counter.

### Potential Future Optimizations

1. **Lazy name resolution**: Only resolve names for visible threads
   initially, resolve the rest on demand. Useful for dumps with thousands
   of threads.
