---
title: 'Progress Observer Trait and Segment-Based Reporting'
slug: 'progress-observer-trait'
created: '2026-03-09'
status: 'completed'
stepsCompleted: [1, 2, 3, 4, 5, 6]
tech_stack: [rust, indicatif, rayon]
files_to_modify:
  - crates/hprof-api/Cargo.toml
  - crates/hprof-api/src/lib.rs
  - crates/hprof-api/src/progress.rs
  - crates/hprof-parser/Cargo.toml
  - crates/hprof-parser/src/lib.rs
  - crates/hprof-parser/src/indexer/first_pass/mod.rs
  - crates/hprof-parser/src/indexer/first_pass/record_scan.rs
  - crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs
  - crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs
  - crates/hprof-parser/src/hprof_file.rs
  - crates/hprof-engine/Cargo.toml
  - crates/hprof-engine/src/engine_impl.rs
  - crates/hprof-tui/src/progress.rs (delete)
  - crates/hprof-cli/Cargo.toml
  - crates/hprof-cli/src/progress.rs (new)
  - crates/hprof-cli/src/main.rs
  - Cargo.toml
code_patterns:
  - observer-trait
  - null-object
  - multi-phase-progress
  - base-offset-parameter
test_patterns:
  - test-observer-collects-events
  - monotonicity-assertion
  - test-utils-feature-gated-integration
---

# Tech-Spec: Progress Observer Trait and Segment-Based Reporting

**Created:** 2026-03-09

## Overview

### Problem Statement

The current progress reporting uses two separate `FnMut` closures
threaded through 4 layers (CLI → Engine → HprofFile → run_first_pass).
With the introduction of parallel heap extraction (Epic 8.3), the
byte-offset progress bar is broken: segments complete out-of-order,
causing the bar to jump forward then regress (e.g. 10% → 40% → 20%).

### Solution

Replace the closure-based progress reporting with a
`ParseProgressObserver` trait in a new `hprof-api` crate. The trait
exposes phase-appropriate signals: byte offsets for the sequential
record scan, segment completion counts for parallel heap extraction,
and done/total for name resolution. A `ProgressNotifier` newtype
wraps `&mut dyn ParseProgressObserver` for internal use, avoiding
generic monomorphisation. The CLI implements the trait with two
separate indicatif progress bars.

### Scope

**In Scope:**

- `ParseProgressObserver` trait with 3 methods
- `NullProgressObserver` no-op implementation
- Replace closures in `run_first_pass`, `HprofFile`, `Engine`
- Segment-count progress in `extract_all` (both parallel and
  sequential paths)
- Two indicatif bars in CLI (scan bytes + segment completion)
- Tests for observer contract (monotonicity, completeness)

**Out of Scope:**

- Chunking large hprof segments into smaller pieces (separate spec)
- Intra-segment progress in parallel workers
- Phase-started/phase-ended lifecycle events
- Dynamic observer subscription (multi-subscriber registry)

## Context for Development

### Codebase Patterns

- Progress throttling via `maybe_report_progress()` (4 MiB / 1s)
  stays for `on_bytes_scanned` only — not needed for segment
  completion (typically 29-100 calls total).
- `NameProgressReporter` already uses lazy init on first call —
  same pattern reused for the segment bar.
- `from_path()` convenience wraps `from_path_with_progress()` —
  same pattern preserved with `NullProgressObserver`.
- First-pass tests use `#[cfg(feature = "test-utils")]` gate for
  integration tests that need `HprofTestBuilder`.
- `build_thread_cache` in `engine_impl.rs:233-272` calls
  `progress_fn(done, total)` **after** `HprofFile` is already
  built as `Arc`. The observer must be passed separately to this
  method — it cannot be consumed by `from_path_with_progress`.
