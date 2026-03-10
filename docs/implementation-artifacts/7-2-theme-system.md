# Story 7.2: Theme System

Status: ready-for-dev

## Story

As a developer,
I want a centralized `theme.rs` module in `hprof-tui` defining all colors using the 16-color ANSI
palette via a `Theme` struct,
so that visual consistency is enforced across all widgets without scattered color constants.

## Acceptance Criteria

1. **`Theme` struct defined** — `theme.rs` defines a `pub struct Theme` with the following named
   `Style` fields: `thread_runnable`, `thread_waiting`, `thread_blocked`, `thread_unknown`,
   `primitive_value`, `string_value`, `null_value`, `expand_indicator`, `loading_indicator`,
   `error_indicator`, `warning`, `selection_bg`, `selection_fg`, `border_focused`,
   `border_unfocused`, `status_bar_bg`.
   > Note: `border_focused` and `border_unfocused` intentionally replace the single `border`
   > role from the AC spec — the existing code requires both variants and collapsing them to one
   > would lose the focused/unfocused distinction.

2. **All widgets use `Theme`** — Every widget in `hprof-tui` that renders colored output
   references colors via the `THEME` constant — no inline `Color::*` literals in
   `thread_list.rs`, `stack_view.rs`, `status_bar.rs`, or any other view file.

3. **Value types are visually distinguished** — In the stack and object trees, each row
   whose value is a primitive (int/long/bool/etc.) uses `THEME.primitive_value` as the row
   style, each row whose value is a string/char uses `THEME.string_value`, and each row whose
   value is `null` uses `THEME.null_value`. V1 scope: the color is applied to the full row
   (field name + value), not just the value fragment — splitting into multi-`Span` lines is
   deferred.

4. **16-color ANSI only** — All colors use only the 16-color ANSI palette
   (`Color::Red`, `Color::Green`, `Color::Yellow`, `Color::Blue`, `Color::Magenta`,
   `Color::Cyan`, `Color::Gray`, `Color::DarkGray`, `Color::White`, `Color::Black`,
   and their `Color::Light*` variants) — no `Color::Rgb(...)` or `Color::Indexed(...)` anywhere
   in `hprof-tui`.

5. **Manual validation** — Running `cargo run -- assets/heapdump-visualvm.hprof` shows the
   TUI with correctly colored thread states (green/yellow/red/gray dots), a gray status bar,
   and no visual regression compared to before this story.

6. **No regressions** — All existing tests pass (`cargo test`). Clippy clean
   (`cargo clippy --all-targets -- -D warnings`).

## Tasks / Subtasks

- [ ] Task 1: Refactor `theme.rs` — replace loose `const` values with `Theme` struct (AC: 1, 4)
  - [ ] Define `pub struct Theme` with all required `Style` fields (see AC1 list + `search_active`)
  - [ ] Declare `pub const THEME: Theme = Theme { ... }` with 16-ANSI-only values
  - [ ] Assign color values per the mapping table in Dev Notes
  - [ ] Remove old standalone `const` values
  - [ ] Update module docstring to describe `Theme` struct and `THEME` singleton
  - [ ] Update `mod tests` to assert all `THEME.field` are `Style` (compile-time exhaustiveness)

- [ ] Task 2: Update `thread_list.rs` (AC: 2)
  - [ ] `theme::STATE_RUNNABLE/WAITING/BLOCKED/UNKNOWN` → `THEME.thread_runnable` etc.
  - [ ] `theme::BORDER_FOCUSED/UNFOCUSED` → `THEME.border_focused` / `THEME.border_unfocused`
  - [ ] `theme::SEARCH_ACTIVE` → `THEME.search_active`
  - [ ] `theme::SEARCH_HINT` → `THEME.null_value`
  - [ ] `theme::LEGEND` → `THEME.null_value` (same DarkGray color)
  - [ ] `theme::SELECTED` → `THEME.selection_bg`

