# Story 1.3: Hprof Header Parsing & Mmap File Access

Status: done

## Story

As a user,
I want to open an hprof file by providing its path as a CLI argument and have the system
parse the header and memory-map the file in read-only mode,
so that I can start working with any hprof file without it being loaded entirely into RAM.

## Acceptance Criteria

1. **Given** a valid hprof file path
   **When** the system opens the file
   **Then** it is memory-mapped in read-only mode via `memmap2` (FR3, NFR9)

2. **Given** an hprof file with version "JAVA PROFILE 1.0.1" and 4-byte IDs
   **When** the header is parsed
   **Then** the system correctly returns `HprofVersion::V1_0_1` and `id_size = 4` (FR2)

3. **Given** an hprof file with version "JAVA PROFILE 1.0.2" and 8-byte IDs
   **When** the header is parsed
   **Then** the system correctly returns `HprofVersion::V1_0_2` and `id_size = 8` (FR2)

4. **Given** a file with an invalid or unrecognised magic string
   **When** the system attempts to parse the header
   **Then** `HprofError::UnsupportedVersion` is returned with the offending string

5. **Given** a byte slice shorter than a valid hprof header
   **When** the system attempts to parse the header
   **Then** `HprofError::TruncatedRecord` is returned (not a panic)

6. **Given** a file path that does not exist
   **When** `open_readonly` is called with that path
   **Then** `Err(HprofError::MmapFailed(_))` is returned (not a panic) (FR1)

7. **Given** a synthetic file built with
   `HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8).add_string(1, "main").build()`
   **When** written to a temp file, mmap'd, and its bytes passed to `parse_header`
   **Then** the header parses successfully with `HprofVersion::V1_0_2` and `id_size = 8`

## Tasks / Subtasks

