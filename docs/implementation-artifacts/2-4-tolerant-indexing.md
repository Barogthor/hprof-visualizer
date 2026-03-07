# Story 2.4: Tolerant Indexing

Status: done

## Story

As a developer,
I want the indexer to handle truncated and corrupted hprof files gracefully, continuing
to index as much as possible and reporting the outcome,
so that users can still inspect partial data from incomplete heap dumps.

## Acceptance Criteria

1. **Given** a truncated hprof file that ends mid-record
   **When** the indexer encounters unexpected EOF
   **Then** it stops gracefully, reports the percentage of records successfully indexed,
   and returns what was indexed (FR8, NFR6)

2. **Given** a file with corrupted record headers (invalid length, payload within bounds)
   **When** the indexer encounters the corruption
   **Then** it reports a warning and continues indexing subsequent records (FR8)

3. **Given** a file that is entirely valid
   **When** the indexer completes
   **Then** no warnings are produced and 100% of records are reported as indexed

## Tasks / Subtasks

- [x] Define `IndexResult` in `crates/hprof-parser/src/indexer/mod.rs` (AC: #1, #2, #3)
  - [x] **Red**: Write test — `IndexResult::percent_indexed()` with 0 attempted → 100.0
  - [x] **Red**: Write test — `percent_indexed()` with 10 attempted, 10 indexed → 100.0
  - [x] **Red**: Write test — `percent_indexed()` with 10 attempted, 8 indexed → 80.0
  - [x] **Green**: Define `pub(crate) struct IndexResult` with fields:
    `index: PreciseIndex`, `warnings: Vec<String>`,
    `records_attempted: u64`, `records_indexed: u64`
  - [x] **Green**: Implement `pub(crate) fn percent_indexed(&self) -> f64`
  - [x] Add `//!` module docstring update mentioning `IndexResult`

- [x] Refactor `run_first_pass` to return `IndexResult` in
  `crates/hprof-parser/src/indexer/first_pass.rs` (AC: #1, #2, #3)
  - [x] **Red**: Write test — EOF mid-header (< 9 bytes) →
        `warnings` non-empty, `records_indexed == 0`
  - [x] **Red**: Write test — payload_end > data.len() →
        `warnings` non-empty, stops (prior valid records still indexed)
  - [x] **Red**: Write test — corrupted payload within window (size mismatch)  →
        warning collected, cursor advances to next record, next record still indexed
  - [x] **Red**: Write test — two records: first corrupt within window, second valid →
        `records_indexed == 1`, `warnings.len() == 1`
  - [x] **Red**: Write test — valid single record → `warnings` empty,
        `records_attempted == 1`, `records_indexed == 1`
  - [x] **Red**: Write test — empty data → `warnings` empty,
        `records_indexed == 0`, `records_attempted == 0`
  - [x] **Green**: Change `run_first_pass` signature to
        `pub(crate) fn run_first_pass(data: &[u8], id_size: u32) -> IndexResult`
        (infallible — all errors become warnings)
  - [x] **Green**: On EOF mid-header (`TruncatedRecord` from `parse_record_header`):
        push warning, break
  - [x] **Green**: On `payload_end > data.len()`: push warning, break
        (cannot safely resync without a framing sequence)
  - [x] **Green**: On parse error within bounded payload window OR size mismatch
        (`CorruptedData`): push warning, increment `records_attempted`,
        advance cursor to `payload_end`, continue
  - [x] **Green**: On successful parse: increment both counters, insert into index
  - [x] Update existing error-asserting tests to check `warnings` field instead:
    - [x] `known_record_with_too_short_declared_length_returns_truncated` →
          rename to `too_short_declared_length_stops_with_warning`, assert warning
    - [x] `known_record_with_extra_payload_returns_corrupted_data` →
          rename to `extra_payload_bytes_produces_warning_and_continues`,
          assert warning + next record indexed
    - [x] `string_record_declared_length_smaller_than_id_size_returns_truncated` →
          rename to `string_declared_length_smaller_than_id_size_stops_with_warning`,
          assert warning
  - [x] Update all existing `run_first_pass(&…).unwrap()` call sites in tests
        to use `run_first_pass(&…).index` (or destructure as needed)
  - [x] Builder integration test: valid file → `warnings` empty,
        `records_indexed == records_attempted`
  - [x] Builder integration test: file built with `truncate_at` →
        `warnings` non-empty, partial index returned

- [x] Update `HprofFile` in `crates/hprof-parser/src/hprof_file.rs` (AC: #1, #2, #3)
  - [x] Add `pub index_warnings: Vec<String>` field
  - [x] Add `pub records_indexed: u64` field
  - [x] Add `pub records_attempted: u64` field
  - [x] Update `from_path`: call `run_first_pass` → destructure `IndexResult` into fields
  - [x] Update docstring on `HprofFile` to document new public fields
  - [x] Update `from_path` docstring: remove `TruncatedRecord` from Errors section
        (truncation is now non-fatal)
  - [x] Update test `from_path_truncated_record_returns_error`:
    - [x] Rename to `from_path_truncated_record_returns_partial_with_warning`
    - [x] Assert `HprofFile::from_path` returns `Ok`, not `Err`
    - [x] Assert `hfile.index_warnings` is non-empty

- [x] Update `lib.rs` re-exports if `IndexResult` needs to be pub (AC: #1)
  - [x] Keep `IndexResult` as `pub(crate)` — not yet part of the public API

- [x] Verify all checks pass
  - [x] `cargo test -p hprof-parser`
  - [x] `cargo test -p hprof-parser --features test-utils`
  - [x] `cargo clippy -p hprof-parser -- -D warnings`
  - [x] `cargo fmt -- --check`

## Dev Notes

### Design: Recovery Strategy

The tolerant indexer distinguishes three failure modes during the record loop:

| Failure | Action | Rationale |
|---------|--------|-----------|
| Header truncated (< 9 bytes) | push warning + `break` | EOF — no valid record starts here |
| `payload_end > data.len()` | push warning + `break` | Can't resync without framing bytes |
| Parse error within bounded window OR size mismatch | push warning + advance to `payload_end` + `continue` | Window is trusted; skip corrupted payload safely |

The payload-window approach from Story 2.3 is the key enabler: because we parse inside a
bounded `payload_cursor`, a corrupt payload cannot overflow into the next record. When it
fails, we advance the outer `cursor` to `payload_end` (which is within `data.len()`) and
continue cleanly from the next record header.

[Source: docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md#Scope Boundaries]
[Source: docs/planning-artifacts/architecture.md#Error Handling]

### New Type: `IndexResult`

Add to `crates/hprof-parser/src/indexer/mod.rs`:

```rust
use crate::indexer::precise::PreciseIndex;

/// Result of a tolerant first-pass index run.
///
/// All non-fatal errors are collected in `warnings` rather than propagated.
/// Use `percent_indexed()` to derive the success ratio.
pub(crate) struct IndexResult {
    pub index: PreciseIndex,
    /// Human-readable description of each skipped or corrupted record.
    pub warnings: Vec<String>,
    /// Records where the header was valid and payload window was within bounds.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
}

impl IndexResult {
    pub(crate) fn percent_indexed(&self) -> f64 {
        if self.records_attempted == 0 {
            return 100.0;
        }
        self.records_indexed as f64 / self.records_attempted as f64 * 100.0
    }
}
```

[Source: docs/planning-artifacts/architecture.md#Error Handling]

### Refactored `run_first_pass` Signature and Loop

```rust
// crates/hprof-parser/src/indexer/first_pass.rs
pub(crate) fn run_first_pass(data: &[u8], id_size: u32) -> IndexResult {
    let mut cursor = Cursor::new(data);
    let mut result = IndexResult {
        index: PreciseIndex::new(),
        warnings: Vec::new(),
        records_attempted: 0,
        records_indexed: 0,
    };

    while (cursor.position() as usize) < data.len() {
        // --- Parse record header ---
        let header = match parse_record_header(&mut cursor) {
            Ok(h) => h,
            Err(e) => {
                result.warnings.push(format!("EOF mid-header: {e}"));
                break;
            }
        };

        // --- Validate payload window ---
        let payload_start = cursor.position() as usize;
        let payload_end = match payload_start.checked_add(header.length as usize) {
            Some(end) if end <= data.len() => end,
            Some(end) => {
                result.warnings.push(format!(
                    "record 0x{:02X} payload end {end} exceeds file size {}",
                    header.tag,
                    data.len()
                ));
                break;
            }
            None => {
                result.warnings.push(format!(
                    "record 0x{:02X} payload length overflow: {}",
                    header.tag, header.length
                ));
                break;
            }
        };

        // --- Skip unknown tags ---
        if !matches!(header.tag, 0x01 | 0x02 | 0x04 | 0x05 | 0x06) {
            cursor.set_position(payload_end as u64);
            continue;
        }

        result.records_attempted += 1;

        // --- Parse within bounded payload window ---
        let mut payload_cursor = Cursor::new(&data[payload_start..payload_end]);
        let parse_result = match header.tag {
            0x01 => parse_string_record(&mut payload_cursor, id_size, header.length)
                .map(|s| { result.index.strings.insert(s.id, s); }),
            0x02 => parse_load_class(&mut payload_cursor, id_size)
                .map(|c| { result.index.classes.insert(c.class_serial, c); }),
            0x04 => parse_stack_frame(&mut payload_cursor, id_size)
                .map(|f| { result.index.stack_frames.insert(f.frame_id, f); }),
            0x05 => parse_stack_trace(&mut payload_cursor, id_size)
                .map(|t| { result.index.stack_traces.insert(t.stack_trace_serial, t); }),
            0x06 => parse_start_thread(&mut payload_cursor, id_size)
                .map(|t| { result.index.threads.insert(t.thread_serial, t); }),
            _ => unreachable!(),
        };

        match parse_result {
            Ok(()) => {
                // Verify exact payload consumption
                if payload_cursor.position() as usize != header.length as usize {
                    result.warnings.push(format!(
                        "record 0x{:02X} consumed {} of {} bytes — skipping",
                        header.tag,
                        payload_cursor.position(),
                        header.length
                    ));
                    // Remove the partially-inserted entry by re-using the
                    // `payload_end` advance below (entry was already inserted;
                    // but size mismatch means data is suspect — still count as
                    // attempted, not indexed)
                    result.records_attempted -= 1; // undo; counted again below
                    result.records_attempted += 1;
                } else {
                    result.records_indexed += 1;
                }
            }
            Err(e) => {
                result.warnings.push(format!(
                    "record 0x{:02X} at offset {payload_start}: {e}",
                    header.tag
                ));
            }
        }

        cursor.set_position(payload_end as u64);
    }

    result
}
```

**Note on size mismatch:** When `payload_cursor.position() != header.length`, the record
was successfully parsed but the entry was already inserted. The implementation should
NOT insert the entry in that case (it indicates corrupted length metadata), and should
count as `records_attempted` but NOT `records_indexed`. The code above is a conceptual
sketch — the actual implementation must not double-insert. Recommended approach: parse
into a local variable first, check consumption, then insert only on success:

```rust
// Preferred parse pattern to avoid double-insert on size mismatch
let parsed = parse_string_record(&mut payload_cursor, id_size, header.length);
let consumed = payload_cursor.position() as usize == header.length as usize;
match (parsed, consumed) {
    (Ok(s), true) => { result.index.strings.insert(s.id, s); result.records_indexed += 1; }
    (Ok(_), false) => { result.warnings.push(format!("...")); }
    (Err(e), _) => { result.warnings.push(format!("...")); }
}
```

[Source: docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md#run_first_pass Implementation Pattern]
[Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]

### Updated `HprofFile`

```rust
// crates/hprof-parser/src/hprof_file.rs
pub struct HprofFile {
    _mmap: Mmap,
    pub header: HprofHeader,
    pub index: PreciseIndex,
    /// Warnings collected during indexing (non-fatal parse errors).
    pub index_warnings: Vec<String>,
    /// Records whose header and payload window were valid.
    pub records_attempted: u64,
    /// Records successfully parsed and inserted into the index.
    pub records_indexed: u64,
}

impl HprofFile {
    pub fn from_path(path: &Path) -> Result<Self, HprofError> {
        let mmap = open_readonly(path)?;
        let header = parse_header(&mmap)?;
        let records_start = header_end(&mmap)?;
        let result = run_first_pass(&mmap[records_start..], header.id_size);
        Ok(Self {
            _mmap: mmap,
            header,
            index: result.index,
            index_warnings: result.warnings,
            records_attempted: result.records_attempted,
            records_indexed: result.records_indexed,
        })
    }
}
```

`from_path` now only fails for truly fatal errors (mmap failure, unsupported version,
malformed header). A truncated record body is no longer fatal.

[Source: docs/planning-artifacts/architecture.md#Error Handling]
[Source: docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md#HprofFile Implementation Pattern]

### Test for Truncated File (builder-based)

```rust
#[cfg(all(test, feature = "test-utils"))]
mod builder_tests {
    #[test]
    fn truncated_file_returns_partial_index_with_warning() {
        // Build file with 2 strings, then truncate mid-second-record
        let bytes = HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8)
            .add_string(1, "main")
            .add_string(2, "worker")
            .build();

        let truncated = &bytes[..bytes.len() - 4]; // cut last 4 bytes
        let start = advance_past_header(truncated);
        let result = run_first_pass(&truncated[start..], 8);

        assert!(!result.warnings.is_empty(), "expected truncation warning");
        // First string should be indexed; second is incomplete
        assert_eq!(result.index.strings.len(), 1);
        assert_eq!(result.index.strings[&1].value, "main");
    }
}
```

[Source: docs/planning-artifacts/architecture.md#Test Builder Location]
[Source: docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md#Test Pattern]

### Updated `hprof_file.rs` Test for Truncated Record

The existing test must change behaviour — truncation is no longer an error at `from_path`
level:

```rust
#[test]
fn from_path_truncated_record_returns_partial_with_warning() {
    // Valid header + incomplete record (tag only, missing time_offset+length)
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"JAVA PROFILE 1.0.2\0");
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(&0u64.to_be_bytes());
    bytes.push(0x01); // tag byte only — truncated mid-header

    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    tmp.write_all(&bytes).unwrap();
    tmp.flush().unwrap();

    let hfile = HprofFile::from_path(tmp.path()).unwrap(); // Ok, not Err
    assert!(!hfile.index_warnings.is_empty());
    assert!(hfile.index.strings.is_empty());
}
```

[Source: docs/planning-artifacts/architecture.md#Error Handling]

### Scope Boundaries — What NOT to Build

| Concern | Story |
|---------|-------|
| BinaryFuse8 / segment filters | 2.5 |
| Progress bar / ETA reporting | 2.6 |
| Object ID → mmap offset resolution | 3.4+ |
| Surface warnings in TUI status bar | 3.x / 6.x |

Story 2.4 introduces `index_warnings` on `HprofFile` but does NOT display them in any
UI. The TUI integration is deferred.

[Source: docs/planning-artifacts/epics.md#Story 2.5, 2.6]

### Key Rules to Follow

- All imports from crate root (`use crate::…`), never from internal submodule paths.
- `run_first_pass` is infallible — never return `Err`, always return `IndexResult`.
- No `unwrap()` / `expect()` in production code.
- Warning messages must use `HprofError`'s `Display` impl (via `format!("{e}")`).
- The `_mmap` field prefix must be preserved.
- All modules need a `//!` docstring.

[Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
[Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]

### Previous Story Intelligence (2.3)

From Story 2.3 and its review:

- `run_first_pass` uses a bounded `payload_cursor` per record — this is the mechanism
  that makes tolerant recovery safe (advance outer cursor to `payload_end` on failure).
- The payload-window approach introduced in Story 2.3 review already enforces that
  known-record parsing is constrained to `header.length` bytes. Story 2.4 extends this
  by converting parse failures from `Err` propagation into collected warnings.
- All 5 structural record types (0x01–0x06, minus 0x03) follow the same pattern.
- `parse_record_header` reads exactly 9 bytes. If fewer than 9 bytes remain, it returns
  `HprofError::TruncatedRecord` — this is the "EOF mid-header" case.
- Total test count after Story 2.3: 70 (no feature) / 102 (test-utils).
  Some existing tests test for `Err(TruncatedRecord)` or `Err(CorruptedData)` on
  `run_first_pass` — these MUST be updated to check `warnings` instead.

[Source: docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md]

### Git Intelligence

- Recent commits show consistent module organization: `indexer/` subdirectory pattern
  established in Story 2.3 — `IndexResult` goes in `indexer/mod.rs`, not a new file.
- Pattern: new struct with tests → inline in the module file where it's used, only
  extract to separate file if it becomes large (YAGNI).
- All type re-exports from crate root remain valid — no changes to `lib.rs` needed
  unless `IndexResult` becomes part of the public API (it is `pub(crate)` for now).

### Project Structure Notes

- `IndexResult` lives in `crates/hprof-parser/src/indexer/mod.rs` (not a new file).
- No new crate dependencies — all required types already available.
- `hprof-engine` crate does not exist yet; `HprofFile` is the handoff point. Story 3.1
  creates `hprof-engine`.
- Do NOT add `pub use indexer::IndexResult` to `lib.rs` — keep it `pub(crate)`.

[Source: docs/planning-artifacts/architecture.md#Project Structure]
[Source: docs/planning-artifacts/epics.md#Story 3.1]

### References

- [Source: docs/planning-artifacts/epics.md#Story 2.4]
- [Source: docs/planning-artifacts/architecture.md#Error Handling]
- [Source: docs/planning-artifacts/architecture.md#Binary Parsing Patterns]
- [Source: docs/planning-artifacts/architecture.md#Project Structure]
- [Source: docs/planning-artifacts/architecture.md#Module Visibility Patterns]
- [Source: docs/planning-artifacts/architecture.md#Enforcement Guidelines]
- [Source: docs/implementation-artifacts/2-3-first-pass-indexer-with-precise-indexes.md]

## Dev Agent Record

### Agent Model Used

claude-sonnet-4-6

### Debug Log References

- Corrupt-within-window tests initially used `make_record_with_declared_length` with payload
  larger than declared_length, causing cursor misalignment. Fixed by using STACK_TRACE records
  that claim 1 frame but provide no frame ID bytes — the window is exactly the declared_length,
  so cursor advances correctly to the next record.

### Completion Notes List

- Added `IndexResult` to `indexer/mod.rs` with `percent_indexed()` (`#[allow(dead_code)]`
  since it is exercised only in tests at this stage; future TUI reporting will consume it).
- `run_first_pass` is now infallible: returns `IndexResult`, never propagates `HprofError`.
- Three error modes: EOF mid-header → break; payload_end > data.len() → break;
  parse error within bounded window OR size mismatch → warning + continue.
- Preferred parse pattern used throughout: parse into local var, check consumption, then
  conditionally insert — avoids any double-insert on size mismatch.
- `HprofFile` gains three new public fields: `index_warnings`, `records_attempted`,
  `records_indexed`. `from_path` is now only fatal for mmap/header errors.
- All 3 previously error-asserting tests renamed and converted to warning assertions.
- 84 tests pass (no feature), 117 tests pass (test-utils). Clippy clean, fmt clean.
- Code review (claude-story-2-4): 1 High + 3 Medium fixed. `IndexResult` now derives
  `Debug`. Docstrings corrected for `records_attempted` and `from_path`. `HprofFile`
  tests now assert all three new fields for valid-file and string-record cases.

### File List

- `crates/hprof-parser/src/indexer/mod.rs`
- `crates/hprof-parser/src/indexer/first_pass.rs`
- `crates/hprof-parser/src/hprof_file.rs`
- `docs/implementation-artifacts/2-4-tolerant-indexing.md`
- `docs/implementation-artifacts/sprint-status.yaml`
- `docs/code-review/claude-story-2-4-code-review.md`
