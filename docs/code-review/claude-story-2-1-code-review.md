# Code Review — Story 2.1: Record Header Parsing, ID Utility & Unknown Record Skip

**Date:** 2026-03-06
**Reviewer:** Amelia (Dev Agent — claude-sonnet-4-6)
**Story file:** `docs/implementation-artifacts/2-1-record-header-parsing-id-utility-and-unknown-record-skip.md`
**Files reviewed:** `crates/hprof-parser/src/id.rs`, `crates/hprof-parser/src/record.rs`, `crates/hprof-parser/src/lib.rs`

---

## Acceptance Criteria Validation

| AC | Status | Evidence |
|----|--------|----------|
| AC1: 9-byte header parsed, cursor advanced | ✅ IMPLEMENTED | `parse_record_header` reads tag + time_offset + length, `parse_valid_record_header` asserts `cursor.position() == 9` |
| AC2: All ID reads through `read_id()` | ✅ IMPLEMENTED | `read_id()` exported from crate root; no hardcoded 4/8 byte reads in new code |
| AC3: Unknown tag skipped gracefully | ✅ IMPLEMENTED | `skip_record` is tag-agnostic; `skip_record_unknown_tag_with_valid_length_succeeds` confirms |
| AC4: `TruncatedRecord` when length > remaining | ✅ IMPLEMENTED | `skip_record` boundary check, `skip_record_length_exceeds_remaining_returns_truncated` confirms |

---

## Issues Found

### 🟡 MEDIUM (1 fixed)

**[M1] Integration path `parse_record_header → skip_record` only tested under `test-utils` feature**
- **Location:** `record.rs` — builder_tests (feature-gated)
- **Problem:** A `cargo test -p hprof-parser` run (without `--features test-utils`) did not exercise `skip_record` from a non-zero cursor position — the only real-world call pattern.
- **Fix applied:** Added `skip_record_from_non_zero_cursor_position` unit test in the base `#[cfg(test)]` block. Calls `parse_record_header` then `skip_record` on a 15-byte slice, asserts `cursor.position() == 15`.

---

### 🟢 LOW (5 fixed)

**[L1] `read_id` truncation test only covered id_size=4**
- **Location:** `id.rs:62`
- **Fix applied:** Renamed test to `read_id_insufficient_bytes_4_returns_truncated`, added `read_id_insufficient_bytes_8_returns_truncated` (4 bytes provided, id_size=8 needed).

**[L2] `CorruptedData` error message inconsistent with `parse_header`**
- **Location:** `id.rs:32-34`
- **Problem:** `"invalid id_size: {id_size}"` vs `parse_header`'s `"invalid id_size: {id_size}, expected 4 or 8"`.
- **Fix applied:** Updated to `"invalid id_size: {id_size}, expected 4 or 8"`.

**[L3] `lib.rs` module docstring outdated**
- **Location:** `lib.rs:1-3`
- **Problem:** Listed future features (indexer, BinaryFuse8) but not the `id` and `record` modules added by this story.
- **Fix applied:** Rewrote to enumerate current public API accurately.

**[L4] `parse_record_header_preserves_tag_and_length` missing cursor position assertion**
- **Location:** `record.rs:89`
- **Fix applied:** Added `assert_eq!(cursor.position(), 9)` to the test.

**[L5] No truncation test for `parse_record_header` failing on `time_offset` read**
- **Location:** `record.rs` — tests block
- **Problem:** Existing test (5 bytes = tag + time_offset) covers truncation at the `length` field only.
- **Fix applied:** Added `parse_record_header_truncated_on_time_offset_returns_error` (1 byte input, tag only).

---

## Post-fix Validation

| Check | Result |
|-------|--------|
| `cargo test -p hprof-parser` | ✅ 33 passed |
| `cargo test -p hprof-parser --features test-utils` | ✅ 57 passed |
| `cargo clippy -p hprof-parser -- -D warnings` | ✅ Clean |
| `cargo fmt -- --check` | ✅ Clean |

---

## Updated File List

- `crates/hprof-parser/src/id.rs` — renamed test, added id_size=8 truncation test, updated error message
- `crates/hprof-parser/src/record.rs` — added 3 tests (cursor position assert, time_offset truncation, non-zero cursor integration)
- `crates/hprof-parser/src/lib.rs` — docstring rewrite