- [ ] Task 3: Update `stack_view.rs` (AC: 2, 3)
  > ⚠️ Task 3 contains two distinct parts: **mechanical renaming** (theme:: references) and
  > **new rendering behavior** (value colors, error/loading indicators). The new behavior is
  > required for AC3. If time is constrained, Tasks 1+2+4 satisfy AC1+AC2; Task 3 wiring
  > is required for AC3.
  - [ ] `theme::SELECTED` → `THEME.selection_bg` (~15 sites — run
    `grep -n "theme::" crates/hprof-tui/src/views/stack_view.rs` to enumerate all)
  - [ ] `theme::SEARCH_HINT` → `THEME.null_value`
  - [ ] `theme::BORDER_FOCUSED/UNFOCUSED` → `THEME.border_focused` / `THEME.border_unfocused`
  - [ ] Wire value-type colors (AC: 3) — see "Wiring value-type colors" in Dev Notes:
    - [ ] Add `fn value_style(v: &FieldValue) -> Style` helper in `stack_view.rs`
    - [ ] Apply in `build_variable_items` / `build_field_items` when constructing `Span` for values
    - [ ] Apply `THEME.expand_indicator` to `+`/`-`/`>`/`v` toggle characters
    - [ ] Apply `THEME.loading_indicator` to `~ Loading...` text
    - [ ] Apply `THEME.error_indicator` to rows in `ExpansionPhase::Failed` state —
    `error_indicator` has priority over `value_style()`: if phase is `Failed`, use
    `THEME.error_indicator` regardless of the underlying `FieldValue` type

- [ ] Task 4: Update `status_bar.rs` (AC: 2)
  - [ ] `theme::STATUS_BAR` → `THEME.status_bar_bg`

- [ ] Task 5: Verify no stray inline colors anywhere (AC: 2, 4)
  - [ ] `grep -rn "Color::" crates/hprof-tui/src/views/` — must return zero results
  - [ ] `grep -rn "use ratatui::style::Color" crates/hprof-tui/src/views/` — must return zero
    results (a `use` import of `Color` in a view file signals an inline usage)
  - [ ] `app.rs`, `input.rs`, `warnings.rs` — confirm no `Color::*` (already clean, verify)

- [ ] Task 6: Validate and finalize (AC: 5, 6)
  - [ ] Run `cargo run -- assets/heapdump-visualvm.hprof` and verify visual output
  - [ ] Run `cargo test` — zero failures
  - [ ] Run `cargo clippy --all-targets -- -D warnings` — zero warnings
  - [ ] Run `cargo fmt` — no changes

## Dev Notes

### Current state of `theme.rs`

`theme.rs` already exists at `crates/hprof-tui/src/theme.rs` with loose `const` style values.
This story converts those into a `Theme` struct + `pub const THEME: Theme` singleton.

**Why a struct instead of keeping loose `const` values?**
The primary reason is **compile-time exhaustiveness**: with a struct literal, forgetting to
initialize any field is a compiler error. With isolated `const` values, a new role can be
added to the module and never referenced anywhere — silently unused. The struct makes
omissions impossible. Secondary benefit: a `&Theme` can be passed to render functions for
future testability, and the struct prepares the post-MVP TOML config path
(UX spec §Customization).

**Mapping from old `const` to new `Theme` fields:**

| Old `const` | New `Theme` field | Color value |
|---|---|---|
| `STATE_RUNNABLE` | `thread_runnable` | `Style::new().fg(Color::Green)` |
| `STATE_WAITING` | `thread_waiting` | `Style::new().fg(Color::Yellow)` |
| `STATE_BLOCKED` | `thread_blocked` | `Style::new().fg(Color::Red)` |
| `STATE_UNKNOWN` | `thread_unknown` | `Style::new().fg(Color::DarkGray)` |
| `SELECTED` | `selection_bg` | `Style::new().add_modifier(Modifier::REVERSED)` |
| `BORDER_FOCUSED` | `border_focused` | `Style::new().fg(Color::White).add_modifier(Modifier::BOLD)` |
| `BORDER_UNFOCUSED` | `border_unfocused` | `Style::new().fg(Color::DarkGray)` |
| `SEARCH_ACTIVE` | `search_active` *(additional field, not in AC list)* | `Style::new().fg(Color::Cyan)` |
| `SEARCH_HINT` + `LEGEND` | `null_value` | `Style::new().fg(Color::DarkGray)` |
| `STATUS_BAR` | `status_bar_bg` | `Style::new().fg(Color::White).bg(Color::DarkGray)` |
| `STATUS_WARNING` | `warning` | `Style::new().fg(Color::Yellow)` |

