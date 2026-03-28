# Code Review — Story 13.5: AZERTY/QWERTY Keymapping

**Commit:** `fd49827`
**Date:** 2026-03-19
**Reviewer:** Amelia (Dev Agent — adversarial pass, post-implementation)
**Story status at time of review:** `review`

> Note: a pre-implementation story-spec review existed under this filename.
> This file supersedes it with the post-implementation code review.

---

## Git vs Story File List — Discrepancies

No discrepancies. The 10 files in commit `fd49827` match the Dev Agent Record File List
exactly. The 3 unstaged files (`theme.rs`, `status_bar.rs`, `thread_list.rs`) belong to
story 13.4 and are correctly absent from the 13.5 list.

**Git discrepancy count: 0**

---

## Acceptance Criteria Validation

| AC | Description | Status |
|----|-------------|--------|
| #1 | Default keymap is `azerty` | ✅ `KeymapPreset::default()` → `Azerty`; `Keymap::default()` delegates to azerty preset |
| #2 | `keymap = "qwerty"` in config or `--keymap qwerty` activates qwerty preset | ✅ CLI flag + TOML field wired in `main.rs:127-135`; precedence CLI > config > default |
| #3 | Help panel shows actual bindings for active layout | ⚠️ **PARTIAL** — 11/12 configurable keys dynamic; `toggle_help` hardcoded as `"?"` regardless of preset (see M1) |
| #4 | `keymap` field documented in TOML config | ✅ `config.toml:10-14` lists accepted values and default; `config.rs:19-21` has doc comment |
| #5 | All existing tests pass, zero regressions | ✅ 1069 tests pass, clippy clean, fmt clean |

---

## 🔴 CRITICAL ISSUES

None.

---

## 🟡 MEDIUM ISSUES

### M1 — `Keymap.toggle_help` is a dead field — AC #3 partially violated

**Files:** `crates/hprof-tui/src/keymap.rs:62,85,183` •
`crates/hprof-tui/src/input.rs:124-126` •
`crates/hprof-tui/src/views/help_bar.rs:87`

`toggle_help: KeyCode` is declared in `Keymap`, initialized in both presets
(`Char('?')`), and included in `all_keys()` for the uniqueness test. However it has zero
effect on behavior:

1. `from_key()` hardcodes `?` in the layout-independent section (line 124) with an early
   `return`. The configurable section (lines 130–165) checks 11 of the 12 `Keymap`
   fields — `toggle_help` is the only one absent.

2. `help_entries()` line 87 hardcodes `"?".to_string()` instead of
   `key_label(keymap.toggle_help)`.

**Consequence:** changing `keymap.toggle_help` to any other `KeyCode` silently has no
effect — the key `?` still triggers `ToggleHelp`, and the help panel still displays `?`.
With both presets currently identical this is unobservable, but the field is misleading
infrastructure.

**Fix (option A — remove, simplest):**
- Remove `pub toggle_help: KeyCode` from `Keymap`
- Remove the `toggle_help` initializer from both preset constructors
- Remove `km.toggle_help` from `all_keys()` in tests
- Leave `help_entries()` line 87 as `"?".to_string()` and the hardcoded arm in `from_key()` as-is

**Fix (option B — wire up, more correct):**
- Add `if code == keymap.toggle_help { return Some(InputEvent::ToggleHelp); }` to the
  configurable section in `from_key()` and remove the hardcoded `?` arm (lines 124–126)
- Replace `"?".to_string()` in `help_entries()` with `key_label(keymap.toggle_help)`

Option A is simpler. Option B makes `toggle_help` truly configurable.

---

### M2 — Keymap precedence tests in `main.rs` test inline expressions, not `run()`

**File:** `crates/hprof-cli/src/main.rs:347-368`

