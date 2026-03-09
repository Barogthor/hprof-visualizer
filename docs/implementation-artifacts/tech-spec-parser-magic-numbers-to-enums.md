---
title: 'Replace Parser Magic Numbers with Enums and Named Constants'
slug: 'parser-magic-numbers-to-enums'
created: '2026-03-09'
status: 'completed'
stepsCompleted: [1, 2, 3, 4]
tech_stack: [rust, byteorder, rayon, memmap2]
files_to_modify:
  - crates/hprof-parser/src/tags.rs (new)
  - crates/hprof-parser/src/java_types.rs
  - crates/hprof-parser/src/indexer/first_pass.rs
  - crates/hprof-parser/src/hprof_file.rs
  - crates/hprof-parser/src/lib.rs
  - crates/hprof-parser/src/record.rs (tests only)
  - crates/hprof-parser/src/types.rs (tests only)
code_patterns:
  - 'From<u8> for enum conversion at match sites'
  - 'Unknown(u8) variant for forward-compatible parsing'
  - 'RecordHeader.tag stays u8 — conversion deferred to match sites'
  - 'Tolerant parsing: unknown tags warned and skipped, not fatal'
  - 'Inline #[cfg(test)] modules + feature-gated builder_tests'
test_patterns:
  - 'Existing test assertions updated to use enum variants'
  - '#[cfg(test)] mod tests inline in each file'
  - '#[cfg(all(test, feature = "test-utils"))] mod builder_tests'
---

# Tech-Spec: Replace Parser Magic Numbers with Enums and Named Constants

**Created:** 2026-03-09

## Overview

### Problem Statement

The hprof-parser crate uses ~55-60 raw hex literals (0x01, 0x02, 0x1C, 0x20, etc.) as record tag identifiers and heap sub-tag identifiers throughout production and test code. This makes the code fragile (a typo in a hex value causes a silent bug), hard to read, and difficult to maintain.

### Solution

Introduce two enums (`RecordTag`, `HeapSubTag`) with `From<u8>` conversions and an `Unknown(u8)` variant for forward-compatibility. Add named constants (`PRIM_TYPE_*`) for Java primitive type codes. Replace all magic number usage in production code and test assertions.

### Scope

**In Scope:**
- New `tags.rs` module with `RecordTag` and `HeapSubTag` enums
- `PRIM_TYPE_*` constants in `java_types.rs`
- Refactor `first_pass.rs` (production + `#[cfg(test)]` helpers) and `hprof_file.rs` to use enums
- Update test assertions in `record.rs` and `types.rs` to use enum variants

**Out of Scope:**
- Hex values in docstrings (informational, matches hprof spec)
- Capacity hints (80, 40)
- Header byte offsets in `header.rs`
- Test data **construction** helpers that write raw tag bytes
  (e.g. `make_instance_sub`, `make_record(0x0C, ...)`,
  `bytes.push(0x01)` in `hprof_file.rs` tests) — these are
  binary format constants, not matching logic
- Any behavioral change

## Context for Development

### Codebase Patterns

- Parsing uses `Cursor<&[u8]>` with `byteorder` for big-endian reads
- Tags are read as `u8` from record headers, then matched in `first_pass.rs`
- Tolerant parsing: unknown tags are warned and skipped, not fatal
- `java_types.rs` already exists for Java type handling

### Files to Reference

| File | Purpose |
| ---- | ------- |
| `crates/hprof-parser/src/indexer/first_pass.rs` | Main parsing loop — most magic numbers live here |
| `crates/hprof-parser/src/hprof_file.rs` | Object resolution + `skip_sub_record` — uses heap sub-tags 0x01–0x09, 0x20–0x23 |
| `crates/hprof-parser/src/java_types.rs` | Java type utilities — target for PRIM_TYPE constants |
| `crates/hprof-parser/src/record.rs` | Record header parsing — tests use raw tag values |
| `crates/hprof-parser/src/types.rs` | Structural record types — tests use raw tag values |
| `crates/hprof-parser/src/lib.rs` | Crate root — needs `mod tags` + re-export |