**New fields (no current equivalent) — from UX spec `ux-design-specification.md` §Color Palette:**

| New `Theme` field | Color value | Rationale |
|---|---|---|
| `primitive_value` | `Style::new().fg(Color::Yellow)` | UX spec: "values by type → numbers: yellow" |
| `string_value` | `Style::new().fg(Color::Green)` | UX spec: "values by type → strings: green" |
| `null_value` | `Style::new().fg(Color::DarkGray)` | UX spec: "null: dim/dark gray" |
| `expand_indicator` | `Style::new().fg(Color::DarkGray)` | Secondary/neutral — not yellow (conflicts with warning+numbers) |
| `loading_indicator` | `Style::new().fg(Color::Cyan)` | Distinct from values; matches search_active |
| `error_indicator` | `Style::new().fg(Color::Red)` | UX spec: "Errors: red text" |
| `selection_fg` | `Style::default()` | Foreground on selected row — no override needed with REVERSED |

> **Color semantics** (UX spec §Value Colors):
> - Cyan = Java type annotations (class names, type labels)
> - Yellow = numeric values AND warnings (both "notable")
> - Green = string values AND thread_runnable
> - DarkGray = null, secondary info, unfocused elements
>
> `primitive_value` = Yellow (NOT cyan — cyan is for type names, not numeric values).

### Wiring value-type colors (AC3 — critical)

The current `format_field_value()` and `format_entry_value()` in `stack_view.rs` return `String`.
To apply per-type styling, add a pure helper alongside them:

```rust
/// Returns the `Style` to apply to a rendered `FieldValue`.
fn value_style(v: &FieldValue) -> Style {
    use hprof_engine::FieldValue::*;
    match v {
        Null => THEME.null_value,
        Bool(_) | Byte(_) | Short(_) | Int(_) | Long(_) |
        Float(_) | Double(_) | Char(_) => THEME.primitive_value,
        ObjectRef { inline_value: Some(_), .. } => THEME.string_value,
        ObjectRef { .. } => Style::default(),
    }
}
```

Then at call sites where a variable/field row is built, apply `value_style()` as the row
style (full row — V1 scope per AC3). When the row is selected, layer selection on top via
`value_style(v).patch(THEME.selection_bg)`.

**Style priority rule** (highest wins):
1. `ExpansionPhase::Failed` → `THEME.error_indicator` (overrides everything)
2. Selected row → `value_style(v).patch(THEME.selection_bg)`
3. Default → `value_style(v)`

**`patch()` behavior in ratatui:** `a.patch(b)` applies `b` on top of `a` — non-None fields
in `b` overwrite `a`, modifiers are OR'd together. So
`Style::new().fg(Color::Yellow).patch(Style::new().add_modifier(Modifier::REVERSED))`
produces `Yellow + REVERSED`. ✅ The value color is preserved under the selection highlight.

> **Note:** `Char` maps to `primitive_value` (Yellow) per the JVM type system.
> `String`/char arrays with `inline_value` map to `string_value` (Green).
> Plain `ObjectRef` without inline value uses `Style::default()` (no color — class name only).

For `expand_indicator` and `loading_indicator`, these are part of the text prefix of a row
(e.g., `"+ fieldName"` or `"~ Loading [100..199]"`). Apply the style to the leading toggle
`Span` only, not the full row text:

```rust
// Example for expand toggle prefix
let toggle_style = if is_loading {
    THEME.loading_indicator
} else if is_expanded {
    THEME.expand_indicator  // "-"
} else {
    THEME.expand_indicator  // "+"
};
Span::styled(toggle, toggle_style)
```

### `selection_bg` vs `selection_fg`

- `selection_bg` = `Style::new().add_modifier(Modifier::REVERSED)` — applied to the full row
  when selected
- `selection_fg` = `Style::default()` — **defined to satisfy AC1 but not used in this story**.
  Story 7.1 (Favorites Panel) may introduce a second panel needing distinct selection colors;
  wire it then. Do not add dead-code references to `selection_fg` in this story.

All current usages of `SELECTED` map to `selection_bg`.

### `error_indicator` — câblage sur `ExpansionPhase::Failed`

