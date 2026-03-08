# Compiled Code Review — Story 3.5: Recursive Expansion & Collection Size Indicators

**Reviewers:** Claude (Amelia) + Codex
**Date:** 2026-03-07
**Sources:**
- `docs/code-review/claude-story-3.5-code-review.md`
- `docs/code-review/codex-story-3.5-code-review.md`

---

## Agreement

Both reviewers confirm:
- ✅ Git matches Story File List (modulo debug artifacts)
- ✅ All 304 tests pass
- ✅ AC1, AC3, AC4, AC5, AC6: implemented
- ✅ `diag.txt` untracked artifact not in File List

---

## Divergence on AC2

**Codex:** AC2 is **Partially Implemented** — story spec says "case-insensitive" suffix matching but `COLLECTION_CLASS_SUFFIXES.contains(&short_name)` is exact-case.

**Claude:** AC2 treated as implemented — in practice JVM class names are always title-case, so exact match works on real dumps.

**Verdict:** Codex is technically correct. Task 3.1 explicitly says "case-insensitive". The implementation is a spec deviation, even if benign in practice.

---

## Compiled Findings

### 🔴 CRITICAL / HIGH

#### C1 — Task 9.1 marked `[x]` but root-var class name not shown after expansion *(Codex only)*

`crates/hprof-tui/src/views/stack_view.rs:549`

Task 9.1 spec: *"Change to use the enriched class name from fields once the object has been expanded at least once."*
Implementation: always renders `"Object [▼]"` regardless of expansion state.

**Nuance:** Task 9.2 explicitly defers this to Story 3.6 with the note *"class name at root-var level requires a separate engine query not covered in this story"*, and the Dev Agent Record documents the deferral. The technical path (fetching class name of the root object) isn't available in `StackState` without a new engine call. The task status `[x]` is misleading given the deferral, but this is not a stealth omission.

**Recommended action:** Change Task 9.1 status to `[ ]` with a note referencing 3.6, or scope Task 9.2 more clearly.

#### H1 — Collection suffix matching is case-sensitive, story says case-insensitive *(Codex only)*

`crates/hprof-engine/src/engine_impl.rs:49`

```rust
if !COLLECTION_CLASS_SUFFIXES.contains(&short_name) { return None; }
```

Story Task 3.1: *"case-insensitive"*. Fix:
```rust
if !COLLECTION_CLASS_SUFFIXES.iter().any(|s| short_name.eq_ignore_ascii_case(s)) {
    return None;
}
```

#### H2 — `class_names_by_id` ordering dependency: STRING records must precede LOAD_CLASS *(Codex only)*

`crates/hprof-parser/src/indexer/first_pass.rs:240-245`

Class name is resolved from `index.strings` immediately when a `LOAD_CLASS` record is parsed. If the corresponding STRING record appears later in the file, the class name is silently stored as `""`. No reconciliation pass exists.

In practice the JVM writes STRING records before LOAD_CLASS, so this doesn't affect real dumps today — but it is a structural fragility. Codex rates this HIGH; it's a latent defect.

**Fix options:** (a) second pass to backfill, or (b) store the `class_name_string_id` and resolve lazily on first use.

#### H3 — Test `build_items_depth1_field_has_4_space_indent` checks item count only, not content *(Claude only)*

`crates/hprof-tui/src/views/stack_view.rs:1189`

The test name promises indentation verification; the body only asserts `items.len() == 3`. No test verifies the actual 4-space or 6-space prefix strings produced by the `2 + 2 * (parent_path.len() + 1)` formula. AC 5.3 is structurally unverified.

---

### 🟡 MEDIUM

#### M1 — Negative `size` field value cast to `u64` gives absurd entry count *(Claude only)*

`crates/hprof-engine/src/engine_impl.rs:94,103`

```rust
return Some(v as u64); // v: i32 — wraps if negative
```

A corrupt or uninitialized `size = -1` displays as `HashMap (18446744073709551615 entries)`. Fix: guard `if v >= 0`.

#### M2 — AC4 collapse of nested expanded field not tested *(Claude only)*

`crates/hprof-tui/src/app.rs` — Task 6.4 asserts *"Enter again collapses it"* for nested fields. Only the *start* is tested; no test presses Enter a second time on a nested expanded ObjectRef and verifies `Collapsed` state.

#### M3 — AC6 failure node indentation/format not tested *(Claude only)*

`crates/hprof-tui/src/views/stack_view.rs` — No test inspects the text of `build_items` output for `ExpansionPhase::Failed` at depth > 0.

#### M4 — Story File List missing `epic-2-retro-2026-03-07.md` *(Codex only)*

Untracked file `docs/implementation-artifacts/epic-2-retro-2026-03-07.md` also appears in `git status`. Minor traceability gap alongside `diag.txt`.

---

### 🟢 LOW

#### L1 — Unused import warning in `precise.rs:77` during `cargo test` *(Codex only)*

Not caught by `clippy -D warnings` (test-only code). Clean up to reduce CI noise.

#### L2 — `selected_field_ref_id` docstring implies phase checking inside the function *(Claude only)*

`stack_view.rs:204` — Phase check is done by the caller (`App`), not here. Misleading for future maintainers.

---

## Summary Table

| ID | Severity | Source | One-liner |
|----|----------|--------|-----------|
| C1 | CRITICAL* | Codex | Task 9.1 `[x]` but root-var class name not shown (acknowledged deferral) |
| H1 | HIGH | Codex | Collection suffix match is case-sensitive, spec says case-insensitive |
| H2 | HIGH | Codex | `class_names_by_id` silently empty when STRING arrives after LOAD_CLASS |
| H3 | HIGH | Claude | `build_items` indentation test checks count only, not text |
| M1 | MEDIUM | Claude | Negative `size` cast to `u64` → absurd entry count display |
| M2 | MEDIUM | Claude | AC4 nested collapse not tested |
| M3 | MEDIUM | Claude | AC6 failure node format/depth not tested |
| M4 | MEDIUM | Codex | Story File List missing `epic-2-retro` |
| L1 | LOW | Codex | Unused import warning in `precise.rs` test module |
| L2 | LOW | Claude | `selected_field_ref_id` docstring misleading re: phase |

*C1 is rated Critical by Codex; debatable given explicit deferral in Task 9.2 note.

---

## Recommended Actions (Priority Order)

1. **H1** — Fix case-insensitive suffix matching (`eq_ignore_ascii_case`)
2. **M1** — Guard negative `size`/`elementCount` values before cast
3. **H3** — Add real indentation assertions to `build_items` tests (depth 1 + depth 2)
4. **M2** — Add test: Enter×2 on nested ObjectRef collapses it
5. **M3** — Add test: `build_object_items` Failed renders `! msg` with correct indent
6. **C1** — Correct task status to `[ ]` or split into 9.1a (done) / 9.1b (deferred to 3.6)
7. **H2** — Document ordering assumption; plan deferred lookup for Story 3.6+
8. **L1/L2/M4** — Minor cleanup

---

## Verdict

**Story status: `in-progress`** — H1 (spec violation) and M1 (latent defect) are actionable fixes.
H3/M2/M3 are test quality gaps that should be closed before final sign-off.
C1 is a documentation inconsistency, not a functional regression.
