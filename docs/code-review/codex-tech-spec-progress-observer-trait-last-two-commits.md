# Code Review - Last 2 Commits vs Tech Spec

Spec reviewed: `docs/implementation-artifacts/tech-spec-progress-observer-trait.md`

Commits reviewed:
- `bb69818e0bcbb1eafee31deebe1acf5fde094b20` - fix: address review findings for progress observer trait
- `84d865f301cfe57a74b2a684dfbbee5646948703` - refactor: replace closure-based progress with ParseProgressObserver trait

## Scope

- `hprof-api` introduction and observer contract wiring
- Parser first-pass progress plumbing (scan + segment)
- Engine progress forwarding (scan + segment + names)
- CLI progress observer implementation
- Test coverage against ACs in the spec

## Validation Performed

- Ran: `cargo test -p hprof-api -p hprof-parser -p hprof-engine -p hprof-cli`
- Result: pass (no test failures)

## Findings

### 1) MEDIUM - AC-2 strictness is not guaranteed by implementation

Spec AC-2 asks for strictly increasing byte progress values, but implementation can emit duplicate final offsets.

Evidence:
- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs:286` reports progress during loop via `maybe_report_progress(...)`.
- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs:296` unconditionally reports final position again.
- `crates/hprof-parser/src/indexer/first_pass/tests.rs:388` validates `w[1] >= w[0]` (non-decreasing), not strict `>`.

Impact:
- Observer consumers expecting strict progression may receive repeated offsets at EOF.

Recommendation:
- Either (a) enforce strictness by suppressing duplicate final emission when unchanged, or (b) update AC-2 wording to non-decreasing monotonicity and keep current behavior.

### 2) MEDIUM - Segment progress ACs are under-tested

The spec defines explicit segment progression requirements (AC-3/AC-4 and part of AC-8), but tests do not directly assert the full `on_segment_completed(done, total)` sequence for real extraction paths.

Evidence:
- No dedicated tests asserting `done` sequence `1..N` with constant `total` across extraction.
- Existing event assertions in engine-level tests only check that segment events are absent in heap-free fixtures (`crates/hprof-engine/src/lib.rs:235`).

Impact:
- Regression risk for segment counting semantics (especially around batching/parallel extraction) without failing tests.

Recommendation:
- Add tests that capture `ProgressEvent::SegmentCompleted` and assert:
  - exact call count equals number of heap segments,
  - `done` sequence is `1..N`,
  - `total` stays constant at `N`,
  - no regression under parallel path.

### 3) MEDIUM - Name resolution observer path lacks behavioral assertions

Observer plumbing for `on_names_resolved(done, total)` exists, but tests mostly verify callability, not event semantics.

Evidence:
- `crates/hprof-engine/src/engine_impl.rs:272` emits `notifier.names_resolved(cache.len(), total)`.
- Test observers in key API tests keep `on_names_resolved` as no-op and do not assert progression (`crates/hprof-engine/src/engine_impl.rs:677`, `crates/hprof-engine/src/lib.rs:136`, `crates/hprof-parser/src/hprof_file.rs:533`).

Impact:
- AC-7 behavior can regress (wrong totals, missing final call, non-monotonic done) without immediate signal.

Recommendation:
- Add an engine test fixture with at least one resolvable thread and assert monotonic `done`, stable `total`, and final `done == total`.

### 4) LOW - TUI crate docs reference removed module

`hprof-tui` docs still mention a `progress` module that was deleted.

Evidence:
- `crates/hprof-tui/src/lib.rs:4` mentions `progress` in module list.
- `crates/hprof-tui/src/lib.rs:6` to `crates/hprof-tui/src/lib.rs:9` declare only `app`, `input`, `theme`, `views`.

Impact:
- Minor documentation drift and possible confusion for contributors.

Recommendation:
- Update module list comment to match current exports.

## Overall

The architecture refactor is coherent and compiles cleanly. The main gaps are verification depth and one AC-contract mismatch around strictness of scan progress monotonicity.
