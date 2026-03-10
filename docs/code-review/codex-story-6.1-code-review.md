# Code Review Report - Story 6.1

- Story: `docs/implementation-artifacts/6-1-toml-configuration-and-cli-precedence.md`
- Story key: `6-1-toml-configuration-and-cli-precedence`
- Reviewer: Codex (gpt-5.3-codex)
- Date: 2026-03-10

## Scope

- Reviewed files:
  - `crates/hprof-cli/Cargo.toml`
  - `crates/hprof-cli/src/config.rs`
  - `crates/hprof-cli/src/main.rs`

## Initial Findings

### High

1. Error message in CLI always prefixed with `invalid --memory-limit` even when
   the failing value came from `config.toml` (`config file memory_limit`).

### Medium

1. Config read errors other than missing file were silently ignored in
   `load_from`.
2. AC4 warning behavior (malformed TOML -> warning on stderr) was not directly
   asserted by tests.

## Fixes Applied

1. Updated CLI error formatting to use neutral prefix:
   - `invalid memory limit: ...`
   - Preserves source-specific context built in `run()`.
2. Refactored config loading with an internal warning sink (`load_from_with`):
   - `NotFound` continues fallback lookup.
   - Other read errors emit warning and fall back to defaults.
   - TOML parse errors emit warning and fall back to defaults.
3. Added tests:
   - `malformed_toml_emits_warning`
   - `unreadable_config_emits_warning_and_defaults`

## Validation

- Command: `cargo test -p hprof-cli`
- Result: `29 passed; 0 failed`

## Outcome

- Status recommendation: `done`
- Sprint sync: `6-1-toml-configuration-and-cli-precedence -> done`