- `extract_heap_segment_impl` currently takes a generic
  `F: FnMut(usize)` for intra-segment progress. This is removed
  entirely — the function simplifies to a single non-generic
  version.

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `crates/hprof-parser/src/indexer/first_pass/mod.rs` | Pipeline orchestration, `run_first_pass` signature |
| `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs` | Parallel/sequential `extract_all`, `merge_segment_result`, `extract_heap_segment_impl<F>` |
| `crates/hprof-parser/src/indexer/first_pass/record_scan.rs` | Sequential record scan with byte progress via `maybe_report_progress` |
| `crates/hprof-parser/src/indexer/first_pass/hprof_primitives.rs` | `maybe_report_progress`, `PROGRESS_REPORT_INTERVAL`, `PARALLEL_THRESHOLD` |
| `crates/hprof-parser/src/indexer/first_pass/tests.rs` | Existing tests — `#[cfg(feature = "test-utils")]` gated |
| `crates/hprof-parser/src/hprof_file.rs` | `HprofFile::from_path_with_progress(path, impl FnMut(u64))` |
| `crates/hprof-engine/src/engine_impl.rs` | `Engine::from_file_with_progress` (2 closures), `build_thread_cache` (name progress) |
| `crates/hprof-tui/src/progress.rs` | `ProgressReporter`, `NameProgressReporter` (to be deleted — moved to cli) |
| `crates/hprof-cli/src/main.rs` | CLI wiring: `MultiProgress`, two reporters |
| `crates/hprof-parser/src/indexer/segment.rs` | `SEGMENT_SIZE = 64 MiB` — xorf segment filters (independent of heap ranges) |

### Technical Decisions

- **Trait location:** `hprof-api` — shared crate for public
  API traits. `hprof-parser`, `hprof-engine`, and `hprof-tui`
  all depend on `hprof-api` for the trait. Avoids tui→parser
  coupling.
- **No throttling for segment completion:** ~30-100 calls total,
  indicatif handles it.
- **Keep `maybe_report_progress` for bytes_scanned:** record scan
  emits per-record, needs throttling.
- **Segment progress = `heap_record_ranges.len()`:** uses existing
  JVM segment boundaries as-is. Chunking deferred to separate spec.
- **Lazy bar init:** segment bar created on first
  `on_segment_completed` call, scan bar created at construction.
- **`NullProgressObserver` is public:** usable by tests and lib
  consumers.
- **Dynamic dispatch:** public API takes `&mut dyn ParseProgressObserver`.
  Internal functions take `&mut ProgressNotifier` (newtype over
  `&mut dyn`). Eliminates monomorphisation — ~130 vtable lookups
  total, negligible cost (~200 ns on a 10-30s parse).
- **Observer lifetime:** the same `&mut dyn ParseProgressObserver`
  is passed to `HprofFile::from_path_with_progress` (scan +
  segments), then to `Engine::build_thread_cache` (name resolution).
  The engine borrows the observer mutably after `HprofFile`
  construction completes — no overlap.
- **Base offset in context:** `FirstPassContext` stores
  `base_offset: u64`. `maybe_report_progress` reads it from
  the context and adds it before calling
  `observer.on_bytes_scanned(base_offset + pos)`.
  No decorator struct needed — simpler than a wrapper.
- **Dependency graph:**
  ```
  hprof-api     ← trait, NullProgressObserver
  hprof-parser  → hprof-api
  hprof-engine  → hprof-api, hprof-parser
  hprof-cli     → hprof-api, hprof-engine, hprof-tui
  ```

## Implementation Plan

### Tasks

#### Task 1: Create `hprof-api` crate with `ParseProgressObserver` trait

**Files:**
- `Cargo.toml` — add `hprof-api` to workspace members
- `crates/hprof-api/Cargo.toml` (new)
- `crates/hprof-api/src/lib.rs` (new)
- `crates/hprof-api/src/progress.rs` (new)

Create `crates/hprof-api/Cargo.toml`:
```toml
[package]
name = "hprof-api"
version = "0.1.0"
edition = "2024"
```

Create the trait in `crates/hprof-api/src/progress.rs`:

