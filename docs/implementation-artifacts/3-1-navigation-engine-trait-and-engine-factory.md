# Story 3.1: Navigation Engine Trait & Engine Factory

Status: done

## Story

As a developer,
I want a `NavigationEngine` trait defining the high-level API and an `Engine` factory that
constructs the engine from a file path and config,
So that the TUI frontend can consume a clean API without knowing about parser internals.

## Acceptance Criteria

1. **Given** the `hprof-engine` crate
   **When** I inspect the `NavigationEngine` trait
   **Then** it defines methods: `list_threads()`, `select_thread(id)`, `get_stack_frames(thread_id)`,
   `get_local_variables(frame_id)`, `expand_object(object_id)`, `get_page(collection_id, offset, limit)`

2. **Given** a valid hprof file path and config
   **When** I call `Engine::from_file(path, config)`
   **Then** the engine internally creates an `HprofFile` (mmap + indexes), runs the first pass,
   and returns a ready-to-use engine implementing `NavigationEngine`

3. **Given** the `hprof-cli` crate
   **When** I inspect its dependencies
   **Then** it depends on `hprof-engine` but NOT on `hprof-parser` — parser types are engine-internal

4. **Given** the engine is constructed from a synthetic hprof file with 3 threads
   **When** I call `list_threads()`
   **Then** it returns exactly 3 threads with their names resolved from structural strings (FR9)

## Tasks / Subtasks

