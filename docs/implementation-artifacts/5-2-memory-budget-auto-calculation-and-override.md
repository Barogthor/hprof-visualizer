# Story 5.2: Memory Budget Auto-Calculation & Override

Status: done

## Story

As a user,
I want the system to auto-calculate a memory budget at launch
(50% of available RAM) and allow me to override it via
`--memory-limit` CLI flag or config file,
so that the tool uses an appropriate amount of memory for my
machine without me having to configure it.

## Acceptance Criteria

### AC1: Default auto-calculation (FR23)

**Given** no `--memory-limit` flag and no config file setting
**When** the engine starts
**Then** the memory budget is set to 50% of available RAM at
launch time

### AC2: CLI override (FR24)

**Given** `--memory-limit 8G` is passed as CLI flag
**When** the engine starts
**Then** the memory budget is set to 8 GB, overriding
auto-calculation

### AC3: Config file override (FR24)

**Given** `memory_limit = "4G"` in config.toml and no CLI flag
**When** the engine starts
**Then** the memory budget is set to 4 GB

### AC4: CLI wins over config (FR24)

**Given** both `--memory-limit 8G` CLI flag and
`memory_limit = "4G"` in config
**When** the engine starts
**Then** the CLI flag wins — budget is 8 GB
(precedence: CLI > config > auto-calc)

### AC5: Correctness on known hardware (FR23)

**Given** a machine with 16 GB available RAM
**When** auto-calculation runs
**Then** the budget is set to 8 GB

## Tasks / Subtasks

- [x] Task 1: System memory detection (AC: 1, 5)
  - [x] Add `sysinfo` dependency to `hprof-engine`:
        `sysinfo = { version = "0.34", default-features = false, features = ["system"] }`
  - [x] Create `crates/hprof-engine/src/cache/system_memory.rs`
  - [x] Implement `fn detect_total_memory() -> u64` using
        `sysinfo::System::new()` + `refresh_memory()` + `total_memory()`
  - [x] Auto-calc: return `detect_total_memory() / 2`
  - [x] Fallback: if `total_memory()` returns 0 (containers,
        broken cgroups), default to 512 MB
  - [x] Unit test: verify returned value > 0 and reasonable

- [x] Task 2: Expand `EngineConfig` with budget field (AC: 1-5)
  - [x] Change `EngineConfig` from unit struct to struct with
        `budget_bytes: Option<u64>` field
  - [x] `None` = auto-calculate at engine construction
  - [x] `Some(n)` = explicit override
  - [x] Update `Default` impl: `budget_bytes: None`
  - [x] Add `fn effective_budget(&self) -> u64` that resolves
        `budget_bytes.unwrap_or_else(auto_calculate)`
  - [x] Update all existing `EngineConfig` usages (tests in
        engine_impl.rs ~18 sites, main.rs) — `EngineConfig`
        becomes `EngineConfig::default()` instead of
        `EngineConfig`
  - [x] Update `StubEngine` in `hprof-tui/src/app.rs` —
        implement `memory_budget()` returning `u64::MAX`
        (stub = no limit)

- [x] Task 3: Wire budget into `MemoryCounter` (AC: 1-5)
  - [x] Change `MemoryCounter::new()` → `MemoryCounter::new(budget: u64)`.
        Budget is immutable after construction.
  - [x] Update all existing `MemoryCounter::new()` call sites
        (Story 5.1 tests, Engine construction) →
        `MemoryCounter::new(u64::MAX)` as "unlimited" default
        until budget is wired
  - [x] Add `fn budget(&self) -> u64` getter
  - [x] Add `fn usage_ratio(&self) -> f64` =
        `current() as f64 / budget as f64`
  - [x] `reset()` resets bytes to 0 but NOT the budget
        (budget is immutable — set at construction)
  - [x] Update `Engine::from_file` and
        `Engine::from_file_with_progress` to pass
        `config.effective_budget()` to the counter
  - [x] Unit tests: budget getter, usage_ratio calculation

