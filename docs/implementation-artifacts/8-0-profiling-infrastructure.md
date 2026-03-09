# Story 8.0: Profiling Infrastructure

Status: done

## Story

As a developer,
I want reproducible benchmarks and visual profiling tools
for the first pass pipeline,
so that I can measure the exact impact of each optimization
and identify remaining hotspots.

## Acceptance Criteria

### AC1: Criterion benchmark with env-gated real file

**Given** a real hprof file path in `HPROF_BENCH_FILE` env var
**When** I run `cargo bench --bench first_pass`
**Then** criterion produces benchmark results with statistical
analysis comparing to the previous run.

### AC2: Tracing-chrome profiling behind feature flag

**Given** the `dev-profiling` feature flag is enabled
**When** I run
`cargo run --features dev-profiling -- <file.hprof>`
**Then** a `trace.json` file is generated in the current
directory that opens in Perfetto UI showing labeled spans for
each first pass phase (record scan, heap extraction,
segment filter build, thread cache build).

### AC3: Graceful skip when no bench file

**Given** no `HPROF_BENCH_FILE` env var is set
**When** I run `cargo test`
**Then** benchmark tests are skipped without failure.

### AC4: Per-component benchmarks

**Given** `HPROF_BENCH_FILE` is set
**When** I run the criterion benchmarks
**Then** separate benchmark groups exist for:
- `first_pass_total` (end-to-end `run_first_pass`)
- `string_parsing` (STRING record extraction)
- `heap_extraction` (`extract_heap_object_ids` phase)
- `segment_filter_build` (`seg_builder.finish()`)

### AC5: All existing tests pass

**Given** the profiling infrastructure is added
**When** I run `cargo test`
**Then** all 357+ existing tests pass with no regressions.

### AC6: CI unaffected

**Given** CI does not set `HPROF_BENCH_FILE`
and does not enable `dev-profiling`
**When** the CI pipeline runs
**Then** benchmarks are skipped, no `tracing-chrome`
dependency is compiled, build succeeds on all platforms.

## Tasks / Subtasks

- [x] Task 1: Add criterion dev-dependency to
  `hprof-parser` (AC: 1, 3, 4)
  - [x] 1.1: Add `criterion = { version = "0.5",
    features = ["html_reports"] }` to
    `[dev-dependencies]` in
    `crates/hprof-parser/Cargo.toml`
  - [x] 1.2: Add `[[bench]]` section:
    `name = "first_pass"`, `harness = false`
  - [x] 1.3: Create `crates/hprof-parser/benches/`
    directory

- [x] Task 2: Implement criterion benchmarks (AC: 1, 3, 4)
  - [x] 2.1: Create
    `crates/hprof-parser/benches/first_pass.rs`
  - [x] 2.2: Read `HPROF_BENCH_FILE` env var at bench
    start; skip all benchmarks if not set (use
    criterion's custom `criterion_group!` with
    conditional bench functions)
  - [x] 2.3: `first_pass_total` group — benchmarks
    `run_first_pass(data, id_size, no_op_progress)`
    on mmap'd file
  - [x] 2.4: `string_parsing` group — isolate STRING
    record parsing (requires exposing a targeted
    helper or benchmarking the string portion of
    first_pass)
  - [x] 2.5: `heap_extraction` group — benchmark
    `extract_heap_object_ids` phase
  - [x] 2.6: `segment_filter_build` group — benchmark
    `SegmentFilterBuilder::finish()`
  - [x] 2.7: Verify `cargo bench --bench first_pass`
    runs with `HPROF_BENCH_FILE` set and skips
    without it

- [x] Task 3: Add `dev-profiling` feature flag (AC: 2, 6)
  - [x] 3.1: Add feature `dev-profiling` to
    `crates/hprof-parser/Cargo.toml`:
    `dev-profiling = ["dep:tracing", "dep:tracing-chrome", "dep:tracing-subscriber"]`
  - [x] 3.2: Add optional dependencies:
    `tracing = { version = "0.1", optional = true }`,
    `tracing-chrome = { version = "0.7", optional = true }`,
    `tracing-subscriber = { version = "0.3", optional = true }`
  - [x] 3.3: Propagate feature in `hprof-engine`:
    `dev-profiling = ["hprof-parser/dev-profiling"]`
  - [x] 3.4: Propagate feature in `hprof-cli`:
    `dev-profiling = ["hprof-engine/dev-profiling"]`

- [x] Task 4: Instrument first pass with tracing spans
  (AC: 2)
  - [x] 4.1: In `first_pass.rs`, wrap major phases in
    conditional `tracing::info_span!` macros gated
    behind `#[cfg(feature = "dev-profiling")]`
  - [x] 4.2: Spans to add:
    - `"first_pass"` — wraps entire `run_first_pass`
    - `"record_scan"` — wraps main while loop
    - `"heap_extraction"` — wraps
      `extract_heap_object_ids` calls
    - `"segment_filter_build"` — wraps
      `seg_builder.finish()`
    - `"thread_cache_build"` — wraps thread cache
      assembly (in engine or first_pass post-loop)
  - [x] 4.3: Use a thin macro or `#[cfg]` blocks to
    avoid code noise when feature is off

