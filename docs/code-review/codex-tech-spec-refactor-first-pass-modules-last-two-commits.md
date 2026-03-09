# Code Review — Last 2 Commits vs Tech Spec

- **Spec:** `docs/implementation-artifacts/tech-spec-refactor-first-pass-modules`
- **Commits reviewed:** `c0e7f67` and `5847884`
- **Reviewer:** Codex
- **Date:** 2026-03-09

## Verdict

**Changes requested** before considering this fully compliant with the spec.

The module split is largely successful and commit `5847884` fixes the tracing-span scope issue from the previous review, but there are still spec/quality gaps.

## Validation Commands Run

- `cargo test -p hprof-parser` ✅ (tests pass)
- `cargo fmt --all -- --check` ✅ (formatting clean)
- `cargo bench -p hprof-parser --no-run` ✅ (bench compiles)
- `cargo clippy -p hprof-parser --all-targets -- -D warnings` ❌
- `cargo clippy -p hprof-parser --all-targets -- -D warnings -A clippy::too_many_arguments` ❌

## Findings

### 1) [HIGH] AC-2 is not met: clippy fails with `-D warnings`

`cargo clippy -p hprof-parser --all-targets -- -D warnings` fails on:

- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs:18`

`parse_and_insert(...)` triggers `clippy::too_many_arguments`.

**Why this matters:** The spec acceptance criterion explicitly requires clippy to pass without new warnings.

---

### 2) [MEDIUM] Additional clippy blockers remain in tests after bypassing finding #1

When running clippy with `-A clippy::too_many_arguments`, it still fails with unused/dead code errors:

- Unused imports: `crates/hprof-parser/src/indexer/first_pass/tests.rs:7`, `crates/hprof-parser/src/indexer/first_pass/tests.rs:9`
- Dead code: `crates/hprof-parser/src/indexer/first_pass/tests.rs:18`, `crates/hprof-parser/src/indexer/first_pass/tests.rs:23`, `crates/hprof-parser/src/indexer/first_pass/tests.rs:80`, `crates/hprof-parser/src/indexer/first_pass/tests.rs:112`

Root cause: helpers/imports used only by `#[cfg(feature = "test-utils")] mod builder_tests` are declared at top-level, so default test builds see them as unused.

**Why this matters:** The spec notes explicitly call out avoiding dead-code warnings after moving `#[cfg(test)]` items.

---

### 3) [MEDIUM] Progress reporting regresses in sequential heap extraction path

For heap records, scan phase immediately continues:

- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs:105`

Sequential extraction path has no `maybe_report_progress(...)` calls:

- `crates/hprof-parser/src/indexer/first_pass/heap_extraction.rs:262`

But documentation still promises periodic progress reports during long processing:

- `crates/hprof-parser/src/indexer/first_pass/mod.rs:14`
- `crates/hprof-parser/src/indexer/first_pass/mod.rs:128`

**Impact:** For sub-threshold heap workloads, progress callbacks can appear stalled until the final callback.

---

### 4) [LOW] AC-7 wording is not strictly satisfied (match-arm size)

The spec says each of the 5 record match arms in `record_scan.rs` should be `<= 10` lines using the helper. At least two are clearly longer:

- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs:138`
- `crates/hprof-parser/src/indexer/first_pass/record_scan.rs:153`

**Impact:** Mostly readability/strict-spec compliance, not a correctness break.

## What Is Good

- Refactor structure aligns with the intended 6-file split.
- `run_first_pass` is now a concise phase orchestrator.
- Public API shape remains intact (`run_first_pass` only public function in this module).
- `5847884` correctly restores `first_pass` span at pipeline root and adds a sequential extraction span.
