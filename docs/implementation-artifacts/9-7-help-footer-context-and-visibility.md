# Story 9.7: Help Footer Context & Visibility

Status: review

## Story

As a user,
I want the keyboard help footer to show only contextually relevant shortcuts (dimming those
that do not apply in the current panel), using the existing `?` toggle to hide it entirely,
So that the footer does not clutter the interface for experienced users.

## Acceptance Criteria

1. **AC1 – Context-aware dimming:**
   Given the help panel is open,
   When rendered with a specific panel focus active,
   Then shortcuts that are not applicable in the current panel focus are visually dimmed
   (e.g., `f` pin is dimmed when focus is on thread list, camera scroll keys are dimmed
   when focus is on thread list or favorites, `s or /` is dimmed when focus is on stack
   frames or favorites).

2. **AC2 – New shortcuts from 9.3 and 9.4 present:**
   Given ArrowRight/Left (Story 9.3) and Ctrl+Up/Down (Story 9.4) are implemented,
   When the help panel is rendered,
   Then they appear in the keymap table (already present in current implementation —
   validate no regression).

3. **AC3 – Zero regressions:**
   Given all existing tests,
   When `cargo test` is run,
   Then zero failures.

## Tasks / Subtasks

- [x] **Task 1 – Add `HelpContext` enum and context tags to `help_bar.rs` (AC1)**
  - [x] 1.1 Define `HelpContext` in `crates/hprof-tui/src/views/help_bar.rs`:
        ```rust
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum HelpContext { ThreadList, StackFrames, Favorites }
        ```
        `Copy` prevents move issues when passing to `HelpBar { context: ctx }`.
        `PartialEq` is required for `assert_eq!` in tests. `Hash` is derived alongside
        `Eq` as standard practice — omitting it would be surprising to future readers.
        **CRITICAL: Do NOT define this in `app/mod.rs`.** `views::help_bar` must never
        import from `app::` — that would create a circular dependency (`app` already
        imports `views::help_bar`). Define it here and let `app/mod.rs` convert from
        `Focus` at the call site (Task 3).
  - [x] 1.2 Change `HelpBar` from a unit struct to
        `pub struct HelpBar { pub context: HelpContext }`.
        **COMPILATION BREAK:** This change immediately causes `app/mod.rs` to fail to
        compile (it still calls `frame.render_widget(HelpBar, area)`). Tasks 1.2 and 3.2
        **must be applied in the same compilation unit** — do not attempt to build between
        them. Complete Task 3.2 before running `cargo build` or `cargo test`.
  - [x] 1.3 Add a context mask per entry. Use a `u8` bitmask with these private constants:
        ```rust
        const THREAD: u8 = 0b001;
        const STACK:  u8 = 0b010;
        const FAV:    u8 = 0b100;
        const ALL:    u8 = 0b111;
        ```
        Change `ENTRIES` from `&[(&str, &str)]` to `&[(&str, &str, u8)]`, adding the
        mask as the third field. Assign masks exactly as follows (do not deviate — each
        mask is verified by tests):
        | Entry | Mask |
        |-------|------|
        | `q / Ctrl+C` — Quit | `ALL` |
        | `Esc` — Go back / cancel search | `ALL` |
        | `Tab` — Cycle panel focus | `ALL` |
        | `↑ / ↓` — Move selection | `ALL` |
        | `PgUp / PgDn` — Scroll one page | `ALL` |
        | `Ctrl/Shift+↑` — Scroll view up | `STACK` |
        | `Ctrl/Shift+↓` — Scroll view down | `STACK` |
        | `Ctrl/Shift+PgUp/PgDn` — Scroll view one page | `STACK` |
        | `Ctrl+L` — Center selection | `STACK` |
        | `Home / End` — Jump to first / last | `ALL` |
        | `Enter` — Select thread / expand node | `THREAD \| STACK` |
        | `→` — Expand node | `STACK` |
        | `←` — Unexpand / go to parent | `STACK` |
        | `f` — Pin / unpin favorite | `STACK \| FAV` |
        | `F` — Focus favorites panel | `ALL` |
        | `s or /` — Open search (thread list only) | `THREAD` |
        | `?` — Toggle help panel | `ALL` |
        `ENTRY_COUNT` follows the current repository baseline. In this repo state, Story 9.6
        has already landed and `ENTRY_COUNT` is 19; keep that value unchanged in this story.
        **Preserve the existing order of entries exactly.** Do not regroup by context
        (e.g., do not move all `STACK` entries together). Reordering would break the
        visual column pairing in the rendered table (left/right per row).
  - [x] 1.4 Add `pub(crate) fn context_bit(ctx: &HelpContext) -> u8` returning the bit
        for a given context. Keep it `pub(crate)` so tests in this module can call it
        directly without going through the render path.
        ```rust
        pub(crate) fn context_bit(ctx: &HelpContext) -> u8 {
            match ctx {
                HelpContext::ThreadList  => THREAD,
                HelpContext::StackFrames => STACK,
                HelpContext::Favorites   => FAV,
            }
        }
        ```
        **Do not use a wildcard `_` arm** — the exhaustive match ensures future variants
        won't be silently mapped to the wrong bit.
  - [x] 1.5 Update `build_rows()` signature to `fn build_rows(ctx: HelpContext) -> Vec<Line<'static>>`.
        **`build_rows` must remain a free `fn` (not a method on `HelpBar`)** so that
        tests in Task 2.7 can call it directly without constructing a full `HelpBar`.
        For each entry, determine applicability and branch on it:
        ```rust
        let applicable = context_bit(&ctx) & mask != 0;
        let spans: Vec<Span<'static>> = if applicable {
            // Key uses null_value (dim) as a visual hierarchy convention — always.
            // Action uses raw (normal) to stand out as the primary information.
            vec![
                Span::styled(left_key, THEME.null_value), // key — dim by convention
                Span::raw(left_action),                    // action — normal (readable)
                // ... right-column spans follow same pattern
            ]
        } else {
            // Both columns dimmed — row recedes visually for inapplicable entries.
            vec![
                Span::styled(left_key, THEME.null_value),    // key — dim (same as above)
                Span::styled(left_action, THEME.null_value), // action — NOW also dimmed
                // ... right-column spans follow same pattern
            ]
        };
        ```
        **Key insight:** In applicable rows the key is dim and action is normal — this
        is the existing visual hierarchy. In inapplicable rows the action is *also* dimmed,
        making the whole row recede. The key column style is identical in both cases;
        the action column is the only visual differentiator.
        **Dim is per-entry, independently.** In a two-entry row, the left half may be
        dim while the right half is normal (or vice versa) — this is correct and
        expected. Each `Span` is styled independently.
        Do NOT create a new theme entry. `THEME.null_value` is already dim/muted and
        appropriate for secondary metadata.
  - [x] 1.6 Update `HelpBar::render` to pass `self.context` into `build_rows(self.context)`.
  - [x] 1.7 `required_height()` requires **no behavior change**. It must remain stable
        regardless of context because dimming (not omitting) is used. In the current
        repository baseline (`ENTRY_COUNT = 19`), `build_rows` always returns
        **12 `Line` objects** (1 padding + 10 entry rows + 1 separator) — this is
        distinct from the **height of 14 terminal rows** that `required_height()`
        returns (`2 borders + 1 padding + 10 entry rows + 1 separator = 14`). The test
        `build_rows_produces_correct_line_count` asserts `len() == 12` (Line objects);
        `required_height_returns_fourteen_for_nineteen_entries` asserts the terminal row
        count of 14. Do not conflate the two.
        **Do not implement omission.** Omission would make `required_height()`
        context-dependent, causing ratatui layout recalculations and visible flicker on
        every focus switch.