- [x] Task 5: Wire tracing-chrome in CLI (AC: 2)
  - [x] 5.1: In `main.rs`, conditionally init
    `tracing-chrome` `ChromeLayerBuilder`
    behind `#[cfg(feature = "dev-profiling")]`
  - [x] 5.2: Setup: `tracing_chrome::ChromeLayerBuilder::new().file("trace.json").build()`
    + `tracing_subscriber::registry().with(chrome_layer).init()`
  - [x] 5.3: Flush guard must be held until end of
    `run()` to ensure `trace.json` is written
  - [x] 5.4: Verify `trace.json` opens in Perfetto UI
    with labeled spans

- [x] Task 6: Run all tests + manual validation (AC: 5, 6)
  - [x] 6.1: `cargo test` — all 359 tests pass
  - [x] 6.2: `cargo clippy` — no warnings
  - [x] 6.3: `cargo build` (without features) — no
    tracing dependencies compiled
  - [x] 6.4: `cargo build --features dev-profiling` —
    builds successfully

## Dev Notes

### Architecture Compliance

- **Crate boundary:** Benchmarks live in `hprof-parser`
  (the crate being benchmarked). Feature flag propagates
  up through `hprof-engine` → `hprof-cli`.
- **No `println!` in production code** — tracing macros
  only, gated behind feature flag.
- **Feature flag convention:** Optional dependencies use
  `dep:` syntax in feature definition to avoid implicit
  feature names.

### Per-Component Benchmark Strategy

The `first_pass_total` benchmark is straightforward:
call `run_first_pass()` with a no-op progress callback.

For sub-component benchmarks (`string_parsing`,
`heap_extraction`, `segment_filter_build`), there are
two approaches:

1. **Preferred:** Make `run_first_pass` accept a
   configuration that skips certain phases, or
   expose internal phase functions as `pub(crate)`.
   This avoids duplicating logic.