### Technical Decisions (ADRs)

#### ADR-1: Enum with `Unknown(u8)` for Record Tags & Heap Sub-Tags

**Chosen over:** plain `const TAG_*: u8` constants, enum without `Unknown`.

- Enum + `Unknown(u8)` provides compile-time exhaustive matching while remaining compatible with tolerant parsing.
- Constants lack exhaustivity — a wildcard `_` silently swallows new tags. Enum without `Unknown` would panic or error on unknown tags.
- `HeapDumpEnd` (0x2C) is a known hprof tag emitted by jvisualvm. It maps to `Unknown(0x2C)` intentionally — the parser already skips it, and adding a dedicated variant would add a match arm with no meaningful action. If future stories need to handle it, a variant can be added then.
- `Unknown(u8)` payload means `#[repr(u8)]` cannot be used — `From<u8>` is implemented manually. `From` (not `TryFrom`) because the conversion is infallible: `Unknown(u8)` catches all unrecognised values.
- Single `HeapSubTag` enum covers both GC roots (0x00–0x09) and heap objects (0x20–0x23) because they are matched in the same `match` block in production code. Splitting into two enums would force a two-step conversion.

#### ADR-2: Conversion at Match Sites, Not Parse Time

**Chosen over:** converting in `parse_record_header`, dual-field struct.

- `RecordHeader.tag` stays `u8`. `From<u8>` conversion happens at ~6 match sites across `run_first_pass`, `extract_heap_segment_parallel`, `extract_heap_object_ids`, and helper functions.
- The hot path skips millions of heap segment records — no conversion overhead on skipped records.

#### ADR-3: Constants for Primitive Type Codes

**Chosen over:** `PrimType` enum.

- Primitive types are numeric keys mapped to byte sizes in `value_byte_size` and `primitive_element_size`. The `_ => 0` fallback is intentional.
- An enum would add conversion ceremony without compile-time safety benefit (the fallback is desired behavior).

#### ADR-4: `Display` Impl on Both Enums

- Format: `TAG_NAME(0xXX)` — zero-padded uppercase hex, consistent across all variants (e.g. `STRING_IN_UTF8(0x01)`, `CLASS_DUMP(0x20)`, `UNKNOWN(0xFF)`).
- Replaces ~15 `format!("record 0x{:02X} ...")` patterns in warning messages.
- Adds `as_u8()` helper method for cases needing the raw value.
- Both `RecordTag` and `HeapSubTag` use the same `Display` convention.

## Implementation Plan

### Task Dependencies

- Tasks 1, 2, 3 are independent — can be done in parallel.
- Task 4 depends on Tasks 1–3 (registers modules and re-exports).
- Tasks 5–10 all depend on Task 4 (import the new types).
- Tasks 5, 6, 7 (first_pass.rs) are sequential within the file.
- Task 8 (hprof_file.rs) is independent of Tasks 5–7.
- Tasks 9, 10 (test updates) are independent of each other.
- Task 11 (verification) runs last.

### Tasks

- [x] Task 1: Create `tags.rs` with `RecordTag` enum
  - File: `crates/hprof-parser/src/tags.rs` (new)
  - Action: Define `RecordTag` enum with variants:
    `StringInUtf8(0x01)`, `LoadClass(0x02)`, `StackFrame(0x04)`,
    `StackTrace(0x05)`, `StartThread(0x06)`, `HeapDump(0x0C)`,
    `HeapDumpSegment(0x1C)`, `Unknown(u8)`.
  - Derive `Debug, PartialEq, Eq, Clone, Copy`.
  - Implement manual `From<u8>`, `as_u8(&self) -> u8`,
    `Display` (format: `TAG_NAME(0xXX)`).
  - Add unit tests: round-trip for each known variant + unknown.

