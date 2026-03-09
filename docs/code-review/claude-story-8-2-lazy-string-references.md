# Code Review — Story 8.2: Lazy String References

**Reviewer:** Claude Opus 4.6 (Amelia — Dev Agent)
**Date:** 2026-03-08
**Story file:** `docs/implementation-artifacts/8-2-lazy-string-references.md`
**Branch:** `feature/epic-8-first-pass-performance-optimization`

## Summary

Story 8.2 replaces eager string allocation during the first
pass with lazy `HprofStringRef` references that store only
offset+length, deferring UTF-8 decoding to on-demand
`resolve_string` calls via the mmap.

## AC Validation

| AC | Status | Evidence |
|---|---|---|
| AC1: STRING records store only offset+length | IMPLEMENTED | `strings.rs:22-26` — `HprofStringRef { id, offset, len }`, `parse_string_ref` allocates nothing |
| AC2: On-demand resolution via mmap | IMPLEMENTED | `hprof_file.rs:129-133` — `resolve_string` reads from mmap with `from_utf8_lossy` |
| AC3: class_names_by_id stays eager | IMPLEMENTED | `first_pass.rs:265-273` — inline resolution at tag 0x02 |
| AC4: All tests pass | IMPLEMENTED | 364 tests pass, 0 clippy warnings, fmt clean |

## Task Audit

All 8 tasks marked [x] — verified against actual implementation.
No false claims detected.

## Git vs Story File List

**Discrepancies:** 0. All 8 modified files + 1 untracked
(story file) match the story's File List exactly.

## Findings

### HIGH

**H1 — Inline string resolution pattern duplicated 6 times
across 4 files**

The exact same pattern
`sref.offset as usize .. sref.offset + sref.len`,
`from_utf8_lossy`, `into_owned` is copy-pasted in:

- `resolver.rs:42-44`
- `engine_impl.rs:97-99`
- `first_pass.rs:270-272`
- `first_pass.rs:299-301`
- `first_pass.rs:914-916`
- `hprof_file.rs:130-132`

A `HprofStringRef::resolve(&self, data: &[u8]) -> String`
method on the struct would eliminate this DRY violation and
centralize the bounds/encoding logic. Each call site would
become `sref.resolve(data)` or `sref.resolve(&records_bytes)`.

### MEDIUM

**M1 — No bounds checking before slicing in any inline
resolution site**

All 6 inline resolution sites do `&data[start..end]` without
verifying `end <= data.len()`. While offsets come from parsed
records within the same data (so they *should* be valid), a
corrupted/malicious hprof file that tricks the parser could
produce out-of-bounds offsets and panic. Adding a bounds check
(or `.get(start..end).unwrap_or_default()`) in a centralized
`resolve` method (see H1) would handle this for all call sites
at once.

**M2 — `HprofStringRef` missing `PartialEq`/`Eq` derives**

`strings.rs:21` derives `Debug, Clone, Copy` but not
`PartialEq`. Tests must assert fields individually (`s.id`,
`s.offset`, `s.len`) instead of `assert_eq!(s, expected)`.
Adding `PartialEq, Eq` is trivial and improves test ergonomics.

### LOW

**L1 — Misleading "absolute offset" wording in
`parse_string_ref` docstring**

`strings.rs:39-41` says `record_body_start: absolute offset of
this record's body within the records section slice`. The word
"absolute" is confusing since the value is actually **relative**
to records section start. Should say "byte position within the
records section slice" without the word "absolute".

**L2 — `collection_entry_count` resolves ALL field names just
to find 3 candidates**

`engine_impl.rs:94-103` resolves every field name via
`from_utf8_lossy` + allocation even for non-candidate fields.
Before lazy strings, this was a free `.value.as_str()`. Now
it's N allocations per call (N = total fields including
inherited). Impact is minimal (per-interaction, not per-index)
but worth noting for future optimization.

## Verdict

All ACs implemented, all tasks genuinely complete, 364 tests
pass. The implementation is sound and follows the story spec
faithfully. The main actionable finding (H1) is a DRY
violation that would also fix M1 if addressed with a
centralized method.
