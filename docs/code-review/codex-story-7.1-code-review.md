# Code Review - Story 7.1 (Favorites Panel)

Date: 2026-03-10
Reviewer: Codex (adversarial review)
Story file: `docs/implementation-artifacts/7-1-favorites-panel.md`

## Scope

- Reviewed all files listed in the story `Dev Agent Record -> File List`.
- Cross-checked story claims vs current git working tree state.
- Ran validation commands:
  - `cargo test -p hprof-tui`
  - `cargo clippy -p hprof-tui --all-targets -- -D warnings`
  - `cargo fmt -- --check`

All tests and checks pass in the current workspace state.

## Findings

### HIGH - Snapshot object limit is not actually enforced

- `SNAPSHOT_OBJECT_LIMIT` is defined as 500, but traversal does not stop at 500 objects.
- `subtree_snapshot` calls `collect_descendants` without a limit guard, then only checks `reachable.len() >= SNAPSHOT_OBJECT_LIMIT` after traversal.
- This can produce snapshots larger than 500 objects, violating the story requirement to stop traversal at the limit.

Evidence:

- `crates/hprof-tui/src/favorites.rs:16`
- `crates/hprof-tui/src/favorites.rs:93`
- `crates/hprof-tui/src/favorites.rs:95`
- `crates/hprof-tui/src/favorites.rs:100`
- `crates/hprof-tui/src/views/stack_view.rs:194`

### HIGH - Snapshot freeze semantics for collection chunks are incomplete

- Story requires frozen snapshots where loading chunks become collapsed placeholders at pin time.
- Current implementation clones `collection_chunks` as-is.
- If a chunk is `Loading` when pinned, favorites may render transient loading state instead of a frozen snapshot.

Evidence:

- `crates/hprof-tui/src/favorites.rs:104`
- `crates/hprof-tui/src/favorites.rs:105`
- `crates/hprof-tui/src/views/stack_view.rs:35`
- `crates/hprof-tui/src/views/tree_render.rs:312`

### MEDIUM - Hidden favorites panel can keep keyboard focus after resize

- Favorites visibility is width-gated at render time.
- Input routing remains based on `self.focus`.
- If focus is `Favorites` and terminal shrinks below 120 columns, panel is hidden but input still routes to favorites logic until user manually exits with `Esc`/`F`.

Evidence:

- `crates/hprof-tui/src/app.rs:161`
- `crates/hprof-tui/src/app.rs:812`
- `crates/hprof-tui/src/app.rs:818`

### HIGH - Story status is `done` while all tasks remain unchecked

- Story metadata says `Status: done`, but task list still shows unchecked items (`[ ]`) for all major tasks/subtasks.
- This is a delivery governance mismatch and weakens traceability of completion claims.

Evidence:

- `docs/implementation-artifacts/7-1-favorites-panel.md:3`
- `docs/implementation-artifacts/7-1-favorites-panel.md:82`

### MEDIUM - Git vs story traceability mismatch in current workspace

- Current git working tree contains untracked files not documented in Story 7.1 file list.
- Story file list claims changed source files, but there are no current staged/unstaged changes for those files in this workspace state.
- This makes incremental-change auditing ambiguous from the current branch snapshot.

Evidence:

- `git status --porcelain` output:
  - `docs/implementation-artifacts/7-3-keyboard-navigation-map.md`
  - `docs/story-review/claude-story-7-3-keyboard-navigation-map.md`
- `docs/implementation-artifacts/7-1-favorites-panel.md:551`

## Acceptance Criteria Audit (quick)

- Initial review: AC #12 was partial due to snapshot freeze semantics.
- After auto-fix: AC #1-#13 are covered by implementation and tests in current workspace state.

## Recommendation

1. Fix the two HIGH implementation issues first (object-limit enforcement and chunk freeze semantics).
2. Auto-correct story status/task checkboxes and file-list traceability in the story document.
3. Add a resize safety guard to exit `Focus::Favorites` when panel becomes hidden.

## Auto-Fix Outcome

Applied after review in this session:

- Fixed: hard cap enforcement for `SNAPSHOT_OBJECT_LIMIT` during snapshot traversal.
- Fixed: `ChunkState::Loading` is frozen to `ChunkState::Collapsed` in pinned snapshots.
- Fixed: hidden favorites panel now auto-restores focus from `Favorites` to `prev_focus`.
- Updated story status to `in-progress` and synced sprint status accordingly.

Remaining note:

- Git/story discrepancy caused by unrelated untracked Story 7.3 docs still exists in the workspace.