- [x] Task 2: Add `HeapSubTag` enum to `tags.rs`
  - File: `crates/hprof-parser/src/tags.rs`
  - Action: Define `HeapSubTag` enum with variants:
    `GcRootUnknown(0x00)` (forward-compatibility placeholder — not
    currently matched in production code),
    `GcRootJniGlobal(0x01)`,
    `GcRootJniLocal(0x02)`, `GcRootJavaFrame(0x03)`,
    `GcRootNativeStack(0x04)`, `GcRootStickyClass(0x05)`,
    `GcRootThreadBlock(0x06)`, `GcRootMonitorUsed(0x07)`,
    `GcRootThreadObj(0x08)`,
    `GcRootInternedString(0x09)` (non-standard — not in the hprof
    spec but handled defensively by the parser),
    `ClassDump(0x20)`, `InstanceDump(0x21)`,
    `ObjectArrayDump(0x22)`, `PrimArrayDump(0x23)`,
    `Unknown(u8)`.
  - Derive `Debug, PartialEq, Eq, Clone, Copy`.
  - Implement manual `From<u8>`, `as_u8(&self) -> u8`,
    `Display`.
  - Add unit tests: round-trip for each known variant + unknown.

- [x] Task 3: Add `PRIM_TYPE_*` constants to `java_types.rs`
  - File: `crates/hprof-parser/src/java_types.rs`
  - Action: Add constants:
    `PRIM_TYPE_OBJECT_REF: u8 = 2`,
    `PRIM_TYPE_BOOLEAN: u8 = 4`, `PRIM_TYPE_CHAR: u8 = 5`,
    `PRIM_TYPE_FLOAT: u8 = 6`, `PRIM_TYPE_DOUBLE: u8 = 7`,
    `PRIM_TYPE_BYTE: u8 = 8`, `PRIM_TYPE_SHORT: u8 = 9`,
    `PRIM_TYPE_INT: u8 = 10`, `PRIM_TYPE_LONG: u8 = 11`.
  - Add unit tests: assert each constant has the expected value
    (guards against copy-paste typos).

- [x] Task 4: Register `tags` module and re-export in `lib.rs`
  - File: `crates/hprof-parser/src/lib.rs`
  - Action: Add `pub mod tags;` and re-export
    `tags::{RecordTag, HeapSubTag}`. Re-export `PRIM_TYPE_*`
    constants from `java_types`.

- [x] Task 5: Refactor `first_pass.rs` — record tag matches
  - File: `crates/hprof-parser/src/indexer/first_pass.rs`
  - Action: In `run_first_pass`, replace record tag magic numbers:
    - Heap tag guard: `0x0C | 0x1C` → `RecordTag::HeapDump | RecordTag::HeapDumpSegment`
    - Known-tag filter: `0x01 | 0x02 | 0x04 | 0x05 | 0x06` → enum match
    - Per-tag match arms: `0x01`..`0x06` → `RecordTag::StringInUtf8`, etc.
    - Replace `format!("record 0x{:02X} ...")` with `Display` on the enum.
  - Notes: Convert `header.tag` to `RecordTag` once before the
    match block. Warning format strings change from:
    `format!("record 0x{:02X} at offset {}: {}", header.tag, ...)`
    to: `format!("record {} at offset {}: {}", tag, ...)`
    where `tag` is the `RecordTag` with `Display` impl.

