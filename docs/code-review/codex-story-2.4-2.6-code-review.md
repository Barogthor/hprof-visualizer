# Code Review - Stories 2.4 to 2.6

**Date:** 2026-03-07  
**Reviewer:** Codex (Amelia / Dev Agent execution)

## Scope

- `docs/implementation-artifacts/2-4-tolerant-indexing.md`
- `docs/implementation-artifacts/2-5-segment-level-binaryfuse8-filters.md`
- `docs/implementation-artifacts/2-6-indexing-progress-bar.md`

Reviewed implementation files:

- `crates/hprof-parser/src/indexer/mod.rs`
- `crates/hprof-parser/src/indexer/segment.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/hprof_file.rs`
- `crates/hprof-parser/src/test_utils.rs`
- `crates/hprof-engine/src/lib.rs`
- `crates/hprof-tui/src/progress.rs`
- `crates/hprof-cli/src/main.rs`
- workspace/crate manifests for new dependencies

## Executive Result

| Severity | Count |
|----------|-------|
| High     | 1     |
| Medium   | 2     |
| Low      | 1     |

Stories 2.4 and 2.5 are largely implemented correctly and test coverage is strong. Story 2.6 has functional progress reporting, but there are correctness/contract gaps around progress semantics and display behavior.

## Findings

### H1 - Progress bar total is computed from full file length while callback bytes are record-section relative

**Why it matters**

The UI progress denominator and numerator are in different coordinate systems. This skews percentage/ETA and can prevent a true 100% progression at end-of-indexing.

**Evidence**

- CLI sets total bytes from full file metadata: `crates/hprof-cli/src/main.rs:29`.
- Callback bytes are documented as relative to records section (header excluded): `crates/hprof-parser/src/hprof_file.rs:63`.
- First-pass progress callback reports cursor offset relative to `data` (records slice): `crates/hprof-parser/src/indexer/first_pass.rs:9`.

**Impact**

- AC mismatch risk for Story 2.6 AC1/AC3 (percentage semantics and completion behavior).
- Speed and ETA are slightly biased because total bytes include header while processed bytes do not.

**Recommended fix**

Use records-section length as the progress bar total (or convert callback bytes back to absolute file bytes consistently).

---

### M1 - Progress update cadence is byte-threshold based only, so "at least once per second" is not guaranteed

**Why it matters**

Story 2.6 AC2 requires a minimum temporal refresh rate. Current logic refreshes every 4 MiB, not every second.

**Evidence**

- Fixed threshold constant: `crates/hprof-parser/src/indexer/first_pass.rs:25`.
- Updates happen only when `pos - last_progress >= PROGRESS_REPORT_INTERVAL`: `crates/hprof-parser/src/indexer/first_pass.rs:97`, `crates/hprof-parser/src/indexer/first_pass.rs:107`, `crates/hprof-parser/src/indexer/first_pass.rs:257`.

**Impact**

On slow media/VMs (throughput below ~4 MiB/s), updates may be less frequent than 1 Hz.

**Recommended fix**

Add a time-based flush condition (e.g., emit if `now - last_emit >= 1s` OR bytes threshold reached).

---

### M2 - Progress callback contract says final callback after loop, but implementation suppresses it for empty record sections

**Why it matters**

Public docs describe a final callback after completion. Header-only files currently produce zero callbacks.

**Evidence**

- Contract text implies final callback behavior: `crates/hprof-parser/src/indexer/first_pass.rs:34` and `crates/hprof-parser/src/hprof_file.rs:57`.
- Actual implementation gates final callback with `if !data.is_empty()`: `crates/hprof-parser/src/indexer/first_pass.rs:263`.

**Impact**

Callers relying on terminal callback semantics can miss completion notifications for empty-record files.

**Recommended fix**

Either always emit a final callback (`0`) or tighten docs to explicitly define the empty-input exception.

---

### L1 - Per-warning stderr lines are not prefixed with `warning:`

**Why it matters**

Output consistency and automated parsing are weaker when only the first line has a warning prefix.

**Evidence**

- Summary warning line uses prefix: `crates/hprof-tui/src/progress.rs:70`.
- Individual warnings are emitted as indented raw text: `crates/hprof-tui/src/progress.rs:75`.

**Impact**

Harder to grep/collect warning details from logs; inconsistent operator UX.

**Recommended fix**

Emit each warning as `warning: <message>`.

## AC Validation Snapshot

- **Story 2.4:** Tolerant indexing behavior (warnings + partial indexing) is implemented and covered by tests in `first_pass.rs` and `hprof_file.rs`.
- **Story 2.5:** Segment-level filter construction and extraction coverage are present (`segment.rs`, `first_pass.rs`, `test_utils.rs`).
- **Story 2.6:** Progress pipeline is wired end-to-end (`run_first_pass` -> `HprofFile` -> `hprof-engine` -> `hprof-tui` -> CLI), but findings above indicate AC-level semantic gaps.

## Validation Commands Run

- `git status --porcelain`
- `git diff --name-only`
- `git diff --cached --name-only`
- `cargo test -p hprof-parser --features test-utils`
- `cargo test -p hprof-engine`
- `cargo test -p hprof-tui`
- `cargo test -p hprof-cli`
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt -- --check`