- [x] Add `memmap2` and `byteorder` to workspace and parser crate (AC: #1, #2, #3)
  - [x] Add `memmap2 = "0.9"` and `byteorder = "1"` to `[workspace.dependencies]` in
        root `Cargo.toml`
  - [x] Add `memmap2 = { workspace = true }` and `byteorder = { workspace = true }` to
        `[dependencies]` in `crates/hprof-parser/Cargo.toml`

- [x] Create `crates/hprof-parser/src/mmap.rs` (AC: #1, #6)
  - [x] Add `//!` module docstring
  - [x] Implement `pub fn open_readonly(path: &Path) -> Result<Mmap, HprofError>`
        using `File::open(path)` then `unsafe { MmapOptions::new().map(&file) }`,
        mapping any `io::Error` to `HprofError::MmapFailed`
  - [x] Write `#[cfg(test)]` unit tests:
        - non-existent path returns `Err(HprofError::MmapFailed(_))`
        - valid temp file (bytes from `HprofTestBuilder`) returns `Ok(mmap)` whose
          length equals the byte count (requires `feature = "test-utils"` guard)

- [x] Create `crates/hprof-parser/src/header.rs` (AC: #2, #3, #4, #5, #7)
  - [x] Add `//!` module docstring
  - [x] Define `HprofVersion` enum: `V1_0_1`, `V1_0_2` with `Debug, Clone, Copy,
        PartialEq, Eq` derives
  - [x] Define `HprofHeader` struct: `pub version: HprofVersion`, `pub id_size: u32`
        with `Debug, Clone` derives
  - [x] Implement `pub fn parse_header(data: &[u8]) -> Result<HprofHeader, HprofError>`
        using `Cursor<&[u8]>` + `ReadBytesExt` (see Dev Notes for full algorithm)
  - [x] Write `#[cfg(test)]` unit tests for invalid/truncated cases (no builder needed)
  - [x] Write `#[cfg(all(test, feature = "test-utils"))]` unit tests for valid cases
        using `HprofTestBuilder`

- [x] Update `crates/hprof-parser/src/lib.rs` (AC: #1–#7)
  - [x] Add `pub mod mmap;` and `pub use mmap::open_readonly;`
  - [x] Add `pub mod header;` and
        `pub use header::{HprofHeader, HprofVersion, parse_header};`

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-parser` (without test-utils feature)
  - [x] `cargo test -p hprof-parser --features test-utils`
  - [x] `cargo clippy -p hprof-parser -- -D warnings`
  - [x] `cargo fmt -- --check`

### Review Follow-ups (AI)

- [x] [AI-Review][High] Implement Story FR1 end-to-end CLI flow: parse file path argument,
      call `open_readonly`, parse header, and surface clear fatal error messages in
      `hprof-cli` (`crates/hprof-cli/src/main.rs:4`,
      `docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:7`).
- [x] [AI-Review][High] Add AC#7 integration test that writes builder bytes to a temp file,
      mmaps it, then calls `parse_header` and asserts `HprofVersion` + `id_size`
      (`docs/implementation-artifacts/1-3-hprof-header-parsing-and-mmap-file-access.md:38`).
- [x] [AI-Review][High] Reconcile completion metadata with implementation scope (task checks,
      completion notes, and status) once AC gaps are closed.
- [x] [AI-Review][Medium] Replace Unix-specific missing-path test fixture with a
      cross-platform guaranteed-missing path strategy (`crates/hprof-parser/src/mmap.rs:50`).

## Dev Notes

### Files to Create or Modify

```
Cargo.toml                              ← add memmap2, byteorder to [workspace.dependencies]
crates/hprof-parser/
├── Cargo.toml                          ← add memmap2, byteorder to [dependencies]
└── src/
    ├── lib.rs                          ← declare + re-export mmap and header modules
    ├── mmap.rs                         ← NEW: open_readonly()
    └── header.rs                       ← NEW: HprofVersion, HprofHeader, parse_header()
```

Initial implementation touched only `hprof-parser`. Post-review fixes also touched
`hprof-engine` and `hprof-cli` to satisfy FR1 end-to-end while preserving architecture
boundaries (`hprof-cli` depends on `hprof-engine`, not `hprof-parser` directly).

[Source: docs/planning-artifacts/architecture.md#Project Structure]

### hprof Binary Header Format

```
[null-terminated version string]   variable length, ends with 0x00
  e.g. "JAVA PROFILE 1.0.2\0"    → 19 UTF-8 bytes + null = 20 bytes total
[id_size: u32 big-endian]          4 bytes — values 4 or 8
[dump_timestamp: u64 big-endian]   8 bytes — millis since epoch (read but not stored)
```

Total minimum header length = len(version_string) + 1 + 4 + 8 bytes.

[Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md#HprofTestBuilder
— hprof Binary Format Reference]

### `parse_header` Algorithm

```rust
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

pub fn parse_header(data: &[u8]) -> Result<HprofHeader, HprofError> {
    // 1. Find null terminator for version string
    let null_pos = data.iter().position(|&b| b == 0)
        .ok_or(HprofError::TruncatedRecord)?;

    // 2. Decode version string
    let version_str = std::str::from_utf8(&data[..null_pos])
        .map_err(|e| HprofError::CorruptedData(e.to_string()))?;

    // 3. Match known versions
    let version = match version_str {
        "JAVA PROFILE 1.0.1" => HprofVersion::V1_0_1,
        "JAVA PROFILE 1.0.2" => HprofVersion::V1_0_2,
        other => return Err(HprofError::UnsupportedVersion(other.to_owned())),
    };

    // 4. Parse id_size and timestamp via Cursor + byteorder
    let mut cursor = Cursor::new(&data[null_pos + 1..]);
    let id_size = cursor.read_u32::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;
    let _timestamp = cursor.read_u64::<BigEndian>()
        .map_err(|_| HprofError::TruncatedRecord)?;

    Ok(HprofHeader { version, id_size })
}
```

Key rules:
- `Cursor` is transient — created within the function, never stored or returned.
- Parse into owned types (`String` via `.to_owned()`, primitive `u32`).
- Map `io::Error` from `ReadBytesExt` to `HprofError::TruncatedRecord` (EOF = truncated).
- Use `map_err(|_| ...)` — the `io::Error` detail is not needed, the variant is self-
  explanatory.

[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
[Source: docs/planning-artifacts/architecture.md#Mmap Lifetime Rule]

### `open_readonly` Implementation Pattern

```rust
use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::path::Path;

pub fn open_readonly(path: &Path) -> Result<Mmap, HprofError> {
    let file = File::open(path).map_err(HprofError::MmapFailed)?;
    // SAFETY: The file is opened read-only. The caller is responsible for not
    // modifying the file during the lifetime of the Mmap.
    unsafe { MmapOptions::new().map(&file) }.map_err(HprofError::MmapFailed)
}
```

The `unsafe` block is required by `memmap2`. The safety invariant is that the backing
file must not be modified while the mapping is alive. Document this clearly in the
`//!` module docstring.

`HprofError::MmapFailed` wraps `std::io::Error` without `#[from]` (established in
story 1.2 to avoid conflict with `IoError`'s `#[from]`). Use direct construction:
`.map_err(HprofError::MmapFailed)`.

[Source: docs/planning-artifacts/architecture.md#Error Handling]
[Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md#HprofError
— Fatal vs. Non-Fatal]

### Dependency Versions

- **`memmap2 = "0.9"`** — latest stable (0.9.x). API: `MmapOptions::new().map(&file)`.
  Read-only mmap; `Mmap` deref to `&[u8]`. No unsafe required on the type itself after
  construction. Supports Linux/macOS/Windows (NFR10).
- **`byteorder = "1"`** — latest stable (1.5.x). Use `ReadBytesExt` trait methods:
  `read_u32::<BigEndian>()`, `read_u64::<BigEndian>()`, `read_u8()`. Works on any
  `std::io::Read` impl including `Cursor<&[u8]>`.

Root `Cargo.toml` addition:
```toml
[workspace.dependencies]
thiserror = "2"
memmap2 = "0.9"
byteorder = "1"
```

[Source: docs/planning-artifacts/architecture.md#Starter Template Evaluation — Core Crates]

### Test Pattern: Feature-Gated Builder Tests

Tests that need `HprofTestBuilder` must be double-gated:

```rust
#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    use super::*;
    use crate::test_utils::HprofTestBuilder;

    #[test]
    fn test_parse_valid_102_8byte_ids() {
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .build();
        let header = parse_header(&bytes).unwrap();
        assert_eq!(header.version, HprofVersion::V1_0_2);
        assert_eq!(header.id_size, 8);
    }
}
```

Tests for invalid/error cases (no builder needed) use plain `#[cfg(test)]`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_version_returns_unsupported() {
        // Craft minimal bytes: "NOT HPROF\0" + id_size(u32) + timestamp(u64)
        let mut data = b"NOT HPROF\0".to_vec();
        data.extend_from_slice(&4u32.to_be_bytes());  // id_size
        data.extend_from_slice(&0u64.to_be_bytes());  // timestamp
        let err = parse_header(&data).unwrap_err();
        assert!(matches!(err, HprofError::UnsupportedVersion(_)));
    }

    #[test]
    fn test_truncated_header_no_null() {
        let data = b"JAVA PROFILE";  // no null terminator
        assert!(matches!(
            parse_header(data),
            Err(HprofError::TruncatedRecord)
        ));
    }

    #[test]
    fn test_truncated_after_version() {
        // Version string present but id_size bytes missing
        let data = b"JAVA PROFILE 1.0.2\0";
        assert!(matches!(
            parse_header(data),
            Err(HprofError::TruncatedRecord)
        ));
    }
}
```

[Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md#TDD Cycle]
[Source: docs/planning-artifacts/architecture.md#Testing Patterns]

### TDD Cycle for This Story

Follow Red → Green → Refactor strictly:

1. **`mmap.rs`**: Write test for non-existent path → `MmapFailed` first. Write
   implementation. Then write test for valid file (using builder bytes in temp file).
2. **`header.rs`**: Write test for truncated (no null) → `TruncatedRecord`. Write
   minimal parse. Add test for unknown version → `UnsupportedVersion`. Add match. Add
   test for 1.0.1 4-byte and 1.0.2 8-byte valid cases (builder-gated). Add full logic.

### Error Propagation Rules

- `unwrap()` and `expect()` forbidden in non-test code — use `?` or `map_err`.
- `.map_err(|_| HprofError::TruncatedRecord)` is correct for `ReadBytesExt` failures
  (EOF is a truncation, not an I/O error in this context).
- `CorruptedData` for invalid UTF-8 in version string — use `.to_string()` on the error.

[Source: docs/planning-artifacts/architecture.md#Error Propagation Patterns]

### Previous Story Intelligence (1.2)

From story 1.2 completion notes:

- `"JAVA PROFILE 1.0.2"` = 18 characters (not 19) — verified during TDD. The null
  terminator is the 19th byte. Header offsets: version bytes [0..18], null at [18],
  id_size at [19..23], timestamp at [23..31].
- `thiserror = "2"` is in `[workspace.dependencies]` — use the same pattern for new deps.
- `HprofError::MmapFailed` wraps `io::Error` WITHOUT `#[from]` — construct directly:
  `.map_err(HprofError::MmapFailed)`.
- `HprofTestBuilder` is in `crate::test_utils` (re-exported as `pub use`). Import as
  `use crate::test_utils::HprofTestBuilder` in intra-crate tests.
- Pattern for feature-gated test modules: `#[cfg(all(test, feature = "test-utils"))]`
  established in `test_utils.rs` — replicate exactly.
- `lib.rs` currently re-exports `HprofError` and `HprofTestBuilder`. New modules follow
  the same `pub mod` + `pub use` pattern.

[Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md]

### Project Structure Notes

- `header.rs` and `mmap.rs` are placed directly under `crates/hprof-parser/src/` —
  flat module design consistent with `error.rs` and `test_utils.rs`.
- No sub-directories needed for this story.
- `HprofVersion` and `HprofHeader` are re-exported from the crate root (`lib.rs`) so
  consumers (future engine crate) import from `hprof_parser::HprofHeader`, not from
  `hprof_parser::header::HprofHeader`.

[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### References

- [Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Mmap Lifetime Rule]
- [Source: docs/planning-artifacts/architecture.md#Error Handling]
- [Source: docs/planning-artifacts/architecture.md#Project Structure — hprof-parser files]
- [Source: docs/planning-artifacts/architecture.md#Test Builder Location]
- [Source: docs/planning-artifacts/epics.md#Story 1.3]
- [Source: docs/implementation-artifacts/1-2-error-types-and-test-builder.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

None.

### Completion Notes List

- Added `memmap2 = "0.9"` and `byteorder = "1"` to workspace and parser crate dependencies.
- Added `tempfile = "3"` as dev-dependency for mmap builder tests.
- Created `mmap.rs`: `open_readonly` using `memmap2::MmapOptions`; safety invariant documented.
  Tests: non-existent path → `MmapFailed`; valid temp file (builder bytes) → correct mmap length.
- Created `header.rs`: `HprofVersion` enum (`V1_0_1`, `V1_0_2`), `HprofHeader` struct,
  `parse_header` using `Cursor` + `byteorder::ReadBytesExt`.
  Tests (plain `#[cfg(test)]`): truncated no-null, truncated after version, truncated missing
  timestamp, invalid version, empty input, valid 1.0.1 4-byte, valid 1.0.2 8-byte.
  Tests (`#[cfg(all(test, feature = "test-utils"))]`): builder-sourced 1.0.1 and 1.0.2 cases.
- Updated `lib.rs`: re-exported `open_readonly`, `HprofHeader`, `HprofVersion`, `parse_header`.
- All 40 tests pass with `--features test-utils`; 18 pass without.
- `cargo clippy -p hprof-parser -- -D warnings` clean.
- `cargo fmt -- --check` clean.
- **Code review fixes (2026-03-06):**
  - Added `id_size` validation (4 or 8) in `parse_header` → `CorruptedData` for invalid values.
  - Added tests for `id_size = 0` and `id_size = 3` edge cases.
  - Changed `pub mod error/mmap/header` to `pub(crate)` in `lib.rs` per architecture visibility rules.
  - Added Change Log section.
  - 40 tests pass after review fixes.
- **Code review follow-up fixes (2026-03-06):**
  - Implemented CLI FR1 flow in `hprof-cli`: one path argument, mmap + header parse via engine,
    clear fatal error output.
  - Added `hprof-engine::open_hprof_header` to keep parser internals out of CLI while enabling
    end-to-end open + parse.
  - Added AC#7 integration test in parser mmap tests: builder bytes → temp file → mmap →
    `parse_header`.
  - Replaced Unix-specific missing-path test with cross-platform guaranteed-missing temp path.

### File List

- `Cargo.toml` (modified — added `memmap2`, `byteorder` to `[workspace.dependencies]`)
- `Cargo.lock` (modified — resolved new dependencies)
- `crates/hprof-parser/Cargo.toml` (modified — added `memmap2`, `byteorder` deps; `tempfile` dev-dep)
- `crates/hprof-parser/src/lib.rs` (modified — declared and re-exported `mmap` and `header` modules; modules are `pub(crate)`)
- `crates/hprof-parser/src/mmap.rs` (new)
- `crates/hprof-parser/src/header.rs` (new)
- `crates/hprof-engine/src/lib.rs` (modified — added `open_hprof_header` helper and parser type re-exports)
- `crates/hprof-cli/src/main.rs` (modified — implemented CLI path parsing and file open+header parse flow)
- `crates/hprof-cli/Cargo.toml` (modified — added `tempfile` dev-dependency for CLI tests)

## Change Log

- 2026-03-06: Story implemented — `open_readonly`, `parse_header`, `HprofVersion`, `HprofHeader`;
  `memmap2` and `byteorder` added as workspace dependencies; 40 tests pass.
- 2026-03-06: Code review fixes — `id_size` validation added, `pub(crate)` module visibility
  enforced, tests for invalid `id_size` (0 and 3) added.
- 2026-03-06: Review follow-ups fixed — CLI FR1 flow implemented through engine helper,
  AC#7 mmap+parse integration test added, and missing-path test made cross-platform.
