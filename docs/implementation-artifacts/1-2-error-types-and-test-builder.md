# Story 1.2: Error Types & Test Builder

Status: done

## Story

As a developer,
I want a comprehensive error enum (`HprofError`) with fatal/non-fatal distinction and a
programmatic test builder for generating synthetic hprof byte sequences,
so that all subsequent parsing code has a consistent error model and tests can be written
against realistic hprof byte sequences without committing large binary files.

## Acceptance Criteria

1. **Given** the `hprof-parser` crate
   **When** I inspect `error.rs`
   **Then** `HprofError` is defined with `thiserror`, including all required variants:
   `TruncatedRecord`, `InvalidId`, `UnknownRecordType`, `CorruptedData`,
   `UnsupportedVersion`, `MmapFailed`, `IoError`

2. **Given** the `hprof-parser` crate with feature `test-utils` enabled
   **When** I use `HprofTestBuilder::new(version, id_size)`
   **Then** I can chain `.add_string(id, content)`, `.add_class(...)`, `.add_thread(...)`,
   `.add_stack_frame(...)`, `.add_instance(...)`, `.truncate_at(offset)`,
   `.corrupt_record_at(index)`, and `.build()` to produce a valid `Vec<u8>`
   representing a synthetic hprof file

3. **Given** the `test-utils` feature is NOT enabled
   **When** I compile `hprof-parser`
   **Then** `HprofTestBuilder` is not available (gated behind `#[cfg(feature = "test-utils")]`)

4. **Given** a synthetic file built with
   `HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8).add_string(1, "main").build()`
   **When** the bytes are inspected
   **Then** the header magic string "JAVA PROFILE 1.0.2\0" is at offset 0, the 4-byte
   big-endian id_size field equals 8 immediately after the null terminator, and a
   STRING_IN_UTF8 record (tag 0x01) follows after the 8-byte timestamp

## Tasks / Subtasks

