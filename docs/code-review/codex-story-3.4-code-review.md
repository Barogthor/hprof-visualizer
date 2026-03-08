# Code Review Report — Story 3.4

Date: 2026-03-07  
Reviewer: Codex (adversarial review)

## Scope

- Story file: `docs/implementation-artifacts/3-4-object-resolution-and-single-level-expansion.md`
- Claimed implementation files reviewed from Dev Agent Record File List (14 files)
- Additional related file reviewed for AC verification: `crates/hprof-parser/src/indexer/segment.rs`

## Git vs Story Discrepancy Audit

- Git repository detected: yes
- `git diff --name-only`: no tracked source file changes
- `git diff --cached --name-only`: none
- `git status --porcelain`: untracked `diag.txt`, untracked `docs/implementation-artifacts/epic-2-retro-2026-03-07.md`

Assessment:

1. Story File List claims code changes across many files, but there are no corresponding uncommitted tracked changes in git.
2. Two untracked files exist and are not listed in the story File List (documentation/transparency gap; not application source changes).

## Findings

### HIGH

1) **Incorrect skip length for heap sub-tag `0x08` (`GC_ROOT_THREAD_OBJECT`)**

- Evidence: `crates/hprof-parser/src/indexer/first_pass.rs:606`, `crates/hprof-parser/src/hprof_file.rs:250`
- Current behavior skips only `id_size` bytes.
- Expected per story guidance is `id_size + 8` bytes for this sub-record kind.
- Impact: cursor desynchronization while scanning heap payloads; can cause missed objects, premature scan termination, and false negatives in object resolution.

2) **Potential infinite recursion in class hierarchy traversal on corrupted cyclic metadata**

- Evidence: `crates/hprof-engine/src/resolver.rs:51`
- `collect_fields` only guards `super_class_id != class_id`; it does not guard multi-node cycles (A -> B -> A).
- Impact: stack overflow / crash risk on malformed heap metadata, conflicting with robustness goals (NFR6).

### MEDIUM

3) **Enter key cancels/collapses while object is Loading instead of no-op**

- Evidence: `crates/hprof-tui/src/app.rs:239`, `crates/hprof-tui/src/app.rs:264`
- On `StackCursor::OnVar`, logic maps all non-collapsed/non-failed phases to `CollapseObj`, including `Loading`.
- Story implementation notes for Task 9 specify Loading Enter should be no-op.
- Impact: user can accidentally cancel an in-flight expansion by pressing Enter on the loading parent row.

4) **Failure message text diverges from story AC wording**

- Evidence: story AC text `docs/implementation-artifacts/3-4-object-resolution-and-single-level-expansion.md:33`, runtime message source `crates/hprof-tui/src/app.rs:316`, rendering `crates/hprof-tui/src/views/stack_view.rs:449`
- AC expects `! Failed to resolve object`; current path produces `! Object not found` for one failure mode.
- Impact: acceptance mismatch and inconsistent user-facing error semantics.

5) **Task tracking inconsistency in story file (implementation done but checklist not aligned)**

- Evidence: unchecked task at `docs/implementation-artifacts/3-4-object-resolution-and-single-level-expansion.md:510` vs implemented trait signature in `crates/hprof-engine/src/engine.rs:157`
- Impact: auditability issue; reviewer cannot trust task checkboxes as source of truth.

### LOW

6) **Manual smoke test remains unchecked with no execution evidence in Dev Agent Record**

- Evidence: `docs/implementation-artifacts/3-4-object-resolution-and-single-level-expansion.md:921`
- Impact: one key UX flow (async loading + cancel) lacks manual verification trace despite story being in review.

## Acceptance Criteria Status (code-level)

- AC1 (async non-blocking expansion): **PARTIAL** (implemented async worker + polling, but Enter behavior while Loading is inconsistent)
- AC2 (loading pseudo-node then real children): **IMPLEMENTED**
- AC3 (Escape on loading pseudo-node cancels): **IMPLEMENTED**
- AC4 (failure pseudo-node): **PARTIAL** (mechanism implemented; user-facing text mismatch)
- AC5 (BinaryFuse8 + targeted scan): **PARTIAL** (overall architecture implemented, but skip-length bug on sub-tag `0x08` can break targeted scans in real files)

## Test Execution

- `cargo test -p hprof-parser` ✅
- `cargo test -p hprof-engine` ✅
- `cargo test -p hprof-tui` ✅

No failing automated tests were observed, but gaps above are still materially relevant.

## Outcome

**Changes Requested**

- Must-fix before approval: Findings 1 and 2
- Should-fix before merge: Findings 3 and 4
- Process hygiene fixes: Findings 5 and 6