- [x] Task 4: Parse `--memory-limit` CLI flag (AC: 2, 4)
  - [x] Add `clap` dependency to `hprof-cli`:
        `clap = { version = "4", features = ["derive"] }`
  - [x] Implement CLI argument parsing with clap (replace
        manual `parse_hprof_path`):
        - positional: `<FILE>` (required)
        - optional: `--memory-limit <SIZE>` (e.g., "8G",
          "512M", "4096M")
  - [x] Implement `fn parse_memory_size(s: &str) -> Result<u64>`
        supporting suffixes: `K`, `M`, `G`, `T`
        (case-insensitive, binary units: 1G = 1024^3)
  - [x] Use `checked_mul` for overflow safety — return error
        on overflow instead of panic
  - [x] Pass parsed value as `EngineConfig { budget_bytes }`
  - [x] Adapt existing CLI tests: `parse_hprof_path` tests
        become `Cli::try_parse_from` tests. `CliError::Usage`
        is replaced by clap's own error handling — test with
        `Cli::try_parse_from` and assert `is_err()`
  - [x] Unit tests: parse_memory_size for all suffixes,
        invalid input, overflow, edge cases
  - [x] Unit tests: `Cli::try_parse_from` with/without
        `--memory-limit`, verify parsed values

- [x] Task 5: Config file support — DEFER (AC: 3, 4)
  - [x] TOML config file parsing is Epic 6, Story 6.1
  - [x] For now: only CLI and auto-calc are wired
  - [x] Document in story completion that config.toml support
        is deferred to 6.1 — AC3/AC4 config path tested
        at the EngineConfig level (budget_bytes = Some(x))
        but not yet wired to file parsing

- [x] Task 6: Integration test (AC: 1-5)
  - [x] Test `EngineConfig::default()` uses auto-calc
        (budget > 0)
  - [x] Test `EngineConfig { budget_bytes: Some(1_000_000) }`
        sets exact budget
  - [x] Test Engine construction with explicit budget —
        `engine.memory_budget()` returns expected value
  - [x] Expose `fn memory_budget(&self) -> u64` on
        `NavigationEngine` trait

## Dev Notes

### Architecture Decisions

- **`sysinfo` crate** — industry standard for cross-platform
  system info (Linux/macOS/Windows). Use minimal features:
  `sysinfo = { default-features = false, features = ["system"] }`.
  Call `System::new_all()` then `system.total_memory()`
  (returns bytes). Do NOT use `available_memory()` — it
  fluctuates with OS load. `total_memory()` gives a stable
  budget baseline. The budget is not a hard allocation — it's
  an eviction trigger threshold (Story 5.3). Progressive
  accumulation + eviction prevents OOM even if total/2
  exceeds currently available RAM.
- **Fallback for broken environments** — if `total_memory()`
  returns 0 (Docker with misconfigured cgroups v1, exotic
  platforms), fall back to 512 MB. Log a warning.
- **Config file parsing deferred** — TOML config is Epic 6
  Story 6.1. This story wires CLI + auto-calc only. AC3/AC4
  config scenarios tested via `EngineConfig { budget_bytes }`
  but not via file I/O.
- **`clap` for CLI** — replaces manual arg parsing. Use
  derive API (`#[derive(Parser)]`). Use minimal features:
  `clap = { features = ["derive"] }` — avoids pulling
  color/suggestions defaults. Keep it minimal: positional
  file path + optional `--memory-limit`. Adopting clap now
  avoids a double migration when Epic 6 (Story 6.1) adds
  `--config`, `--verbose`, etc.
- **Budget lives in `MemoryCounter`** — the counter already
  tracks usage. Adding the budget target to it keeps budget
  logic centralized. Story 5.3 will use `usage_ratio()` to
  trigger eviction at 80%.
- **Memory size units** — use binary (1G = 1024^3 = GiB) to
  match developer expectations for RAM sizes.
- **`effective_budget()` is called once** — resolved at
  Engine construction, stored in `MemoryCounter`. The method
  calls `sysinfo` internally so it must NOT be called
  repeatedly. After construction, use
  `memory_counter.budget()` for the cached value.