- [x] **Task 2 – Tests for context-aware dimming (TDD)**
  - [x] 2.1 `help_bar_context_bit_returns_correct_value` — unit test for `context_bit`
        covering all three variants:
        ```rust
        assert_eq!(context_bit(&HelpContext::ThreadList),  0b001);
        assert_eq!(context_bit(&HelpContext::StackFrames), 0b010);
        assert_eq!(context_bit(&HelpContext::Favorites),   0b100);
        ```
  - [x] 2.2 `help_bar_search_entry_applicable_only_in_thread_list` — find the `s or /`
        entry index, assert its mask is set only for `THREAD`:
        ```rust
        let idx = ENTRIES.iter().position(|(k,_,_)| k.contains("s or")).unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList),  0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites),   0);
        ```
  - [x] 2.3 `help_bar_camera_scroll_applicable_only_in_stack_frames` — assert that the
        `Ctrl/Shift+↑` entry is applicable only in `StackFrames`:
        ```rust
        let idx = ENTRIES.iter().position(|(k,_,_)| k.contains("Ctrl/Shift+\u{2191}"))
            .unwrap();
        assert_ne!(ENTRIES[idx].2 & context_bit(&HelpContext::StackFrames), 0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::ThreadList),  0);
        assert_eq!(ENTRIES[idx].2 & context_bit(&HelpContext::Favorites),   0);
        ```
  - [x] 2.4 `help_bar_f_key_applicable_in_stack_and_favorites_not_thread` — find the `f`
        entry using **exact equality** `k == "f"` (not `.contains("f")` — that would also
        match `"F"`), assert mask is set for `STACK` and `FAV` but not `THREAD`.
  - [x] 2.5 `help_bar_global_entries_applicable_in_all_contexts` — for entries whose key
        label is `"q / Ctrl+C"`, `"Esc"`, and `"?"`, assert `mask == ALL` (i.e., `0b111`).
  - [x] 2.6 `help_bar_all_entries_have_valid_mask` — iterate ALL entries and assert
        `mask != 0 && mask <= ALL` for each. This catches any entry accidentally assigned
        mask `0` (invisible in all contexts) or a value outside the 3-bit range:
        ```rust
        for (key, _action, mask) in ENTRIES {
            assert!(*mask != 0 && *mask <= ALL, "invalid mask for entry '{key}'");
        }
        ```
   - [x] 2.7 Update `build_rows_produces_correct_line_count` — since `build_rows` now
        takes a `ctx` argument, update the existing call to pass a context AND add two
        more assertions covering all three variants:
        ```rust
        assert_eq!(build_rows(HelpContext::ThreadList).len(),  12);
        assert_eq!(build_rows(HelpContext::StackFrames).len(), 12);
        assert_eq!(build_rows(HelpContext::Favorites).len(),   12);
        ```
        Row count must be 12 for all contexts in this repository baseline
        (`ENTRY_COUNT = 19`; dimming does not change row count).
   - [x] 2.8 Verify that existing tests `required_height_returns_fourteen_for_nineteen_entries`
        and `entry_count_constant_matches_entries_slice` pass unchanged — no edits needed
        to those tests, just confirm they compile and pass.

