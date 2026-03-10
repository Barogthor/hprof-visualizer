# Story Review: 7.3 Keyboard Navigation Map

**Date:** 2026-03-10
**Reviewer:** Bob (SM) + Claude adversarial review
**Story file:** `docs/implementation-artifacts/7-3-keyboard-navigation-map.md`
**Story status at review:** ready-for-dev (spec review — no implementation yet)

---

## Review Outcome: Changes Requested → Applied

All findings corrected directly in the story file before dev handoff.

---

## Findings

### 🔴 CRITIQUES (3)

| # | Finding | Correction applied |
|---|---|---|
| C1 | Task 4 layout code would destroy the status bar — help bar must go between `main_area` and `status_area`, not replace it | Task 4 rewritten with correct three-level decomposition |
| C2 | Dev Notes "Current `input.rs` state" was stale — Story 7.1 already added `f`/`F` arms (uncommitted) | Dev Notes updated to reflect post-7.1 working tree state |
| C3 | Merge conflict described as "risk" but already materializing — `input.rs` and `stack_view.rs` have uncommitted 7.1 changes | Elevated to "Action immédiate" with concrete options |

### 🟡 HAUTES (2)

| # | Finding | Correction applied |
|---|---|---|
| H1 | `StubEngine` already exists in `app.rs:914` — story said "check if one exists" | Task 6 and Dev Notes updated to reference existing stub directly |
| H2 | `required_height()` formula unspecified — dev would guess | Formula documented: `2 + 1 + ceil(entries/2) + 1 = 10`, with `ENTRY_COUNT` constant |

### 🟢 MOYENNES (2)

| # | Finding | Correction applied |
|---|---|---|
| M1 | `is_search_active()` is `pub` — caveat "verify visibility" was unnecessary uncertainty | Task 6 updated with direct reference to line 166 |
| M2 | Task 4 imported `{HelpBar, required_height}` but used `help_bar::required_height()` path | Unified to `{self, HelpBar}` import with `help_bar::required_height()` usage |

---

## Story quality post-corrections

- All ACs testable and unambiguous ✅
- All tasks actionable with precise file locations ✅
- Merge conflict situation clearly communicated ✅
- No contradictions between tasks and Dev Notes ✅

_Reviewer: Claude (SM agent) — 2026-03-10_