- **Budget is `u64`, counter is `usize`** — `MemoryCounter`
  uses `AtomicUsize` (Story 5.1, shipped). Budget is `u64`
  to match `sysinfo` return type. In `usage_ratio()`, cast
  both to `f64`. On 64-bit targets (only practical target
  for multi-GB dumps), `usize` == `u64` so no precision
  loss.
- **`parse_memory_size` stays in hprof-cli** — YAGNI. When
  Epic 6 Story 6.1 adds TOML config parsing, it will likely
  need to extract this function to a shared location
  (hprof-api or hprof-engine). Note for future migration.

### Key Code Locations

- `crates/hprof-engine/src/cache/budget.rs` — MemoryCounter
  (add budget field here)
- `crates/hprof-engine/src/cache/mod.rs` — add
  `pub mod system_memory;`
- `crates/hprof-engine/src/lib.rs` — EngineConfig definition
  (line ~50, currently unit struct)
- `crates/hprof-engine/src/engine_impl.rs` — Engine
  construction (`from_file`, `from_file_with_progress`)
- `crates/hprof-engine/src/engine.rs` — NavigationEngine
  trait (add `memory_budget()`)
- `crates/hprof-cli/src/main.rs` — CLI entry point
  (currently manual arg parsing)
- `crates/hprof-tui/src/app.rs` — StubEngine impl (must
  update for new trait method)

### Previous Story Intelligence (5.1)

- **MemoryCounter** is in `cache/budget.rs` with
  `AtomicUsize` + `add/subtract/current/reset` methods
- **Engine** stores `memory_counter: Arc<MemoryCounter>` and
  calls `initial_memory()` at construction
- **NavigationEngine trait** already has `memory_used()` —
  add `memory_budget()` next to it
- **Task 6 deferred work**: wiring counter into
  expand/collapse paths → deferred to Story 5.3 (needs LRU
  cache first)
- **FxHashMap** is used throughout (rustc-hash dependency
  already in hprof-engine)
- **All tests use `EngineConfig`** as unit struct — must
  update ~15 test sites to `EngineConfig::default()`

### Anti-Patterns to Avoid

- Do NOT use `available_memory()` — it fluctuates with OS
  load. Use `total_memory()` for deterministic budget.
- Do NOT create a separate BudgetManager struct — keep
  budget in MemoryCounter (KISS)
- Do NOT parse config.toml in this story — that's 6.1
- Do NOT add `--memory-limit` validation for values < some
  minimum — let users shoot themselves in the foot if they
  want 1 byte budget
- Do NOT make `EngineConfig` generic or trait-based — plain
  struct with fields is sufficient
- Do NOT use unchecked arithmetic in `parse_memory_size` —
  use `checked_mul` to handle overflow (e.g. "999999T")
  gracefully with an error, not a panic

### Testing Strategy

- **Unit tests**: parse_memory_size (all suffix combos,
  invalid input, zero, overflow)
- **Unit tests**: MemoryCounter budget/usage_ratio
- **Unit tests**: EngineConfig::effective_budget with
  None/Some
- **Unit tests**: system_memory::detect_total_memory > 0
- **Integration test**: Engine construction with explicit
  budget, verify `memory_budget()` returns it
- **Integration test**: Engine construction with default
  config, verify `memory_budget()` > 0
- **CLI tests**: `Cli::try_parse_from` with/without
  `--memory-limit`, adapt existing `parse_hprof_path` tests

### Project Structure Notes

New files:
```
crates/hprof-engine/src/cache/
└── system_memory.rs   # detect_total_memory()
```

Modified files:
```
crates/hprof-engine/Cargo.toml       # + sysinfo dep
crates/hprof-engine/src/cache/mod.rs  # + pub mod system_memory
crates/hprof-engine/src/cache/budget.rs  # budget field
crates/hprof-engine/src/lib.rs        # EngineConfig fields
crates/hprof-engine/src/engine.rs     # memory_budget() trait
crates/hprof-engine/src/engine_impl.rs # wire budget
crates/hprof-cli/Cargo.toml          # + clap dep
crates/hprof-cli/src/main.rs         # clap CLI parsing
crates/hprof-tui/src/app.rs          # StubEngine update
```