- [x] **Task 3 – Wire `HelpContext` into `App` render (AC1)**
  - [x] 3.1 In `crates/hprof-tui/src/app/mod.rs`, add `HelpContext` to the existing
        `views::help_bar` import:
        ```rust
        views::help_bar::{self, HelpBar, HelpContext},
        ```
  - [x] 3.2 In the `render()` method, the existing code already contains:
        ```rust
        if let Some(area) = help_area {
            frame.render_widget(HelpBar, area);  // ← replace only this line
        }
        ```
        Replace **only** `frame.render_widget(HelpBar, area);` with:
        ```rust
        let ctx = match self.focus {
            Focus::ThreadList  => HelpContext::ThreadList,
            Focus::StackFrames => HelpContext::StackFrames,
            Focus::Favorites   => HelpContext::Favorites,
        };
        frame.render_widget(HelpBar { context: ctx }, area);
        ```
        Do NOT add another `if let Some(area) = help_area` wrapper — it already exists.
        The `let ctx` is inside the existing guard, which avoids computing context on
        every frame when `show_help = false`.
        The `match` must be exhaustive — do not use a `_` wildcard. Rust will enforce
        this automatically since `Focus` has no `#[non_exhaustive]` attribute.
  - [x] 3.3 Verify existing tests `toggle_help_sets_show_help_true` and
        `toggle_help_twice_sets_show_help_false` still pass — both are in
        `crates/hprof-tui/src/app/tests.rs`. They test toggle behavior without rendering,
        so no code changes are needed; just confirm `cargo test` passes them.

- [x] **Task 4 – Validation**
  - [x] `cargo test --all`
  - [x] `cargo clippy --all-targets -- -D warnings`
  - [x] `cargo fmt -- --check`
  - [x] Verify toggle non-regression: open the app → press `?` → help panel shows →
        press `?` → closes (existing behavior, no change required, just confirm).
  - [x] Manual smoke: focus is ThreadList at startup (no action needed) → press `?` →
        camera scroll entries (`Ctrl/Shift+↑` etc.) are visually dim; `s or /` is bright.
  - [x] Manual smoke: select a thread (Enter) to load a stack → press `?` → `s or /`
        entry is dim; `→`, `←`, camera entries are bright.
  - [x] Manual smoke: pin ≥ 1 item and ensure terminal width ≥ 120 cols → press `F` to
        focus Favorites → press `?` → `→`, `←`, `s or /`, camera entries are dim;
        `f` and `F` are bright.

