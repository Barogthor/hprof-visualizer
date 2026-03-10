# Story 6.1: TOML Configuration & CLI Precedence

Status: ready-for-dev

## Story

As a user,
I want to configure default settings via a TOML config file with a clear lookup
order and have CLI flags take precedence over config values,
so that I can customize the tool's behavior without repeating CLI flags every time.

## Acceptance Criteria

### AC1: Config from CWD

**Given** a `config.toml` file in the current working directory
**When** the tool launches
**Then** settings are loaded from that file (FR31, FR32)

### AC2: Config from binary directory (fallback)

**Given** no `config.toml` in the working directory but one next to the binary
**When** the tool launches
**Then** settings are loaded from the fallback location (FR32)

### AC3: Silent defaults on missing config

**Given** no config file exists anywhere
**When** the tool launches
**Then** it operates with built-in defaults silently — no error, no warning (FR34)

### AC4: Malformed TOML → warning + fallback

**Given** a malformed config file (invalid TOML syntax)
**When** the tool attempts to parse it
**Then** a warning is logged **to stderr** and the tool falls back to built-in defaults (FR35)

### AC5: CLI wins over config

**Given** `memory_limit = "4G"` in config and `--memory-limit 8G` on the CLI
**When** the tool resolves the effective configuration
**Then** CLI wins: memory limit is 8 GB (FR33)

### AC6: Unknown keys ignored

**Given** a config file with an unknown key
**When** parsed
**Then** the unknown key is ignored — no crash, no error

## Tasks / Subtasks

- [ ] Task 1: Add `serde` and `toml` dependencies (AC: 1–6)
  - [ ] In `crates/hprof-cli/Cargo.toml`, add under `[dependencies]`:
        ```toml
        serde = { version = "1", features = ["derive"] }
        toml  = "0.8"
        ```

- [ ] Task 2: Create `crates/hprof-cli/src/config.rs` (AC: 1–6)
  - [ ] Add module-level docstring `//!`
  - [ ] Define `AppConfig` struct:
        ```rust
        // unknown fields are silently ignored (serde default — satisfies AC6)
        #[derive(Debug, Default, serde::Deserialize)]
        pub struct AppConfig {
            pub memory_limit: Option<String>,
        }
        ```
        `Default` gives silent zero-value fallback (AC3).
  - [ ] Implement two functions:
        ```rust
        /// Public entry point — uses real CWD.
        pub fn load(binary_path: &Path) -> AppConfig {
            // unwrap_or_default() yields PathBuf::new() on failure.
            // PathBuf::new().join("config.toml") = "config.toml" (relative),
            // which fs::read_to_string resolves against the process CWD —
            // same effective path as current_dir(). Intentionally acceptable.
            let cwd = std::env::current_dir().unwrap_or_default();
            load_from(&cwd, binary_path)
        }

        /// Testable core — CWD injected explicitly.
        /// `pub(crate)` so tests in this module can call it directly
        /// without exposing it as part of the crate's public API.
        pub(crate) fn load_from(cwd: &Path, binary_path: &Path) -> AppConfig
        ```
        `load_from` logic (sequential, early-return on first success):
        1. Try `cwd / "config.toml"` (AC1)
        2. Try `binary_path.canonicalize().unwrap_or_else(|_| binary_path.to_path_buf()).parent()
           .map(|p| p.join("config.toml"))` (AC2) — canonicalize resolves symlinks,
           falls back to raw path if it fails
        3. For each candidate path: if file does not exist → skip silently
        4. If file exists but is malformed TOML → `eprintln!("[warn] config: {}: {}", path.display(), err)`
           + return `AppConfig::default()` (AC4)
        5. If file parses successfully → return immediately, do not check remaining paths (AC3 early-return)
        6. If no candidate yielded a file → return `AppConfig::default()` silently (AC3)
  - [ ] Unit tests inside `#[cfg(test)] mod tests` at bottom of `config.rs`
        (all tests call `load_from` with explicit CWD to avoid ambient `config.toml`):
        - `no_file_returns_defaults` — `load_from(&nonexistent_dir, &nonexistent_bin)`
          → `AppConfig { memory_limit: None }`
        - `config_loaded_from_cwd` — create `tempdir/config.toml` with
          `memory_limit = "4G"`, call `load_from(&tempdir, &nonexistent_bin)`
          → `memory_limit == Some("4G")` (tests AC1)
        - `config_loaded_from_binary_dir` — create `tempdir/config.toml`,
          call `load_from(&nonexistent_cwd, &tempdir.join("bin"))` (binary inside tempdir)
          → `memory_limit == Some("4G")` (tests AC2)
          Note: `tempdir.join("bin")` does not need to exist on disk.
          `canonicalize()` will fail on it → fallback to raw path →
          `.parent()` resolves to `tempdir` → finds `config.toml`. This is
          the intended behaviour, do NOT create the `bin` file.
        - `cwd_takes_priority_over_binary_dir` — create `cwd_dir/config.toml`
          with `memory_limit = "2G"` and `bin_dir/config.toml` with `memory_limit = "8G"`,
          call `load_from(&cwd_dir, &bin_dir.join("bin"))` → `memory_limit == Some("2G")`
        - `malformed_toml_returns_defaults` — write `not valid toml !!!` to
          `tempdir/config.toml`, call `load_from(&tempdir, &nonexistent_bin)`
          → `memory_limit == None` (no panic)
        - `unknown_key_ignored` — write `unknown_key = 42\nmemory_limit = "2G"`,
          → `memory_limit == Some("2G")` (no crash)

