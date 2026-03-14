# Code Review — Story 9.9: Value Hiding & Reset in Pinned Snapshots

**Date:** 2026-03-14
**Reviewer:** Amelia (Dev Agent — Claude Opus 4.6)
**Story file:** `docs/implementation-artifacts/9-9-value-hiding-and-reset-in-pinned-snapshots.md`
**Branch:** `feature/epic-9-navigation-data-fidelity`
**Commits:** `050b096`, `14a376a`, `0d4e264`

## Summary

Story 9.9 adds field/variable hiding in pinned snapshots (`h` to hide,
`H` to toggle reveal mode showing placeholders). All 6 ACs are correctly
implemented. 17 tests pass. No HIGH severity issues found.

## Git vs Story File List

| Source | Files |
|--------|-------|
| Story File List | 8 source files |
| Git (3 commits) | 8 source files + story + sprint-status |

**Discrepancies: 0** — perfect match.

## AC Validation

| AC | Status | Evidence |
|----|--------|----------|
| AC1 – Hide field via `h` | IMPLEMENTED | `app/mod.rs:709-722` |
| AC2 – `H` reveals placeholders | IMPLEMENTED | `app/mod.rs:725-733`, `tree_render.rs:106-112` |
| AC3 – Restore via `h` on placeholder | IMPLEMENTED | `app/mod.rs:713-714` (`is_hidden` → `remove`) |
| AC4 – `H` toggles reveal off | IMPLEMENTED | `app/mod.rs:728` (`show_hidden = !show_hidden`) |
| AC5 – Scope (instance fields + vars only) | IMPLEMENTED | `HideKey` only in Frame/Subtree paths |
| AC6 – Help panel entries | IMPLEMENTED | `help_bar.rs:43-44`, `FAV` mask |

## Task Audit

All 7 tasks (17 subtasks) marked `[x]` are genuinely implemented.
No false claims detected.

## Issues Found

### MEDIUM

**M1 — `field_row_maps` not cleared in `set_items_len(0)`**
`favorites_panel/mod.rs:77` — `row_counts`, `row_kind_maps`, and
`chunk_sentinel_maps` are all `.clear()`'d when `len == 0`, but
`field_row_maps` was missing. Stale data persists after full unpin.
Not exploitable (the `pinned.get_mut(idx)` guard prevents mutation),
but violates the symmetry invariant with the other 3 Vecs.
**Status: FIXED** — added `self.field_row_maps.clear()`.

**M2 — Stale docstring in `help_bar.rs:84`**
Comment says `ENTRY_COUNT = 19` with calculation `2 + 1 + 10 + 1 = 14`
but `ENTRY_COUNT` is now 21. Code is correct (result = 15), only the
comment was stale.
**Status: FIXED** — updated to `ENTRY_COUNT = 21` / `2 + 1 + 11 + 1 = 15`.

### LOW

**L1 — AC IDs swapped in code comments**
`app/mod.rs:704`: comment says `(AC1, AC2)` but `h` implements AC1
(hide) and AC3 (restore on placeholder). `H` implements AC2/AC4.

**L2 — Instance fields inside collection sub-trees not hideable**
`collect_collection_entry_obj_rows` does not generate `field_row_map`
entries. Instance fields of objects reached via collection entries
cannot be hidden. Consistent with story scope but could surprise users.

**L3 — `show_hidden` toggleable when `hidden_fields` is empty**
`H` toggles `show_hidden` even when no fields are hidden. No visual
effect — silent state change. Harmless but potentially confusing in
debug scenarios.

**L4 — No visual cue when all instance fields of an object are hidden**
If all fields are hidden (`show_hidden=false`), the expanded object
shows its `-` row followed directly by static fields or the next
sibling. No `(all fields hidden)` indicator. Row-count invariant holds
(debug_assert passes), but UX could be confusing.

## Verification

```
cargo test --all        → 0 failures
cargo clippy --all-targets -- -D warnings → clean
cargo fmt -- --check    → clean
```

## Outcome

**Issues fixed:** 2 (M1, M2)
**Action items:** 0
**Story status recommendation:** done
