# Code Review - Story 9.2

- Story: `docs/implementation-artifacts/9-2-collection-data-fidelity.md`
- Reviewer: Codex
- Date: 2026-03-10
- Outcome: Changes Requested

## Scope Reviewed

- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-engine/src/pagination.rs`
- `crates/hprof-tui/src/app.rs`
- `crates/hprof-tui/src/favorites.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/views/tree_render.rs`
- `docs/implementation-artifacts/9-2-collection-data-fidelity.md`
- `docs/implementation-artifacts/sprint-status.yaml`

## Validation Snapshot

- `cargo test --all`: PASS
- `cargo clippy --all-targets -- -D warnings`: PASS
- Git working tree changes (uncommitted): only `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md` is untracked

## Findings

### HIGH

1. **Story claims changed files without matching git evidence (traceability break).**
   - Story `File List` declares 9 touched files including source files and sprint tracking (`docs/implementation-artifacts/9-2-collection-data-fidelity.md:718`).
   - Current git uncommitted changes do not include any of those files (`git status --porcelain` only reports `docs/implementation-artifacts/9-3-arrow-expand-unexpand-parent-navigation.md`).
   - Impact: review cannot verify "what changed for this story" from current branch state; this violates reviewability and makes audit difficult.

### MEDIUM

2. **Task tracking is inconsistent with implemented test coverage.**
   - Story keeps one subtask unchecked (`docs/implementation-artifacts/9-2-collection-data-fidelity.md:524`) for ArrayList `get_page` coverage.
   - Equivalent test exists and passes: `crates/hprof-engine/src/pagination.rs:1230` (`get_page_with_arraylist_instance_id_returns_elements`).
   - Impact: task board is stale and no longer reliable for completion status.

3. **Story metadata sections required for review bookkeeping are missing.**
   - No `Senior Developer Review (AI)` section present in story file.
   - No `Change Log` section entry present for this review pass.
   - Impact: review history is not persisted inside the story artifact.

4. **Sprint sync evidence declared in File List is not verifiable from current branch state.**
   - Story claims `docs/implementation-artifacts/sprint-status.yaml` was updated (`docs/implementation-artifacts/9-2-collection-data-fidelity.md:728`).
   - No uncommitted diff exists for this file in current branch state.
   - Impact: status synchronization cannot be audited from local git reality.

## Acceptance Criteria Audit (Implementation Evidence)

- AC1-AC4: implemented via `entry_count` propagation + `OnVar` `StartCollection` dispatch + array fallback chain in pagination.
- AC5: collapsed label formatting path uses `format_object_ref_collapsed(..., entry_count)`.
- AC6: nested arrays in collection entries now dispatch `StartCollection` through `selected_collection_entry_count()`.
- AC7: full test suite passes.

## Recommended Next Actions

1. Update story artifact to align with current code truth (task checkbox + review record + change log).
2. Decide whether to treat this review as against committed history (PR diff) instead of uncommitted git state.
3. If staying on local-state workflow, ensure story `File List` and sprint sync claims are regenerated from actual branch diffs.
