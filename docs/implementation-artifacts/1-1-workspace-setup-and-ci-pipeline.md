# Story 1.1: Workspace Setup & CI Pipeline

Status: done

## Story

As a developer,
I want a Cargo workspace with 4 crates (hprof-parser, hprof-engine, hprof-tui, hprof-cli)
and a GitHub Actions CI pipeline,
so that the project has a solid foundation with automated quality checks on every push.

## Acceptance Criteria

1. **Given** a fresh clone of the repository
   **When** I run `cargo build`
   **Then** all 4 crates compile successfully with no errors

2. **Given** a push to the main branch or a PR
   **When** GitHub Actions CI runs
   **Then** the pipeline executes `cargo fmt --check`, `cargo clippy`, `cargo test`, and
   `cargo build --release` on Linux, macOS, and Windows

3. **Given** each crate's `lib.rs` (or `main.rs` for hprof-cli)
   **When** I inspect the source
   **Then** each has a `//!` module docstring describing its single responsibility

## Tasks / Subtasks

- [x] Convert project root from single-package to Cargo workspace (AC: #1)
  - [x] Replace root `Cargo.toml` `[package]` with `[workspace]` manifest listing 4 crates
  - [x] Create `crates/` directory containing `hprof-parser/`, `hprof-engine/`,
        `hprof-tui/`, `hprof-cli/`
  - [x] Move current `src/main.rs` content to `crates/hprof-cli/src/main.rs`
  - [x] Remove the old `src/` directory
- [x] Scaffold each crate with minimal valid source (AC: #1, #3)
  - [x] `crates/hprof-parser/src/lib.rs` — `//!` docstring, empty public module
  - [x] `crates/hprof-engine/src/lib.rs` — `//!` docstring, empty public module
  - [x] `crates/hprof-tui/src/lib.rs` — `//!` docstring, empty public module
  - [x] `crates/hprof-cli/src/main.rs` — `//!` docstring, minimal `fn main()`
- [x] Configure inter-crate dependencies in Cargo.toml files (AC: #1)
  - [x] `hprof-engine` depends on `hprof-parser`
  - [x] `hprof-tui` depends on `hprof-engine`
  - [x] `hprof-cli` depends on `hprof-engine` and `hprof-tui`
  - [x] `hprof-cli` must NOT depend on `hprof-parser` directly
- [x] Add workspace-level `[workspace.dependencies]` for shared crate versions (AC: #1)
- [x] Create GitHub Actions CI pipeline (AC: #2)
  - [x] `.github/workflows/ci.yml` with matrix: `ubuntu-latest`, `macos-latest`,
        `windows-latest`
  - [x] Steps: `cargo fmt --check`, `cargo clippy -- -D warnings`,
        `cargo test`, `cargo build --release`
  - [x] Triggered on push and pull_request to `main`
- [x] Verify `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt -- --check` all pass (AC:
      #1, #2)

## Dev Notes

### Current Project State — CRITICAL

The repository is currently a **single-package** Cargo project (not a workspace). The root
`Cargo.toml` contains `[package]` with `name = "hprof-visualizer"`. There is a `src/main.rs`
with a placeholder `fn main()`. **This must be converted to a workspace** — the old `src/`
directory and root-level `[package]` section will be replaced entirely.

The project has no git commits yet. The `.gitignore` and CI pipeline will be new files.

### Workspace Layout (from Architecture)

```
hprof-visualizer/
├── Cargo.toml                    # workspace manifest ONLY — no [package]
├── Cargo.lock
├── .gitignore
├── .github/
│   └── workflows/
│       └── ci.yml
└── crates/
    ├── hprof-parser/
    │   ├── Cargo.toml
    │   └── src/
    │       └── lib.rs
    ├── hprof-engine/
    │   ├── Cargo.toml
    │   └── src/
    │       └── lib.rs
    ├── hprof-tui/
    │   ├── Cargo.toml
    │   └── src/
    │       └── lib.rs
    └── hprof-cli/
        ├── Cargo.toml
        └── src/
            └── main.rs
```

[Source: docs/planning-artifacts/architecture.md#Project Structure]

### Dependency Direction (compile-time enforced)

```
hprof-cli → hprof-engine → hprof-parser
          → hprof-tui    → hprof-engine
```

`hprof-cli` must NOT import from `hprof-parser` directly. The engine factory
(`Engine::from_file`) encapsulates parser internals.

[Source: docs/planning-artifacts/architecture.md#Dependency Direction]

### Crate Responsibilities (for `//!` docstrings)

- **hprof-parser**: Binary hprof format parsing, domain types, first-pass indexer,
  BinaryFuse8 segment filter construction, and test builder (feature-gated `test-utils`).
- **hprof-engine**: Navigation Engine trait, `Engine::from_file()` factory, LRU cache,
  `MemorySize` tracking, object resolution, and pagination logic.
- **hprof-tui**: ratatui-based TUI frontend, thin client consuming the NavigationEngine API.
- **hprof-cli**: Binary entry point, `clap` CLI argument parsing, TOML config loading,
  memory budget calculation, and frontend selection.

[Source: docs/planning-artifacts/architecture.md#Crate Decomposition]

### Root Cargo.toml Structure

The root `Cargo.toml` must use `[workspace]` with `resolver = "2"` and list the 4 members.
Shared dependency versions should go in `[workspace.dependencies]` to avoid drift across
crates. Each crate's `Cargo.toml` then references shared deps with
`dep = { workspace = true }`.

```toml
[workspace]
members = [
    "crates/hprof-parser",
    "crates/hprof-engine",
    "crates/hprof-tui",
    "crates/hprof-cli",
]
resolver = "2"

[workspace.dependencies]
# placeholder — future stories will populate this
```

### Crate Cargo.toml Stubs

At this story stage, crates have no external dependencies yet. Each `Cargo.toml` should
specify `edition = "2024"` and declare the dependency path for inter-crate deps:

```toml
# crates/hprof-engine/Cargo.toml
[package]
name = "hprof-engine"
version = "0.1.0"
edition = "2024"

[dependencies]
hprof-parser = { path = "../hprof-parser" }
```

```toml
# crates/hprof-cli/Cargo.toml
[package]
name = "hprof-cli"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "hprof-visualizer"
path = "src/main.rs"

[dependencies]
hprof-engine = { path = "../hprof-engine" }
hprof-tui    = { path = "../hprof-tui" }
```

### `//!` Docstring Requirements

Every `lib.rs` and `main.rs` MUST have a `//!` module docstring (not `//`). No docstring =
clippy warning and architecture violation. The docstring must describe the crate's single
responsibility concisely.

[Source: docs/planning-artifacts/architecture.md#Crate Documentation]
[Source: CLAUDE.md#Documentation Requirements]

### GitHub Actions CI Pipeline

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  ci:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - name: Format check
        run: cargo fmt -- --check
      - name: Clippy
        run: cargo clippy -- -D warnings
      - name: Test
        run: cargo test
      - name: Build release
        run: cargo build --release
```

Use `dtolnay/rust-toolchain@stable` (current ecosystem standard) with explicit `components`
for `rustfmt` and `clippy`. Edition 2024 is supported on stable Rust since 1.85.

### Coding Standards to Apply

- No `unwrap()` or `expect()` in production code (forbidden by CLAUDE.md and architecture)
- `println!` in `main.rs` is acceptable as a placeholder at this stage only
- Max 100 characters per line
- All public items need docstrings (none exist yet in stubs, so just `//!` at crate level)

[Source: CLAUDE.md#Coding Standards]
[Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]

### Project Structure Notes

- The existing `src/` directory and root `[package]` section in `Cargo.toml` are **replaced**,
  not modified alongside. After this story, there must be no `src/` at the root.
- The `target/` directory is shared across all workspace crates automatically by Cargo.
- `Cargo.lock` stays at the workspace root (already present).

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- **Code review fixes applied (2026-03-06):**
  - `.github/workflows/ci.yml`: added `Swatinem/rust-cache@v2` (M2), `--all-targets`
    on clippy (M1)
  - `crates/hprof-cli/src/main.rs`: removed `println!` anti-pattern, empty `fn main()`
    (M3)
- Converted single-package project to Cargo workspace with 4 crates under `crates/`.
- Root `Cargo.toml` replaced with `[workspace]` manifest (resolver = "2") and
  `[workspace.dependencies]` placeholder.
- Each crate has a `//!` module docstring describing its single responsibility.
- Inter-crate dependency chain enforced: `hprof-cli → hprof-engine/hprof-tui`,
  `hprof-engine → hprof-parser`; `hprof-cli` has no direct `hprof-parser` dep.
- `.github/workflows/ci.yml` created with ubuntu/macos/windows matrix running
  `fmt --check`, `clippy -D warnings`, `test`, `build --release`.
- All local checks pass: `cargo build`, `cargo test`, `cargo clippy -- -D warnings`,
  `cargo fmt -- --check`.

### File List

- Cargo.toml (modified — replaced [package] with [workspace])
- Cargo.lock (modified — regenerated for workspace)
- crates/hprof-parser/Cargo.toml (new)
- crates/hprof-parser/src/lib.rs (new)
- crates/hprof-engine/Cargo.toml (new)
- crates/hprof-engine/src/lib.rs (new)
- crates/hprof-tui/Cargo.toml (new)
- crates/hprof-tui/src/lib.rs (new)
- crates/hprof-cli/Cargo.toml (new)
- crates/hprof-cli/src/main.rs (new)
- .github/workflows/ci.yml (new — updated by code review: added Swatinem/rust-cache@v2,
  clippy --all-targets)
- src/main.rs (deleted)
