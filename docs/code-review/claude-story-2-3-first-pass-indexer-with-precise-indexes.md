# Code Review — Story 2.3: First Pass Indexer with Precise Indexes

**Date:** 2026-03-06
**Reviewer:** Amelia (dev agent, claude-sonnet-4-6)
**Story:** `docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md`
**Outcome:** Changes Requested → Fixed

---

## Summary

**Issues Found:** 3 High, 3 Medium, 2 Low
**Issues Fixed:** 6 (all HIGH + MEDIUM)
**Remaining:** 2 LOW (documented, acceptable)

---

## 🔴 HIGH — Fixed

### H1 — `HprofFile` did not implement `Debug`

**File:** `crates/hprof-parser/src/hprof_file.rs:23`

Architecture rule (`architecture.md#Enforcement Guidelines`): "All public structs derive at
minimum `Debug`". `HprofFile` was public but had no `#[derive(Debug)]`.
`memmap2::Mmap` implements `Debug`, so the derive is straightforward.

**Fix:** Added `#[derive(Debug)]` to `HprofFile`.

---

### H2 — Builder test `full_index_round_trip`: 1 thread instead of 2, task marked [x] prematurely

**File:** `crates/hprof-parser/src/indexer/first_pass.rs:295`

Story task (marked [x]): *"Build file with 2 threads, 1 class, 2 strings, 1 stack_trace,
1 stack_frame → verify all 7 entries indexed."*

Actual implementation had only `add_thread(1, ...)` → 6 entries, assertion checked
`index.threads.len() == 1`. This was a spec-vs-implementation mismatch introduced by
the Dev Notes code example which already had the wrong count.

**Fix:** Added `add_thread(2, 400, 0, 1, 0, 0)` and updated assertions to
`index.threads.len() == 2` + `index.threads[&2].object_id == 400`. Total = 7 entries.

---

### H3 — `HprofError::InvalidHeader` in docstring does not exist

**File:** `crates/hprof-parser/src/hprof_file.rs:38`

`from_path` docstring listed `HprofError::InvalidHeader` which is not a variant of
`HprofError`. The correct variant emitted by `parse_header` for unrecognised version
strings is `HprofError::UnsupportedVersion`.

**Fix:** Changed to `HprofError::UnsupportedVersion`.

---

## 🟡 MEDIUM — Fixed

### M1 — `.gitignore` modified but absent from story File List

**File:** `docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md`

`git status` showed `M .gitignore` — a real modification not documented in Dev Agent
Record → File List.

**Fix:** Added `.gitignore (modified)` to File List.

---

### M2 — `clippy --features test-utils` not run; pre-existing `collapsible_if` violations exposed

**File:** `crates/hprof-parser/src/test_utils.rs:223`

The verification checklist omitted `cargo clippy -p hprof-parser --features test-utils`.
Running it revealed two `clippy::collapsible_if` violations in `test_utils.rs` that had
been present since Story 2.2.

**Fix:** Collapsed the nested `if let` blocks using Rust `&&` pattern in `build()`.

---

### M3 — No test for `TruncatedRecord` error propagation through `HprofFile::from_path`

**File:** `crates/hprof-parser/src/hprof_file.rs`

`from_path` calls `run_first_pass` which can return `HprofError::TruncatedRecord`, but no
test exercised this path. Only success paths and `MmapFailed` were tested.

**Fix:** Added `from_path_truncated_record_returns_error`: builds a file with a valid header
followed by a single truncated record (tag byte only) and asserts
`Err(HprofError::TruncatedRecord)`.

---

## 🟢 LOW — Documented, Not Fixed

### L1 — `header_end` rescans null byte already found by `parse_header`

`hprof_file.rs:61`: double null-byte scan. Acceptable for correctness; a future refactor
could have `parse_header` return its consumed byte count. Out of scope for this story.

### L2 — `header_end` error path is dead code

`header_end`'s `ok_or(HprofError::TruncatedRecord)` can never trigger in practice because
`parse_header` would have already failed on the same missing null byte. Acceptable;
the defensive check is safe and does not affect correctness.

---

## Files Modified by Code Review

- `crates/hprof-parser/src/hprof_file.rs` (derive Debug, fix docstring, add error test)
- `crates/hprof-parser/src/indexer/first_pass.rs` (fix builder test: 2 threads, 7 entries)
- `crates/hprof-parser/src/test_utils.rs` (fix collapsible_if clippy violations)
- `docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md` (File List)

---

## Final State

- Tests: 103 passed, 0 failed (with `--features test-utils`)
- `cargo clippy -p hprof-parser -- -D warnings`: clean
- `cargo clippy -p hprof-parser --features test-utils -- -D warnings`: clean
- `cargo fmt -- --check`: clean
