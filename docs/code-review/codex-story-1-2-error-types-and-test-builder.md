# Code Review — Story 1.2: Error Types & Test Builder

**Date:** 2026-03-06  
**Reviewer:** Amelia (Dev Agent, Codex execution)  
**Story:** `docs/implementation-artifacts/1-2-error-types-and-test-builder.md`

## Summary

| Severity | Count |
|----------|-------|
| High     | 0     |
| Medium   | 1     |
| Low      | 0     |

Story 1.2 is largely implemented and test coverage is strong, but one contract-level issue remains in `HprofTestBuilder`.

## Findings

### 🟡 M1 — `HprofTestBuilder::new` does not enforce its `id_size` contract early

**Why it matters**

`HprofTestBuilder` documents that `id_size` must be 4 or 8, but this is only enforced in `encode_id()`. If a caller creates a builder with invalid `id_size` and builds without adding any records, the builder can emit an invalid header silently.

**Evidence**

- `crates/hprof-parser/src/test_utils.rs:39` stores `id_size` with no validation.
- `crates/hprof-parser/src/test_utils.rs:198` always writes `id_size` to header.
- `crates/hprof-parser/src/test_utils.rs:231` validates only inside `encode_id()`, which is not called when zero records are added.

**Recommendation**

Validate `id_size` in `new()` (or at latest at start of `build()`) and return a deterministic failure path for invalid values.

## Git vs Story Cross-Check

- Working tree is clean (`git status --porcelain` returned empty).
- No active discrepancy to flag between current uncommitted changes and Story 1.2 File List.

## AC Verification

- AC1 (`HprofError` variants): implemented in `crates/hprof-parser/src/error.rs`.
- AC2 (builder chaining API): implemented in `crates/hprof-parser/src/test_utils.rs`.
- AC3 (feature-gated availability): verified in `crates/hprof-parser/src/lib.rs`.
- AC4 (header + id_size + first STRING record layout): covered by builder tests, notably `ac4_header_and_string_record`.

## Validation Commands Run

- `cargo test -p hprof-parser`
- `cargo test -p hprof-parser --features test-utils`
- `cargo clippy --all-targets -- -D warnings`
