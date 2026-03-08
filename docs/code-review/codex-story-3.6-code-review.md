# Code Review Report — Story 3.6

- Story: `docs/implementation-artifacts/3-6-lazy-value-string-loading.md`
- Story status at review time: `review`
- Reviewer: Codex
- Date: 2026-03-07
- Outcome: **Changes Requested**

## Scope Reviewed

- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-parser/src/hprof_file.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/app.rs`
- `docs/implementation-artifacts/3-6-lazy-value-string-loading.md`
- `docs/implementation-artifacts/sprint-status.yaml`

## Validation Executed

- `cargo test --workspace` (pass)
- `cargo clippy --workspace -- -D warnings` (pass)
- `cargo fmt --check` (pass)

Note: `cargo test --workspace` output contains test-target warnings in `crates/hprof-tui/src/app.rs` (`unused import`, `dead_code`), but no test failures.

## Git vs Story File List

- Working tree is clean (`git status --porcelain` empty).
- The Story 3.6 file list matches the Story 3.6 implementation commit file set (`c29eb26`).
- Discrepancy count: **0**.

## Acceptance Criteria Audit

1. AC1 (String placeholder before navigation): **Implemented** (`crates/hprof-tui/src/views/stack_view.rs:586`, `crates/hprof-tui/src/views/stack_view.rs:591`).
2. AC2 (Enter loads string async and updates display): **Partially Implemented** (async + update + truncation implemented; UTF-16 surrogate pair decoding is lossy).
3. AC3 (unresolved string emits warning visible in status bar count): **Partially Implemented** (warning is collected, but not surfaced in status bar count).
4. AC4 (no duplicate loads while loading): **Implemented** (`crates/hprof-tui/src/app.rs:339`, `crates/hprof-tui/src/app.rs:254`, `crates/hprof-tui/src/app.rs:267`).
5. AC5 (collapse clears string load state from memory): **Partially Implemented** (maps are cleared, but in-flight lazy-string jobs are not cancelled and can repopulate state post-collapse).

## Findings

### 1) [HIGH] AC3 not satisfied: lazy-string warnings do not affect status-bar warning count

- Requirement: unresolved String load must emit a warning visible in status bar count (`docs/implementation-artifacts/3-6-lazy-value-string-loading.md:24`).
- Implementation collects runtime string warnings in `app_warnings` (`crates/hprof-tui/src/app.rs:372`).
- Status bar count uses only `warning_count` captured once from `engine.warnings()` at startup (`crates/hprof-tui/src/app.rs:75`, `crates/hprof-tui/src/app.rs:487`).
- Result: unresolved string warnings are not reflected in the visible warning count.

### 2) [HIGH] AC5 gap: collapsing objects does not cancel pending string loads

- Requirement: collapsing parent object clears string load state from memory (`docs/implementation-artifacts/3-6-lazy-value-string-loading.md:34`).
- `collapse_object_recursive` clears string maps (`crates/hprof-tui/src/views/stack_view.rs:351`).
- However, collapse paths only remove pending object expansions, not pending string jobs (`crates/hprof-tui/src/app.rs:287`, `crates/hprof-tui/src/app.rs:294`).
- `poll_strings` can later write loaded/failed string state back after collapse (`crates/hprof-tui/src/app.rs:363`, `crates/hprof-tui/src/app.rs:370`).
- Result: collapsed subtree string state can reappear, violating the intended cleanup semantics.

### 3) [MEDIUM] UTF-16 surrogate pairs are decoded incorrectly for char[] strings

- AC2 expects display of the actual string value (`docs/implementation-artifacts/3-6-lazy-value-string-loading.md:20`).
- `decode_prim_array_as_string` decodes UTF-16 by mapping each 16-bit code unit directly to `char` (`crates/hprof-engine/src/engine_impl.rs:148`).
- This breaks surrogate pair handling (non-BMP chars), replacing halves with `\u{FFFD}`.
- Existing test codifies this lossy behavior (`crates/hprof-engine/src/engine_impl.rs:672`).
- Result: some valid Java strings are rendered inaccurately.

### 4) [MEDIUM] Story record claims warning-style rendering for failed StringRef, but implementation uses hint style

- Dev Agent completion note claims failed StringRef renders `<unresolved>` in warning style (`docs/implementation-artifacts/3-6-lazy-value-string-loading.md:314`).
- Failed StringRef rows are styled with `theme::SEARCH_HINT` (dark gray), not `theme::STATUS_WARNING` (`crates/hprof-tui/src/views/stack_view.rs:785`, `crates/hprof-tui/src/theme.rs:35`).
- Result: implementation does not match documented completion claim.

## Recommended Actions

1. Include runtime `app_warnings` in status-bar warning count and keep it live (do not rely on startup snapshot only).
2. On recursive collapse, remove/cancel all pending string loads reachable from collapsed subtrees (or ignore stale completions safely).
3. Replace UTF-16 code-unit-to-char mapping with proper UTF-16 decoding (`String::from_utf16_lossy`-style pipeline over `u16` units).
4. Update failed-StringRef styling to warning semantics, or adjust the story/dev record to match actual behavior.