- [ ] Task 3: Wire `config.rs` into `main.rs` (AC: 1, 5)
  - [ ] Add `mod config;` alongside the existing `mod progress;` (~line 17 of `main.rs`)
  - [ ] In `fn run()`, before `Cli::parse()`:
        - Resolve binary path: `std::env::current_exe().unwrap_or_default()`
        - Note: if `current_exe()` fails, `unwrap_or_default()` yields `PathBuf::new()`
          (empty path). `load()` will then skip the AC2 lookup silently — this is
          intentional and acceptable.
  - [ ] After `Cli::parse()`, call `let app_config = config::load(&binary_path);`
  - [ ] Merge precedence — CLI wins:
        ```rust
        let effective_memory_limit: Option<&str> = cli
            .memory_limit
            .as_deref()
            .or(app_config.memory_limit.as_deref());
        ```
  - [ ] Replace direct `cli.memory_limit.as_deref()` usage in `parse_memory_size` call
        with `effective_memory_limit`
  - [ ] Source-aware error message: wrap `parse_memory_size` call to distinguish
        origin in the error:
        ```rust
        let budget_bytes = match effective_memory_limit {
            None => None,
            Some(val) => {
                let source = if cli.memory_limit.is_some() { "--memory-limit" }
                             else { "config file memory_limit" };
                Some(parse_memory_size(val)
                    .map_err(|e| CliError::InvalidMemoryLimit(
                        format!("{source}: {e}")
                    ))?)
            }
        };
        ```
  - [ ] Add integration tests in `main.rs`:
        - `cli_overrides_config_memory_limit` — cli=Some("8G"), config=Some("4G")
          → effective = "8G"
        - `config_used_when_cli_absent` — cli=None, config=Some("4G") →
          effective = "4G"
        - `both_absent_is_none` — cli=None, config=None → effective = None
        - `config_bad_value_error_message_names_source` — cli=None,
          config=Some("not-a-size") → error message contains "config file"

## Dev Notes

### New file to create