- [x] Task 6: Refactor `first_pass.rs` — heap sub-tag matches
  - File: `crates/hprof-parser/src/indexer/first_pass.rs`
  - Action: Replace heap sub-tag magic numbers in:
    - `gc_root_skip_size`: `0x01`–`0x09` → `HeapSubTag` variants
    - `skip_heap_object`: `0x21`, `0x22`, `0x23` → enum variants
    - `extract_heap_segment_parallel`: `0x03`, `0x08`, `0x20`–`0x23` → enum variants
    - `extract_heap_object_ids`: same sub-tags → enum variants
    - `extract_class_dumps_only`: `0x20` → `HeapSubTag::ClassDump`
    - `prepass_and_subdivide_segment`: `0x20` → enum
    - `subdivide_segment`: `0x20` → enum
    - `read_raw_instance_at`: convert via `HeapSubTag::from()`,
      compare to `HeapSubTag::InstanceDump`
    - `extract_obj_refs`: `field.field_type == 2` → `field.field_type == PRIM_TYPE_OBJECT_REF`
  - Notes: Convert `sub_tag` to `HeapSubTag` once at start of each match. `GcRootUnknown` is a named variant (not `Unknown(0x00)`), so wildcard arms like `_ => break` or `_ => None` must cover it explicitly — either via `HeapSubTag::GcRootUnknown | HeapSubTag::Unknown(_) => ...` or a trailing `_` wildcard. Current behavior must be preserved: 0x00 hits the fallback/break path.

- [x] Task 7: Refactor `first_pass.rs` — primitive type constants
  - File: `crates/hprof-parser/src/indexer/first_pass.rs`
  - Action: Replace numeric literals with `PRIM_TYPE_*` constants:
    - `primitive_element_size`: types 4–11 only (no object ref).
      `4 => 1` → `PRIM_TYPE_BOOLEAN => 1`,
      `5 => 2` → `PRIM_TYPE_CHAR => 2`, etc.
    - `value_byte_size`: types 2, 4–11.
      `2 => id_size` → `PRIM_TYPE_OBJECT_REF => id_size`,
      plus same 4–11 mappings as above.
  - Notes: `PRIM_TYPE_OBJECT_REF` goes in `value_byte_size`
    only — `primitive_element_size` handles primitive arrays
    which cannot contain object refs.

- [x] Task 8: Refactor `hprof_file.rs` — heap sub-tag matches
  - File: `crates/hprof-parser/src/hprof_file.rs`
  - Action: Replace magic numbers in:
    - `read_instance_at_offset`: convert `sub_tag` via
      `HeapSubTag::from(sub_tag)`, compare to
      `HeapSubTag::InstanceDump`.
    - `read_prim_array_at_offset`: same pattern with
      `HeapSubTag::PrimArrayDump`.
    - `scan_for_instance`: `0x21` → `HeapSubTag::InstanceDump`
      in match arm.
    - `scan_for_prim_array`: `0x23` → `HeapSubTag::PrimArrayDump`
      in match arm.
    - `skip_sub_record`: `0x01`–`0x09`, `0x20`–`0x23` → enum variants.
      `GcRootUnknown(0x00)` and `Unknown(_)` must map to
      `_ => false` to preserve current behavior (unrecognised
      sub-tags abort the scan).

- [x] Task 9: Update test assertions in `record.rs`
  - File: `crates/hprof-parser/src/record.rs`
  - Action: Replace `assert_eq!(rec.tag, 0x01)` with
    `assert_eq!(rec.tag, RecordTag::StringInUtf8.as_u8())` in
    `builder_tests::round_trip_string_record_header_and_skip`.
  - Notes: Only tests that assert on known tag values. Tests
    using `0xFF` (unknown) stay as-is — that's a raw byte test.

- [x] Task 10: Update test assertions in `types.rs`
  - File: `crates/hprof-parser/src/types.rs`
  - Action: Replace `assert_eq!(rec.tag, 0x02)` etc. in
    `builder_tests` with `RecordTag::*.as_u8()`:
    - `round_trip_load_class`: `0x02` → `RecordTag::LoadClass.as_u8()`
    - `round_trip_start_thread`: `0x06` → `RecordTag::StartThread.as_u8()`
    - `round_trip_stack_frame`: `0x04` → `RecordTag::StackFrame.as_u8()`
    - `round_trip_stack_trace`: `0x05` → `RecordTag::StackTrace.as_u8()`

- [x] Task 11: Final verification
  - Action: Run `cargo test`, `cargo clippy`, `cargo fmt -- --check`.
  - Verify zero behavioral change: same warnings, same index results.

### Acceptance Criteria

