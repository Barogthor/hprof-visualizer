# Code Review — Story 1.3: Hprof Header Parsing & Mmap File Access

**Date:** 2026-03-06  
**Reviewer:** Amelia (Dev Agent, Codex execution)  
**Story:** `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md`

## Summary

| Severity | Count |
|----------|-------|
| High     | 3     |
| Medium   | 1     |
| Low      | 0     |

Core parser primitives are implemented and tested, but Story 1.3 cannot be considered fully complete because key acceptance criteria are not satisfied end-to-end.

## Findings

### 🔴 H1 — FR1/CLI story intent is not implemented in application entrypoint

**Why it matters**

The story is user-facing: opening an hprof file from a CLI argument. Current `main` is a no-op, so there is no argument parsing, file opening, header parsing, or user-facing error handling path.

**Evidence**

- Story scope includes CLI file opening intent: `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:7`.
- `crates/hprof-cli/src/main.rs:4` has `fn main() {}` only.
- No calls to `open_readonly`/`parse_header` exist in CLI crate implementation.

**Impact**

User cannot execute the Story 1.3 flow from CLI despite story status being `done`.

---

### 🔴 H2 — AC #7 mmap -> parse integration scenario is not tested

**Why it matters**

AC #7 explicitly requires synthetic bytes written to a temp file, mmaped, then parsed through `parse_header`. Current tests cover each piece separately but not the required integration path.

**Evidence**

- AC #7 requirement: `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:38`.
- mmap test validates length only: `crates/hprof-parser/src/mmap.rs:62`.
- header builder tests parse builder bytes directly (not mmap): `crates/hprof-parser/src/header.rs:179`.

**Impact**

A story-critical regression path (file I/O + mmap + header parser interaction) is unverified.

---

### 🔴 H3 — Story marked `done` while high-severity AC gaps remain

**Why it matters**

The story checklist and status indicate completion, but H1/H2 show mandatory behavior is missing.

**Evidence**

- Story status is `done`: `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:3`.
- Tasks are all checked: `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:45`.
- Dev notes explicitly defer CLI wiring: `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:98`.

**Impact**

Traceability is broken: completion metadata does not match actual delivered scope.

---

### 🟡 M1 — Non-existent-path test is Unix-specific and brittle cross-platform

**Why it matters**

A hardcoded Unix path is used in unit tests. The project targets Linux/macOS/Windows, and this test can become environment-dependent.

**Evidence**

- `crates/hprof-parser/src/mmap.rs:50` uses `/non/existent/file.hprof`.

**Recommendation**

Generate a guaranteed-missing path using `tempfile`/`std::env::temp_dir()` with random suffix, then assert `MmapFailed`.

## Git vs Story Cross-Check

- Working tree is clean (`git status --porcelain` returned empty).
- No active discrepancy to flag between current uncommitted changes and Story 1.3 File List.

## AC Verification

- AC1 (read-only mmap): parser-level capability exists (`open_readonly`), but CLI user flow not implemented (see H1).
- AC2/AC3 (version + id_size parse): implemented and passing tests in `header.rs`.
- AC4 (unsupported version): implemented and tested.
- AC5 (truncated header handling): implemented and tested.
- AC6 (missing file -> `MmapFailed`): implemented and tested.
- AC7 (builder bytes -> temp file -> mmap -> parse_header): missing end-to-end test (see H2).

## Validation Commands Run

- `cargo build`
- `cargo test`
- `cargo test -p hprof-parser --features test-utils`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt -- --check`