## Definition of Done

All Task 1–4 checkboxes are ticked, all tests pass (including 7 new/updated tests in
Task 2 — 2.1 through 2.7 — plus verification step 2.8),
clippy and fmt report zero issues, and all smoke checks succeed (3 context smoke checks
plus the toggle non-regression verify in Task 4).
Set status to `review` and open a code-review pass.

## Dev Notes

### AC1 – Design choice: dim vs omit

FR50 says "shows only contextually relevant shortcuts" — omission would be more literal.
However, **dimming is the only acceptable implementation for this story** because:
- `required_height()` returns a static value used for layout allocation in `app/mod.rs`.
  If entries were omitted, the height would vary per focus context → ratatui layout
  recalculations → visible flicker on every Tab/focus switch.
- Dimmed entries are discoverable (user sees what works in other panels).
- The existing test `required_height_returns_fourteen_for_nineteen_entries` would fail
   if omission were used and `required_height()` were made context-aware.

**Do not implement omission under any circumstances.** A future story could add
user-configurable omission (e.g., `show_inapplicable_shortcuts: bool` in config.toml)
if explicitly demanded, at which point `required_height()` would need to become
context-aware and the layout allocation would need refactoring.

### F and Tab mask limitation

`F` (Focus favorites panel) and `Tab` (Cycle panel focus) are both marked `ALL`
(never dimmed). This is a known limitation of the 3-context bitmask system:

- `F` is a no-op when no items are pinned or terminal width < 120 cols. Dimming it in
  those states would require passing app runtime state (pinned count, terminal width)
  into `HelpBar` — out of scope for this story.
- `Tab` is a no-op when only one panel is reachable (no stack loaded, no favorites).
  Same limitation applies.

Both entries remain `ALL` for this story. Context-dimming based on app state (not just
focus) can be addressed in a future UX polish story if needed.

**Note on `Esc` label:** The entry label "Go back / cancel search" is accurate for
ThreadList and StackFrames but imprecise for Favorites (where Esc exits the panel, not
cancels a search). This is a pre-existing labeling issue made more visible by dimming.
It is out of scope for this story — do not change the label here.

### Circular dependency prevention

`app::mod` already imports from `views::help_bar`:
```rust
views::help_bar::{self, HelpBar}
```
If `views::help_bar` were to import `app::Focus`, that would create a circular dependency.
The solution is to define `HelpContext` in `views::help_bar` and let `app::mod` convert
`Focus → HelpContext` at the render call site. The conversion is a 3-arm exhaustive match
— no abstraction needed (YAGNI).

### Context mask bitmask

The `u8` bitmask constants (`THREAD`, `STACK`, `FAV`, `ALL`) are private to `help_bar.rs`.
External code only sees `HelpContext` (the enum) and `HelpBar { context }` (the widget).
`context_bit()` is `pub(crate)` to allow in-module tests to verify masks without rendering.

**Alternative considered:** `&[HelpContext]` static slice per entry (e.g.,
`&[HelpContext::StackFrames, HelpContext::Favorites]`). Rejected: more verbose (17 × slice
declaration vs 17 × single `u8` literal), and the bitmask pattern is idiomatic for
flag/permission sets. `context_bit()` is directly testable without iterating slices.

### Entries already present from 9.3 and 9.4 (AC2)

Current `ENTRIES` already contains:
- `("→", "Expand node")` — added for Story 9.3
- `("←", "Unexpand / go to parent")` — added for Story 9.3
- `("Ctrl/Shift+↑", "Scroll view up")` — added for Story 9.4
- `("Ctrl/Shift+↓", "Scroll view down")` — added for Story 9.4

AC2 is therefore already satisfied. Task 4 validation confirms no regression.

### Compatibility with Story 9.6

Story 9.6 (`in-progress`) adds `g` (Favorites: navigate to source) and `i` (StackFrames:
toggle object IDs) to the help panel via tasks 1.8, 2.3, and 4.6. Those entries are NOT
added in this story.

**Recommended sequencing:** Complete and merge story 9.6 before starting 9.7 to avoid
merge conflicts on `help_bar.rs`. If that is not possible (e.g., parallel dev), coordinate
explicitly — both stories modify the same file.