```rust
/// Observer for first-pass indexing progress.
///
/// Implementors receive phase-appropriate signals:
/// - `on_bytes_scanned`: sequential record scan (monotone
///   byte offset, throttled by caller)
/// - `on_segment_completed`: heap segment extraction
///   (done/total, always called per segment)
/// - `on_names_resolved`: thread name resolution
///   (done/total)
pub trait ParseProgressObserver {
    /// Sequential record scan progress.
    ///
    /// `position` is an absolute byte offset from the
    /// start of the file (includes the header).
    /// Guaranteed monotonically increasing. Throttled
    /// to ~4 MiB / 1s intervals by the caller.
    fn on_bytes_scanned(&mut self, position: u64);

    /// A heap segment finished extraction.
    ///
    /// `done` increases by 1 each call, from 1 to `total`.
    /// Called once per segment regardless of parallel or
    /// sequential path. `done` is strictly monotonically
    /// increasing — the counter is incremented in the main
    /// thread after `par_iter().collect()`, never inside
    /// workers.
    fn on_segment_completed(
        &mut self,
        done: usize,
        total: usize,
    );

    /// Thread name resolution progress.
    ///
    /// `done` ranges from `chunk_size` to `total`,
    /// increasing by `chunk_size` each call (final call
    /// may be less than a full chunk). Unlike
    /// `on_segment_completed`, this does NOT increment
    /// by 1. Never called when `total == 0` (no threads
    /// to resolve) — the caller skips the loop entirely.
    fn on_names_resolved(
        &mut self,
        done: usize,
        total: usize,
    );
}

/// No-op observer for callers that don't need progress.
pub struct NullProgressObserver;

impl ParseProgressObserver for NullProgressObserver {
    fn on_bytes_scanned(&mut self, _position: u64) {}
    fn on_segment_completed(
        &mut self,
        _done: usize,
        _total: usize,
    ) {}
    fn on_names_resolved(
        &mut self,
        _done: usize,
        _total: usize,
    ) {}
}

/// Newtype wrapping `&mut dyn ParseProgressObserver`.
///
/// Public so that downstream crates (`hprof-parser`,
/// `hprof-engine`) can accept it in their internal
/// functions without re-importing the trait. The public
/// entry points (`HprofFile`, `Engine`) accept
/// `&mut dyn ParseProgressObserver`; a `ProgressNotifier`
/// is created at the boundary and threaded inward.
pub struct ProgressNotifier<'a>(
    &'a mut dyn ParseProgressObserver,
);

impl<'a> ProgressNotifier<'a> {
    /// Wraps an observer for internal use.
    pub fn new(
        observer: &'a mut dyn ParseProgressObserver,
    ) -> Self {
        Self(observer)
    }

    /// Reports sequential scan progress (absolute
    /// file offset).
    pub fn bytes_scanned(&mut self, position: u64) {
        self.0.on_bytes_scanned(position);
    }

    /// Reports a heap segment extraction completion.
    pub fn segment_completed(
        &mut self,
        done: usize,
        total: usize,
    ) {
        self.0.on_segment_completed(done, total);
    }

    /// Reports thread name resolution progress.
    pub fn names_resolved(
        &mut self,
        done: usize,
        total: usize,
    ) {
        self.0.on_names_resolved(done, total);
    }
}
```

Re-export from `crates/hprof-api/src/lib.rs`:
```rust
pub mod progress;
pub use progress::{
    NullProgressObserver,
    ParseProgressObserver,
    ProgressNotifier,
};
```

Add `hprof-api` as dependency to `hprof-parser`, `hprof-engine`,
and `hprof-cli` Cargo.toml files:
```toml
hprof-api = { path = "../hprof-api" }
```

Add `test-utils` feature to `crates/hprof-api/Cargo.toml`:
```toml
[features]
test-utils = []
```

Add test utilities under feature gate in `progress.rs`:

```rust
#[cfg(feature = "test-utils")]
#[derive(Debug, Clone, PartialEq)]
pub enum ProgressEvent {
    BytesScanned(u64),
    SegmentCompleted { done: usize, total: usize },
    NamesResolved { done: usize, total: usize },
}

#[cfg(feature = "test-utils")]
#[derive(Debug, Default)]
pub struct TestObserver {
    pub events: Vec<ProgressEvent>,
}

#[cfg(feature = "test-utils")]
impl ParseProgressObserver for TestObserver {
    fn on_bytes_scanned(&mut self, position: u64) {
        self.events.push(
            ProgressEvent::BytesScanned(position),
        );
    }
    fn on_segment_completed(
        &mut self,
        done: usize,
        total: usize,
    ) {
        self.events.push(
            ProgressEvent::SegmentCompleted {
                done, total,
            },
        );
    }
    fn on_names_resolved(
        &mut self,
        done: usize,
        total: usize,
    ) {
        self.events.push(
            ProgressEvent::NamesResolved {
                done, total,
            },
        );
    }
}
```

Re-export under feature gate in `lib.rs`:
```rust
#[cfg(feature = "test-utils")]
pub use progress::{ProgressEvent, TestObserver};
```

Add `hprof-api/test-utils` to dev-dependencies in
`hprof-parser`, `hprof-engine` Cargo.toml files:
```toml
[dev-dependencies]
hprof-api = { path = "../hprof-api", features = ["test-utils"] }
```

**Tests** (`progress.rs`):
- `null_observer_compiles_and_is_callable` — call all 3 methods,
  no panic.
- `test_observer_collects_all_event_types` — call each method,
  assert `events` contains correct variants.

#### Task 2: Wire observer into `run_first_pass` and record scan

**Files:**
- `crates/hprof-parser/src/indexer/first_pass/mod.rs`
- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs`

Change `run_first_pass` signature:

```rust
pub fn run_first_pass(
    data: &[u8],
    id_size: u32,
    base_offset: u64,
    notifier: &mut ProgressNotifier,
) -> IndexResult
```

`base_offset` is the byte offset of the records section start
(i.e. `records_start as u64`), stored in `FirstPassContext`:

```rust
struct FirstPassContext<'a> {
    // ... existing fields ...
    base_offset: u64,  // NEW
}
```

`maybe_report_progress` reads `base_offset` from context and
adds it before calling the observer.

Replace `progress_fn: impl FnMut(u64)` parameter everywhere in
`record_scan::scan_records` with `notifier: &mut ProgressNotifier`.
The `maybe_report_progress` calls now invoke
`notifier.bytes_scanned(base_offset + pos as u64)` instead of
`progress_fn(pos as u64)`.

Update `maybe_report_progress` signature:

```rust
pub(super) fn maybe_report_progress(
    pos: usize,
    base_offset: u64,
    last_progress_bytes: &mut usize,
    last_progress_at: &mut Instant,
    notifier: &mut ProgressNotifier,
)
```

**Important:** throttling logic stays on relative `pos`
(comparing `pos - *last_progress_bytes` against the
interval threshold). Only the final report call uses the
absolute offset: `notifier.bytes_scanned(base_offset + pos as u64)`.
`last_progress_bytes` is also stored as relative.

Called from `record_scan` as:
```rust
maybe_report_progress(
    pos,
    ctx.base_offset,
    &mut ctx.last_progress_bytes,
    &mut ctx.last_progress_at,
    notifier,
);
```

The final `bytes_scanned` call is emitted at the end of
`scan_records` (NOT at the end of `run_first_pass`), so the
scan bar completes before `extract_all` begins:

```rust
// end of scan_records:
notifier.bytes_scanned(
    ctx.base_offset + ctx.cursor_position,
);
```

The call that was previously at the end of `run_first_pass`
(`progress_fn(ctx.cursor_position)`) is removed — scan progress
is fully reported before heap extraction starts.

**Tests** (first_pass `tests` module):
- Existing tests using `|_| {}` closures → use
  `ProgressNotifier::new(&mut NullProgressObserver)` with
  `base_offset: 0`.
- Add `scan_phase_reports_monotonic_bytes` — use `TestObserver`,
  filter `ProgressEvent::BytesScanned` events, assert strictly
  increasing values.

#### Task 3: Segment-based progress in `extract_all`

**File:**
`crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs`

Change `extract_all` signature:

```rust
pub(super) fn extract_all(
    ctx: &mut FirstPassContext,
    notifier: &mut ProgressNotifier,
)
```

**Parallel path:**

`segments_done` is incremented in the **main thread** after
`par_iter().collect()` — never inside workers. This guarantees
`done` is strictly increasing regardless of segment completion
order.

```rust
let total_segments = ranges.len();
let mut segments_done: usize = 0;