`crates/hprof-cli/src/config.rs` — does not exist yet. Architecture maps this
file to FR31–FR35.
[Source: docs/planning-artifacts/architecture.md#Project Structure]

### Crate ownership

Config loading is exclusively in `hprof-cli`. The `hprof-engine` crate is
not involved — `EngineConfig { budget_bytes }` is already assembled in `main.rs`
before passing to `Engine::from_file_with_progress`.
[Source: docs/planning-artifacts/architecture.md#Crate Decomposition]

### Lookup order rationale

Architecture specifies: CWD first, then binary directory, then defaults.
[Source: docs/planning-artifacts/epics.md#Story 6.1 AC]

The two lookups are **independent and sequential with early-return**: as soon
as one candidate file is found and parsed successfully, `load_from` returns
immediately without attempting the next path. This avoids double-warning if
two config files exist and also prevents the edge case where a file is deleted
between the first and second lookup attempt.

`current_exe()` may return a symlink path on Linux — call `.canonicalize()`
before `.parent()` to resolve the real binary directory. If canonicalize fails,
fall back to the raw path's parent (still better than skipping entirely).

Tests use `load_from(cwd, binary_path)` with explicit injected paths to avoid
reading an ambient `config.toml` from the real CWD during `cargo test`.

### `parse_memory_size` reuse

`parse_memory_size` is already in `main.rs` (private). The config module stores
the raw `Option<String>`. Precedence merge happens in `run()` before calling
`parse_memory_size` on the effective string. No duplication needed.
[Source: crates/hprof-cli/src/main.rs — `parse_memory_size` (~line 115)]

### Validation scope — AC4 vs semantic errors

AC4 (warning + fallback) covers **TOML syntax errors only** — i.e., the file
cannot be parsed as valid TOML. It does NOT cover semantic errors like
`memory_limit = "not-a-size"`. A semantically invalid value is valid TOML, so
no warning is emitted at load time. The error surfaces later via
`parse_memory_size`, with a source-aware message (see Task 3).

### `AppConfig` extensibility

`AppConfig` is designed as a forward-compatible container. Future stories
(e.g., theme, default filter) will add fields here without breaking changes.
Do not flatten config loading into ad-hoc logic — keep the typed struct.

### `deny_unknown_fields`

Do NOT add `#[serde(deny_unknown_fields)]` to `AppConfig`. The default serde
behavior ignores unknown fields, which is exactly what AC6 requires. The struct
carries a comment explaining this so future devs don't accidentally add the
attribute.

### Warning format

Match existing warning style in `main.rs`:
```rust
eprintln!("[warn] config: {}: {}", path.display(), err);
```

### `toml` crate version note

`toml = "0.8"` uses `toml::from_str::<T>` for deserialisation, returning
`Result<T, toml::de::Error>`. The `serde::Deserialize` derive on `AppConfig`
is sufficient. No need for `toml::Value` intermediary.

### Testing with `tempfile`

`tempfile` is already in `[dev-dependencies]` of `hprof-cli/Cargo.toml`.
Use `tempfile::tempdir()` to create isolated config files for tests.
[Source: crates/hprof-cli/Cargo.toml]

### Previous story context

Story 5.4 — last story before 6.1. Changes were confined to `hprof-engine`.
No config-related work was done in Epic 5. The CLI crate has 163 passing tests
as of that point.
[Source: docs/implementation-artifacts/5-4-transparent-re-parse-and-multi-cycle-stability.md]

### Commit style

`feat: Story 6.1 — TOML configuration and CLI precedence`
(no co-author lines per CLAUDE.md)

### Project Structure Notes

Files to create or modify:
```
crates/hprof-cli/Cargo.toml          # +serde, +toml deps
crates/hprof-cli/src/config.rs       # NEW — AppConfig + load()
crates/hprof-cli/src/main.rs         # +mod config; wire load() + merge
```

### References

- [Source: docs/planning-artifacts/epics.md#Story 6.1]
- [Source: docs/planning-artifacts/architecture.md#Crate Decomposition — hprof-cli]
- [Source: docs/planning-artifacts/architecture.md#Project Structure — config.rs]
- [Source: crates/hprof-cli/src/main.rs — `parse_memory_size`, `EngineConfig`, `Cli`]
- [Source: crates/hprof-cli/Cargo.toml — existing deps, dev-deps]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List
