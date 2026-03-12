# Adversarial Code Review - Story 9.4 (Camera Scroll)

Date: 2026-03-12
Reviewer: Codex (dev agent / CR workflow)
Story: `docs/implementation-artifacts/9-4-camera-scroll.md`

## Scope Reviewed

- Story requirements and completion claims in `docs/implementation-artifacts/9-4-camera-scroll.md`
- Implementation files listed in Dev Agent Record File List
- Workspace git reality (`git status --porcelain`, `git diff --name-only`, `git diff --cached --name-only`)

## AC Validation (Story 9.4)

- AC1 (`Ctrl+Down` camera scroll without cursor move): implemented in `crates/hprof-tui/src/input.rs:61`, `crates/hprof-tui/src/app/mod.rs:459`, `crates/hprof-tui/src/views/stack_view/state.rs:792`.
- AC2 (`Ctrl+Up` camera scroll without cursor move): implemented in `crates/hprof-tui/src/input.rs:60`, `crates/hprof-tui/src/app/mod.rs:454`, `crates/hprof-tui/src/views/stack_view/state.rs:762`.
- AC3 (snap-back keeps selection visible): implemented in `crates/hprof-tui/src/views/stack_view/state.rs:782` and `crates/hprof-tui/src/views/stack_view/state.rs:810`.
- AC4 (help panel entries): implemented in `crates/hprof-tui/src/views/help_bar.rs:26` and `crates/hprof-tui/src/views/help_bar.rs:27`.
- AC5 (`cargo test --all` no regressions): verified in this review run (full suite passing).

## Findings

### HIGH

1. Story File List does not match git working tree reality for this review context.
   - Story claims changed files: `docs/implementation-artifacts/9-4-camera-scroll.md:601`..`docs/implementation-artifacts/9-4-camera-scroll.md:608`.
   - Current git changes are different (`crates/hprof-engine/src/pagination/tests.rs`, `tools/hprof-redact-custom/src/main/java/io/hprofvisualizer/redact/PathOnlyTransformer.java`, plus untracked docs).
   - Per workflow rule, this is a traceability gap: files listed in story with no corresponding git delta, and git deltas not reflected in story File List.

### MEDIUM

2. Potential overflow panic in `scroll_view_down` on stale/invalid offset.
   - `crates/hprof-tui/src/views/stack_view/state.rs:807` computes `(offset + 1)` before clamping.
   - If `offset == usize::MAX` (possible from future state corruption or test setup), debug builds panic on overflow.
   - `scroll_view_up` already defensively clamps stale offset first (`crates/hprof-tui/src/views/stack_view/state.rs:774`), so behavior is inconsistent.

3. No app-level tests for `CameraScrollUp/CameraScrollDown` routing behavior.
   - Handlers exist in `crates/hprof-tui/src/app/mod.rs:454` and `crates/hprof-tui/src/app/mod.rs:459`.
   - CameraScroll tests currently cover key mapping only (`crates/hprof-tui/src/input.rs:241`, `crates/hprof-tui/src/input.rs:249`).
   - Missing explicit tests for focus-gating/no-op behavior outside stack panel and while search is active.

### LOW

4. Help panel documentation drift and stale user-facing annotation.
   - Comment in `crates/hprof-tui/src/views/help_bar.rs:46` still says `ENTRY_COUNT = 13`, but constant is `15` (`crates/hprof-tui/src/views/help_bar.rs:17`).
   - `TODO(7.1)` remains in user help entries (`crates/hprof-tui/src/views/help_bar.rs:32`) with labels still containing `(Story 7.1)` (`crates/hprof-tui/src/views/help_bar.rs:33`, `crates/hprof-tui/src/views/help_bar.rs:34`).

## Test/Lint Verification Run During Review

- `cargo test --all`: pass
- `cargo clippy --all-targets -- -D warnings`: pass

## Resolution (Post-Fix)

- HIGH-1: addressed by updating story traceability and explicit git-reality notes in `docs/implementation-artifacts/9-4-camera-scroll.md`.
- MEDIUM-2: fixed by clamping stale offset before increment in `crates/hprof-tui/src/views/stack_view/state.rs` and adding regression test `scroll_view_down_clamps_stale_offset_before_increment`.
- MEDIUM-3: fixed by adding app-level routing/no-op coverage for camera scroll in `crates/hprof-tui/src/app/tests.rs`.
- LOW-4: fixed by cleaning stale help labels and updating entry-count doc in `crates/hprof-tui/src/views/help_bar.rs`.
