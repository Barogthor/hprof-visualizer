# Code Review Report — Story 3.8

- Story: `docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md`
- Story status at review time: `review`
- Reviewer: Codex
- Date: 2026-03-08
- Outcome: **Changes Requested**

## Scope Reviewed

- `crates/hprof-parser/src/indexer/segment.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/indexer/precise.rs`
- `crates/hprof-parser/src/hprof_file.rs`
- `crates/hprof-engine/src/engine_impl.rs`
- `crates/hprof-engine/src/lib.rs`
- `crates/hprof-tui/src/progress.rs`
- `crates/hprof-cli/src/main.rs`
- `docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md`
- `docs/implementation-artifacts/sprint-status.yaml`

## Validation Executed

- `cargo test --workspace` (pass)
- `cargo clippy --workspace -- -D warnings` (pass)
- `cargo fmt --check` (pass)

Note: test compilation emits non-failing warnings in `crates/hprof-tui/src/app.rs` (unused import + dead code in test-only items).

## Git vs Story File List

- Working tree code deltas for this story are already committed (no staged/unstaged code changes).
- Story commit identified from story history: `d199713`.
- Discrepancies found between Story 3.8 File List and commit file set: **7**.
  - Changed in commit but missing from story list: `Cargo.lock`, `crates/hprof-tui/src/views/stack_view.rs`, `docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md`, `docs/implementation-artifacts/sprint-status.yaml`, `docs/report/parsing-phases-analysis.md`.
  - Listed in story but not changed in commit: `crates/hprof-parser/src/indexer/mod.rs`, `crates/hprof-parser/Cargo.toml`.

## Acceptance Criteria Audit

1. AC1 (inline segment filter build + free raw IDs): **Implemented** (`crates/hprof-parser/src/indexer/segment.rs:73`, `crates/hprof-parser/src/indexer/segment.rs:85`).
2. AC2 (peak memory bounded to one segment's raw ID vectors): **Partial** (raw vectors are bounded, but a full-file temporary offset map is introduced: `crates/hprof-parser/src/indexer/first_pass.rs:109`, `crates/hprof-parser/src/indexer/first_pass.rs:724`).
3. AC3 (parallel segment filter construction with rayon): **Missing** (`docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:34`, `crates/hprof-parser/src/indexer/segment.rs:83`, `crates/hprof-parser/Cargo.toml:6`).
4. AC4 (direct-offset thread resolution replacing linear scans): **Partial** (thread object lookup is offset-aware, but name String + backing array still fall back to scans: `crates/hprof-parser/src/indexer/first_pass.rs:482`, `crates/hprof-engine/src/engine_impl.rs:340`, `crates/hprof-engine/src/engine_impl.rs:351`, `crates/hprof-engine/src/engine_impl.rs:368`).
5. AC5 (single unified scan/filter progress phase): **Implemented** (`crates/hprof-cli/src/main.rs:35`, `crates/hprof-tui/src/progress.rs:19`).
6. AC6 (parallel thread resolution): **Implemented** (`crates/hprof-engine/src/engine_impl.rs:232`).
7. AC7 (1 GB release load under 5s): **Missing** (`docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:52`, `docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:96`).
8. AC8 (tests pass, no regressions): **Implemented** (validation run passed).

## Findings

### 1) [HIGH] AC3 is not implemented: segment filter build remains sequential

- AC3 explicitly requires rayon-based parallel segment filter construction (`docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:34`).
- Actual implementation finalizes filters synchronously in-place (`crates/hprof-parser/src/indexer/segment.rs:83`) and parser crate has no rayon dependency (`crates/hprof-parser/Cargo.toml:6`).
- Impact: one acceptance criterion is not delivered while Task 4 is marked complete.

### 2) [HIGH] AC4 is only partially implemented: thread name chain still hits linear scans

- First pass stores offsets only for thread object IDs (`crates/hprof-parser/src/indexer/first_pass.rs:482`).
- Name resolution follows Thread -> String -> char[]/byte[] (`crates/hprof-engine/src/engine_impl.rs:368`, `crates/hprof-engine/src/engine_impl.rs:380`) and falls back to scan-based methods when offsets are absent (`crates/hprof-engine/src/engine_impl.rs:340`, `crates/hprof-engine/src/engine_impl.rs:351`).
- Impact: the expensive part of name resolution is still scan-based for many threads, which undermines the <1ms/thread objective in AC4.

### 3) [HIGH] AC7 performance target is unmet but Task 5 is marked complete

- AC7 target is under 5 seconds (`docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:52`).
- Reported benchmark is 8.4s for the 1.1 GB dump (`docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:96`).
- Impact: completion status overstates readiness against an explicit acceptance criterion.

### 4) [MEDIUM] Temporary `all_offsets` map scales with full heap and weakens memory optimization

- First pass allocates a global object-id -> offset map for all instances/arrays (`crates/hprof-parser/src/indexer/first_pass.rs:109`, `crates/hprof-parser/src/indexer/first_pass.rs:724`, `crates/hprof-parser/src/indexer/first_pass.rs:742`, `crates/hprof-parser/src/indexer/first_pass.rs:760`).
- Only thread object IDs are retained in final index (`crates/hprof-parser/src/indexer/first_pass.rs:482`).
- Impact: large temporary memory footprint during indexing, which conflicts with the story's memory-reduction intent.

### 5) [MEDIUM] Parallel thread-name progress reporting is effectively final-only

- Parallel resolver increments `done` (`crates/hprof-engine/src/engine_impl.rs:251`) but does not emit intermediate callbacks; only `progress_fn(total, total)` is called at the end (`crates/hprof-engine/src/engine_impl.rs:257`).
- CLI wiring expects progressive updates (`crates/hprof-cli/src/main.rs:43`) and reporter API is designed for per-step updates (`crates/hprof-tui/src/progress.rs:77`).
- Impact: progress UX is inaccurate for long-running thread resolution phases.

### 6) [MEDIUM] Story File List is inaccurate versus implementation commit

- Story list includes non-changed files and omits changed files (`docs/implementation-artifacts/3-8-inline-filters-and-optimized-thread-resolution.md:210`).
- Impact: reviewability and traceability are reduced; future audits become error-prone.

## Recommended Actions

1. Implement or explicitly de-scope AC3 (parallel filter build), then reconcile Tasks/ACs.
2. Complete offset indexing for thread name transitives (Thread `name` String + `value` backing array) to remove scan fallback in the hot path.
3. Resolve AC7 mismatch: either optimize to <5s on 1 GB release or formally revise the target.
4. Replace full `all_offsets` accumulation with selective indexing to reduce peak temporary memory.
5. Fix progress callback plumbing so thread-name progress updates are incremental and accurate.
6. Align Story 3.8 File List with the actual commit file set.
