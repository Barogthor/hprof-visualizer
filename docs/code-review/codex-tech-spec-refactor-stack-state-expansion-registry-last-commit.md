# Code Review - Last Commit

- Scope: `360876edb432dbb684bf64115da791105a982015`
- Reference spec: `docs/implementation-artifacts/tech-spec-refactor-stack-state-expansion-registry.md`
- Reviewer: Codex
- Date: 2026-03-11

## Validation Run

- `cargo build --all-targets` -> pass
- `cargo test` -> pass
- `cargo clippy --all-targets -- -D warnings` -> pass

## Findings

### 1) HIGH - AC5 is not satisfied (`format_entry_line` still exists in `state.rs`)

The tech spec states that `format_entry_line` must be removed from `state.rs` and live only in `format.rs`.

Evidence:
- `crates/hprof-tui/src/views/stack_view/state.rs:924` still defines `StackState::format_entry_line`.
- `crates/hprof-tui/src/views/stack_view/state.rs:929` delegates to `super::format::format_entry_line(...)`.
- `crates/hprof-tui/src/views/stack_view/format.rs:185` also defines `format_entry_line`.

Impact:
- The function body migration happened, but the old API surface remains in `StackState`, so the AC "no occurrence in `state.rs`" is currently false.

Recommendation:
- Either remove `StackState::format_entry_line` and update call sites/tests, or update the AC to explicitly allow a delegating wrapper.

### 2) MEDIUM - File list drift vs implementation (`app/tests.rs` changed but not declared)

The spec frontmatter `files_to_modify` does not include `crates/hprof-tui/src/app/tests.rs`, but the commit modifies it.

Evidence:
- Spec list: `docs/implementation-artifacts/tech-spec-refactor-stack-state-expansion-registry.md:9`
- Commit file list includes `crates/hprof-tui/src/app/tests.rs`

Impact:
- Traceability is weaker: reviewers following the spec file list can miss one modified file.

Recommendation:
- Add `crates/hprof-tui/src/app/tests.rs` to `files_to_modify` (or explicitly call out that collateral test updates are expected).

### 3) MEDIUM - AC4 text conflicts with implemented/required visibility

AC4 says `ExpansionRegistry` should have 4 fields `pub(super)`, but implementation uses `pub(crate)`.

Evidence:
- AC text: `docs/implementation-artifacts/tech-spec-refactor-stack-state-expansion-registry.md:273`
- Implementation: `crates/hprof-tui/src/views/stack_view/expansion.rs:14`

Impact:
- Review ambiguity: strictly by AC text this looks non-compliant; by the spec notes and actual cross-module usage this is intentional.

Recommendation:
- Normalize AC4 wording to match the intended visibility (`pub(crate)`), or adjust implementation and all dependent accesses accordingly.

## Overall Assessment

- Code quality and behavior are stable (full build/test/clippy green).
- Main blocker for strict spec compliance is AC5 (`format_entry_line` still present in `state.rs`).
- Remaining findings are documentation/spec alignment issues, not runtime regressions.
