# Compiled Code Review — Story 3.6: Lazy Value String Loading

- Story: `docs/implementation-artifacts/3-6-lazy-value-string-loading.md`
- Reviewers: Codex (2026-03-07) + Claude/Amelia (2026-03-07)
- Sources: `docs/code-review/codex-story-3.6-code-review.md`, `docs/code-review/claude-story-3.6-code-review.md`
- Outcome: **Changes Requested**

## Validation

- `cargo test --workspace` — 335 tests pass
- `cargo clippy --workspace -- -D warnings` — clean
- `cargo fmt --check` — clean

## Git vs Story File List

Working tree clean. All story files in commit `c29eb26`. Discrepancy count: **0**.

## Acceptance Criteria Audit

| AC | Description | Status |
|----|-------------|--------|
| AC1 | String placeholder `"..."` before load | Implemented |
| AC2 | Enter triggers async load, display updates | Partial — surrogate pairs lossy |
| AC3 | Unresolved warning visible in status bar count | **NOT implemented** |
| AC4 | No duplicate load while loading | Implemented |
| AC5 | Collapse clears string state from memory | Partial — in-flight receivers not cancelled |

## Findings

### [HIGH] 1 — AC3: runtime warnings invisible in status bar

*Both reviewers independently identified this.*

- `App::warning_count` is captured once at construction from `engine.warnings().len()` (`app.rs:75`)
- `poll_strings()` appends to `app_warnings` on failure (`app.rs:372-374`)
- `render()` passes the frozen `self.warning_count` to `StatusBar` (`app.rs:487`) — never updated
- Runtime unresolved-string warnings are completely invisible in the UI

**Fix:** `app.rs:487`
```rust
// Before
warning_count: self.warning_count,
// After
warning_count: self.warning_count + self.app_warnings.len(),
```

### [HIGH] 2 — AC5: collapsing objects does not cancel pending string loads

*Both reviewers independently identified this (Claude: MEDIUM, Codex: HIGH — Codex severity retained).*

- `collapse_object_recursive` clears `string_phases/values/errors` in `StackState` (`stack_view.rs:351`)
- But `App::pending_strings` is not cleaned for in-flight StringRef receivers belonging to collapsed subtrees (`app.rs:287, 294`)
- When thread completes, `poll_strings()` re-inserts `Loaded`/`Failed` state into the already-cleared maps
- Phantom state: if the same object is re-expanded, the StringRef appears as `Loaded` without user action

**Fix:** In the `CollapseObj` / `CollapseNestedObj` arms, collect StringRef IDs from the collapsed subtree before collapsing, then remove them from `pending_strings`.

### [MEDIUM] 3 — AC2: UTF-16 surrogate pairs decoded incorrectly for char[] strings

*Both reviewers identified this (Claude: LOW, Codex: MEDIUM — Codex severity retained as it violates AC2).*

- `decode_prim_array_as_string` maps each 16-bit UTF-16 code unit independently to `char` (`engine_impl.rs:148`)
- Surrogate pairs (0xD800–0xDFFF) that encode supplementary chars (emoji, U+10000+) are each replaced with `\u{FFFD}`
- Java strings containing emoji or CJK extension chars display as replacement characters
- Existing test codifies the lossy behavior (`engine_impl.rs:672`)

**Fix:** Replace the chunk-based map with `String::from_utf16_lossy()`:
```rust
5 => {
    let units: Vec<u16> = bytes.chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units).into_owned()
}
```

### [MEDIUM] 4 — Failed StringRef styled with `SEARCH_HINT` instead of `STATUS_WARNING`

*Codex only.*

- Dev Agent record states: "Failed as `<unresolved>` in warning style" (`story:314`)
- Implementation uses `theme::SEARCH_HINT` (dark gray) for failed StringRef rows (`stack_view.rs:785`)
- `theme::STATUS_WARNING` is yellow — the semantic warning color defined in `theme.rs:35`
- Result: failed string fields are nearly invisible rather than highlighted as a problem

**Fix:** `stack_view.rs:780-789` — replace `theme::SEARCH_HINT` with `theme::STATUS_WARNING` for `StringPhase::Failed`.

### [MEDIUM] 5 — No visual distinction between `Unloaded` and `Loading` for StringRef

*Claude only.*

- `format_field_value` renders both `Unloaded` and `Loading` as `String = "..."` (`stack_view.rs:586-591`)
- Object expansion shows `~ Loading...` while async load runs; string loading shows nothing different
- Users cannot distinguish "not yet requested" from "load in progress"
- Inconsistent with the established `OnObjectLoadingNode` UX pattern from story 3.5

**Fix:** Add a `Loading` arm in `format_field_value`:
```rust
Some((StringPhase::Loading, _)) => "String = \"~\"".to_string(),
```
And update the rendering style to `theme::SEARCH_HINT` (matching the loading node style).

### [MEDIUM] 6 — AC4 cursor-movement case not covered by tests

*Claude only.*

- AC4: *"no additional load is started"* when pressing Enter again **or moving cursor**
- `enter_on_loading_string_ref_is_noop` (`app.rs:1092`) only tests the Enter case
- No test verifies `move_up()`/`move_down()` during Loading does not trigger a new spawn
- Behavior is implicitly correct but unprotected against regression

**Fix:** Add a test asserting that after `start_string_loading`, calling `handle_input(InputEvent::Down)` does not grow `pending_strings`.

## Low

### [LOW] 7 — `find_prim_array` duplicates 40 lines from `find_instance` (DRY)

*Claude only.*

- The segment-filter loop (candidate_segs, overlaps, bounds checks) is copy-pasted between `find_prim_array` and `find_instance` (`hprof_file.rs:136-229`)
- Only the inner scanner differs
- Story devnotes intentionally specified "mirror find_instance exactly" — acceptable for now, worth refactoring before the function count grows

### [LOW] 8 — Test `enter_on_loading_string_ref_is_noop` missing `pending_strings` assertion

*Claude only.*

- `app.rs:1092-1115`: asserts phase stays `Loading` but does not assert `app.pending_strings.is_empty()`
- A regression that spawns a duplicate load thread would not be caught

## Summary Table

| ID | Severity | Source | AC | Description |
|----|----------|--------|----|-------------|
| 1 | HIGH | Both | AC3 | `app_warnings` never shown in status bar |
| 2 | HIGH | Both | AC5 | In-flight string loads not cancelled on collapse |
| 3 | MEDIUM | Both | AC2 | UTF-16 surrogate pairs decoded incorrectly |
| 4 | MEDIUM | Codex | — | Failed StringRef uses wrong theme color |
| 5 | MEDIUM | Claude | — | Unloaded/Loading states visually identical |
| 6 | MEDIUM | Claude | AC4 | Cursor movement during Loading not tested |
| 7 | LOW | Claude | — | DRY: `find_prim_array` duplicates `find_instance` loop |
| 8 | LOW | Claude | — | Test missing `pending_strings` emptiness assert |

**Story status recommendation:** remain `in-progress` until findings 1–4 are resolved.