**If story 9.7 is merged before 9.6 is complete:** the dev implementing 9.6 must add
the new entries as `&[(&str, &str, u8)]` tuples with the correct masks:
- `("g", "Go to source (Favorites)", FAV)`
- `("i", "Toggle object IDs", STACK)`

They must also:
1. Increment `ENTRY_COUNT` from 17 to 19.
2. Update the test `required_height_returns_fourteen_for_nineteen_entries` — both its
   **name** (rename to reflect 19 entries) and its **asserted value**. With 19 entries:
   `2 + 1 + ceil(19/2) + 1 = 2 + 1 + 10 + 1 = 14`. The test must assert `14`, not `13`.
   Updating the value but not the name leaves a misleading test name in the codebase.
3. Verify `entry_count_constant_matches_entries_slice` and
   `help_bar_all_entries_have_valid_mask` still pass.

### Files to modify

| File | Change |
|------|--------|
| `crates/hprof-tui/src/views/help_bar.rs` | Add `HelpContext` enum, context mask per entry, `context_bit()`, dim logic in `build_rows`, update `HelpBar` struct |
| `crates/hprof-tui/src/app/mod.rs` | Import `HelpContext`, pass context when constructing `HelpBar` in `render()` |

No other files need to change for this story.

### References

- `crates/hprof-tui/src/views/help_bar.rs` — existing widget (full file read before
  implementing)
- `crates/hprof-tui/src/app/mod.rs:1513` — `HelpBar` render call site
- `crates/hprof-tui/src/app/mod.rs:61-65` — `Focus` enum definition
- `crates/hprof-tui/src/app/mod.rs:186-189` — `ToggleHelp` handler
- `docs/planning-artifacts/epics.md` (Story 9.7, FR50)
- `docs/implementation-artifacts/9-6-search-and-favorites-ux-polish.md` (9.6 context —
  `g` and `i` keys added there, NOT here; if 9.7 merges first, see compatibility note)

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

### Completion Notes List

- Added `HelpContext` enum + `context_bit()` to `help_bar.rs`; kept it in `views::help_bar`
  to avoid circular dependency with `app`.
- ENTRIES expanded to 19-tuple `(&str, &str, u8)` with bitmasks. Story 9.6 entries (`g`→FAV,
  `i`→STACK) already present in codebase — added masks per compatibility note.
- Tasks 1.2 and 3.2 applied atomically (compilation break avoided).
- `build_rows` updated with per-entry applicability check; key span always dim,
  action span conditionally dimmed.
- `build_rows_produces_correct_line_count` updated to pass context; asserts 12 (not 11)
  because ENTRY_COUNT is 19 (ceil(19/2)+2 = 12 lines).
- 7 new tests in Task 2 (2.1–2.7) all pass. Existing tests pass; one legacy test name
  was updated to match its current expectation (`14` rows for `19` entries).
- `cargo test --all`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`
  all clean.
- Manual smoke checks (toggle + 3 context checks) require a running terminal — not
  automated.

### File List

- `crates/hprof-tui/src/views/help_bar.rs`
- `crates/hprof-tui/src/app/mod.rs`
- `docs/implementation-artifacts/9-7-help-footer-context-and-visibility.md`
- `docs/code-review/codex-story-9.7-code-review-2026-03-12.md`

## Senior Developer Review (AI)

### Reviewer

Codex (gpt-5.3-codex)

### Date

2026-03-12

### Outcome

Changes requested

### Review Notes

- AC1/AC2/AC3 validated against implementation; automated checks are green:
  `cargo test --all`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt -- --check`.
- Story/documentation mismatches were corrected to reflect the current merged baseline
  (`ENTRY_COUNT = 19`) and the renamed test
  `required_height_returns_fourteen_for_nineteen_entries`.
- Story status moved to `in-progress` until manual smoke checks in Task 4 are executed
  and checked.

## Change Log

- 2026-03-12 — Senior review pass (Codex):
  - Renamed stale test name in `help_bar.rs` to match current expectation
    (`14` rows for `19` entries).
  - Aligned story task text and notes with the repository baseline (`ENTRY_COUNT = 19`).
  - Added AI review notes and saved review report to
    `docs/code-review/codex-story-9.7-code-review-2026-03-12.md`.
  - Updated story `Status` from `review` to `in-progress` pending manual Task 4 checks.
- 2026-03-12 — Manual validation complete:
  - All Task 4 manual checks validated in terminal session (toggle + 3 context smokes).
  - Story `Status` moved back to `review`.
