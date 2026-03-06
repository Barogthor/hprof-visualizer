# Code Review — Stories 2.1 to 2.3: Structural Parsing and First-Pass Indexing

**Date:** 2026-03-07  
**Reviewer:** Amelia (Dev Agent, Codex execution)  
**Stories:**
- `docs/implementation-artifacts/2-1-record-header-parsing-id-utility-and-unknown-record-skip.md`
- `docs/implementation-artifacts/2-2-structural-record-parsing.md`
- `docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md`

## Summary

| Severity | Count |
|----------|-------|
| High     | 3     |
| Medium   | 2     |
| Low      | 0     |

Core functionality is present and all automated checks pass, but record payload boundary handling has correctness gaps that can desynchronize parsing on malformed input. These gaps materially affect Story 2.2/2.3 acceptance claims around robust sequential parsing.

## Findings

### 🔴 H1 — Fixed-size record parsers do not enforce `header.length` boundaries

**Why it matters**

`LOAD_CLASS`, `START_THREAD`, `STACK_FRAME`, and `STACK_TRACE` parsing currently reads expected fields directly from the global stream without constraining reads to the declared payload length for that record. If `header.length` is too small, parsing can consume bytes from subsequent records and still return `Ok`, corrupting index state.

**Evidence**

- First pass dispatches known tags without slicing/limiting payload by `header.length`: `crates/hprof-parser/src/indexer/first_pass.rs:33`.
- `parse_load_class` has no payload length input and no boundary check against record header: `crates/hprof-parser/src/types.rs:84`.
- Same pattern for `parse_start_thread`: `crates/hprof-parser/src/types.rs:107`.
- Same pattern for `parse_stack_frame`: `crates/hprof-parser/src/types.rs:137`.
- Same pattern for `parse_stack_trace`: `crates/hprof-parser/src/types.rs:167`.

**Impact**

Malformed record lengths can cause cross-record over-read and invalid indexing while avoiding immediate failure, violating strict parser correctness guarantees.

---

### 🔴 H2 — Known-record path does not guarantee cursor alignment to payload end

**Why it matters**

For known tags, the first pass does not verify that a parser consumed exactly `header.length` bytes. If parser-consumed bytes are fewer than `header.length`, trailing payload bytes are interpreted as the next record header; if more, subsequent record bytes are consumed early.

**Evidence**

- Loop expects parser calls to leave cursor ready for the next header, but no post-condition is checked: `crates/hprof-parser/src/indexer/first_pass.rs:31`.
- Known-tag branches parse and immediately continue without reconciling consumed bytes vs `header.length`: `crates/hprof-parser/src/indexer/first_pass.rs:34`.
- Only unknown tags use length-driven cursor movement (`skip_record`): `crates/hprof-parser/src/indexer/first_pass.rs:55`.

**Impact**

Sequential scan invariants (FR4) are not enforced under malformed-length conditions, which can produce cascading parse corruption.

---

### 🔴 H3 — `parse_string_record` accepts impossible payload-length contracts

**Why it matters**

For `STRING` records, `payload_length < id_size` is structurally invalid, but code uses `saturating_sub`, allowing `content_len = 0` and returning success after reading an ID from the stream. This can consume bytes beyond the declared payload and desynchronize parsing.

**Evidence**

- ID is read first regardless of payload contract: `crates/hprof-parser/src/strings.rs:40`.
- Content length uses `saturating_sub` instead of explicit validation: `crates/hprof-parser/src/strings.rs:41`.

**Impact**

Malformed `STRING` lengths can be treated as valid records, breaking stream consistency and index correctness.

---

### 🟡 M1 — `STACK_TRACE` frame count is not bounded by declared payload length

**Why it matters**

`num_frames` is trusted directly. Without validating that `num_frames * id_size` fits the current record payload, parser work can balloon on malformed data and continue reading until truncation from the broader stream.

**Evidence**

- `num_frames` read then looped over directly: `crates/hprof-parser/src/types.rs:177` and `crates/hprof-parser/src/types.rs:181`.

**Impact**

Increases exposure to malformed-input performance degradation and stream misalignment.

---

### 🟡 M2 — Regression coverage is missing for header-length mismatch scenarios

**Why it matters**

Current tests validate happy paths and truncation, but do not assert behavior when declared record length conflicts with parser-consumed bytes (too short / too long). This allowed H1-H3 to pass CI undetected.

**Evidence**

- First-pass tests cover normal indexing and unknown-tag skip, but no mismatched-length cases: `crates/hprof-parser/src/indexer/first_pass.rs:188`.
- `STRING` tests cover truncation and UTF-8 errors, but not `payload_length < id_size` with sufficient stream bytes: `crates/hprof-parser/src/strings.rs:79`.

**Impact**

High-risk malformed-input behaviors are unguarded by tests, increasing regression probability.

## Fixes Applied (2026-03-07)

- [x] H1 fixed: known-record parsing now runs on a bounded payload window derived from
  `header.length`, preventing cross-record reads.
- [x] H2 fixed: first pass now enforces exact payload consumption for known tags and
  preserves cursor alignment invariants.
- [x] H3 fixed: `parse_string_record` now rejects `payload_length < id_size` explicitly.
- [x] M1 fixed: `parse_stack_trace` now validates `num_frames * id_size` against
  remaining payload bytes before frame reads.
- [x] M2 fixed: regression tests added for too-short declared length, extra payload,
  and invalid STRING payload contract.

## Git vs Story Cross-Check

- Review started from current HEAD with no tracked diffs; remediation then introduced
  tracked changes in parser source and story artifacts for Stories 2.2/2.3.
- Untracked entries still unrelated to Stories 2.1-2.3 implementation scope:
  - `docs/implementation-artifacts/epic-1-retro-2026-03-06.md`
  - `tools/`

## AC Verification (Current State)

- **Story 2.1:** ACs are implemented for record header parse, `read_id`, unknown-tag skip, and skip truncation behavior.
- **Story 2.2:** Structural parsers and type outputs are implemented, with malformed payload-length checks now enforced (`parse_string_record`, `parse_stack_trace`).
- **Story 2.3:** First-pass indexer and `HprofFile` integration are implemented, with known-record payload windowing and exact-consumption checks now enforced.

## Validation Commands Run

- `git status --porcelain`
- `git diff --name-only`
- `git diff --cached --name-only`
- `cargo test -p hprof-parser`
- `cargo test -p hprof-parser --features test-utils`
- `cargo clippy -p hprof-parser -- -D warnings`
- `cargo fmt -- --check`