- [x] Add `engine.rs` to `crates/hprof-engine/src/` — trait + view model types (AC: #1, #4)
  - [x] **Red**: Write test (compile-only) — `NavigationEngine` trait exists with all 6 methods
  - [x] **Green**: Define view model types:
    - `pub struct ThreadInfo { pub thread_serial: u32, pub name: String }`
    - `pub struct FrameInfo {}` — placeholder, Story 3.3
    - `pub struct VariableInfo {}` — placeholder, Story 3.3
    - `pub struct FieldInfo {}` — placeholder, Story 3.4
    - `pub struct EntryInfo {}` — placeholder, Story 4.2
  - [x] **Green**: Define `NavigationEngine` trait with all 6 methods and correct signatures
        (see Dev Notes for exact signatures)
  - [x] Add `//!` module docstring

- [x] Add `engine_impl.rs` to `crates/hprof-engine/src/` — `Engine` struct + factory + impl (AC: #2, #3, #4)
  - [x] **Red**: Write test — `Engine::from_file` on missing path returns `Err(HprofError::MmapFailed(_))`
  - [x] **Red**: Write test — `Engine::from_file` on valid hprof file returns `Ok(engine)`
  - [x] **Red**: Write test — `list_threads()` on an engine from a file with 0 threads returns empty vec
  - [x] **Red** (test-utils): Write test — `list_threads()` on file with 3 named threads returns exactly
        3 `ThreadInfo` values with correct `thread_serial` and resolved `name`
  - [x] **Red** (test-utils): Write test — `list_threads()` when `name_string_id` is unknown (no matching
        string record) returns a placeholder name `"<unknown:{id}>"` — not a panic
  - [x] **Red**: Write test — `select_thread(serial)` returns `Some(ThreadInfo)` for an existing thread
        and `None` for a non-existent serial
  - [x] **Green**: Define `pub struct Engine { hfile: HprofFile }` (hfile is engine-internal, not public)
  - [x] **Green**: Implement `pub fn from_file(path: &Path, _config: &EngineConfig) -> Result<Self, HprofError>`
        that calls `HprofFile::from_path(path)?` and wraps the result
  - [x] **Green**: Implement `NavigationEngine for Engine`:
    - `list_threads()` — iterates `hfile.index.threads`, resolves names via `hfile.index.strings`,
      returns sorted `Vec<ThreadInfo>` (sorted by `thread_serial` for deterministic ordering)
    - `select_thread(thread_serial)` — looks up in `hfile.index.threads`, resolves name, returns `Option`
    - Other 4 methods — return `Vec::new()` stubs (will be implemented in Stories 3.3–4.2)
  - [x] Add `//!` module docstring

- [x] Add `EngineConfig` to `crates/hprof-engine/src/lib.rs` (AC: #2)
  - [x] **Red**: Compile check — `hprof_engine::EngineConfig` exists and can be constructed
        with `EngineConfig::default()`
  - [x] **Green**: Define `pub struct EngineConfig;` with `impl Default for EngineConfig`
        (unit struct, placeholder for Story 6.1 TOML config)

- [x] Update `crates/hprof-engine/src/lib.rs` — declare new modules and re-export public types
  - [x] Add `pub mod engine;`
  - [x] Add `pub mod engine_impl;`
  - [x] Re-export: `NavigationEngine`, `Engine`, `EngineConfig`, `ThreadInfo`, `FrameInfo`,
        `VariableInfo`, `FieldInfo`, `EntryInfo`
  - [x] Update `//!` crate docstring to mention the new public API surface

- [x] Add `hprof-parser` test-utils to `hprof-engine` dev-dependencies (AC: #4)
  - [x] Add to `crates/hprof-engine/Cargo.toml`:
        `hprof-parser = { path = "../hprof-parser", features = ["test-utils"] }`
        under `[dev-dependencies]`

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-engine`
  - [x] `cargo test -p hprof-engine` (no test-utils feature needed — test-utils is in dev-deps)
  - [x] `cargo test --workspace`
  - [x] `cargo clippy --workspace -- -D warnings`
  - [x] `cargo fmt -- --check`

## Dev Notes

### `NavigationEngine` Trait — Exact Signatures

```rust
pub trait NavigationEngine {
    /// Returns all threads indexed in the heap dump, with names resolved
    /// from structural strings. Sorted by `thread_serial` for determinism.
    fn list_threads(&self) -> Vec<ThreadInfo>;

    /// Returns `Some(ThreadInfo)` for the given `thread_serial`, `None` if
    /// not found.
    fn select_thread(&self, thread_serial: u32) -> Option<ThreadInfo>;

    /// Returns the stack frames for the given thread serial.
    /// Stub — implemented in Story 3.3.
    fn get_stack_frames(&self, thread_serial: u32) -> Vec<FrameInfo>;

    /// Returns the local variables for the given frame ID.
    /// Stub — implemented in Story 3.3.
    fn get_local_variables(&self, frame_id: u64) -> Vec<VariableInfo>;

    /// Expands an object and returns its fields.
    /// Stub — implemented in Story 3.4.
    fn expand_object(&self, object_id: u64) -> Vec<FieldInfo>;

    /// Returns a page of entries from a collection.
    /// Stub — implemented in Story 4.2.
    fn get_page(&self, collection_id: u64, offset: usize, limit: usize) -> Vec<EntryInfo>;
}
```

### `Engine::from_file` — Exact Signature

```rust
pub fn from_file(path: &Path, _config: &EngineConfig) -> Result<Self, HprofError> {
    let hfile = HprofFile::from_path(path)?;
    Ok(Self { hfile })
}
```

The `_config` parameter is a named underscore (`_config`) to make its future use explicit without
producing a dead-code warning. When Story 6.1 adds TOML config, this becomes live.

`from_file` delegates to `HprofFile::from_path` (no-progress variant). The progress bar wiring
(story 2.6) remains in `open_hprof_file_with_progress` for CLI use until the TUI story (3.2) wires
the engine factory into the interactive flow.

### `list_threads()` — Name Resolution

```rust
fn list_threads(&self) -> Vec<ThreadInfo> {
    let mut threads: Vec<ThreadInfo> = self
        .hfile
        .index
        .threads
        .values()
        .map(|t| {
            let name = self
                .hfile
                .index
                .strings
                .get(&t.name_string_id)
                .map(|s| s.value.clone())
                .unwrap_or_else(|| format!("<unknown:{}>", t.name_string_id));
            ThreadInfo {
                thread_serial: t.thread_serial,
                name,
            }
        })
        .collect();
    threads.sort_by_key(|t| t.thread_serial);
    threads
}
```

The `<unknown:{id}>` fallback is intentional: an hprof file might have `START_THREAD` records
referencing string IDs that don't appear in the strings section (corrupted or incomplete files).
The engine must never panic in such cases (NFR6).

### `EngineConfig` Design

Unit struct for now:

```rust
/// Configuration for the navigation engine.
///
/// Currently a placeholder; Story 6.1 will populate this from TOML config
/// and CLI overrides. Implements `Default` for zero-config construction.
pub struct EngineConfig;

impl Default for EngineConfig {
    fn default() -> Self {
        Self
    }
}
```

### Cargo Dev-Dependency for test-utils

Add to `crates/hprof-engine/Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
hprof-parser = { path = "../hprof-parser", features = ["test-utils"] }
```

Cargo merges the dev-dependency entry with the existing regular dependency, enabling `test-utils`
only during testing. No conflict with the existing `hprof-parser = { path = "../hprof-parser" }`
under `[dependencies]`.

### Test Scaffolding for the 3-thread AC

The key integration test (using `test-utils`) to prove AC4:

```rust
#[cfg(all(test, feature = "test-utils"))]  // NOTE: no feature flag needed - test-utils is in dev-deps
mod builder_tests {
    // test-utils is unconditionally available in dev builds via Cargo.toml
}
```

Wait — because `test-utils` is added to dev-dependencies (not a feature of hprof-engine itself),
the test module does NOT need a `#[cfg(feature = "test-utils")]` guard. The feature is activated
by the dev-dependency declaration. Just use a regular `#[cfg(test)]` module.

The 3-thread test:

```rust
#[test]
fn list_threads_returns_three_threads_with_resolved_names() {
    use hprof_parser::HprofTestBuilder;

    let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
        .add_string(10, "main")
        .add_string(11, "worker-1")
        .add_string(12, "worker-2")
        .add_thread(1, 100, 0, 10, 0, 0)  // thread_serial=1, name_string_id=10
        .add_thread(2, 101, 0, 11, 0, 0)  // thread_serial=2, name_string_id=11
        .add_thread(3, 102, 0, 12, 0, 0)  // thread_serial=3, name_string_id=12
        .build();

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&bytes).unwrap();
    tmp.flush().unwrap();

    let config = EngineConfig::default();
    let engine = Engine::from_file(tmp.path(), &config).unwrap();
    let threads = engine.list_threads();

    assert_eq!(threads.len(), 3);
    assert_eq!(threads[0].thread_serial, 1);
    assert_eq!(threads[0].name, "main");
    assert_eq!(threads[1].thread_serial, 2);
    assert_eq!(threads[1].name, "worker-1");
    assert_eq!(threads[2].thread_serial, 3);
    assert_eq!(threads[2].name, "worker-2");
}
```

Note: `add_thread` signature is `add_thread(thread_serial, object_id, stack_trace_serial,
name_string_id, group_name_string_id, group_parent_name_string_id)`.

### Module Structure

New files created:

```
crates/hprof-engine/src/
├── lib.rs             # updated: new pub mods + re-exports + EngineConfig
├── engine.rs          # NEW: NavigationEngine trait + view model types
└── engine_impl.rs     # NEW: Engine struct + from_file() + NavigationEngine impl
```

### `hprof-cli` Dependency Check

`crates/hprof-cli/Cargo.toml` currently has:
```toml
[dependencies]
hprof-engine = { path = "../hprof-engine" }
hprof-tui    = { path = "../hprof-tui" }
```

No direct `hprof-parser` dependency — AC3 is already satisfied. No changes needed to `hprof-cli`
in this story. The CLI continues to use `open_hprof_file_with_progress` for the indexing
progress display; wiring `Engine::from_file` into the CLI interactive loop is deferred to
Story 3.2 when the TUI is built.

### `HprofFile::segment_filters` — Comment to Update

`crates/hprof-parser/src/hprof_file.rs` line 50 has:
```rust
// `SegmentFilter` is `pub(crate)` until the engine crate is built (Story 3.1).
```
This comment should be updated to remove the `(Story 3.1)` note since that milestone is now
reached. The `SegmentFilter` type remains `pub(crate)` within `hprof-parser` — the engine crate
holds an `HprofFile` and doesn't directly type-reference `SegmentFilter` yet (object resolution
via segment filters is Story 3.4). So no visibility change is needed in this story; just update
the comment.

### What NOT to Build in This Story

| Concern | Story |
|---------|-------|
| TUI thread list view (ratatui rendering) | 3.2 |
| Stack frame display | 3.3 |
| Object expansion via BinaryFuse8 resolver | 3.4 |
| Recursive expansion | 3.5 |
| Lazy string loading | 3.6 |
| LRU cache / memory budget | 5.1–5.4 |
| TOML config (EngineConfig fields) | 6.1 |
| Wiring `Engine::from_file` into `hprof-cli` main flow | 3.2 |

### Previous Story Intelligence (2.6)

- `hprof-engine/src/lib.rs` currently has 144 lines: `IndexSummary`, `open_hprof_file_with_progress`,
  `open_hprof_file`, `open_hprof_header`, and their tests. Do NOT remove any of these — they are
  still used by `hprof-cli`. New code is additive.
- `HprofFile` struct fields: `_mmap`, `header`, `index` (PreciseIndex), `index_warnings`,
  `records_attempted`, `records_indexed`, `segment_filters`. The engine only needs `index` for 3.1.
- `PreciseIndex` exposes: `strings: HashMap<u64, HprofString>`, `threads: HashMap<u32, HprofThread>`,
  `stack_frames`, `stack_traces`, `classes`. All public — no visibility changes needed.
- `HprofThread.name_string_id: u64` — used to resolve the thread name from `index.strings`.
- `HprofString.value: String` — the resolved UTF-8 string content.
- All 110 non-feature-gated tests pass. All 251 tests with test-utils pass. Expect 6–8 new tests
  in `hprof-engine`.

### Git Intelligence (recent commits)

```
f32720f feat: add ETA to segment filter progress bar
0199f92 Fix: freeze scan bar during filter-build phase
a29cdd2 Fix: lazy second bar + MultiProgress to prevent visual interference
86801f9 Refactor: separate filter build phase into a second progress bar
e2aa8d7 Fix: show progress during heap dump inner loop and filter build phase
ad7bcc2 Story 2.6: indexing progress bar with runtime fixes
```

Pattern: new module in `hprof-engine` → new file + `pub mod` declaration in `lib.rs` +
re-export from `lib.rs`. Consistent with story 2.6 pattern for `hprof-tui/src/progress.rs`.

### Project Structure Notes

- `engine.rs` lives at `crates/hprof-engine/src/engine.rs` (per architecture `engine.rs` entry).
- `engine_impl.rs` lives at `crates/hprof-engine/src/engine_impl.rs` (per architecture).
- `EngineConfig` lives in `lib.rs` — it's a cross-cutting type, not specific to trait or impl.
- View model types (`ThreadInfo`, etc.) live in `engine.rs` alongside the trait — they are part
  of the trait contract.

### References

- [Source: docs/planning-artifacts/epics.md#Story 3.1]
- [Source: docs/planning-artifacts/architecture.md#Frontend Architecture]
- [Source: docs/planning-artifacts/architecture.md#Engine Factory Pattern]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]
- [Source: crates/hprof-engine/src/lib.rs]
- [Source: crates/hprof-parser/src/hprof_file.rs]
- [Source: crates/hprof-parser/src/indexer/precise.rs]
- [Source: crates/hprof-parser/src/types.rs — HprofThread, StackFrame etc.]
- [Source: crates/hprof-parser/src/strings.rs — HprofString]
- [Source: crates/hprof-parser/src/test_utils.rs — HprofTestBuilder.add_thread()]
- [Source: docs/implementation-artifacts/2-6-indexing-progress-bar.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- `engine.rs`: `NavigationEngine` trait + 5 view model types (`ThreadInfo`, `FrameInfo`,
  `VariableInfo`, `FieldInfo`, `EntryInfo`). All types derive `Debug`. 1 compile-time test.
- `engine_impl.rs`: `Engine` struct wrapping `HprofFile`. `from_file` delegates to
  `HprofFile::from_path`. Private `resolve_name()` helper eliminates DRY duplication.
  `list_threads()` resolves names with `<unknown:{id}>` fallback, sorted by serial.
  `select_thread()` returns `Option`. Stub implementations for 4 remaining methods.
  8 tests (4 minimal-hprof, 3 with `HprofTestBuilder`, 1 unknown name placeholder).
- `lib.rs`: Added `EngineConfig` with `#[derive(Debug, Default)]`, modules private with
  re-exports at crate root, updated crate docstring.
- `Cargo.toml`: Added `hprof-parser` with `test-utils` feature to dev-dependencies.
- Fixed 3 pre-existing clippy warnings: 2 doc-continuation, 1 dead_code on `build()`.
- Code review fixes: modules made private (encapsulation), DRY helper, Debug derives,
  misleading test name corrected, redundant `#[cfg(test)]` removed.
- All 160 workspace tests pass. Clippy clean. Fmt clean.

### File List

- `crates/hprof-engine/src/engine.rs` (NEW)
- `crates/hprof-engine/src/engine_impl.rs` (NEW)
- `crates/hprof-engine/src/lib.rs` (modified)
- `crates/hprof-engine/Cargo.toml` (modified)
- `crates/hprof-parser/src/hprof_file.rs` (modified — comment update + doc fix)
- `crates/hprof-parser/src/indexer/segment.rs` (modified — `#[allow(dead_code)]`)

## Senior Developer Review (AI)

### Review Date

2026-03-07

### Reviewer

Codex (Amelia / Dev Agent execution)

### Outcome

Approved for Story 3.1 scope.

### Notes

- Cross-story review covered Stories 3.1-3.3.
- No additional 3.1-specific defects remained after verification.
- Trait/factory boundary and dependency encapsulation remain compliant (`hprof-cli` depends on
  `hprof-engine`, not `hprof-parser`).

## Change Log

- 2026-03-07 — Added Senior Developer Review (AI) outcome for Story 3.1 (approved, no additional
  code changes required).
