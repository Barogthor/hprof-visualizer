# Code Review Report — Story 3.7

- Story: `docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md`
- Story status at review time: `review`
- Reviewer: Codex
- Date: 2026-03-08
- Outcome: **Changes Requested**

## Scope Reviewed

- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/test_utils.rs`
- `crates/hprof-engine/src/engine.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-tui/src/views/stack_view.rs`
- `crates/hprof-tui/src/app.rs`
- `docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md`
- `docs/implementation-artifacts/sprint-status.yaml`

## Validation Executed

- `cargo test --workspace` (pass)
- `cargo clippy --workspace -- -D warnings` (pass)
- `cargo fmt --check` (pass)

Note: `cargo test --workspace` emits test-target warnings in `crates/hprof-tui/src/app.rs`, but no failures.

## Git vs Story File List

- Working tree code deltas for this story are already committed (no staged/unstaged code changes).
- Story commit `0ac20c5` changed exactly the files listed in Story 3.7 file list.
- Discrepancy count: **0**.

## Acceptance Criteria Audit

1. AC1 (real thread names from `ROOT_THREAD_OBJ`): **Implemented** (`crates/hprof-parser/src/indexer/first_pass.rs:649`, `crates/hprof-engine/src/engine_impl.rs:237`, `crates/hprof-engine/src/engine_impl.rs:754`).
2. AC2 (synthetic threads resolve via `ROOT_THREAD_OBJ` without `START_THREAD`): **Implemented** (`crates/hprof-parser/src/indexer/first_pass.rs:410`, `crates/hprof-parser/src/indexer/first_pass.rs:440`).
3. AC3 (fallback `Thread-{serial}` when thread object missing): **Implemented** (`crates/hprof-engine/src/engine_impl.rs:231`, `crates/hprof-engine/src/engine_impl.rs:804`).
4. AC4 (frame root correlation bug fixed for no-`START_THREAD` dumps): **Implemented** (`crates/hprof-parser/src/indexer/first_pass.rs:414`, `crates/hprof-parser/src/indexer/first_pass.rs:1661`).
5. AC5 (local variable displays resolved class name): **Implemented** (`crates/hprof-engine/src/engine_impl.rs:343`, `crates/hprof-tui/src/views/stack_view.rs:696`, `crates/hprof-engine/src/engine_impl.rs:698`).
6. AC6 (`Object` fallback when instance missing): **Implemented** (`crates/hprof-engine/src/engine_impl.rs:353`, `crates/hprof-engine/src/engine_impl.rs:727`).
7. AC7 (manual e2e on `heapdump-visualvm.hprof` and `heapdump-rustrover.hprof`): **Missing / Pending** (`docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:77`, `docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:78`).

## Findings

### 1) [HIGH] AC7 is still pending, but parent Task 5 is marked complete

- Task 5 is checked complete (`docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:73`).
- Required manual e2e subtasks remain unchecked (`docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:77`, `docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:78`).
- Impact: story is not actually complete against all acceptance criteria.

### 2) [MEDIUM] Story completion metadata is internally inconsistent

- Completion notes explicitly state manual e2e is pending (`docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:228`).
- Change log still says "all 5 tasks complete" (`docs/implementation-artifacts/3-7-real-thread-names-and-local-variable-resolution.md:231`).
- Impact: review/readiness signals are ambiguous for sprint tracking.

### 3) [MEDIUM] Truncated `ROOT_THREAD_OBJ` parsing breaks silently without warning

- In `ROOT_THREAD_OBJ` parsing, truncated reads break the heap sub-record loop with no warning (`crates/hprof-parser/src/indexer/first_pass.rs:650`, `crates/hprof-parser/src/indexer/first_pass.rs:653`, `crates/hprof-parser/src/indexer/first_pass.rs:656`).
- Nearby `GC_ROOT_JAVA_FRAME` path emits explicit truncation warnings (`crates/hprof-parser/src/indexer/first_pass.rs:613`, `crates/hprof-parser/src/indexer/first_pass.rs:623`, `crates/hprof-parser/src/indexer/first_pass.rs:633`).
- Impact: degraded dumps lose diagnosability and can hide why name resolution partially fails.

### 4) [LOW] Heap-based thread-name resolution lacks class/shape validation

- `resolve_thread_name_from_heap()` accepts any object with a field named `name` and attempts `resolve_string` (`crates/hprof-engine/src/engine_impl.rs:237`, `crates/hprof-engine/src/engine_impl.rs:243`).
- There is no guard that the object is actually a `Thread`-compatible instance before trusting the `name` field.
- Impact: malformed dumps could produce incorrect thread names.

### 5) [LOW] Stack expansion indicators use Unicode arrows despite ASCII-only UX guidance

- UI strings render Unicode indicators (`crates/hprof-tui/src/views/stack_view.rs:582`, `crates/hprof-tui/src/views/stack_view.rs:633`, `crates/hprof-tui/src/views/stack_view.rs:697`, `crates/hprof-tui/src/views/stack_view.rs:704`).
- UX spec defines ASCII-only indicators (`docs/planning-artifacts/ux-design-specification.md:451`, `docs/planning-artifacts/ux-design-specification.md:452`).
- Impact: minor consistency/portability drift across terminal environments.

## Recommended Actions

1. Complete manual AC7 e2e checks and update task checkboxes/status based on actual results.
2. Reconcile story metadata (Task 5 checkbox + changelog wording) so completion signals are consistent.
3. Emit explicit warnings for truncated `ROOT_THREAD_OBJ` parsing (same pattern as `GC_ROOT_JAVA_FRAME`).
4. Add defensive class/shape checks in heap-based thread-name resolution path.
5. If UX spec enforcement is desired, normalize stack indicators to ASCII (`>`/`v`).