### References

- [Source: docs/planning-artifacts/epics.md#Story 5.2]
- [Source: docs/planning-artifacts/architecture.md — cache
  module, EngineConfig, memory budget]
- [Source: docs/implementation-artifacts/5-1-memorysize-trait-and-budget-tracking.md — MemoryCounter, Engine integration]
- [Source: crates/hprof-engine/src/cache/budget.rs —
  MemoryCounter current impl]
- [Source: crates/hprof-engine/src/lib.rs:50 — EngineConfig
  unit struct]
- [Source: crates/hprof-cli/src/main.rs — current CLI arg
  parsing]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

No debug issues encountered.

### Completion Notes List

- Task 1: Created `system_memory.rs` with `auto_budget()` (total_memory / 2) and 512 MB fallback. Used `sysinfo` v0.34 with minimal features. 3 unit tests.
- Task 2: Expanded `EngineConfig` from unit struct to struct with `budget_bytes: Option<u64>`. Added `effective_budget()`. Updated 18 test sites in engine_impl.rs + main.rs. Added `memory_budget()` to `NavigationEngine` trait and StubEngine. 3 unit tests.
- Task 3: Added `budget: u64` field to `MemoryCounter::new(budget)`. Added `budget()` getter and `usage_ratio()`. Updated `Default` to use `u64::MAX`. Wired `config.effective_budget()` into Engine construction. `reset()` preserves budget. 4 new unit tests.
- Task 4: Replaced manual CLI parsing with `clap` derive API. Added `--memory-limit` flag with `parse_memory_size()` supporting K/M/G/T suffixes (binary, case-insensitive) with `checked_mul` overflow safety. Removed `CliError::Usage` (clap handles it). 12 unit tests (10 parse_memory_size + 4 Cli::try_parse_from, minus 2 old tests replaced).
- Task 5: DEFERRED — config.toml parsing is Epic 6 Story 6.1. AC3/AC4 config scenarios tested via `EngineConfig { budget_bytes: Some(x) }` but not via file I/O.
- Task 6: Added 2 integration tests verifying `memory_budget()` with auto-calc (> 0) and explicit override (exact value). `memory_budget()` exposed on `NavigationEngine` trait.

### Change Log

- 2026-03-10: Story 5.2 implementation complete — system memory detection, EngineConfig budget field, MemoryCounter budget wiring, clap CLI with --memory-limit, integration tests. Config file support deferred to 6.1.
- 2026-03-10: Code review fixes — removed fragile detect_total_memory test (covered by auto_budget tests), guarded usage_ratio() against NaN when budget=0, added AC2 CLI→EngineConfig integration test, completed File List with 8 cargo-fmt-only files.

### File List

New files:
- crates/hprof-engine/src/cache/system_memory.rs

Modified files:
- crates/hprof-engine/Cargo.toml
- crates/hprof-engine/src/cache/mod.rs
- crates/hprof-engine/src/cache/budget.rs
- crates/hprof-engine/src/lib.rs
- crates/hprof-engine/src/engine.rs
- crates/hprof-engine/src/engine_impl.rs
- crates/hprof-engine/src/pagination.rs (cargo fmt only)
- crates/hprof-cli/Cargo.toml
- crates/hprof-cli/src/main.rs
- crates/hprof-tui/src/app.rs
- crates/hprof-tui/src/lib.rs (cargo fmt only)
- crates/hprof-tui/src/views/stack_view.rs (cargo fmt only)
- crates/hprof-tui/src/views/thread_list.rs (cargo fmt only)
- crates/hprof-api/src/memory_size.rs (cargo fmt only)
- crates/hprof-parser/src/indexer/precise.rs (cargo fmt only)
- crates/hprof-parser/src/strings.rs (cargo fmt only)
- crates/hprof-parser/src/types.rs (cargo fmt only)
- docs/implementation-artifacts/sprint-status.yaml