Tests `cli_keymap_overrides_config_keymap`, `config_keymap_used_when_cli_absent`, and
`both_absent_defaults_to_azerty` each manually inline `cli_val.or(config_val)` — they
don't call `run()`. If the precedence logic at lines 127–131 of `run()` were changed,
these tests would still pass because they test an independent copy of the logic.

Same weakness exists for `memory_limit` tests (pre-existing pattern).

**Fix:** Extract a `resolve_keymap(cli, config) -> &str` helper that `run()` calls and
the tests exercise. Or add one integration-style test that constructs a real `Cli` +
`AppConfig` and verifies the precedence through the actual code path.

---

### M3 — `ENTRY_COUNT = 23` constant not derived from `help_entries()` — `required_height()` can silently drift

**File:** `crates/hprof-tui/src/views/help_bar.rs:18,130-132`

`required_height()` computes `2 + 1 + ENTRY_COUNT.div_ceil(2) + 1`. If a new entry is
added to `help_entries()`, the developer must manually update `ENTRY_COUNT`. The tests
catch the mismatch, so the build breaks — but the fix window (stale constant + wrong
height before the constant is updated) could cause layout bugs that are hard to attribute.

**Fix:**
```rust
pub fn required_height() -> u16 {
    let n = help_entries(&Keymap::default()).len() as u16;
    2 + 1 + n.div_ceil(2) + 1
}
```
Then `ENTRY_COUNT` can be removed entirely. The constant was needed for the old static
array; the dynamic function makes it redundant.

---

## 🟢 LOW ISSUES

### L1 — `KeymapPreset::from_str` is case-sensitive with no test for case variants

**File:** `crates/hprof-tui/src/keymap.rs:25-31`

`"AZERTY".parse::<KeymapPreset>()` returns an error and causes the app to exit at
startup. The error message says "expected azerty or qwerty" which hints at lowercase, but
a user writing `keymap = "AZERTY"` in `config.toml` gets a hard failure. Adding
`.to_ascii_lowercase()` before the match would be a minor UX improvement. Per spec this
is intentional; flagged for awareness.

---

## Task / Subtask Audit

| Task | Status | Evidence |
|------|--------|----------|
| 1 — `keymap.rs` module | ✅ Done | `KeymapPreset`, `Keymap`, `FromStr`, `Default`, `build()`; 8 tests |
| 2 — Promote variants | ✅ Done | `HideField`, `RevealHidden`, `PrevPin`, `NextPin`, `BatchExpand` in `InputEvent`; no `SearchChar('h'/'H'/'b'/'n'/'c'/'s')` remain |
| 3 — `from_key(key, keymap)` | ✅ Done | Signature updated; `Keymap` in `App`; `run_loop` passes `&app.keymap` at line 2688 |
| 4 — `AppConfig.keymap`, CLI flag | ✅ Done | `keymap: Option<String>` in config; `--keymap` in `Cli`; precedence wired; `config.toml` documented |
| 5 — Dynamic help panel | ✅ Done | `help_entries(keymap)` function; `Navigating` override preserved; entry count 23 for both presets |
| 6.1–6.3 — CI verification | ✅ Done | 1069 tests pass, clippy clean, fmt clean |
| 6.4–6.5 — Manual tests | ⬜ Left to user | Correctly marked `[ ]` |
| 1.6 — Uniqueness tests | ✅ Done | `no_two_actions_share_same_keycode_azerty/qwerty` in `keymap.rs` |

**No `[x]` task found to be falsely marked.**

---

## Summary

| Severity | Count | Description |
|----------|-------|-------------|
| 🔴 Critical | 0 | — |
| 🟡 Medium | 3 | M1: dead `toggle_help` field; M2: shallow precedence tests; M3: drifting `ENTRY_COUNT` |
| 🟢 Low | 1 | L1: case-sensitive `from_str` |

The implementation is solid. M1 is the most actionable — either remove `toggle_help` from
`Keymap` (option A) or wire it up properly (option B). M3 is a one-line cleanup. M2 is a
test-quality concern with low risk given both presets are currently identical.