for batch in ranges.chunks(batch_size) {
    let batch_results: Vec<HeapSegmentResult> = batch
        .par_iter()
        .map(|&(offset, len)| {
            let payload =
                &data[offset as usize..(offset + len) as usize];
            extract_heap_segment(
                payload,
                offset as usize,
                id_size,
            )
        })
        .collect();

    for seg_result in batch_results {
        merge_segment_result(ctx, seg_result);
        segments_done += 1;
        notifier.segment_completed(
            segments_done,
            total_segments,
        );
    }
}
```

**Sequential path:** same pattern — call
`notifier.segment_completed` after each segment merge.

Remove all `maybe_report_progress` calls and its import from
`heap_extraction.rs`.
Remove `extract_heap_segment_with_progress` and the generic
`F: FnMut(usize)` parameter from `extract_heap_segment_impl`.
Collapse into a single non-generic function:

```rust
fn extract_heap_segment(
    payload: &[u8],
    data_offset: usize,
    id_size: u32,
) -> HeapSegmentResult
```

The `on_progress` callback inside the extraction loop (called
per sub-record) is removed — intra-segment progress is no longer
reported. This is an intentional trade-off: segment-level
granularity is sufficient for progress bars, and removing the
per-sub-record callback simplifies the code and avoids threading
a closure through the hot loop.

**Tests:**
- `parallel_path_reports_all_segments` — TestObserver asserts
  `on_segment_completed` called exactly `total` times with
  `done` from 1 to total.
- `sequential_path_reports_all_segments` — same for small files
  below `PARALLEL_THRESHOLD`.
- `segment_done_never_exceeds_total` — assert
  `done <= total` for every call.

#### Task 4: Wire observer through `HprofFile` and `Engine`

**Files:**
- `crates/hprof-parser/src/hprof_file.rs`
- `crates/hprof-engine/src/engine_impl.rs`

`HprofFile::from_path_with_progress` signature changes:

```rust
pub fn from_path_with_progress(
    path: &Path,
    observer: &mut dyn ParseProgressObserver,
) -> Result<Self, HprofError>
```

Creates the `ProgressNotifier` at the boundary and passes
it to `run_first_pass`:

```rust
let mut notifier = ProgressNotifier::new(observer);
let result = run_first_pass(
    &mmap[records_start..],
    header.id_size,
    records_start as u64,
    &mut notifier,
);
```

`HprofFile::from_path` uses `NullProgressObserver`:

```rust
pub fn from_path(path: &Path) -> Result<Self, HprofError> {
    Self::from_path_with_progress(
        path,
        &mut NullProgressObserver,
    )
}
```

`Engine::from_file_with_progress` replaces two closures with
one observer:

```rust
pub fn from_file_with_progress(
    path: &Path,
    config: &EngineConfig,
    observer: &mut dyn ParseProgressObserver,
) -> Result<Self, HprofError>
```

Creates `ProgressNotifier` and passes to `build_thread_cache`:

```rust
let hfile = Arc::new(
    HprofFile::from_path_with_progress(
        path, observer,
    )?,
);
let mut notifier = ProgressNotifier::new(observer);
let thread_cache = Self::build_thread_cache(
    &hfile, &mut notifier,
);
```

`Engine::build_thread_cache` signature changes:

```rust
fn build_thread_cache(
    hfile: &HprofFile,
    notifier: &mut ProgressNotifier,
) -> HashMap<u32, ThreadMetadata>
```

Replace `progress_fn(cache.len(), total)` (line 269) with:
`notifier.names_resolved(cache.len(), total)`.

`Engine::from_file` uses `NullProgressObserver`:

```rust
pub fn from_file(
    path: &Path,
    _config: &EngineConfig,
) -> Result<Self, HprofError> {
    let hfile = Arc::new(
        HprofFile::from_path(path)?,
    );
    let mut null_obs = NullProgressObserver;
    let mut notifier =
        ProgressNotifier::new(&mut null_obs);
    let thread_cache = Self::build_thread_cache(
        &hfile, &mut notifier,
    );
    Ok(Self { hfile, thread_cache })
}
```

**Tests:**
- Update all existing `from_path_with_progress` tests to use
  a TestObserver instead of closures.
- `bytes_scanned_includes_base_offset` — verify that
  `on_bytes_scanned` values are >= `records_start`.
- `from_path_convenience_wrapper_succeeds` — verify that
  `HprofFile::from_path(path)` still works after the
  refactor (uses `NullProgressObserver` internally).

#### Task 5: CLI multi-bar implementation

**Files:**
- `crates/hprof-cli/src/progress.rs` (new)
- `crates/hprof-cli/src/main.rs`
- `crates/hprof-tui/src/progress.rs` (delete)

Move progress bar logic from `hprof-tui` to `hprof-cli`.
`CliProgressObserver` uses `indicatif` which is a CLI
concern, not a TUI concern — it belongs in the binary
crate. Delete `hprof-tui/src/progress.rs` (remove
`ProgressReporter` and `NameProgressReporter`). Remove
`indicatif` dependency from `hprof-tui` if no longer used.

Replace with a single `CliProgressObserver` in
`crates/hprof-cli/src/progress.rs`:

```rust
use hprof_api::ParseProgressObserver;