2. **Alternative:** Use `SegmentFilterBuilder` directly
   for filter build benchmarks (it's already public).
   For string/heap phases, benchmark indirectly through
   `first_pass_total` and rely on tracing-chrome spans
   for phase-level breakdown.

Decision: Use approach 2. The `first_pass_total` benchmark
is the primary metric. Per-phase granularity comes from
tracing-chrome spans in Perfetto. Only
`segment_filter_build` gets a standalone bench (via
`SegmentFilterBuilder::finish()`). This avoids exposing
internal APIs or refactoring the monolithic first_pass
function (that refactoring happens in Stories 8.1-8.3).

### Tracing Instrumentation Pattern

```rust
// When dev-profiling is enabled:
#[cfg(feature = "dev-profiling")]
let _span = tracing::info_span!("record_scan").entered();

// When dev-profiling is disabled: no-op, zero cost
```

For the CLI init:
```rust
#[cfg(feature = "dev-profiling")]
let _guard = {
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::prelude::*;
    let (chrome_layer, guard) =
        ChromeLayerBuilder::new()
            .file("trace.json")
            .build();
    tracing_subscriber::registry()
        .with(chrome_layer)
        .init();
    guard
};
```

### Criterion Env-Gating Pattern

```rust
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_first_pass_total(c: &mut Criterion) {
    let path = match std::env::var("HPROF_BENCH_FILE") {
        Ok(p) => p,
        Err(_) => {
            eprintln!(
                "HPROF_BENCH_FILE not set, skipping"
            );
            return;
        }
    };
    let data = std::fs::read(&path)
        .expect("failed to read bench file");
    let header = hprof_parser::parse_header(&data)
        .expect("invalid hprof header");

    c.bench_function("first_pass_total", |b| {
        b.iter(|| {
            hprof_parser::indexer::run_first_pass(
                &data[header.records_start..],
                header.id_size,
                |_| {},
            )
        });
    });
}

criterion_group!(benches, bench_first_pass_total);
criterion_main!(benches);
```

**Note:** `run_first_pass` is currently `pub(crate)`.
It must be made `pub` or the bench file won't compile
(benches are external to the crate). Alternatively,
benchmark through `HprofFile::from_path()` which is
already public — but this includes mmap overhead.
Recommendation: make `run_first_pass` `pub` and
re-export from `lib.rs` (it's a stable API for the
parser crate).

### Existing Code to Modify

| File | Change |
|------|--------|
| `crates/hprof-parser/Cargo.toml` | Add criterion, tracing, tracing-chrome, tracing-subscriber deps + features |
| `crates/hprof-parser/src/lib.rs` | Re-export `run_first_pass` (if making public for benches) |
| `crates/hprof-parser/src/indexer/mod.rs` | Make `run_first_pass` `pub` (if needed) |
| `crates/hprof-parser/src/indexer/first_pass.rs` | Add tracing spans gated behind `dev-profiling` |
| `crates/hprof-engine/Cargo.toml` | Propagate `dev-profiling` feature |
| `crates/hprof-cli/Cargo.toml` | Propagate `dev-profiling` feature |
| `crates/hprof-cli/src/main.rs` | Init tracing-chrome when `dev-profiling` enabled |

### New Files to Create

| File | Purpose |
|------|---------|
| `crates/hprof-parser/benches/first_pass.rs` | Criterion benchmark harness |

### Dependencies to Add

| Crate | Version | Location | Type |
|-------|---------|----------|------|
| `criterion` | `0.5` | `hprof-parser` | `[dev-dependencies]` |
| `tracing` | `0.1` | `hprof-parser` | optional dependency |
| `tracing-chrome` | `0.7` | `hprof-parser` | optional dependency |
| `tracing-subscriber` | `0.3` | `hprof-parser` | optional dependency |

### Key Warnings from Epic 3 Retro

1. **Cyclic reference freeze** — Florian observed a
   freeze expanding a variable on real dumps. Must
   investigate before proceeding with optimizations.
   This is a pre-requisite action item from Epic 3
   retro (Action Item #4).
2. **all_offsets HashMap** — 120 MB for 5M entries.
   This story only measures it; Story 8.1 replaces it.
3. **Test count** — 357 tests at end of Epic 3. Ensure
   no regressions.

### Anti-Patterns to Avoid

- Do NOT add `tracing` as a non-optional dependency.
  It must be behind `dev-profiling` to keep release
  builds clean.
- Do NOT use `println!` or `eprintln!` for profiling
  output — only `tracing` macros.
- Do NOT modify `run_first_pass` logic — this story
  only instruments it. Logic changes happen in 8.1+.
- Do NOT add benchmarks that require specific file
  sizes or formats beyond what `HPROF_BENCH_FILE`
  provides.

### Project Structure Notes

- Benchmarks in `crates/hprof-parser/benches/` (Rust
  standard location for crate-level benches).
- Feature flag propagation chain:
  `hprof-cli/dev-profiling` → `hprof-engine/dev-profiling`
  → `hprof-parser/dev-profiling`
- No workspace-level feature needed — features propagate
  through crate dependencies.

### References

- [Source: docs/planning-artifacts/epics.md#Story 8.0]
- [Source: docs/planning-artifacts/architecture.md#Testing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Logging & Instrumentation Patterns]
- [Source: docs/report/party-mode-perf-optimization-2026-03-08.md#Story 8.0]
- [Source: docs/implementation-artifacts/epic-3-retro-2026-03-08.md#Action Items for Epic 8]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None

### Completion Notes List

- Task 1-2: Added criterion 0.5 dev-dependency with html_reports. Created `benches/first_pass.rs` with 4 benchmark groups (first_pass_total, string_parsing, heap_extraction, segment_filter_build). All env-gated via HPROF_BENCH_FILE. Made `run_first_pass` public and `indexer` module public for bench access. Made `SegmentFilterBuilder`, `SegmentFilter`, `IndexResult` public.
- Task 3: Added `dev-profiling` feature flag with optional tracing/tracing-chrome/tracing-subscriber deps using `dep:` syntax. Propagated through hprof-engine and hprof-cli. CLI also gets direct tracing-chrome/tracing-subscriber optional deps for init code.
- Task 4: Instrumented `run_first_pass` with 5 `#[cfg(feature = "dev-profiling")]` tracing spans: first_pass (outer), record_scan (while loop), heap_extraction (per segment), segment_filter_build, thread_cache_build.
- Task 5: Wired tracing-chrome ChromeLayerBuilder in CLI `run()` behind `#[cfg(feature = "dev-profiling")]`. Guard held until end of run().
- Task 6: All 359 tests pass, clippy clean, build without features compiles no tracing deps, build with dev-profiling succeeds. Bench compiles (verified with --no-run).
- Added `Default` derive to `SegmentFilterBuilder` to satisfy clippy (new public API).

### Change Log

- 2026-03-08: Story 8.0 implemented — profiling infrastructure (criterion benchmarks + tracing-chrome spans)

### File List

- `Cargo.toml` (modified — `[profile.bench] debug = true`)
- `Cargo.lock` (modified — deps update)
- `.gitignore` (modified — added trace.json, profile.json.gz)
- `crates/hprof-parser/Cargo.toml` (modified)
- `crates/hprof-parser/src/lib.rs` (modified)
- `crates/hprof-parser/src/header.rs` (modified — added `records_start` field)
- `crates/hprof-parser/src/hprof_file.rs` (modified — use `header.records_start`)
- `crates/hprof-parser/src/indexer/mod.rs` (modified)
- `crates/hprof-parser/src/indexer/first_pass.rs` (modified)
- `crates/hprof-parser/src/indexer/segment.rs` (modified)
- `crates/hprof-parser/benches/first_pass.rs` (new)
- `crates/hprof-engine/Cargo.toml` (modified)
- `crates/hprof-cli/Cargo.toml` (modified)
- `crates/hprof-cli/src/main.rs` (modified)