- [x] AC-1: Given a known record tag byte (e.g. `0x01`), when
  `RecordTag::from(0x01)` is called, then it returns
  `RecordTag::StringInUtf8`.
- [x] AC-2: Given an unknown record tag byte (e.g. `0xFF`), when
  `RecordTag::from(0xFF)` is called, then it returns
  `RecordTag::Unknown(0xFF)`.
- [x] AC-3: Given a `RecordTag::LoadClass`, when `as_u8()` is
  called, then it returns `0x02`.
- [x] AC-4: Given a `RecordTag::StringInUtf8`, when formatted
  with `Display`, then it outputs `STRING_IN_UTF8(0x01)`.
- [x] AC-5: Given a known heap sub-tag byte (e.g. `0x21`), when
  `HeapSubTag::from(0x21)` is called, then it returns
  `HeapSubTag::InstanceDump`.
- [x] AC-6: Given an unknown heap sub-tag byte (e.g. `0xFF`),
  when `HeapSubTag::from(0xFF)` is called, then it returns
  `HeapSubTag::Unknown(0xFF)`.
- [x] AC-7: Given the full existing test suite, when `cargo test`
  is run after all refactoring, then all tests pass with zero
  failures.
- [x] AC-8: Given `first_pass.rs` production code, when record
  tags are matched, then no raw hex literals remain (only enum
  variants).
- [x] AC-9: Given `hprof_file.rs` production code, when heap
  sub-tags are matched, then no raw hex literals remain (only
  enum variants or `as_u8()`).
- [x] AC-10: Given `primitive_element_size` and `value_byte_size`,
  when primitive types are matched, then `PRIM_TYPE_*` constants
  are used instead of raw integers.
- [x] AC-11: Given a `HeapSubTag::InstanceDump`, when formatted
  with `Display`, then it outputs `INSTANCE_DUMP(0x21)` (zero-
  padded uppercase hex).
- [x] AC-12: Given `first_pass.rs` `#[cfg(test)]` code, when
  heap sub-tags are used in test helper functions
  (`extract_class_dumps_only`, `skip_heap_object`,
  `subdivide_segment`, `prepass_and_subdivide_segment`), then
  enum variants are used instead of raw hex literals.
- [x] AC-13: Given `RecordTag::Unknown(0x2C)`, when formatted
  with `Display`, then it outputs `UNKNOWN(0x2C)`.
- [x] AC-14: Given `read_raw_instance_at` and `extract_obj_refs`
  in `first_pass.rs`, when sub-tags or type codes are compared,
  then enum `as_u8()` or `PRIM_TYPE_*` constants are used
  instead of raw literals.

## Additional Context

### Dependencies

None — pure refactoring, no new crates.

### Testing Strategy

- **Existing tests**: All must pass unchanged — behavior-preserving refactor.
- **New unit tests** (in `tags.rs`):
  - `From<u8>` round-trip for each known `RecordTag` variant.
  - `From<u8>` round-trip for each known `HeapSubTag` variant.
  - Unknown tag values map to `Unknown(u8)`.
- **Updated assertions** (in `record.rs`, `types.rs`):
  - `assert_eq!(rec.tag, RecordTag::LoadClass.as_u8())` pattern.
- **Regression gate**: `cargo test && cargo clippy` after each task.

### Notes

- Pure refactoring — zero behavioral change expected.
- `#[repr(u8)]` is NOT used because `Unknown(u8)` has a payload.
  `From<u8>` is implemented manually.
- Warning messages will change from `"record 0x02"` to
  `"record LOAD_CLASS(0x02)"` — this is intentional and improves
  debuggability. No code depends on warning message format.

## Review Notes
- Adversarial review completed
- Findings: 4 total, 0 fixed, 4 skipped (all pre-existing or by-design)
- Resolution approach: auto-fix (no real findings to fix)
- Pre-existing GC root skip size mismatches (F1/F2) should be
  audited in a follow-up ticket against OpenJDK `hprof_io.c`