pub struct CliProgressObserver {
    mp: MultiProgress,
    scan_bar: ProgressBar,
    segment_bar: Option<ProgressBar>,
    name_bar: Option<ProgressBar>,
    start: Instant,
    total_bytes: u64,
}

impl ParseProgressObserver for CliProgressObserver {
    fn on_bytes_scanned(&mut self, position: u64) {
        self.scan_bar.set_position(position);
    }

    fn on_segment_completed(
        &mut self,
        done: usize,
        total: usize,
    ) {
        let bar = self.segment_bar.get_or_insert_with(|| {
            // Phase transition: finish scan bar before
            // showing segment bar
            self.scan_bar.finish_and_clear();
            let pb = self.mp.add(
                ProgressBar::new(total as u64),
            );
            pb.set_style(/* segment style */);
            pb
        });
        bar.set_length(total as u64);
        bar.set_position(done as u64);
        if done == total {
            bar.finish_and_clear();
        }
    }

    fn on_names_resolved(
        &mut self,
        done: usize,
        total: usize,
    ) {
        // same lazy init as current NameProgressReporter
        let bar = self.name_bar.get_or_insert_with(|| {
            let pb = self.mp.add(
                ProgressBar::new(total as u64),
            );
            pb.set_style(/* spinner style */);
            pb.enable_steady_tick(
                Duration::from_millis(120),
            );
            pb
        });
        bar.set_length(total as u64);
        bar.set_position(done as u64);
    }
}
```

`CliProgressObserver` also has a `finish()` method (NOT on the
trait — bar cleanup is the concrete type's responsibility):

```rust
impl CliProgressObserver {
    /// Finishes all active bars and prints the elapsed
    /// summary line ("Loaded in X (Y GB/s)").
    pub fn finish(&mut self) {
        self.scan_bar.finish_and_clear();
        if let Some(bar) = self.segment_bar.take() {
            bar.finish_and_clear();
        }
        if let Some(bar) = self.name_bar.take() {
            bar.finish_and_clear();
        }
        let elapsed = self.start.elapsed();
        let gb_per_sec = self.total_bytes as f64
            / elapsed.as_secs_f64()
            / 1_073_741_824.0;
        eprintln!(
            "Loaded in {:.1?} ({:.2} GB/s)",
            elapsed, gb_per_sec,
        );
    }
}
```

Segment bar style:
`[{elapsed_precise}] [{bar:40.green/blue}] {pos}/{len} segments ({percent}%, ETA {eta})`

Update `main.rs`:

```rust
mod progress;
// ...
let mut observer = progress::CliProgressObserver
    ::new(&mp, file_len);
