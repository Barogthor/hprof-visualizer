# Code Review Report — Story 3.5

- Story: `docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md`
- Story status at review time: `review`
- Reviewer: Codex
- Date: 2026-03-07
- Outcome: **Changes Requested**

## Scope Reviewed

- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/resolver.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/app.rs`
- `docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md`
- `docs/implementation-artifacts/sprint-status.yaml`

## Validation Executed

- `cargo test --workspace` (pass)
- `cargo clippy --workspace -- -D warnings` (pass)
- `cargo fmt --check` (pass)

Note: `cargo test --workspace` output contains one warning (`unused import`) from `crates/hprof-parser/src/indexer/precise.rs:77`.

## Git vs Story File List

Discrepancies found:

1. Files changed in git but missing from Story 3.5 File List:
   - `diag.txt`
   - `docs/implementation-artifacts/epic-2-retro-2026-03-07.md`

## Acceptance Criteria Audit

1. AC1 (nested async expansion): **Implemented** (`crates/hprof-tui/src/app.rs:250`, `crates/hprof-tui/src/app.rs:282`)
2. AC2 (collection size indicator before expansion): **Partially Implemented** (works for exact-case known names, but suffix/case-insensitive requirement is not met)
3. AC3 (correct deeper indentation): **Implemented** (`crates/hprof-tui/src/views/stack_view.rs:596`)
4. AC4 (nested toggle collapse): **Implemented** (`crates/hprof-tui/src/app.rs:256`, `crates/hprof-tui/src/app.rs:283`)
5. AC5 (recursive cleanup on root collapse): **Implemented** (`crates/hprof-tui/src/app.rs:279`, `crates/hprof-tui/src/views/stack_view.rs:277`)
6. AC6 (failure node at correct depth): **Implemented** (`crates/hprof-tui/src/views/stack_view.rs:664`)

## Findings

### 1) [CRITICAL] Task marked complete but not implemented (Task 9.1)

- Story requirement says root var label should show class name after first expansion (`docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md:157`).
- Current implementation still renders hardcoded `"Object [▼]"` for expanded root vars (`crates/hprof-tui/src/views/stack_view.rs:549`).
- This is a direct mismatch between checked task state and delivered behavior.

### 2) [HIGH] Collection detection does not follow suffix/case-insensitive rule

- Story task specifies suffix matching should be case-insensitive (`docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md:77`).
- Implementation performs exact membership check on short class name (`crates/hprof-engine/src/engine_impl.rs:49`).
- Result: classes that only satisfy `ends_with`/case-insensitive semantics are missed, causing missing entry count indicators (AC2 partial).

### 3) [HIGH] `class_names_by_id` can remain empty for valid classes depending on record ordering

- `LOAD_CLASS` handling resolves class name immediately from `index.strings` and falls back to empty string when not present (`crates/hprof-parser/src/indexer/first_pass.rs:240`, `crates/hprof-parser/src/indexer/first_pass.rs:245`).
- There is no later reconciliation pass to backfill names once corresponding STRING records appear.
- Impact: class-name enrichment and collection-size detection can silently degrade to `"Object"`/no count for valid objects.

### 4) [MEDIUM] Story File List is incomplete vs actual working tree

- Story file lists changed files under Dev Agent Record (`docs/implementation-artifacts/3-5-recursive-expansion-and-collection-size-indicators.md:349`).
- Current git state includes additional changed files not documented: `diag.txt`, `docs/implementation-artifacts/epic-2-retro-2026-03-07.md`.
- This reduces traceability of what was touched during the story cycle.

### 5) [LOW] Test run is not warning-free

- `cargo test --workspace` reports an unused import warning in `crates/hprof-parser/src/indexer/precise.rs:77`.
- Not a functional defect, but should be cleaned to keep CI output noise-free.

## Recommended Actions

1. Implement Task 9.1 fully: derive and display root-var class label after first successful expansion.
2. Update `collection_entry_count` to use case-insensitive `ends_with` matching for configured suffixes.
3. Add a post-pass backfill for `class_names_by_id` (or deferred lookup path) so class names do not depend on parse ordering.
4. Update story File List to include all files touched in the working tree.
5. Remove unused test import in `precise.rs`.
