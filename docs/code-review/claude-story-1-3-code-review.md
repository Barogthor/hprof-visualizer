# Code Review — Story 1-3: Hprof Header Parsing & Mmap File Access

**Date:** 2026-03-06
**Reviewer:** Amelia (Dev Agent — claude-sonnet-4-6)
**Story:** `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md`
**Final Status:** done

---

## Summary

| Severity | Count | Fixed |
|----------|-------|-------|
| High     | 0     | —     |
| Medium   | 3     | 3 ✅  |
| Low      | 3     | 1 ✅  |

All Acceptance Criteria (AC #1–#7) verified as implemented. All Medium issues fixed.

---

## Findings

### 🟡 M1 — `parse_header` did not validate `id_size` [FIXED]

**File:** `crates/hprof-parser/src/header.rs`

`id_size` was read from the binary header but not validated against the allowed values
(4 or 8). Invalid values (e.g., 0, 3, 16) were silently accepted.

The architecture mandates: "invalid header → fatal error" (`architecture.md#Error Handling`).
An invalid `id_size` constitutes a malformed header and would cause downstream `read_id()`
calls to panic or misbehave.

**Fix applied:** Added validation after reading `id_size`:
```rust
if id_size != 4 && id_size != 8 {
    return Err(HprofError::CorruptedData(format!(
        "invalid id_size: {id_size}, expected 4 or 8"
    )));
}
```
Two tests added: `invalid_id_size_3_returns_corrupted_data`,
`invalid_id_size_0_returns_corrupted_data`.

---

### 🟡 M2 — `pub mod` violated module visibility architecture rule [FIXED]

**File:** `crates/hprof-parser/src/lib.rs`

`pub mod error`, `pub mod mmap`, `pub mod header` exposed internal modules publicly,
allowing consumers to import via both `hprof_parser::open_readonly` AND
`hprof_parser::mmap::open_readonly`.

Architecture mandates: "Re-export public types from module root. Consumers import from the
module, not from internal sub-files." (`architecture.md#Module Visibility Patterns`)

**Fix applied:** Changed all three to `pub(crate) mod`. Public API is still accessible
via the `pub use` re-exports at the crate root. No public surface change.

---

### 🟡 M3 — Change Log section missing from story file [FIXED]

The `dev-story` workflow mandates a Change Log entry. The section was absent.

**Fix applied:** Added `## Change Log` section with implementation and code review entries.

---

### 🟢 L1 — `Cargo.lock` absent from story File List [FIXED]

`Cargo.lock` was modified (new transitive dependencies resolved) but not listed in the
story's File List. Added to File List.

---

### 🟢 L2 — `tempfile` unconditional dev-dependency [NOT FIXED — ACCEPTED]

`tempfile = "3"` is compiled for all test runs but only used in
`#[cfg(all(test, feature = "test-utils"))]` tests. Cargo does not support
feature-gating dev-dependencies, so this cannot be fixed without restructuring.
Accepted as-is — only affects dev build time, not production.

---

### 🟢 L3 — No tests for invalid `id_size` [FIXED — covered by M1 fix]

Tests for `id_size = 0` and `id_size = 3` added alongside the M1 validation fix.

---

## AC Verification

| AC | Description | Status |
|----|-------------|--------|
| #1 | File memory-mapped read-only via `memmap2` | ✅ `mmap.rs::open_readonly` |
| #2 | V1_0_1 + id_size=4 parsed correctly | ✅ `valid_101_4byte_ids` |
| #3 | V1_0_2 + id_size=8 parsed correctly | ✅ `valid_102_8byte_ids` |
| #4 | Invalid magic → `UnsupportedVersion` with string | ✅ `invalid_version_returns_unsupported_version` |
| #5 | Short slice → `TruncatedRecord` (no panic) | ✅ 4 truncation tests |
| #6 | Non-existent path → `MmapFailed` (no panic) | ✅ `non_existent_path_returns_mmap_failed` |
| #7 | Builder bytes → mmap → parse succeeds | ✅ `parse_valid_102_8byte_ids_from_builder` |

## Final Test Count

- Without `test-utils`: 18 tests pass
- With `--features test-utils`: 38 tests pass
- `cargo clippy -p hprof-parser -- -D warnings`: clean
- `cargo fmt -- --check`: clean