let engine = hprof_engine::Engine::from_file_with_progress(
    &path,
    &config,
    &mut observer as &mut dyn ParseProgressObserver,
)?;
observer.finish();
```

Remove `ProgressReporter` and `NameProgressReporter` structs.

**Tests:**
- `cli_observer_constructs_without_panic`
- `cli_observer_on_bytes_scanned_does_not_panic`
- `cli_observer_on_segment_completed_does_not_panic`

### Acceptance Criteria

**AC-1: Observer trait compiles and is usable**
- Given a `struct MyObserver` implementing `ParseProgressObserver`
- When passed to `run_first_pass`
- Then all 3 methods are called at appropriate phases

**AC-2: Byte progress is monotonically increasing**
- Given a valid hprof file processed through record scan
- When `on_bytes_scanned` calls are collected
- Then each value is strictly greater than the previous

**AC-3: Segment progress counts correctly**
- Given a file with N heap segments
- When processed through `extract_all`
- Then `on_segment_completed` is called exactly N times
- And `done` values are 1, 2, ..., N
- And `total` is the same value (N) for every call

**AC-4: No progress regression in parallel mode**
- Given a file above PARALLEL_THRESHOLD
- When processed with parallel extraction
- Then `on_segment_completed(done, _)` has `done` strictly
  increasing (no regression)

**AC-5: NullObserver works for from_path**
- Given `HprofFile::from_path(path)`
- When called on a valid file
- Then succeeds without progress output

**AC-6: CLI displays two distinct bars**
- Given a dump above 32 MiB
- When loaded via CLI
- Then scan bar shows bytes/total_bytes with ETA
- Then segment bar shows pos/total segments with ETA
- And scan bar finishes before segment bar appears

**AC-7: Name resolution spinner works via observer**
- Given a file with threads to resolve
- When `on_names_resolved` is called
- Then spinner shows done/total

**AC-8: End-to-end integration with real dump**
- Given `assets/heapdump-visualvm.hprof` loaded via
  `Engine::from_file_with_progress` with a TestObserver
- When indexing completes
- Then `on_bytes_scanned` emits values >= `records_start`
  and final value == file size
- And `on_segment_completed` is called exactly N times
  (where N = `heap_record_ranges.len()`, currently 29
  for the test asset) with `done` from 1..N and constant
  `total` == N
- And `on_names_resolved` is called with `total` > 0

## Additional Context

### Dependencies

- New workspace crate `hprof-api` (no external dependencies).
- `indicatif` and `rayon` already in use, no new external deps.

### Testing Strategy

- **Unit tests:** `TestObserver` from `hprof-api` (feature-gated
  `test-utils`) collects all calls into `Vec<ProgressEvent>`.
  Assert event sequences, monotonicity, and completeness.
  Shared across all crates — no duplication.
- **Integration test:** Load `assets/heapdump-visualvm.hprof`
  with a TestObserver. Verify scan phase emits bytes, segment
  phase emits 29 segments, names phase emits thread count.
- **No mocks** per project convention — TestObserver is a real
  implementation, not a mock.

### Notes

- This spec does NOT address the problem of JVM dumps with
  very few very large segments (e.g. 1 segment of 3 GB).
  A separate spec will add logical chunking of large segments
  for better parallelism and progress granularity.
- `maybe_report_progress` is kept but only used in `record_scan`.
  It could be moved into that module in a future cleanup.
- Bar cleanup (`finish()`) is NOT on the `ParseProgressObserver`
  trait — it is a concrete concern of `CliProgressObserver`.
  The caller in `main.rs` calls `observer.finish()` on the
  concrete type after `Engine` construction completes.
  `NullProgressObserver` needs no cleanup.
