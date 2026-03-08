# Code Review Report — Story 3.4: Object Resolution & Single-Level Expansion

Date: 2026-03-07
Reviewers: Amelia (Claude Sonnet 4.6) + Codex (cross-review synthesis)

## Scope

- Story: `docs/implementation-artifacts/3-4-object-resolution-and-single-level-expansion.md`
- Files reviewed: all 14 files in the story File List + `segment.rs` for AC5 verification
- Prior Codex review: `docs/code-review/codex-story-3.4-code-review.md`

## Git vs Story Discrepancy

Story File List matches `git diff HEAD~1..HEAD` exactly. Two untracked files
(`diag.txt`, `epic-2-retro-2026-03-07.md`) are not application source — no discrepancy.

## Cross-Review Divergence Notes

### Codex Finding 1 — 0x08 skip length (dismissed as false positive)

Codex flagged `0x08 => skip_n(id_size)` as a bug, claiming `id_size + 8` is needed for
`GC_ROOT_THREAD_OBJECT`. However, per the official hprof spec:

```
0x08 = GC_ROOT_MONITOR_USED  : object_id only             → id_size bytes   (correct)
0x09 = GC_ROOT_THREAD_OBJECT : object_id + serial + serial → id_size + 8     (correct)
```

The story comment incorrectly labels 0x08 as "GC_ROOT_THREAD_OBJECT"; the code is correct.
`first_pass.rs:606` and `hprof_file.rs:250` are both consistent with the spec. **Not a bug.**

---

## Findings

### HIGH

**H1 — Multi-node cycle not guarded in `collect_fields` (`resolver.rs:51`)**
*Found by: Codex*

The recursion guard only catches a direct self-loop (`super_class_id == class_id`). A
two-node cycle (A → B → A) is undetected and causes a stack overflow.

```rust
// resolver.rs:51 — current guard
if info.super_class_id != 0 && info.super_class_id != class_id {
    collect_fields(info.super_class_id, index, out);  // ← A→B→A loops forever
}
```

Fix: track visited class IDs with a `HashSet<u64>` passed through the recursion, or
convert to an iterative loop with a visited set.

Corrupted heap dumps with cyclic class metadata are uncommon in practice, but the fix is
trivial and NFR6 (graceful handling of corrupted data) explicitly covers this case.

---

### MEDIUM

**M1 — Failure message text mismatches AC4 (`app.rs:317`)**
*Found by: Amelia + Codex*

AC4 specifies the pseudo-node text: `! Failed to resolve object`

```rust
// app.rs:317
s.set_expansion_failed(object_id, "Object not found".to_string());
```

The `build_items` fallback at `stack_view.rs:437` uses `"Failed to resolve object"` as a
default, but that default is never reached since the message is always set explicitly.
Display is `! Object not found`, not `! Failed to resolve object`.

**M2 — Enter on `OnVar` while Loading cancels instead of being no-op (`app.rs:241`)**
*Found by: Amelia + Codex*

Story Task 9 specifies `// Loading: Enter is no-op`. Implementation maps `Loading` to the
`_ =>` arm which resolves to `Cmd::CollapseObj(oid)`:

```rust
// app.rs:241
match s.expansion_state(oid) {
    ExpansionPhase::Collapsed | ExpansionPhase::Failed => Cmd::StartObj(oid),
    _ => Cmd::CollapseObj(oid),  // Loading also lands here
}
```

Result: pressing Enter on the parent var row during loading silently cancels the
expansion. This also means AC1 is partially broken (the async operation is not
"non-blocking" from the user's perspective if a casual keypress kills it).

**M3 — Task 6 story checkbox false-incomplete (`engine.rs:157`)**
*Found by: Amelia + Codex*

```
- [ ] Change `expand_object` signature on `NavigationEngine` trait
```

This is marked incomplete but the trait signature is fully and correctly implemented:
```rust
fn expand_object(&self, object_id: u64) -> Option<Vec<FieldInfo>>;
```

Story checkboxes cannot be trusted as source of truth — auditability gap.

---

### LOW

**L1 — `FieldInfo` missing `PartialEq` derive (`engine.rs:119`)**
*Found by: Amelia*

Story spec: `#[derive(Debug, Clone, PartialEq)]`. Implementation: `#[derive(Debug, Clone)]`.
No current test breaks, but future tests comparing `FieldInfo` values directly with
`assert_eq!` will fail to compile.

**L2 — Float/Double rendered without type context (`stack_view.rs:319`)**
*Found by: Amelia*

Task 10 examples show `42 (int)`, `3.14 (float)`. Implementation:
```rust
FieldValue::Int(n) => n.to_string(),      // "42"   not "42 (int)"
FieldValue::Float(f) => format!("{f}"),   // "3.14" not "3.14 (float)"
```
The "Field Value Display Conventions" table is ambiguous on this, but the inline
description in the story explicitly uses suffixed format.

**L3 — Manual smoke test unchecked (`story:921`)**
*Found by: Amelia + Codex*

`- [ ] Manual smoke test: cargo run -- assets/heapdump-visualvm.hprof` is not marked as
done. The end-to-end async loading + cancel UX flow has no execution trace in the Dev
Agent Record.

---

## Acceptance Criteria Status

| AC | Status |
|---|---|
| AC1 — non-blocking async expansion | PARTIAL — Enter+Loading cancels unexpectedly (M2) |
| AC2 — Loading node → real children | IMPLEMENTED |
| AC3 — Escape on loading node cancels | IMPLEMENTED |
| AC4 — failure node text | PARTIAL — message text mismatch (M1) |
| AC5 — BinaryFuse8 + targeted scan | IMPLEMENTED — 0x08 skip is correct per spec |

## Test Results

- `cargo test --workspace`: 289 tests, 0 failures (story notes "287" — minor doc gap)
- `cargo clippy --workspace -- -D warnings`: clean
- `cargo fmt -- --check`: clean

## Fixes Applied (2026-03-07)

All HIGH and MEDIUM issues corrected:

- **H1** — `collect_fields` rewritten as iterative walk with `HashSet<u64>` visited guard
  (`resolver.rs:47-76`)
- **M1** — Failure message changed to `"Failed to resolve object"` (`app.rs:317`)
- **M2** — `ExpansionPhase::Loading` now explicitly matched as no-op (`app.rs:241`)
- **M3** — Task 6 `expand_object` trait signature checkbox corrected to `[x]`

Test suite after fixes: 289 tests, 0 failures. Clippy clean.

## Outcome

**Status: Approved** (L1, L2, L3 deferred — non-blocking)

- Must-fix: ✅ H1, M1, M2, M3 — all fixed
- Deferred: L1 (`PartialEq` on `FieldInfo`), L2 (float display suffix), L3 (smoke test)