- [x] Add `thiserror` dependency and `test-utils` feature to `hprof-parser/Cargo.toml` (AC: #1, #2, #3)
  - [x] Add `thiserror` to `[dependencies]` in `crates/hprof-parser/Cargo.toml`
  - [x] Add `[features] test-utils = []` section

- [x] Create `crates/hprof-parser/src/error.rs` with `HprofError` enum (AC: #1)
  - [x] Define enum using `#[derive(Debug, thiserror::Error)]`
  - [x] Add variant `TruncatedRecord` (non-fatal: truncated record during parse)
  - [x] Add variant `InvalidId` (non-fatal: ID value outside expected range)
  - [x] Add variant `UnknownRecordType { tag: u8 }` (non-fatal: unknown tag byte)
  - [x] Add variant `CorruptedData(String)` (non-fatal: structurally malformed payload)
  - [x] Add variant `UnsupportedVersion(String)` (fatal: unknown magic string)
  - [x] Add variant `MmapFailed(#[from] std::io::Error)` for mmap errors — OR split IoError
  - [x] Add variant `IoError(#[from] std::io::Error)` for file I/O errors
  - [x] Add `#[error("...")]` display message to each variant
  - [x] Write unit tests in the same file for Display output and From conversions

- [x] Create `crates/hprof-parser/src/test_utils.rs` behind feature flag (AC: #2, #3, #4)
  - [x] Gate entire file with `#[cfg(feature = "test-utils")]`
  - [x] Add `//!` module docstring
  - [x] Implement `HprofTestBuilder` struct holding `version: &'static str`, `id_size: u32`,
        and a `records: Vec<Vec<u8>>` buffer
  - [x] Implement `new(version: &'static str, id_size: u32) -> Self`
  - [x] Implement `add_string(mut self, id: u64, content: &str) -> Self`
        → encodes a STRING_IN_UTF8 record (tag 0x01)
  - [x] Implement `add_class(mut self, class_serial: u32, object_id: u64,
        stack_trace_serial: u32, class_name_string_id: u64) -> Self`
        → encodes a LOAD_CLASS record (tag 0x02)
  - [x] Implement `add_stack_frame(mut self, frame_id: u64, method_name_id: u64,
        method_sig_id: u64, source_file_id: u64, class_serial: u32,
        line_number: i32) -> Self`
        → encodes a STACK_FRAME record (tag 0x04)
  - [x] Implement `add_stack_trace(mut self, stack_trace_serial: u32, thread_serial: u32,
        frame_ids: &[u64]) -> Self`
        → encodes a STACK_TRACE record (tag 0x05)
  - [x] Implement `add_thread(mut self, thread_serial: u32, object_id: u64,
        stack_trace_serial: u32, name_string_id: u64, group_name_string_id: u64,
        group_parent_name_string_id: u64) -> Self`
        → encodes a START_THREAD record (tag 0x06)
  - [x] Implement `add_instance(mut self, object_id: u64, stack_trace_serial: u32,
        class_object_id: u64, instance_data: &[u8]) -> Self`
        → encodes a HEAP_DUMP_SEGMENT (tag 0x1C) wrapping an INSTANCE_DUMP sub-record
  - [x] Implement `truncate_at(mut self, offset: usize) -> Self`
        → marks truncation point: `build()` truncates final bytes at that offset
  - [x] Implement `corrupt_record_at(mut self, record_index: usize) -> Self`
        → marks record index whose tag byte will be overwritten with 0xFF
  - [x] Implement `build(self) -> Vec<u8>` → serializes header + all records, applies
        truncation and corruption mutations
  - [x] Write unit tests (under `#[cfg(test)]` inside the file) verifying:
        - header magic string at offset 0
        - id_size at correct offset (len(version) + 1 + 4 bytes position)
        - STRING record tag at correct offset after header
        - `truncate_at` produces correct byte length
        - `corrupt_record_at` overwrites tag byte to 0xFF

- [x] Update `crates/hprof-parser/src/lib.rs` to declare and re-export modules (AC: #1, #2)
  - [x] Add `pub mod error;` and `pub use error::HprofError;`
  - [x] Add `#[cfg(feature = "test-utils")] pub mod test_utils;`
  - [x] Add `#[cfg(feature = "test-utils")] pub use test_utils::HprofTestBuilder;`

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-parser` (without test-utils feature)
  - [x] `cargo test -p hprof-parser --features test-utils`
  - [x] `cargo clippy -p hprof-parser -- -D warnings`
  - [x] `cargo fmt -- --check`

### Review Follow-ups (AI)

- [x] [AI-Review][Medium] Validate `id_size` (must be 4 or 8) in `HprofTestBuilder::new`
      or at the start of `build()` so invalid values fail deterministically even when no
      records are added (`crates/hprof-parser/src/test_utils.rs:39`,
      `crates/hprof-parser/src/test_utils.rs:198`, `crates/hprof-parser/src/test_utils.rs:231`).

## Dev Notes

### Crate & Module Structure

This story touches **only** `crates/hprof-parser`. No other crate changes.

Files to create or modify:

```
crates/hprof-parser/
├── Cargo.toml          ← add thiserror dep + [features] test-utils = []
└── src/
    ├── lib.rs          ← declare mod error; + #[cfg(feature="test-utils")] mod test_utils;
    ├── error.rs        ← NEW: HprofError enum
    └── test_utils.rs   ← NEW: HprofTestBuilder (feature-gated)
```

[Source: docs/planning-artifacts/architecture.md#Project Structure]

### HprofError — Fatal vs. Non-Fatal

Architecture designates two error severity levels:

- **Fatal** (exit with clear message): `UnsupportedVersion`, `MmapFailed`, `IoError`
- **Non-fatal** (collect as warning, continue): `TruncatedRecord`, `InvalidId`,
  `UnknownRecordType`, `CorruptedData`

The enum itself does not encode severity — callers decide at the call site (e.g., header
parser treats `UnsupportedVersion` as fatal; record iterator treats `TruncatedRecord` as
non-fatal). No `is_fatal()` method needed at this story stage (YAGNI).

`IoError` and `MmapFailed` both wrap `std::io::Error`. Consider two distinct variants to
preserve semantic context — mmap failure at file-open time vs. general IO. Both may use
`#[from]` BUT `thiserror` cannot have two `#[from] std::io::Error` variants in the same
enum — use `#[from]` only on `IoError` and construct `MmapFailed` explicitly.

[Source: docs/planning-artifacts/architecture.md#Error Handling]

### HprofError — `thiserror` Usage Pattern

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HprofError {
    #[error("record is truncated — insufficient bytes remaining")]
    TruncatedRecord,

    #[error("invalid object ID: {0}")]
    InvalidId(u64),

    #[error("unknown record tag: 0x{tag:02X}")]
    UnknownRecordType { tag: u8 },

    #[error("corrupted data: {0}")]
    CorruptedData(String),

    #[error("unsupported hprof version: {0}")]
    UnsupportedVersion(String),

    #[error("mmap failed: {0}")]
    MmapFailed(std::io::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}
```

Note: `MmapFailed` wraps `io::Error` without `#[from]` to avoid conflict with `IoError`.
Callers will do: `HprofError::MmapFailed(io_err)` directly.

### Error Propagation Rules (Architecture Enforcement)

- `unwrap()` and `expect()` are **forbidden** in all non-test code. Use `?` or explicit `match`.
- Use `.map_err()` when `?` alone loses context (e.g., byte offset of the failing read).
- Error conversions: `impl From<io::Error> for HprofError` via `thiserror`'s `#[from]`.

[Source: docs/planning-artifacts/architecture.md#Error Propagation Patterns]

### HprofTestBuilder — hprof Binary Format Reference

The builder must produce bytes conforming to the hprof binary format:

**File Header:**

```
[null-terminated version string] e.g. "JAVA PROFILE 1.0.2\0"  (19+1 = 20 bytes)
[id_size: u32 big-endian]                                       (4 bytes)
[dump_timestamp: u64 big-endian]  (millis since epoch, use 0)  (8 bytes)
```

**Record structure** (repeated):

```
[tag: u8]
[time_offset: u32 big-endian]  (offset from dump timestamp, use 0)
[length: u32 big-endian]       (byte length of payload)
[payload: [u8; length]]
```

**Record payloads:**

| Tag    | Name                | Payload layout |
|--------|---------------------|----------------|
| `0x01` | STRING_IN_UTF8      | `id(id_size)` + `utf8_chars` |
| `0x02` | LOAD_CLASS          | `class_serial(u32)` + `class_object_id(id_size)` + `stack_trace_serial(u32)` + `class_name_string_id(id_size)` |
| `0x04` | STACK_FRAME         | `frame_id(id_size)` + `method_name_id(id_size)` + `method_sig_id(id_size)` + `source_file_id(id_size)` + `class_serial(u32)` + `line_number(i32)` |
| `0x05` | STACK_TRACE         | `stack_trace_serial(u32)` + `thread_serial(u32)` + `num_frames(u32)` + `frame_ids([id_size; num_frames])` |
| `0x06` | START_THREAD        | `thread_serial(u32)` + `object_id(id_size)` + `stack_trace_serial(u32)` + `name_string_id(id_size)` + `group_name_string_id(id_size)` + `group_parent_name_id(id_size)` |
| `0x1C` | HEAP_DUMP_SEGMENT   | payload = sub-record stream; for INSTANCE_DUMP sub-record: `sub_tag(0x21=u8)` + `object_id(id_size)` + `stack_trace_serial(u32)` + `class_object_id(id_size)` + `num_bytes(u32)` + `instance_data` |

All multi-byte integers are **big-endian**.

### HprofTestBuilder — Implementation Tips

- Use `byteorder` crate is NOT available yet — implement BE writes manually using
  `.to_be_bytes()` on integer types (no external dep needed for the builder itself).
- `id_size` is stored as `u32` in the struct. Encode IDs by taking the low bytes of the
  `u64` argument: `&id.to_be_bytes()[8 - id_size as usize..]`.
- `truncate_at` should store the target length; `build()` calls `bytes.truncate(offset)`
  at the end. If `offset > bytes.len()`, it's a no-op (don't panic).
- `corrupt_record_at(index)` should store the record index; `build()` finds the record's
  start offset in the assembled bytes and overwrites its tag byte with `0xFF`. Track
  record start offsets during serialization to make this straightforward.
- The `add_instance` sub-record goes inside a HEAP_DUMP_SEGMENT wrapper. Wrap the
  INSTANCE_DUMP sub-record bytes into a HEAP_DUMP_SEGMENT record (`0x1C`) when building.

### Test Builder Feature Flag — Cargo.toml Pattern

```toml
# crates/hprof-parser/Cargo.toml
[package]
name = "hprof-parser"
version = "0.1.0"
edition = "2024"

[dependencies]
thiserror = { workspace = true }

[features]
test-utils = []
```

The workspace-level `Cargo.toml` must also declare `thiserror` in `[workspace.dependencies]`.
Check current root `Cargo.toml` for the `[workspace.dependencies]` section and add:

```toml
thiserror = "2"
```

(Use the latest stable version — as of early 2025, thiserror 2.x is current.)

Other crates and integration tests use the test builder via dev-dependencies:

```toml
# In another crate's Cargo.toml or tests/Cargo.toml
[dev-dependencies]
hprof-parser = { path = "../hprof-parser", features = ["test-utils"] }
```

[Source: docs/planning-artifacts/architecture.md#Test Builder Location]

### TDD Cycle

Follow Red → Green → Refactor strictly:

1. **For `error.rs`**: Write tests for Display strings and From<io::Error> conversion before
   writing the enum.
2. **For `test_utils.rs`**: Write tests asserting header bytes and record bytes before
   implementing each builder method.

Test placement:
- Unit tests: `#[cfg(test)] mod tests { ... }` at the bottom of each source file.
- Test-utils tests must themselves be gated: use `#[cfg(all(test, feature = "test-utils"))]`.

[Source: docs/planning-artifacts/architecture.md#Testing Patterns]
[Source: CLAUDE.md#TDD Cycle]

### Logging

No logging is needed in `error.rs` or `test_utils.rs` at this story stage. The `tracing`
crate is not yet a dependency. Do not add it here.

### Previous Story Intelligence (1.1)

From story 1.1 completion notes:

- Workspace uses `resolver = "2"`, edition 2024 across all crates.
- `[workspace.dependencies]` placeholder exists in root `Cargo.toml` — add `thiserror` there.
- `hprof-parser/src/lib.rs` currently contains only the `//!` module docstring — it will be
  extended here with module declarations.
- Code review revealed: no `println!` in committed code; empty `fn main()` is acceptable
  for cli stub.
- CI pipeline: `Swatinem/rust-cache@v2` is in use — `cargo test --features test-utils` will
  be cached correctly.

[Source: docs/implementation-artifacts/1-1-workspace-setup-and-ci-pipeline.md#Completion Notes]

### Current Codebase State

```
crates/hprof-parser/src/lib.rs  — only //! docstring, no mod declarations
crates/hprof-parser/Cargo.toml  — [package] + empty [dependencies]
```

No `error.rs`, no `test_utils.rs` exist yet. Both are created fresh in this story.

### Project Structure Notes

- `error.rs` and `test_utils.rs` are placed directly under `crates/hprof-parser/src/`,
  matching the architecture's flat-module design for the parser crate at this early stage.
- Do NOT create an `errors/` subdirectory — single file is correct per architecture.
- The `test_utils.rs` module is feature-gated at the `mod` declaration level in `lib.rs`,
  not via a separate `cfg` attribute on the file itself.

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- Implemented `HprofError` enum in `error.rs` using `thiserror` 2.x with 7 variants covering
  all fatal/non-fatal cases. `MmapFailed` wraps `io::Error` without `#[from]` to avoid conflict
  with `IoError`'s `#[from]` impl — callers construct it explicitly.
- Implemented `HprofTestBuilder` in `test_utils.rs` gated behind `#[cfg(feature = "test-utils")]`.
  All 7 builder methods implemented: `add_string`, `add_class`, `add_stack_frame`,
  `add_stack_trace`, `add_thread`, `add_instance`, plus `truncate_at` and `corrupt_record_at`.
- Bug found and fixed during TDD: "JAVA PROFILE 1.0.2" = 18 chars (not 19) — test assertions
  for magic string slice and header offsets corrected to reflect actual byte layout.
- 8 unit tests in `error.rs`, 16 unit tests + 1 doctest in `test_utils.rs` — all pass.
- All acceptance criteria satisfied: AC#1 (HprofError enum), AC#2 (builder API), AC#3 (feature
  gate — `HprofTestBuilder` absent without `test-utils`), AC#4 (byte layout verified).
- **Code review fixes applied (2026-03-06):**
  - Added early `id_size` validation in `HprofTestBuilder::new` and `build` to fail
    deterministically even when no records are added.
  - Added test coverage for invalid `id_size` with no records.

### File List

- `Cargo.toml` (root workspace — added `thiserror = "2"` to `[workspace.dependencies]`)
- `Cargo.lock` (updated — `thiserror` 2.x dependency resolved)
- `crates/hprof-parser/Cargo.toml` (added `thiserror` dep + `[features] test-utils = []`)
- `crates/hprof-parser/src/lib.rs` (added module declarations and re-exports)
- `crates/hprof-parser/src/error.rs` (NEW)
- `crates/hprof-parser/src/test_utils.rs` (NEW)
