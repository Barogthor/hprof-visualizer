# Story 2.6: Indexing Progress Bar

Status: done

## Story

As a user,
I want to see a progress bar during the indexing pass showing current progress, speed (GB/s),
and estimated time remaining,
so that I know the tool is working and can estimate when navigation will be available.

## Acceptance Criteria

1. **Given** the first pass indexer is running
   **When** progress updates occur
   **Then** a progress bar is displayed showing: bytes processed / total bytes, percentage,
   speed in GB/s, and ETA (FR27)

2. **Given** the indexing pass on a multi-GB file
   **When** I observe the progress bar
   **Then** it updates at a reasonable frequency — at least once per second, not flooding the
   terminal

3. **Given** the indexing completes successfully
   **When** the progress bar reaches 100%
   **Then** a summary line is displayed: total time elapsed, average speed, number of records
   indexed

4. **Given** indexing of a truncated file
   **When** the file ends before expected
   **Then** the progress bar reflects actual progress and the summary includes a warning about
   incomplete indexing with percentage of records successfully processed

## Tasks / Subtasks

- [x] Add workspace `indicatif` dependency (AC: #1, #2)
  - [x] **Red**: confirm `hprof-tui` compiles with `use indicatif::ProgressBar` import
  - [x] Add `indicatif = "0.17"` (verify latest stable on crates.io) to
        `[workspace.dependencies]` in root `Cargo.toml`
  - [x] Add `indicatif = { workspace = true }` to `[dependencies]` in
        `crates/hprof-tui/Cargo.toml`

- [x] Add progress callback to `run_first_pass` in `first_pass.rs` (AC: #1, #2, #4)
  - [x] **Red**: Write test — `run_first_pass` with empty data calls progress callback 0 times
        (callback count tracked via captured `Vec`)
  - [x] **Red**: Write test — `run_first_pass` with one string record calls progress callback
        exactly once (final report), with `bytes == data.len()`
  - [x] **Red**: Write test — `run_first_pass` on data larger than `PROGRESS_REPORT_INTERVAL`
        calls progress callback more than once (monotonically increasing values)
  - [x] **Red**: Write test — `run_first_pass` on truncated data (payload exceeds file size)
        calls progress callback with final cursor position < data.len()
  - [x] **Green**: Add `pub(crate) const PROGRESS_REPORT_INTERVAL: usize = 4 * 1024 * 1024`
  - [x] **Green**: Change signature to
        `pub(crate) fn run_first_pass(data: &[u8], id_size: u32, mut progress_fn: impl FnMut(u64)) -> IndexResult`
  - [x] **Green**: Track `last_progress: usize = 0` before the loop; inside the loop after
        `cursor.set_position(payload_end)`, if
        `cursor.position() as usize - last_progress >= PROGRESS_REPORT_INTERVAL`, call
        `progress_fn(cursor.position())` and update `last_progress`
  - [x] **Green**: After the loop, call `progress_fn(cursor.position())` once for the final
        position (for truncated files this is < data.len(); for complete files == data.len())
  - [x] Update `//!` module docstring to mention progress reporting

- [x] Update `HprofFile` in `hprof_file.rs` (AC: #1, #4)
  - [x] **Red**: Write test — `from_path_with_progress` on a valid file calls the callback at
        least once
  - [x] **Red**: Write test — `from_path` on a valid file compiles and succeeds (regression:
        no-op callback must work)
  - [x] **Green**: Add
        `pub fn from_path_with_progress(path: &Path, progress_fn: impl FnMut(u64)) -> Result<Self, HprofError>`
        that calls `run_first_pass(…, progress_fn)`
  - [x] **Green**: Refactor `from_path` to delegate to `from_path_with_progress` with `|_| {}`
  - [x] Update docstring for both functions

- [x] Expose `IndexSummary` and `open_hprof_file_with_progress` in `hprof-engine` (AC: #3, #4)
  - [x] **Red**: Compile check — `hprof_engine::IndexSummary` struct with fields
        `records_attempted: u64`, `records_indexed: u64`, `warnings: Vec<String>` exists
  - [x] **Red**: Write test — `open_hprof_file_with_progress` on a valid file returns `Ok`,
        callback called at least once
  - [x] **Red**: Write test — `open_hprof_file_with_progress` on a missing path returns
        `Err(HprofError::MmapFailed(_))`
  - [x] **Green**: Add `pub struct IndexSummary { pub records_attempted: u64,
        pub records_indexed: u64, pub warnings: Vec<String> }` to `hprof-engine/src/lib.rs`
  - [x] **Green**: Add `pub fn open_hprof_file_with_progress(path: &Path, progress_fn: impl FnMut(u64)) -> Result<IndexSummary, HprofError>`
        that calls `HprofFile::from_path_with_progress` and maps its fields to `IndexSummary`
  - [x] **Green**: Add `pub fn open_hprof_file(path: &Path) -> Result<IndexSummary, HprofError>`
        as a convenience wrapper calling with `|_| {}`
  - [x] Update `hprof-engine/src/lib.rs` `//!` docstring

- [x] Create `crates/hprof-tui/src/progress.rs` (AC: #1, #2, #3, #4)
  - [x] **Red**: Write test — `ProgressReporter::new(1024)` constructs without panic
  - [x] **Red**: Write test — `on_bytes_processed(512)` on a reporter does not panic
  - [x] **Green**: Define `pub struct ProgressReporter { pb: indicatif::ProgressBar,
        start: std::time::Instant, total_bytes: u64 }`
  - [x] **Green**: Implement `pub fn new(total_bytes: u64) -> Self` — create
        `ProgressBar::new(total_bytes)`, set style with bytes template showing:
        `[{elapsed_precise}] [{bar:40}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})`
  - [x] **Green**: Implement `pub fn on_bytes_processed(&mut self, bytes: u64)` — calls
        `self.pb.set_position(bytes)`
  - [x] **Green**: Implement `pub fn finish(self, summary: &hprof_engine::IndexSummary)` —
        calls `self.pb.finish_and_clear()`, prints summary line:
        `Indexed {records_indexed}/{records_attempted} records in {elapsed:.1?} ({speed:.2} GB/s)`
        where `speed = total_bytes as f64 / elapsed.as_secs_f64() / 1e9`
  - [x] **Green**: In `finish`, if `!summary.warnings.is_empty()`, print each warning prefixed
        with `"warning: "` to stderr
  - [x] Add `//!` module docstring
  - [x] **Declare** `pub mod progress;` in `crates/hprof-tui/src/lib.rs`

- [x] Update `crates/hprof-cli/src/main.rs` to wire progress (AC: #1, #2, #3, #4)
  - [x] **Update** existing test `run_returns_open_failed_for_missing_path` — after this story,
        a missing path hits `std::fs::metadata` first, so the error becomes
        `Err(CliError::MetadataFailed(_))`, not `Err(CliError::OpenFailed(_))`. Update the
        assertion accordingly.
  - [x] **Red**: Write test — `run` on a valid hprof file succeeds (does not return Err)
        (regression: extends existing `run_succeeds_for_valid_hprof_header_file`)
  - [x] **Green**: In `run`, get file length via `std::fs::metadata(&path)?.len()`
  - [x] **Green**: Construct `let mut reporter = hprof_tui::progress::ProgressReporter::new(file_len)`
  - [x] **Green**: Call `hprof_engine::open_hprof_file_with_progress(&path, |bytes| reporter.on_bytes_processed(bytes))`
        (replace the existing `open_hprof_header` call)
  - [x] **Green**: Call `reporter.finish(&summary)` on success
  - [x] **Green**: Add `CliError::MetadataFailed(std::io::Error)` variant for the `metadata` call
  - [x] Update `//!` module docstring

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-parser`
  - [x] `cargo test -p hprof-parser --features test-utils`
  - [x] `cargo test -p hprof-engine`
  - [x] `cargo test -p hprof-tui`
  - [x] `cargo test -p hprof-cli`
  - [x] `cargo clippy --workspace -- -D warnings`
  - [x] `cargo fmt -- --check`

## Dev Notes

### Crate Dependency Notes

`hprof-cli` already depends on both `hprof-engine` and `hprof-tui` (confirmed in
`crates/hprof-cli/Cargo.toml`). `hprof-tui` already depends on `hprof-engine`
(`crates/hprof-tui/Cargo.toml`). No new crate dependency edges are introduced by this story.

### Progress Callback Design

`run_first_pass` signature change:
```rust
pub(crate) const PROGRESS_REPORT_INTERVAL: usize = 4 * 1024 * 1024; // 4 MiB

pub(crate) fn run_first_pass(
    data: &[u8],
    id_size: u32,
    mut progress_fn: impl FnMut(u64),
) -> IndexResult
```

Inside the loop, after each `cursor.set_position(payload_end as u64)`:
```rust
let pos = cursor.position() as usize;
if pos - last_progress >= PROGRESS_REPORT_INTERVAL {
    progress_fn(pos as u64);
    last_progress = pos;
}
```

After the loop (unconditional final report):
```rust
progress_fn(cursor.position());
```

For truncated files the final `cursor.position()` < `data.len()`. The `ProgressBar` total is
`data.len()`, so the bar will stop before 100%, which satisfies AC4.

### `HprofFile::from_path_with_progress`

```rust
pub fn from_path_with_progress(
    path: &Path,
    progress_fn: impl FnMut(u64),
) -> Result<Self, HprofError> {
    let mmap = open_readonly(path)?;
    let header = parse_header(&mmap)?;
    let records_start = header_end(&mmap)?;
    let result = run_first_pass(&mmap[records_start..], header.id_size, progress_fn);
    Ok(Self { _mmap: mmap, header, index: result.index, index_warnings: result.warnings,
               records_attempted: result.records_attempted,
               records_indexed: result.records_indexed,
               segment_filters: result.segment_filters })
}

pub fn from_path(path: &Path) -> Result<Self, HprofError> {
    Self::from_path_with_progress(path, |_| {})
}
```

The progress callback receives bytes relative to the records section start, not the full file.
The difference (header bytes) is negligible for display purposes.

### `IndexSummary` in `hprof-engine`

```rust
/// Summary of a completed first-pass indexing run.
///
/// Returned by [`open_hprof_file`] and [`open_hprof_file_with_progress`].
pub struct IndexSummary {
    /// Total known-type records whose payload window was within bounds.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
    /// Non-fatal warnings collected during indexing (truncated payloads, etc.).
    pub warnings: Vec<String>,
}
```

`open_hprof_file_with_progress` implementation:
```rust
pub fn open_hprof_file_with_progress(
    path: &Path,
    progress_fn: impl FnMut(u64),
) -> Result<IndexSummary, HprofError> {
    let hfile = HprofFile::from_path_with_progress(path, progress_fn)?;
    Ok(IndexSummary {
        records_attempted: hfile.records_attempted,
        records_indexed: hfile.records_indexed,
        warnings: hfile.index_warnings,
    })
}
```

### `indicatif` Progress Bar Style

```rust
use indicatif::{ProgressBar, ProgressStyle};

let pb = ProgressBar::new(total_bytes);
pb.set_style(
    ProgressStyle::with_template(
        "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} \
         ({bytes_per_sec}, ETA {eta})"
    )
    .unwrap()
    .progress_chars("=>-"),
);
```

`indicatif` internally throttles terminal redraws (~15 fps by default), so calling
`set_position` on every callback (every 4 MiB) is safe — it will never flood the terminal.
The "at least once per second" AC is satisfied: even at 4 MiB/s (very slow), the callback
fires once per second.

### `ProgressReporter::finish` — Summary Format

```
Indexed 142 057 / 142 057 records in 3.2s (1.24 GB/s)
```

For a truncated file with warnings:
```
Indexed 98 342 / 98 342 records in 2.1s (0.89 GB/s)
warning: record 0x1C payload end 74512384 exceeds file size 74507001
warning: ...
```

The speed is computed from `total_bytes` (records section length) divided by elapsed seconds.
Use `f64` for both fields.

### `run` function in `hprof-cli` — Updated Flow

```rust
fn run<I>(args: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let path = parse_hprof_path(args)?;
    let file_len = std::fs::metadata(&path)
        .map_err(CliError::MetadataFailed)?
        .len();
    let mut reporter = hprof_tui::progress::ProgressReporter::new(file_len);
    let summary = hprof_engine::open_hprof_file_with_progress(
        &path,
        |bytes| reporter.on_bytes_processed(bytes),
    )
    .map_err(CliError::OpenFailed)?;
    reporter.finish(&summary);
    Ok(())
}
```

Note: `file_len` includes the header bytes while `progress_fn` receives records-section
offsets. The discrepancy is at most a few dozen bytes on files of any meaningful size —
the progress bar may briefly show 100% before `finish_and_clear` is called. This is
acceptable.

### What NOT to Build in This Story

| Concern | Story |
|---------|-------|
| Full NavigationEngine (Engine::from_file) | 3.1 |
| TUI application loop (ratatui) | 3.2 |
| Status bar warnings in TUI | 3.2 / 6.2 |
| TOML config / CLI flags | 6.1 |
| Memory budget | 5.1+ |
| Loading indicator during object resolution | 6.2 |

After this story, `hprof-visualizer <file>` will: show the progress bar, print the summary,
and exit. The full interactive TUI is deferred to Story 3.1+.

### Key Rules to Follow

- `progress_fn` in `run_first_pass` must be called after the main loop (not just inside it) —
  this ensures the final position is always reported.
- `run_first_pass` remains infallible; the progress callback must never propagate errors.
- No `println!` in production code — the summary output in `ProgressReporter::finish` uses
  `println!` for the summary line (acceptable: this is deliberate user-facing output, not
  a debug log). Warnings go to `eprintln!`.
- Do not add `tracing` instrumentation to `ProgressReporter` — it is a pure UI component.
- `PROGRESS_REPORT_INTERVAL` must be `pub(crate)` so it is referenceable in tests.
- The `indicatif::ProgressBar` uses `bytes_per_sec` in its template — this is a built-in
  indicatif placeholder that computes speed automatically. Do NOT compute speed manually
  inside `on_bytes_processed`; only compute it in `finish` for the summary line.

### Previous Story Intelligence (2.5)

- `run_first_pass` currently takes `(data: &[u8], id_size: u32)`. All call sites are in
  `hprof_file.rs` (`from_path`), `first_pass.rs` tests, and `builder_tests` in
  `first_pass.rs`. All call sites must be updated to pass a no-op `|_| {}` or a real callback.
- `IndexResult` struct in `mod.rs` is NOT changing — only `run_first_pass` signature changes.
- `HprofFile` already has `records_attempted` and `records_indexed` fields — no new fields
  needed on `HprofFile`.
- Test count after Story 2.5: 90 (no feature) / 131 (test-utils). Expect ~12–18 new tests
  across all crates for Story 2.6.

### Git Intelligence

- Pattern: new sub-module in `hprof-tui` → new file `progress.rs`, declared in `lib.rs`.
- `hprof-engine/src/lib.rs` currently has 13 lines; it will grow with `IndexSummary` and
  the two new public functions. Keep all in one file for now (YAGNI — no split needed yet).
- The `hprof-cli` tests use `tempfile` for `dev-dependencies` already.

### Project Structure Notes

- `progress.rs` lives at `crates/hprof-tui/src/progress.rs` (per architecture structure).
- `indicatif` added to workspace dependencies and `hprof-tui` only — no other crate needs it.
- `IndexSummary` lives in `crates/hprof-engine/src/lib.rs` — re-exported as a top-level
  public type from `hprof-engine`.
- `PROGRESS_REPORT_INTERVAL` lives in `crates/hprof-parser/src/indexer/first_pass.rs` as
  `pub(crate) const` — not exposed outside the crate.

### References

- [Source: docs/planning-artifacts/epics.md#Story 2.6]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture]
- [Source: docs/planning-artifacts/architecture.md#Logging & Instrumentation Patterns]
- [Source: docs/implementation-artifacts/2-5-segment-level-binaryfuse8-filters.md]
- [Source: crates/hprof-parser/src/indexer/first_pass.rs]
- [Source: crates/hprof-parser/src/hprof_file.rs]
- [Source: crates/hprof-engine/src/lib.rs]
- [Source: crates/hprof-cli/src/main.rs]
- [Source: crates/hprof-tui/src/lib.rs]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- Added `indicatif = "0.17"` to workspace deps; `hprof-tui` is the only consumer.
- `run_first_pass` now accepts `mut progress_fn: impl FnMut(u64)`; progress checks at all
  three `cursor.set_position` points (heap dump, unknown-tag skip, indexed record). Final
  unconditional call guarded with `!data.is_empty()` to satisfy the 0-calls-for-empty test.
- `HprofFile::from_path_with_progress` added; `from_path` delegates to it with `|_| {}`.
- `IndexSummary` + `open_hprof_file_with_progress` + `open_hprof_file` added to
  `hprof-engine/src/lib.rs`. `open_hprof_header` left in place (used by nothing after this
  story but still public API — removal deferred to natural cleanup).
- `ProgressReporter` in `hprof-tui/src/progress.rs` uses `indicatif`'s built-in
  `bytes_per_sec` placeholder for live display; speed in `finish` computed manually from
  `total_bytes / elapsed`.
- `hprof-cli` `run` now: `metadata` → `ProgressReporter::new` → `open_hprof_file_with_progress`
  → `reporter.finish`. `CliError::MetadataFailed` added; missing-path test updated accordingly.
- 16 new tests added across all crates (4 progress-callback in first_pass, 2 in hprof_file,
  3 in hprof-engine, 2 in hprof-tui, 1 updated + 1 new in hprof-cli).
- All 251 tests pass (100 + 142 parser, 3 engine, 2 tui, 4 cli).

**Code review fixes (2026-03-07):**
- ✅ Resolved [High] H1: Added `{percent:.1}%` to `ProgressStyle` template (`progress.rs:32`)
- ✅ Resolved [Medium] M1: `finish()` now computes percentage, shows it in summary line, and
  emits user-friendly "indexing incomplete — X% of records processed" warning to stderr
- ✅ Resolved [Medium] M2: Fixed trivially-true assertion in
  `progress_callback_reports_partial_position_for_truncated_data` — now uses 50-byte partial
  payload and asserts `reported <= data.len()` (non-trivial bound of 59)
- ✅ Resolved [Medium] M3: Added `Cargo.lock` to File List

### File List

- Cargo.lock
- Cargo.toml
- crates/hprof-engine/Cargo.toml
- crates/hprof-engine/src/lib.rs
- crates/hprof-cli/src/main.rs
- crates/hprof-parser/src/hprof_file.rs
- crates/hprof-parser/src/indexer/first_pass.rs
- crates/hprof-tui/Cargo.toml
- crates/hprof-tui/src/lib.rs
- crates/hprof-tui/src/progress.rs (new)
- docs/implementation-artifacts/sprint-status.yaml
- docs/implementation-artifacts/2-6-indexing-progress-bar.md