`stack_view.rs` modélise l'état d'une expansion via `ExpansionPhase` (`Collapsed`, `Loading`,
`Expanded`, `Failed`). Les rows `Failed` n'ont actuellement pas de style distinctif.
Appliquer `THEME.error_indicator` (`Color::Red`) à ces rows est le seul endroit où ce rôle
sémantique est utilisé dans cette story. Sans ce câblage, `error_indicator` serait défini mais
mort — violant le principe "chaque rôle existe parce qu'un widget le demande".

```rust
// Dans le rendu d'une row expanded/failed :
let row_style = match phase {
    Some(ExpansionPhase::Failed) => THEME.error_indicator,
    _ => value_style(v),
};
```

### `const`-compatibility — validation implicite

`pub const THEME: Theme = Theme { ... }` échoue à la compilation si un champ utilise une
valeur non-`const`. `cargo build` est donc la vérification d'exhaustivité et de
const-compatibility en une seule commande. Aucun test supplémentaire n'est nécessaire pour
cette propriété — le compilateur la garantit.

### Inline `Style::default()` and `Modifier::BOLD` in `stack_view.rs`

- `ratatui::style::Style::default()` — **no `Color::*` literal**, acceptable per AC2, no change
- `.highlight_style(Style::default().add_modifier(Modifier::BOLD))` at line ~1688 — **not a
  `Color::*`**, acceptable per AC2. Leave as is (KISS).

### Key files

```
crates/hprof-tui/src/
├── theme.rs                    ← primary: refactor to Theme struct
├── views/
│   ├── thread_list.rs          ← ~10 theme:: references
│   ├── stack_view.rs           ← ~15 theme:: references + value_style() addition
│   └── status_bar.rs           ← 1 theme:: reference
└── app.rs / input.rs / warnings.rs  ← verify clean (no Color:: expected)
```

### Usage pattern in widgets

Before:
```rust
use crate::theme;
Span::styled("o", theme::STATE_RUNNABLE)
```

After:
```rust
use crate::theme::THEME;
Span::styled("o", THEME.thread_runnable)
```

Or `theme::THEME.thread_runnable` if keeping module-level import.

### Architecture constraint

`Theme` lives entirely in `hprof-tui`. No engine changes needed for this story.

### Testing approach

Update the existing `all_style_constants_are_of_type_style` test to enumerate all `THEME` fields:

```rust
fn assert_style(_: Style) {}
assert_style(THEME.thread_runnable);
assert_style(THEME.primitive_value);
assert_style(THEME.string_value);
// ... all fields — compile error if a field is missed
```

The `const` nature of `THEME` makes non-ANSI color enforcement self-enforcing at compile time
(ratatui's `Style::new().fg()` `const fn` only accepts `Color` variants, and `Color::Rgb`/
`Color::Indexed` are valid variants — so this must be verified manually, not by the compiler).
Add a `#[test]` that at minimum calls `assert_style` for every field to catch accidental
addition of an un-initialized field.

For AC3 (value colors wired), add a unit test for `value_style()`:
```rust
assert_eq!(value_style(&FieldValue::Int(42)), THEME.primitive_value);
assert_eq!(value_style(&FieldValue::Null), THEME.null_value);
```

### Epic 6 retro action items applicable here

- **#1 (Manual validation AC):** AC5 covers `heapdump-visualvm.hprof` visual check.
- **#2 (Pre-review semantic AC checklist):** Before review, run
  `grep -rn "Color::" crates/hprof-tui/src/views/` — must return zero. Also verify AC3
  by manually confirming value rows show distinct colors in the TUI.

### References

- Current `theme.rs`: `crates/hprof-tui/src/theme.rs`
- Stack view: `crates/hprof-tui/src/views/stack_view.rs` — `format_field_value()` line 1022,
  `format_entry_value()` line 1066
- Thread list: `crates/hprof-tui/src/views/thread_list.rs`
- Status bar: `crates/hprof-tui/src/views/status_bar.rs`
- UX color palette: `docs/planning-artifacts/ux-design-specification.md` §Value Colors (line ~437)
- Story spec: `docs/planning-artifacts/epics.md` — Epic 7, Story 7.2
- Architecture: `docs/planning-artifacts/architecture.md` — Frontend Architecture section
- Epic 6 retro: `docs/implementation-artifacts/epic-6-retro-2026-03-10.md`

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

### File List
