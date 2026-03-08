# Code Review - Stories 3.1 to 3.3

**Date:** 2026-03-07  
**Reviewer:** Codex (Amelia / Dev Agent execution)

## Scope

- `docs/implementation-artifacts/3-1-navigation-engine-trait-and-engine-factory.md`
- `docs/implementation-artifacts/3-2-thread-list-and-search-in-tui.md`
- `docs/implementation-artifacts/3-3-stack-frame-and-local-variable-display.md`

Reviewed implementation files:

- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-engine/src/lib.rs`
- `crates/hprof-engine/Cargo.toml`
- `crates/hprof-parser/src/hprof_file.rs`
- `crates/hprof-parser/src/java_types.rs`
- `crates/hprof-parser/src/lib.rs`
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/indexer/segment.rs`
- `crates/hprof-parser/src/test_utils.rs`
- `crates/hprof-tui/src/app.rs`
- `crates/hprof-tui/src/input.rs`
- `crates/hprof-tui/src/theme.rs`
- `crates/hprof-tui/src/views/thread_list.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/views/status_bar.rs`
- `crates/hprof-tui/src/views/mod.rs`
- `crates/hprof-tui/src/lib.rs`
- `crates/hprof-cli/src/main.rs`
- workspace manifests (`Cargo.toml`, `crates/hprof-tui/Cargo.toml`, `crates/hprof-cli/Cargo.toml`)

## Executive Result

| Severity | Count |
|----------|-------|
| High     | 1     |
| Medium   | 3     |
| Low      | 0     |

Stories 3.1 and most of 3.3 are implemented with good test coverage. Story 3.2 has a functional regression on browse-and-preview behavior, and there are reliability/observability gaps in terminal setup and tolerant parsing.

## Findings

### H1 - Story 3.2 browse-and-preview AC is not implemented (Enter is required)

**Why it matters**

Story 3.2 AC2 requires real-time stack preview while moving in the thread list (no Enter). Current behavior only loads frames on Enter.

**Evidence**

- AC definition requires browse-and-preview with no Enter: `docs/implementation-artifacts/3-2-thread-list-and-search-in-tui.md:20`.
- Frames are loaded only on `InputEvent::Enter`: `crates/hprof-tui/src/app.rs:107`.
- While browsing thread list, render path uses `stack_state` only; when `None`, it renders an empty stack panel and does not query engine frames for current selection: `crates/hprof-tui/src/app.rs:178`.

**Impact**

- Users cannot scan threads by moving selection and reading live stack previews.
- Thread discovery workflow is slower and diverges from 3.2 acceptance criteria.

**Recommended fix**

- Keep an always-updated preview state in ThreadList focus (or query frames for selected serial during render), while preserving Enter to transition focus into StackFrames.

---

### M1 - Terminal cleanup is not guaranteed if `Terminal::new` fails

**Why it matters**

`run_tui` enables raw mode and alternate screen before creating the guard. If terminal initialization fails at that point, cleanup does not run.

**Evidence**

- Raw mode + alternate screen are enabled before terminal construction: `crates/hprof-tui/src/app.rs:234`.
- Guard is created only after `Terminal::new` succeeds: `crates/hprof-tui/src/app.rs:241`.

**Impact**

- On init errors, the user terminal can remain in raw/alternate state.

**Recommended fix**

- Create cleanup guard immediately after entering alternate screen, or add explicit cleanup on every pre-guard error path.

---

### M2 - Stack line metadata is dropped when source file is unavailable

**Why it matters**

Line metadata (`(?)`, `(compiled)`, `(native)`, `:N`) should remain visible even when source file name is missing/unresolved.

**Evidence**

- Line label is computed: `crates/hprof-tui/src/views/stack_view.rs:151`.
- But line label is only appended inside source-file block; if source file is empty, both source and line metadata are dropped: `crates/hprof-tui/src/views/stack_view.rs:158`.

**Impact**

- Native/compiled/unknown line states can disappear from UI.
- AC1 display completeness is weakened for unresolved source-file cases.

**Recommended fix**

- Render `line_label` independently of `source_file` (e.g., `Class.method() (native)` when source is empty).

---

### M3 - Corrupted `GC_ROOT_JAVA_FRAME` sub-records are silently discarded (no warning)

**Why it matters**

Tolerant parsing should surface non-fatal corruption as warnings. Current behavior silently breaks out of heap sub-record parsing on `GC_ROOT_JAVA_FRAME` read errors.

**Evidence**

- `GC_ROOT_JAVA_FRAME` branch uses `break` on partial reads without warning emission: `crates/hprof-parser/src/indexer/first_pass.rs:521`.
- Story notes require warning on read errors for this sub-record path: `docs/implementation-artifacts/3-3-stack-frame-and-local-variable-display.md:214`.

**Impact**

- Lost observability: users cannot tell local-variable roots were dropped due to corruption.
- Harder diagnosis for partial/truncated dumps.

**Recommended fix**

- Thread warning collection into `extract_heap_object_ids` and push a warning before terminating the sub-record loop.

## AC Validation Snapshot

- **Story 3.1:** AC1-AC4 implemented. Trait surface and factory pattern are present; CLI dependency boundary (`hprof-cli` -> `hprof-engine`, no direct parser dependency) is respected.
- **Story 3.2:** AC1/3/4/5/6/7 mostly implemented; AC2 (browse-and-preview without Enter) is not met (H1).
- **Story 3.3:** Core frame/local-variable pipeline is implemented end-to-end; line metadata rendering has an edge-case display gap when source file is missing (M2).

## Git vs Story Notes

- Current working tree has no tracked code deltas for these stories (already committed).
- One unrelated untracked file exists: `docs/implementation-artifacts/epic-2-retro-2026-03-07.md`.
- Review is based on current source state versus story requirements (not uncommitted diff audit).

## Validation Commands Run

- `git status --porcelain`
- `git diff --name-only`
- `git diff --cached --name-only`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo fmt -- --check`
