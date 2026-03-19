# Story 13.5: AZERTY/QWERTY Keymapping

Status: done

## Story

As a user,
I want keyboard mappings that support AZERTY layout by default
with an alternative QWERTY mode,
so that I can use the tool comfortably on my keyboard layout
without wrist strain.

## Acceptance Criteria

1. **Given** the default configuration
   **When** the tool starts
   **Then** the active keymap preset is `azerty` (default).
   Note: both presets start with identical bindings —
   the primary value is the keymap abstraction and the
   dynamic help panel. Preset divergence is reserved
   for future stories once ergonomic gaps are identified.

2. **Given** a `keymap = "qwerty"` setting in config or
   `--keymap qwerty` CLI flag
   **When** the tool starts
   **Then** the active keymap preset is `qwerty`

3. **Given** either keymap
   **When** the help panel is displayed
   **Then** it shows the actual key bindings for the active
   layout

4. **Given** the keymap configuration
   **When** persisted in the TOML config file
   **Then** the setting is documented with available options

5. All existing tests pass with zero regressions

## Tasks / Subtasks

- [x] Task 1: Define `KeymapPreset` enum and keybinding
      table structure — TDD (AC: #1, #2)
  - [x] 1.1 Write tests first: both presets produce valid
        non-overlapping bindings, default is Azerty,
        `from_str` round-trips for both preset names
  - [x] 1.2 Create `keymap.rs` module in `hprof-tui/src/`
        with `KeymapPreset` enum (`Azerty`, `Qwerty`) and
        `Keymap` struct holding all action→key mappings
  - [x] 1.3 Implement `FromStr` on `KeymapPreset` with
        `type Err = String` — unknown values return
        `Err(format!("unknown keymap '{}': expected \
        azerty or qwerty", s))`
  - [x] 1.4 Define the two preset tables using a flat
        struct with one `KeyCode` field per action —
        type-safe and easy to match. See Dev Notes for
        the mapping inventory. Keep each preset
        constructor under 80 lines (CLAUDE.md limit).
  - [x] 1.5 Add `Default` impl returning `Azerty` preset
  - [x] 1.6 Add test asserting no two actions share the
        same `KeyCode` within each preset (uniqueness
        check — prevents silent key shadowing)
  - [x] 1.7 Green: make all tests pass

- [x] Task 2: Promote special-cased `SearchChar` actions
      to proper `InputEvent` variants — TDD (AC: #1)
  - [x] 2.1 Write tests: `HideField`, `RevealHidden`,
        `PrevPin`, `NextPin`, `BatchExpand` variants exist
        in `InputEvent`, and `SearchActivate` is produced
        by `from_key()` for both `s` and `/`
  - [x] 2.2 Add `HideField`, `RevealHidden`, `PrevPin`,
        `NextPin`, `BatchExpand` variants to `InputEvent`
        enum in `input.rs`
  - [x] 2.3 Map `h` → `HideField`, `H` → `RevealHidden`,
        `b` → `PrevPin`, `n` → `NextPin`, `c` → `BatchExpand`
        in `from_key()` as hardcoded mappings (Keymap does
        not exist yet; Task 3 will migrate these to keymap
        lookup)
  - [x] 2.4 Move `s` mapping: currently `s` falls through
        to `SearchChar('s')` and is special-cased in
        `handle_thread_list_input()` (app/mod.rs:1158).
        Instead, map `s` → `SearchActivate` in `from_key()`
        as hardcoded, and remove the `SearchChar('s')` arm
        from `handle_thread_list_input()`
  - [x] 2.5 Update `handle_favorites_input()` to match on
        `InputEvent::HideField` / `InputEvent::RevealHidden`
        / `InputEvent::PrevPin` / `InputEvent::NextPin`
        / `InputEvent::BatchExpand` instead of the
        corresponding `SearchChar(c)` arms
  - [x] 2.6 Green: all tests pass

- [x] Task 3: Refactor `input.rs::from_key()` to use
      `Keymap` lookup — TDD (AC: #1, #2)
  - [x] 3.1 Write tests first: both presets correctly map
        their keys to expected `InputEvent` variants.
        All existing input.rs tests must be updated to
        pass a `&Keymap` parameter to `from_key()`
  - [x] 3.2 Change `from_key()` signature to
        `from_key(key: KeyEvent, keymap: &Keymap)
        -> Option<InputEvent>`. Store `Keymap` in `App`
        struct so `run_loop()` can pass `&app.keymap` at
        the call site (`app/mod.rs:2683`)
  - [x] 3.3 Replace hardcoded `KeyCode::Char('q')` etc.
        match arms with keymap lookups
  - [x] 3.4 Keep non-configurable keys as hardcoded —
        these are layout-independent (see inventory below)
  - [x] 3.5 Keep `SearchChar(c)` catch-all for unbound
        printable keys
  - [x] 3.6 Green: all tests pass, run full suite

- [x] Task 4: Extend `AppConfig` with `keymap` field
      (AC: #2, #4)
  - [x] 4.1 Write test first: config deserialization with
        `keymap = "qwerty"` and `keymap = "azerty"`
  - [x] 4.2 Add `keymap: Option<String>` to `AppConfig`
        in `hprof-cli/src/config.rs` (deserialize as
        String, NOT as KeymapPreset — avoids serde
        dependency in hprof-tui)
  - [x] 4.3 Add `--keymap <azerty|qwerty>` CLI flag to
        clap args in `main.rs`
  - [x] 4.4 Convert `String` → `KeymapPreset` in `main.rs`
        using `FromStr` (error on invalid value)
  - [x] 4.5 Implement precedence: CLI flag > TOML config >
        default (Azerty)
  - [x] 4.6 Pass resolved `Keymap` into `run_tui()` —
        this is a **public API signature change**:
        `run_tui(engine, filename)` → `run_tui(engine,
        filename, keymap)`. Update the function in
        `app/mod.rs` only; the `pub use app::run_tui`
        re-export in `lib.rs` is transparent and requires
        no change.
  - [x] 4.7 Write test: `keymap = "azerty"` in TOML
        deserializes without error and produces no warnings;
        `keymap = "bogus"` is rejected with a clear error
        message at startup
  - [x] 4.8 Green: tests pass
  - [x] 4.9 Add a comment block in the sample
        `config.toml` (or its template) documenting the
        `keymap` field with accepted values
        (`azerty` | `qwerty`) and the default (AC: #4)

- [x] Task 5: Update help panel to show active bindings
      — TDD (AC: #3)
  - [x] 5.1 Write tests first: help entries reflect correct
        keys for each preset, `HelpContext::Navigating`
        override preserved, entry count correct
  - [x] 5.2 Refactor `help_bar.rs` `ENTRIES` from static
        array to a function `help_entries(keymap: &Keymap)`
        that builds entries dynamically
  - [x] 5.3 Each entry reads its key label from the `Keymap`
  - [x] 5.4 Preserve `HelpContext::Navigating` special-case
        (overrides Esc label to "Cancel navigation")
  - [x] 5.5 Preserve context bitmask pattern (THREAD,
        STACK, FAV, ALL)
  - [x] 5.6 Update existing help_bar tests:
        `required_height_returns_sixteen_for_twenty_three_entries`
        (current `ENTRY_COUNT = 23`, height = 16) and
        `entry_count_constant_matches_entries_slice`
        must adapt to the dynamic function — verify the
        entry count is still 23 for both presets
  - [x] 5.7 Green: all tests pass

- [x] Task 6: Final verification (AC: #5)
  - [x] 6.1 `cargo test --all-targets`
  - [x] 6.2 `cargo clippy --all-targets -- -D warnings`
  - [x] 6.3 `cargo fmt -- --check`
  - [ ] 6.4 Manual test: launch with default (AZERTY),
        verify keys work
  - [ ] 6.5 Manual test: launch with `--keymap qwerty`,
        verify keys work and help panel updates

## Dev Notes

### Current Key Inventory (from input.rs analysis)

Configurable single-char bindings (layout-sensitive):

| Action              | Current key | InputEvent variant    | Where handled        |
|---------------------|-------------|-----------------------|----------------------|
| Quit                | `q`         | `Quit`                | input.rs             |
| Toggle favorite     | `f`         | `ToggleFavorite`      | input.rs             |
| Focus favorites     | `F`         | `FocusFavorites`      | input.rs             |
| Go to source        | `g`         | `NavigateToSource`    | input.rs             |
| Hide/show field     | `h`         | `SearchChar('h')` (*) | app/mod.rs:980       |
| Reveal hidden       | `H`         | `SearchChar('H')` (*) | app/mod.rs:1005      |
| Prev pinned item    | `b`         | `SearchChar('b')` (*) | app/mod.rs:997       |
| Next pinned item    | `n`         | `SearchChar('n')` (*) | app/mod.rs:1001      |
| Batch expand        | `c`         | `SearchChar('c')` (*) | app/mod.rs:894       |
| Toggle object IDs   | `i`         | `ToggleObjectIds`     | input.rs             |
| Search activate     | `s`         | `SearchChar('s')` (*) | app/mod.rs:1158      |
| Search activate     | `/`         | `SearchActivate`      | input.rs             |
| Toggle help         | `?`         | `ToggleHelp`          | input.rs             |

(*) These 8 keys bypass `input.rs` — they fall through
to `SearchChar(c)` catch-all and are special-cased in
`app/mod.rs`. Task 2 promotes all of them to proper
`InputEvent` variants.

Non-configurable (layout-independent — hardcoded):
- Arrow keys, Enter, Esc, Tab, PgUp/PgDn, Home/End
- Ctrl+C (quit)
- Ctrl/Shift+Arrow (camera scroll)
- Ctrl/Shift+PgUp/PgDn (camera page scroll)
- Ctrl+L (center selection — modifier key, not char)
- Backspace (search delete)

### AZERTY vs QWERTY Key Differences

On a standard French AZERTY keyboard:
- `q` is on home row left (QWERTY `a` position) — fine
- `a` is on top row left (QWERTY `q` position)
- `z` is on top row (QWERTY `w` position)
- `w` is difficult to reach (AltGr or far position)
- `/` requires Shift on AZERTY — but `input.rs` already
  accepts `/` with `NONE | SHIFT` modifiers (line 101),
  so it works on AZERTY today
- `?` also already accepts `NONE | SHIFT` (line 110)

Most current bindings (`q`, `f`, `g`, `h`, `i`, `s`) are
mnemonic and reachable on both layouts. Both presets start
identical — the infrastructure supports future divergence.
Only diverge keys where there's a clear ergonomic reason.
The primary value is the keymap abstraction and the dynamic
help panel, not artificial differences between presets.

### Cross-Story Dependency: Story 13.2

Story 13.2 (Enhanced Favorites Navigation) has already
been implemented (commits `455e969`, `eef3fb5`). It
delivered `b`/`n` for prev/next pin navigation — not
`[`/`]` as originally planned. These are already included
in the key inventory above and in Task 2's promotion list.

### Architecture Patterns

- **input.rs** is the primary translation layer:
  `KeyEvent` → `InputEvent`. Most changes are here.
- **app/mod.rs** has char-specific logic: `s` at l.1158
  in `handle_thread_list_input()`, and `h`/`H`/`b`/`n`/`c`
  in `handle_favorites_input()`. Task 2 promotes all of
  these to proper `InputEvent` variants.
- **help_bar.rs** has `ENTRIES` static array with 23
  entries (`ENTRY_COUNT: u16 = 23`) using bitmask context
  flags (THREAD=0b001, STACK=0b010, FAV=0b100, ALL=0b111).
  Also has `HelpContext::Navigating` variant that overrides
  the Esc label to "Cancel navigation" — must be preserved.
- **config.rs** only has `memory_limit: Option<String>`.

### Keymap Internal Representation

Use a **flat struct** with one `KeyCode` field per
configurable action:

```rust
pub struct Keymap {
    pub quit: KeyCode,
    pub toggle_favorite: KeyCode,
    pub focus_favorites: KeyCode,
    pub navigate_to_source: KeyCode,
    pub hide_field: KeyCode,
    pub reveal_hidden: KeyCode,
    pub prev_pin: KeyCode,
    pub next_pin: KeyCode,
    pub batch_expand: KeyCode,
    pub toggle_object_ids: KeyCode,
    pub search_activate: KeyCode,
    pub toggle_help: KeyCode,
}
```

This is type-safe (no runtime key-not-found), explicit,
and easy to match in `from_key()`. Avoid `HashMap` —
unnecessary heap allocation for 12 entries. Each preset
constructor should stay under 80 lines.

### Crate Boundary: Config Deserialization

`KeymapPreset` lives in hprof-tui. `AppConfig` lives in
hprof-cli. To avoid adding serde as a dependency to
hprof-tui:
- Deserialize `keymap` as `Option<String>` in config.rs
- Convert to `KeymapPreset` via `FromStr` in main.rs
  (where hprof-tui is already imported)

### `run_tui()` Signature Change

Current: `pub fn run_tui(engine, filename)` (2 params),
re-exported from `lib.rs:41`.
New: `pub fn run_tui(engine, filename, keymap)` (3 params).
Update `app/mod.rs` only. The `pub use app::run_tui` in
`lib.rs` is a transparent re-export — it reflects the new
signature automatically, no edit required.

The event loop lives in `run_loop()` inside `app/mod.rs`,
which calls `input::from_key(key)` at line 2683. After
Task 3, the call becomes `input::from_key(key, &app.keymap)`.
The `Keymap` is threaded: `run_tui()` → `App::new()` →
stored in `App` struct → accessed in `run_loop()` via
`&app.keymap`.

### File Locations

Files to create:
- `crates/hprof-tui/src/keymap.rs` — new module

Files to modify:
- `crates/hprof-tui/src/lib.rs` — add `pub mod keymap`
  (the `pub use app::run_tui` re-export needs no change)
- `crates/hprof-tui/src/input.rs` — add new `InputEvent`
  variants (`HideField`, `RevealHidden`, `PrevPin`,
  `NextPin`, `BatchExpand`); change `from_key()` to
  `from_key(key, keymap: &Keymap)`
- `crates/hprof-tui/src/views/help_bar.rs` — dynamic
  entries from `Keymap`, preserve `Navigating` context
- `crates/hprof-tui/src/app/mod.rs` — store `Keymap` in
  `App`, update `run_tui`/`run_loop`/`App::new` signatures,
  replace `SearchChar('h')`/`SearchChar('H')`/`SearchChar('b')`
  /`SearchChar('n')`/`SearchChar('c')` with proper variants,
  remove `SearchChar('s')` from `handle_thread_list_input()`
- `crates/hprof-cli/src/config.rs` — add
  `keymap: Option<String>`
- `crates/hprof-cli/src/main.rs` — add CLI flag,
  convert String→KeymapPreset, pass to `run_tui()`

### Regression Risk — Specific Tests at Risk

**High risk — input.rs::tests (22 tests):**
All hardcode `KeyCode::Char(...)` and call `from_key()`
without a keymap parameter. Every test must be updated
to pass a `&Keymap`.

**Medium risk — help_bar.rs::tests:**
- `required_height_returns_sixteen_for_twenty_three_entries`
  hardcodes expected height (16) based on 23 entries
- `entry_count_constant_matches_entries_slice` asserts
  `ENTRY_COUNT == ENTRIES.len()` — breaks if ENTRIES
  becomes dynamic
- String-matching tests (`k.contains("s or")`,
  `*k == "f"`, `*k == "h"`) must adapt to dynamic fn

**Medium risk — app/tests.rs:**
- Tests using `InputEvent::SearchChar('h')` must change
  to `InputEvent::HideField`
- Tests using `InputEvent::SearchChar('H')` must change
  to `InputEvent::RevealHidden`
- Tests using `InputEvent::SearchChar('s')` must change
  to `InputEvent::SearchActivate`
- Tests using `InputEvent::SearchChar('b')` must change
  to `InputEvent::PrevPin`
- Tests using `InputEvent::SearchChar('n')` must change
  to `InputEvent::NextPin`
- Tests using `InputEvent::SearchChar('c')` must change
  to `InputEvent::BatchExpand`

**Low risk — main.rs::tests:**
Existing CLI tests are additive — no breakage expected.

### Testing Strategy (TDD)

Each task follows Red-Green-Refactor:
1. Write failing test for the new behavior
2. Implement minimal code to pass
3. Refactor while keeping tests green
4. Run full suite after each subtask

### Previous Story Intelligence

Story 13.4 (warning color) was focused on `status_bar.rs`
and `theme.rs` — no overlap with this story's scope.
Story 13.0 (progress bar) modified parallel extraction —
no overlap either. The pattern of isolated single-module
changes has worked well in Epic 13.

### Project Structure Notes

- New `keymap.rs` module follows the existing pattern of
  focused single-responsibility modules in hprof-tui
- No conflicts with ongoing work detected
- The `input.rs` → `keymap.rs` dependency is clean
  (keymap provides data, input consumes it)

### References

- [Source: docs/planning-artifacts/epics.md#Story 13.5]
  FR70, P2
- [Source: crates/hprof-tui/src/input.rs] `from_key()`
  function, `InputEvent` enum, `/` accepts SHIFT (l.101)
- [Source: crates/hprof-tui/src/views/help_bar.rs]
  `ENTRIES` static array, context bitmask,
  `HelpContext::Navigating` variant
- [Source: crates/hprof-cli/src/config.rs] `AppConfig`
  struct (only `memory_limit`)
- [Source: crates/hprof-tui/src/app/mod.rs]
  `handle_favorites_input()` for `h`/`H`/`b`/`n`/`c`
  (l.980, l.1005, l.997, l.1001, l.894),
  `handle_thread_list_input()` for `s` (l.1158),
  `run_loop()` calls `input::from_key(key)` at l.2683,
  `run_tui(engine, filename)` signature (2 params)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6 (2026-03-19)

### Debug Log References

None — no unexpected issues encountered.

### Completion Notes List

- Task 1: Created `crates/hprof-tui/src/keymap.rs` with `KeymapPreset` (#[derive(Default)] with
  `#[default] Azerty`), `Keymap` flat struct (12 `KeyCode` fields), `FromStr` for preset names,
  `KeymapPreset::build()` factory. Both presets start identical. 8 tests covering uniqueness,
  roundtrip, error messages.
- Task 2: Added `HideField`, `RevealHidden`, `PrevPin`, `NextPin`, `BatchExpand` to `InputEvent`.
  Hardcoded h/H/b/n/c/s mappings in `from_key()`. Updated `handle_favorites_input()` and
  `handle_stack_frames_input()` to use proper variants. Removed `SearchChar('s')` arm from
  `handle_thread_list_input()`. Updated 8 tests in `app/tests.rs`. Added 6 tests in `input.rs`.
- Task 3: Changed `from_key(key) → from_key(key, keymap: &Keymap)`. Two-phase matching: (1)
  layout-independent hardcoded keys via match, (2) configurable keys via keymap field comparisons,
  (3) SearchChar catch-all. Added `keymap: Keymap` to `App` struct. Updated `App::new`,
  `run_tui`, `run_loop` signatures. Updated all 111 `App::new` calls in tests.rs.
- Task 4: Added `keymap: Option<String>` to `AppConfig`. Added `--keymap` CLI flag to `Cli`.
  Added `InvalidKeymap(String)` to `CliError`. Precedence: CLI > TOML > default ("azerty").
  Created `config.toml` sample at project root with documented keymap field.
- Task 5: Replaced `ENTRIES` static array with `help_entries(keymap: &Keymap)` dynamic function.
  Added `key_label(KeyCode) -> String` helper. Added `keymap: Keymap` field to `HelpBar` widget.
  Updated `build_rows` to accept `keymap: &Keymap`. Preserved `HelpContext::Navigating` override
  and bitmask pattern. Entry count = 23 for both presets. Updated all help_bar tests.
- Task 6: `cargo test --all-targets` — 1069 tests pass. `cargo clippy --all-targets -- -D
  warnings` — clean. `cargo fmt -- --check` — clean. Manual tests 6.4/6.5 left to user.
- Code review fixes (2026-03-19): Removed dead `Keymap.toggle_help` field (M1 — `?` is
  hardcoded/layout-independent, not configurable); extracted `resolve_keymap()` helper in
  `main.rs` so precedence tests exercise the real code path (M2); made `required_height()`
  derive entry count dynamically from `help_entries()` and moved `ENTRY_COUNT` to
  `#[cfg(test)]` scope (M3).

### File List

- `crates/hprof-tui/src/keymap.rs` — new module
- `crates/hprof-tui/src/lib.rs` — added `pub mod keymap`
- `crates/hprof-tui/src/input.rs` — new `InputEvent` variants, `from_key(key, keymap)` signature
- `crates/hprof-tui/src/app/mod.rs` — `Keymap` in `App`, updated `run_tui`/`run_loop`/`App::new`
- `crates/hprof-tui/src/app/tests.rs` — updated `App::new` calls, `SearchChar` → proper variants
- `crates/hprof-tui/src/views/help_bar.rs` — `help_entries(keymap)` dynamic fn, `HelpBar.keymap`;
  `required_height()` now derived dynamically (review fix)
- `crates/hprof-cli/src/config.rs` — added `keymap: Option<String>` field
- `crates/hprof-cli/src/main.rs` — `--keymap` flag, `InvalidKeymap` error, pass keymap to TUI;
  `resolve_keymap()` helper extracted (review fix)
- `config.toml` — sample config file with keymap documentation (project root)
- `docs/code-review/claude-story-13.5-adversarial-review.md` — post-implementation code review
- `docs/implementation-artifacts/sprint-status.yaml` — status updated
- `docs/implementation-artifacts/13-5-azerty-qwerty-keymapping.md` — this story file

## Change Log

- 2026-03-19: Implemented story 13.5 — AZERTY/QWERTY keymap abstraction. Added `keymap.rs`
  module with `KeymapPreset`/`Keymap` types; promoted `h/H/b/n/c/s` from `SearchChar` catch-all
  to proper `InputEvent` variants; refactored `from_key()` to accept `&Keymap`; added `--keymap`
  CLI flag and `keymap` TOML field with CLI > config > default precedence; updated `help_bar.rs`
  to render active key labels dynamically from `Keymap`.
- 2026-03-19: Code review fixes — removed dead `toggle_help` field from `Keymap`; extracted
  `resolve_keymap()` in `main.rs`; made `required_height()` dynamic.
