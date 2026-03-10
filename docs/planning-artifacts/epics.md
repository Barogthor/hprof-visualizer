---
stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics', 'step-03-create-stories', 'step-04-final-validation']
status: 'complete'
completedAt: '2026-03-06'
revisedAt: '2026-03-08'
revisionNotes: 'Added Epic 8: First Pass Performance Optimization (stories 8.0-8.3) — FxHashMap, lazy strings, parallel heap parsing. Informed by Algorithm Olympics, Performance Profiler Panel, Pre-mortem Analysis, and Red Team vs Blue Team elicitation sessions. Validated step-04.'
inputDocuments:
  - 'docs/planning-artifacts/prd.md'
  - 'docs/planning-artifacts/architecture.md'
  - 'docs/planning-artifacts/ux-design-specification.md'
  - 'docs/report/party-mode-perf-optimization-2026-03-08.md'
---

# hprof-visualizer - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for hprof-visualizer, decomposing the requirements from the PRD and Architecture into implementable stories.

## Requirements Inventory

### Functional Requirements

- FR1: User can open an hprof file by providing its path as a CLI argument
- FR2: System can parse hprof file headers to detect format version (1.0.1, 1.0.2) and ID size (4/8 bytes)
- FR3: System can access the hprof file for read-only access without loading it into RAM
- FR4: System can perform a single sequential pass of the file, extracting structural metadata (threads, stack frames, class definitions)
- FR5: System can construct fast-lookup indexes per file segment during the indexing pass
- FR6: System can load structural strings (class/method names) eagerly during indexing
- FR7: System can skip unknown hprof record types without interruption
- FR8: System can index a truncated or corrupted file and report the percentage of records successfully indexed
- FR9: User can view the list of all threads captured in the heap dump
- FR10: User can jump to a specific thread by typing a substring that matches the thread name
- FR11: User can select a thread and view its stack frames
- FR12: User can select a stack frame and view its local variables
- FR13: User can expand a complex object to view its fields
- FR14: User can navigate nested objects by expanding fields recursively
- FR15: User can scroll through lists using Page Up/Down for page-by-page navigation and arrow keys for line-by-line movement
- FR16: System can display primitive values and nulls inline without requiring expansion
- FR17: System can display Java types in human-readable format (HashMap instead of Ljava/util/HashMap;)
- FR18: System can display collection sizes as indicators before expansion (e.g., "ConcurrentHashMap (524,288 entries)")
- FR19: System can load value strings (object String content) lazily, only when the user navigates to them
- FR20: System can paginate collections exceeding 1000 entries, loading entries in batches of 1000
- FR21: User can scroll through paginated collections with automatic batch loading when reaching page boundary
- FR22: System can resolve object references using segment-level indexes to locate objects without scanning the full file
- FR23: System can auto-calculate a memory budget at launch based on 50% of available RAM
- FR24: User can override the memory budget via --memory-limit CLI flag (e.g., 8G, 512M) or config file
- FR25: System can evict parsed object subtrees from memory using LRU policy when memory usage exceeds 80% of the budget
- FR26: System can evict data without affecting the user's ability to re-navigate to evicted content (re-parsed on demand with the same latency as initial parse)
- FR27: System can display a progress bar during indexing showing speed and ETA
- FR28: System can display loading indicators during operations exceeding 1 second (collection expansion, object resolution)
- FR29: System can display warnings for truncated/corrupted files without crashing
- FR30: System can display a persistent status bar indicator throughout the session when operating on an incomplete file
- FR31: User can configure default settings (memory_limit and additional parameters) via a TOML config file
- FR32: System can locate the config file using lookup order: working directory first, next to binary as fallback
- FR33: System can apply precedence rules: CLI flags override config file, config file overrides built-in defaults
- FR34: System can operate with missing config file using built-in defaults silently
- FR35: System can warn and fall back to defaults when config file is malformed

### NonFunctional Requirements

- NFR1: Initial indexing pass completes within 10 minutes for a 70 GB file on SSD/NVMe storage
- NFR2: Common navigation operations (list threads, open stack frame, display primitives) respond in under 1 second wall clock time from user action to display update, on a file with 300+ threads
- NFR3: Expanding collections with 500K+ entries completes within 5 seconds wall clock time from expand action to full page render
- NFR4: Batch loading of paginated collection pages completes without visible UI freeze (event loop yields within 16ms)
- NFR5: Memory usage never exceeds the configured budget during normal operation
- NFR6: The tool never crashes on malformed, truncated, or corrupted hprof input — all parsing errors result in warnings with graceful degradation
- NFR7: Displayed values are byte-accurate representations of the data in the hprof file — no silent data corruption from parsing errors
- NFR8: LRU eviction and re-parsing produces identical results to the original parse — data integrity is preserved across eviction cycles
- NFR9: The source hprof file is never modified (read-only access enforced at OS level)
- NFR10: The tool runs on Linux, macOS, and Windows (platforms supported by ratatui + crossterm + memmap2)
- NFR11: No runtime dependencies beyond the compiled binary and optional config file

### Additional Requirements

From Architecture:
- Cargo workspace with 4 crates: hprof-parser, hprof-engine, hprof-tui, hprof-cli
- Starter approach: cargo init + crate selection (no starter template)
- Core crates: memmap2, ratatui, crossterm, toml, serde, clap, thiserror, tracing, xorf, byteorder
- CI/CD: GitHub Actions with build matrix (Linux, macOS, Windows), pipeline: fmt > clippy > test > build --release
- Logging: tracing crate for structured logging, never println! in production
- Error handling: thiserror enum HprofError with fatal/non-fatal distinction
- Binary parsing: Cursor + byteorder, dynamic ID size via read_id() utility
- Mmap lifetime: parse into owned types, never persist Cursors
- Module visibility: private by default, re-export from module root
- Test infrastructure: HprofTestBuilder in hprof-parser behind test-utils feature flag
- Test fixtures: small synthetic hprof files in tests/fixtures/
- Large file benchmarks gated behind HPROF_BENCH_FILE env var
- Naming: HprofThread (prefixed when ambiguous), StackFrame (no prefix when clear)
- unwrap()/expect() forbidden outside tests
- Never log heap dump values, metadata only

### FR Coverage Map

- FR1: Epic 1 Story 1.3 - CLI file path argument
- FR2: Epic 1 Story 1.3 - Header parsing (version, ID size)
- FR3: Epic 1 Story 1.3 - Read-only mmap file access
- FR4: Epic 2 Story 2.3 - Sequential first pass indexing
- FR5: Epic 2 Stories 2.3, 2.5 - Segment-level fast-lookup indexes
- FR6: Epic 2 Stories 2.2, 2.3 - Eager structural string loading
- FR7: Epic 2 Story 2.1 - Unknown record type skipping
- FR8: Epic 2 Story 2.4 - Truncated/corrupted file indexing
- FR9: Epic 3 Story 3.2 - Thread list display
- FR10: Epic 3 Story 3.2 - Thread search/jump-to by name
- FR11: Epic 3 Story 3.3 - Stack frame viewing
- FR12: Epic 3 Story 3.3 - Local variable viewing
- FR13: Epic 3 Story 3.4 - Complex object expansion
- FR14: Epic 3 Story 3.5 - Recursive nested object navigation
- FR15: Epic 4 Story 4.2 - Page Up/Down and arrow key navigation
- FR16: Epic 3 Stories 3.3, 3.4 - Inline primitive/null display
- FR17: Epic 3 Story 3.3 - Human-readable Java types
- FR18: Epic 3 Story 3.5 - Collection size indicators
- FR19: Epic 3 Story 3.6 - Lazy value string loading
- FR20: Epic 4 Story 4.1 - Collection pagination (batches of 1000)
- FR21: Epic 4 Story 4.2 - Automatic batch loading on scroll
- FR22: Epic 3 Story 3.4 - Object resolution via segment indexes
- FR23: Epic 5 Story 5.2 - Auto-calculated memory budget
- FR24: Epic 5 Story 5.2 - Memory budget CLI/config override
- FR25: Epic 5 Story 5.3 - LRU eviction by subtree
- FR26: Epic 5 Story 5.4 - Transparent re-parse after eviction
- FR27: Epic 2 Story 2.6 - Indexing progress bar (speed + ETA)
- FR28: Epic 3 Story 3.4, Epic 6 Story 6.2 - Loading indicators for slow operations (async expansion pseudo-node + general)
- FR29: Epic 6 Story 6.2 - Corrupted file warnings
- FR30: Epic 6 Story 6.2 - Persistent incomplete file indicator
- FR31: Epic 6 Story 6.1 - TOML config file support
- FR32: Epic 6 Story 6.1 - Config file lookup order
- FR33: Epic 6 Story 6.1 - CLI > config > defaults precedence
- FR34: Epic 6 Story 6.1 - Silent defaults on missing config
- FR35: Epic 6 Story 6.1 - Warning + fallback on malformed config

### NFR Coverage Map

- NFR1: Epic 2 Stories 2.3, 2.5 - Indexing performance (verified via benchmarks)
- NFR2: Epic 3 Stories 3.2, 3.3, 3.4 - Navigation response time
- NFR3: Epic 4 Story 4.1 - Collection expansion time
- NFR4: Epic 4 Story 4.2 - UI responsiveness during batch loading
- NFR5: Epic 5 Stories 5.2, 5.4 - Memory budget enforcement
- NFR6: Epic 2 Story 2.4, Epic 3 Story 3.6, Epic 6 Story 6.2 - No crash on malformed input
- NFR7: Epic 3 Stories 3.3, 3.4 - Byte-accurate value display
- NFR8: Epic 5 Story 5.4 - Eviction/re-parse data integrity
- NFR9: Epic 1 Story 1.3 - Read-only file access
- NFR10: Epic 1 Story 1.1 - Cross-platform CI build matrix
- NFR11: Epic 1 Story 1.1 - Single binary, no runtime dependencies

## Epic List

### Epic 1: Open and Validate Heap Files
The user can open an hprof file via CLI, the system validates the header, and memory-maps it in read-only mode. The project workspace and CI pipeline are established.
**FRs covered:** FR1, FR2, FR3
**NFRs verified:** NFR9, NFR10, NFR11

### Epic 2: Structural Indexing
The system performs a complete first pass of the file, indexes structural metadata (threads, stack frames, classes, strings), constructs BinaryFuse8 filters per segment, tolerates corrupted/truncated files, and displays a progress bar with speed and ETA throughout.
**FRs covered:** FR4, FR5, FR6, FR7, FR8, FR27
**NFRs verified:** NFR1, NFR6

### Epic 3: Thread-Centric Navigation & Value Display
The user can navigate threads, drill into stack frames, view local variables, expand complex objects recursively, with inline primitives, human-readable Java types, collection size indicators, and lazy value string loading. Object resolution uses segment-level indexes.
**FRs covered:** FR9, FR10, FR11, FR12, FR13, FR14, FR16, FR17, FR18, FR19, FR22
**NFRs verified:** NFR2, NFR6, NFR7

### Epic 4: Large Collection Handling & Pagination
The user can explore massive collections (500K+ entries) page by page in batches of 1000, with automatic batch loading on scroll and full keyboard navigation (Page Up/Down, arrow keys) without UI freeze.
**FRs covered:** FR15, FR20, FR21
**NFRs verified:** NFR3, NFR4

### Epic 5: Memory Management & LRU Eviction
The system auto-calculates a memory budget at launch, supports CLI/config overrides, evicts parsed subtrees via LRU when approaching the budget, and transparently re-parses evicted data on demand — enabling exploration of 100 GB files without exceeding available RAM.
**FRs covered:** FR23, FR24, FR25, FR26
**NFRs verified:** NFR5, NFR8

### Epic 6: Configuration & Error Resilience
The user can configure the tool via a TOML config file with CLI overrides and proper precedence rules. The system displays loading indicators for slow operations, clear warnings for corrupted files, and a persistent status bar indicator for incomplete files.
**FRs covered:** FR28, FR29, FR30, FR31, FR32, FR33, FR34, FR35
**NFRs verified:** NFR6

### Epic 7: TUI UX & Interaction Design
The TUI has a consistent color theme (16 ANSI colors), a complete keyboard navigation map, and a
favorites/pin panel that appears conditionally when pins exist to support value comparison across threads.

### Epic 8: First Pass Performance Optimization
The system loads and indexes heap dumps significantly faster through optimized data structures, lazy string loading, and parallel heap segment parsing — reducing RustRover dump load time from ~30s to 10-15s.
**NFRs improved:** NFR1 (indexing performance beyond original target)

## Epic 1: Open and Validate Heap Files

The user can open an hprof file via CLI, the system validates the header, and memory-maps it in read-only mode. The project workspace and CI pipeline are established.

### Story 1.1: Workspace Setup & CI Pipeline

As a developer,
I want a Cargo workspace with 4 crates (hprof-parser, hprof-engine, hprof-tui, hprof-cli) and a GitHub Actions CI pipeline,
So that the project has a solid foundation with automated quality checks on every push.

**NFRs verified:** NFR10 (cross-platform build matrix), NFR11 (single binary)

**Acceptance Criteria:**

**Given** a fresh clone of the repository
**When** I run `cargo build`
**Then** all 4 crates compile successfully with no errors

**Given** a push to the main branch or a PR
**When** GitHub Actions CI runs
**Then** the pipeline executes fmt check, clippy, test, and release build on Linux, macOS, and Windows

**Given** each crate's `lib.rs` (or `main.rs` for hprof-cli)
**When** I inspect the source
**Then** each has a `//!` module docstring describing its single responsibility

### Story 1.2: Error Types & Test Builder

As a developer,
I want a comprehensive error enum (HprofError) with fatal/non-fatal distinction and a programmatic test builder for generating synthetic hprof data,
So that all subsequent parsing code has a consistent error model and tests can be written against realistic hprof byte sequences.

**Acceptance Criteria:**

**Given** the `hprof-parser` crate
**When** I inspect `error.rs`
**Then** `HprofError` is defined with `thiserror`, including variants: `TruncatedRecord`, `InvalidId`, `UnknownRecordType`, `CorruptedData`, `UnsupportedVersion`, `MmapFailed`, `IoError`

**Given** the `hprof-parser` crate with feature `test-utils` enabled
**When** I use `HprofTestBuilder::new(version, id_size)`
**Then** I can chain `.add_string(id, content)`, `.add_class(...)`, `.add_thread(...)`, `.truncate_at(offset)`, `.corrupt_record_at(index)`, and `.build()` to produce a valid `Vec<u8>` representing a synthetic hprof file

**Given** the `test-utils` feature is NOT enabled
**When** I compile `hprof-parser`
**Then** `HprofTestBuilder` is not available (feature-gated behind `#[cfg(feature = "test-utils")]`)

**Given** a synthetic hprof file built with `HprofTestBuilder::new("JAVA PROFILE 1.0.2", 8).add_string(1, "main").build()`
**When** the bytes are written to a temp file and opened by the header parser (Story 1.3)
**Then** the header is parsed successfully with version 1.0.2 and ID size 8, confirming the builder produces valid hprof output

### Story 1.3: Hprof Header Parsing & Mmap File Access

As a user,
I want to open an hprof file by providing its path as a CLI argument and have the system parse the header and memory-map the file in read-only mode,
So that I can start working with any hprof file without it being loaded entirely into RAM.

**NFRs verified:** NFR9 (read-only access)

**Acceptance Criteria:**

**Given** a valid hprof file path provided as CLI argument
**When** the system opens the file
**Then** the file is memory-mapped in read-only mode via `memmap2` (FR3, NFR9)

**Given** an hprof file with version "JAVA PROFILE 1.0.1" and 4-byte IDs
**When** the header is parsed
**Then** the system correctly detects version 1.0.1 and ID size 4 (FR2)

**Given** an hprof file with version "JAVA PROFILE 1.0.2" and 8-byte IDs
**When** the header is parsed
**Then** the system correctly detects version 1.0.2 and ID size 8 (FR2)

**Given** a file that is not a valid hprof file (wrong magic string)
**When** the system attempts to parse the header
**Then** a fatal `HprofError::UnsupportedVersion` is returned with a clear message

**Given** a file path that doesn't exist
**When** provided as CLI argument (FR1)
**Then** the system exits with a clear error message (not a panic)

## Epic 2: Structural Indexing

The system performs a complete first pass of the file, indexes structural metadata (threads, stack frames, classes, strings), constructs BinaryFuse8 filters per segment, tolerates corrupted/truncated files, and displays a progress bar with speed and ETA throughout.

### Story 2.1: Record Header Parsing, ID Utility & Unknown Record Skip

As a developer,
I want the system to parse hprof record headers (tag + timestamp + length), provide a dynamic `read_id()` utility for 4/8-byte IDs, and skip unknown record types gracefully,
So that the parser infrastructure is in place for all subsequent record-level parsing.

**Acceptance Criteria:**

**Given** a byte slice containing an hprof record header (tag + timestamp + length)
**When** the parser reads the record header
**Then** it correctly identifies the record type, extracts the payload length, and advances the cursor past the header

**Given** all ID reads in the parser
**When** any ID field is read
**Then** it goes through a `read_id(cursor, id_size) -> u64` utility — never hardcoded to 4 or 8 bytes

**Given** a record with an unknown tag byte
**When** the parser encounters it
**Then** it skips the record using the length field and continues parsing without error (FR7)

**Given** a record header where the length field exceeds remaining bytes
**When** the parser reads it
**Then** it returns `HprofError::TruncatedRecord` instead of panicking

### Story 2.2: Structural Record Parsing

As a developer,
I want the system to parse STRING, LOAD_CLASS, START_THREAD, STACK_FRAME, and STACK_TRACE records into well-defined domain types,
So that the indexer can build precise indexes from these records during the first pass.

**Acceptance Criteria:**

**Given** a STRING record with a known ID and UTF-8 content
**When** parsed
**Then** a structural string entry is produced with the correct ID and content (FR6)

**Given** a LOAD_CLASS record
**When** parsed
**Then** a `ClassDef` is produced with class serial number, object ID, stack trace serial, and class name string ID

**Given** a START_THREAD record
**When** parsed
**Then** an `HprofThread` is produced with thread serial number, object ID, stack trace serial, thread name string ID, and group name string ID

**Given** a STACK_FRAME record
**When** parsed
**Then** a `StackFrame` is produced with frame ID, method name string ID, method signature string ID, source file string ID, class serial, and line number

**Given** a STACK_TRACE record
**When** parsed
**Then** the stack trace is produced with serial number, thread serial, and ordered list of frame IDs

### Story 2.3: First Pass Indexer with Precise Indexes

As a developer,
I want the system to perform a single sequential mmap pass over the entire file, building precise HashMap indexes for threads, stack frames, stack traces, class definitions, and structural strings,
So that all structural metadata is available for instant lookup after indexing.

**NFRs verified:** NFR1 (indexing performance target)

**Acceptance Criteria:**

**Given** a valid hprof file opened via mmap
**When** the first pass indexer runs
**Then** it reads every record sequentially from start to end, calling the record parser for each (FR4)

**Given** the first pass completes
**When** I query the precise index
**Then** all threads, stack frames, stack traces, class definitions, and structural strings are retrievable by their IDs in O(1) via HashMap (FR5, FR6)

**Given** a file with both 4-byte and 8-byte ID variants (separate test files)
**When** each is indexed
**Then** the indexer handles both correctly using the ID size from the header

### Story 2.4: Tolerant Indexing

As a developer,
I want the indexer to handle truncated and corrupted hprof files gracefully, continuing to index as much as possible and reporting the outcome,
So that users can still inspect partial data from incomplete heap dumps.

**NFRs verified:** NFR6 (no crash on malformed input)

**Acceptance Criteria:**

**Given** a truncated hprof file that ends mid-record
**When** the indexer encounters unexpected EOF
**Then** it stops gracefully, reports the percentage of records successfully indexed, and returns what was indexed (FR8, NFR6)

**Given** a file with corrupted record headers (invalid length)
**When** the indexer encounters the corruption
**Then** it reports a warning and attempts to continue from the next valid record boundary (FR8)

**Given** a file that is entirely valid
**When** the indexer completes
**Then** no warnings are produced and 100% of records are reported as indexed

### Story 2.5: Segment-Level BinaryFuse8 Filters

As a developer,
I want the indexer to construct BinaryFuse8 filters per fixed-size segment (64 MB) during the first pass, containing all object IDs found in each segment,
So that object resolution can quickly identify which segment contains a given object without scanning the full file.

**NFRs verified:** NFR1 (indexing performance — filter construction overhead)

**Acceptance Criteria:**

**Given** the first pass is running over a file
**When** heap dump segment records are encountered
**Then** object IDs within each 64 MB file segment are collected and a BinaryFuse8 filter is built per segment using the `xorf` crate (FR5)

**Given** a completed index with segment filters
**When** I query for an object ID that exists in segment 3
**Then** the BinaryFuse8 filter for segment 3 returns `contains = true` and most other segments return `false` (with ~0.4% false positive rate)

**Given** a file smaller than 64 MB
**When** indexed
**Then** a single segment filter is created covering the entire file

**Given** a truncated file where the last segment is incomplete
**When** the indexer finishes
**Then** a filter is still built for the partial last segment with whatever object IDs were successfully parsed

### Story 2.6: Indexing Progress Bar

As a user,
I want to see a progress bar during the indexing pass showing current progress, speed (GB/s), and estimated time remaining,
So that I know the tool is working and can estimate when navigation will be available.

**Acceptance Criteria:**

**Given** the first pass indexer is running
**When** progress updates occur
**Then** a progress bar is displayed showing: bytes processed / total bytes, percentage, speed in GB/s, and ETA (FR27)

**Given** the indexing pass on a multi-GB file
**When** I observe the progress bar
**Then** it updates at a reasonable frequency (not flooding the terminal, not stalling) — at least once per second

**Given** the indexing completes successfully
**When** the progress bar reaches 100%
**Then** a summary is displayed: total time elapsed, average speed, number of records indexed

**Given** indexing of a truncated file
**When** the file ends before expected
**Then** the progress bar reflects actual progress and the summary includes a warning about incomplete indexing with percentage of records successfully processed

## Epic 3: Thread-Centric Navigation & Value Display

The user can navigate threads, drill into stack frames, view local variables, expand complex objects recursively, with inline primitives, human-readable Java types, collection size indicators, and lazy value string loading. Object resolution uses segment-level indexes.

### Story 3.1: Navigation Engine Trait & Engine Factory

As a developer,
I want a NavigationEngine trait defining the high-level API and an Engine factory that constructs the engine from a file path and config,
So that the TUI frontend can consume a clean API without knowing about parser internals.

**Acceptance Criteria:**

**Given** the `hprof-engine` crate
**When** I inspect the NavigationEngine trait
**Then** it defines methods: `list_threads()`, `select_thread(id)`, `get_stack_frames(thread_id)`, `get_local_variables(frame_id)`, `expand_object(object_id)`, `get_page(collection_id, offset, limit)`

**Given** a valid hprof file path and config
**When** I call `Engine::from_file(path, config)`
**Then** the engine internally creates an `HprofFile` (mmap + indexes), runs the first pass, and returns a ready-to-use engine implementing NavigationEngine

**Given** the `hprof-cli` crate
**When** I inspect its dependencies
**Then** it depends on `hprof-engine` but NOT on `hprof-parser` — parser types are engine-internal

**Given** the engine is constructed from a synthetic hprof file with 3 threads
**When** I call `list_threads()`
**Then** it returns exactly 3 threads with their names resolved from structural strings (FR9)

### Story 3.2: Thread List & Search in TUI

As a user,
I want to see the list of all captured threads in a TUI view, browse them with real-time stack trace
preview, and jump to a specific thread by typing a substring match,
So that I can quickly find the thread I'm investigating among hundreds of threads.

**NFRs verified:** NFR2 (navigation < 1s)

**Acceptance Criteria:**

**Given** the engine has completed indexing
**When** the TUI launches
**Then** a list of all threads is displayed with their resolved names (FR9)

**Given** the thread list is displayed
**When** I move the cursor to a thread using arrow keys
**Then** the stack trace panel updates in real-time to show that thread's frames — no Enter required
(browse-and-preview)

**Given** each thread entry in the list
**When** displayed
**Then** a colored ANSI dot (using 16-color palette) precedes the thread name to indicate thread state
(RUNNABLE, WAITING, BLOCKED, etc.), and a legend bar below the list maps dot colors to state names

**Given** a thread is selected and a filter is active
**When** the filter changes
**Then** the previously selected thread remains selected if still visible — selection tracks thread_id,
not list index

**Given** the thread list is displayed with 300+ threads
**When** I type a substring (e.g., "cluster")
**Then** the list filters to threads whose names contain the substring (FR10)

**Given** the search matches no threads
**When** I look at the TUI
**Then** a clear "no match" indicator is shown

**Given** I press Enter on a thread
**When** the stack frames are shown
**Then** the view transitions to the stack frame panel with full keyboard focus (FR11)

### Story 3.3: Stack Frame & Local Variable Display

As a user,
I want to view the stack frames of a selected thread and the local variables of a selected frame,
So that I can drill down to the exact execution context I need to inspect.

**NFRs verified:** NFR2 (navigation < 1s), NFR7 (byte-accurate values)

**Acceptance Criteria:**

**Given** a thread is selected
**When** the stack frames view loads
**Then** all stack frames are displayed with method name, class name, source file, and line number — all resolved from structural strings (FR11)

**Given** method names and class names in JVM internal format
**When** displayed
**Then** they are shown in human-readable Java format (e.g., `HashMap` not `Ljava/util/HashMap;`) (FR17)

**Given** a stack frame is selected
**When** I press Enter
**Then** the local variables for that frame are displayed (FR12)

**Given** local variables are displayed
**When** a variable holds a primitive value or null
**Then** the value is shown inline without requiring expansion (FR16)

**Given** local variables are displayed
**When** a variable holds a complex object
**Then** the type is shown in human-readable format with an expand indicator (FR13, FR17)

**Given** a stack frame is selected in the frame panel
**When** local variables are shown
**Then** they appear as a tree section below the selected frame (inline within the same panel), matching
the VisualVM layout — not in a separate dedicated panel

### Story 3.4: Object Resolution & Single-Level Expansion

As a user,
I want to expand a complex object to see its fields, with the system resolving the object from the hprof file using segment-level indexes,
So that I can inspect individual object state at the point of the heap dump.

**NFRs verified:** NFR2 (navigation < 1s), NFR7 (byte-accurate values)

**Acceptance Criteria:**

**Given** a complex object with an expand indicator
**When** I press Enter to expand it
**Then** expansion is initiated as a non-blocking async operation — the UI remains usable immediately (FR13)

**Given** an expansion is in progress
**When** the object's children are loading
**Then** a pseudo-node child appears under the expanded node showing `~ Loading... XX%` with a progress
indicator; when complete it is replaced by the real children (primitives inline, complex objects with
expand indicators) (FR13, FR16, FR28)

**Given** an expansion is in progress
**When** I press Escape or a cancel key
**Then** the async operation is cancelled and the node reverts to its collapsed state

**Given** an expansion fails (unresolvable reference, corrupted data)
**When** the error occurs
**Then** the pseudo-node is replaced by an error child node (e.g., `✗ Failed to resolve object`) and
navigation continues (NFR6)

**Given** an object that needs to be resolved from the hprof file
**When** the engine resolves it
**Then** it uses BinaryFuse8 segment filters to identify candidate segments, then performs a targeted
scan within those segments to find the object (FR22)

### Story 3.5: Recursive Expansion & Collection Size Indicators

As a user,
I want to navigate nested objects by expanding fields recursively and see collection sizes before expanding them,
So that I can explore the full object graph and know the size of collections before loading them.

**Acceptance Criteria:**

**Given** an expanded object with a nested complex field
**When** I expand that nested field
**Then** its fields are loaded recursively with the same display rules (FR14)

**Given** a collection-type object (e.g., HashMap, ArrayList)
**When** it is displayed before expansion
**Then** the entry count is shown as an indicator (e.g., "ConcurrentHashMap (524,288 entries)") (FR18)

**Given** an object expanded to depth 3 (object > field > nested field)
**When** I collapse the top-level object
**Then** all nested expansions are collapsed and their display state is cleaned up

### Story 3.6: Lazy Value String Loading

As a user,
I want String object values to be loaded lazily only when I navigate to them,
So that memory is not consumed by string content I never inspect.

**NFRs verified:** NFR6 (no crash on malformed input)

**Acceptance Criteria:**

**Given** a String object
**When** the user navigates to it
**Then** the string value is loaded lazily from the hprof file only at that moment (FR19)

**Given** a String object that has not been navigated to
**When** it appears in a field list
**Then** it shows its type and a placeholder (e.g., `String "..."`) without loading the value

**Given** a String object whose backing data cannot be found in the file
**When** lazy loading is attempted
**Then** a non-fatal warning is displayed showing the unresolved string ID, and navigation continues

## Epic 4: Large Collection Handling & Pagination

The user can explore massive collections (500K+ entries) page by page in batches of 1000, with automatic batch loading on scroll and full keyboard navigation (Page Up/Down, arrow keys) without UI freeze.

### Story 4.1: Collection Pagination Engine

As a developer,
I want the engine to paginate collections exceeding 1000 entries into batches of 1000, exposable via the `get_page(collection_id, offset, limit)` method,
So that the TUI never needs to load an entire large collection into memory at once.

**NFRs verified:** NFR3 (collection expansion < 5s)

**Acceptance Criteria:**

**Given** a collection object with 524,288 entries
**When** `get_page(collection_id, 0, 1000)` is called
**Then** the first 1000 entries are resolved and returned with their values (FR20)

**Given** a page request with `offset=5000, limit=1000`
**When** the engine processes it
**Then** entries 5000-5999 are returned, resolving object references as needed via segment filters

**Given** a collection with 500 entries (below threshold)
**When** expanded
**Then** all entries are returned in a single batch without pagination

**Given** the last page of a collection where remaining entries < 1000
**When** requested
**Then** only the remaining entries are returned with correct count

### Story 4.2: Paginated Collection View & Keyboard Navigation

As a user,
I want to scroll through paginated collections with automatic batch loading when I reach a page boundary, using Page Up/Down for fast navigation and arrow keys for line-by-line movement,
So that I can efficiently browse large collections without UI freezes.

**NFRs verified:** NFR4 (event loop yields within 16ms)

**Acceptance Criteria:**

**Given** a paginated collection displayed in the TUI
**When** I use arrow keys (Up/Down)
**Then** the selection moves one line at a time within the current batch (FR15)

**Given** a collection with more than 1000 entries
**When** it is first expanded
**Then** the first 1000 entries are loaded eagerly and displayed immediately (FR20)

**Given** more entries exist beyond the current batch
**When** I reach the end of the current batch
**Then** a "Load next 1000" action item is shown at the bottom — loading the next batch requires an
explicit user action (Enter or dedicated key), not automatic scroll (FR21)

**Given** I trigger the next batch load
**When** the operation is in progress
**Then** a loading indicator is shown and the UI remains responsive (NFR4)

**Given** I press Page Down
**When** there are more entries to display
**Then** the view jumps forward by one screen height worth of entries (FR15)

**Given** a batch is loading
**When** the operation takes longer than expected
**Then** the event loop continues yielding within 16ms to keep the UI responsive (NFR4)

**Given** I am on the last page of the collection
**When** I press Page Down or Down arrow past the last entry
**Then** nothing happens — no crash, no wrap-around

## Epic 5: Memory Management & LRU Eviction

The system auto-calculates a memory budget at launch, supports CLI/config overrides, evicts parsed subtrees via LRU when approaching the budget, and transparently re-parses evicted data on demand — enabling exploration of 100 GB files without exceeding available RAM.

### Story 5.1: MemorySize Trait & Budget Tracking

As a developer,
I want a `MemorySize` trait implemented by all parsed structures that reports their estimated heap footprint, and a global counter that tracks total memory usage,
So that the system always knows how much memory is consumed by parsed data and can make eviction decisions.

**Acceptance Criteria:**

**Given** any parsed domain type (HprofThread, StackFrame, ClassDef, expanded object, etc.)
**When** I call `.memory_size()` on it
**Then** it returns `std::mem::size_of::<Self>()` for the static part plus manual counting of heap allocations (Vec capacity, String length, HashMap entries)

**Given** an object is parsed and added to the cache
**When** the global counter is updated
**Then** it increments by exactly the value reported by `memory_size()`

**Given** an object is evicted from the cache
**When** the global counter is updated
**Then** it decrements by exactly the value reported by `memory_size()`

**Given** key domain structs
**When** unit tests run
**Then** they verify coherence between reported `memory_size()` and actual allocations for all key types

### Story 5.2: Memory Budget Auto-Calculation & Override

As a user,
I want the system to auto-calculate a memory budget at launch (50% of available RAM) and allow me to override it via `--memory-limit` CLI flag or config file,
So that the tool uses an appropriate amount of memory for my machine without me having to configure it.

**NFRs verified:** NFR5 (memory within budget)

**Acceptance Criteria:**

**Given** no `--memory-limit` flag and no config file setting
**When** the engine starts
**Then** the memory budget is set to 50% of available RAM at launch time (FR23)

**Given** `--memory-limit 8G` is passed as CLI flag
**When** the engine starts
**Then** the memory budget is set to 8 GB, overriding auto-calculation (FR24)

**Given** `memory_limit = "4G"` in config.toml and no CLI flag
**When** the engine starts
**Then** the memory budget is set to 4 GB (FR24)

**Given** both `--memory-limit 8G` CLI flag and `memory_limit = "4G"` in config
**When** the engine starts
**Then** the CLI flag wins — budget is 8 GB (CLI > config > defaults)

**Given** a machine with 16 GB available RAM
**When** auto-calculation runs
**Then** the budget is set to 8 GB

### Story 5.3: LRU Eviction Core

As a user,
I want the system to automatically evict the least recently used expanded subtrees when memory usage approaches the budget,
So that memory stays within bounds during extended exploration sessions.

**Carryover from Story 5.1 (Task 6):** Wire `Engine::memory_counter` into
`expand_object()` (add) and the eviction path (subtract) so that AC2/AC3 of
Story 5.1 are fully exercised at runtime. The `MemoryCounter` field and
`memory_used()` trait method already exist — only the call sites are missing.

**NFRs verified:** NFR5 (memory within budget)

**Acceptance Criteria:**

**Given** memory usage reaches 80% of the configured budget
**When** the eviction trigger fires
**Then** the least recently accessed expanded subtree is evicted — all parsed data under that subtree is freed (FR25)

**Given** multiple subtrees in the cache
**When** eviction is needed
**Then** the LRU subtree (least recently navigated to) is evicted first

**Given** eviction is running
**When** the user continues navigating
**Then** eviction does not block the UI — the event loop remains responsive

### Story 5.4: Transparent Re-Parse & Multi-Cycle Stability

As a user,
I want evicted data to be transparently re-parsed on demand when I navigate back, with identical results, and the system to remain stable across many eviction cycles,
So that I can freely explore large heap dumps without worrying about data loss or degradation.

**NFRs verified:** NFR5 (memory within budget), NFR8 (eviction/re-parse integrity)

**Acceptance Criteria:**

**Given** a subtree that was previously evicted
**When** the user navigates back to it
**Then** the data is re-parsed from mmap on demand with the same latency as the initial parse (FR26)

**Given** a subtree is evicted and then re-parsed
**When** the values are displayed
**Then** they are identical to the original parse — byte-accurate, no data corruption (NFR8)

**Given** the memory budget is very small (e.g., 256 MB) relative to the data being explored
**When** the user navigates extensively
**Then** the system remains stable — eviction keeps memory within budget, and re-parse works correctly even after multiple eviction cycles (NFR5)

## Epic 6: Configuration & Error Resilience

The user can configure the tool via a TOML config file with CLI overrides and proper precedence rules. The system displays loading indicators for slow operations, clear warnings for corrupted files, and a persistent status bar indicator for incomplete files.

### Story 6.1: TOML Configuration & CLI Precedence

As a user,
I want to configure default settings via a TOML config file with a clear lookup order and have CLI flags take precedence over config values,
So that I can customize the tool's behavior without repeating CLI flags every time.

**Acceptance Criteria:**

**Given** a `config.toml` file in the current working directory
**When** the tool launches
**Then** settings are loaded from that file (FR31, FR32)

**Given** no `config.toml` in the working directory but one next to the binary
**When** the tool launches
**Then** settings are loaded from the fallback location (FR32)

**Given** no config file exists anywhere
**When** the tool launches
**Then** it operates with built-in defaults silently — no error, no warning (FR34)

**Given** a malformed config file (invalid TOML syntax)
**When** the tool attempts to parse it
**Then** a warning is logged and the tool falls back to built-in defaults (FR35)

**Given** `memory_limit = "4G"` in config and `--memory-limit 8G` on the CLI
**When** the tool resolves the effective configuration
**Then** CLI wins: memory limit is 8 GB (FR33)

**Given** a config file with an unknown key
**When** parsed
**Then** the unknown key is ignored — no crash, no error

### Story 6.2: Loading Indicators & Status Bar Warnings

As a user,
I want to see loading indicators when operations take more than 1 second and clear warnings with a persistent status bar indicator when working on corrupted or truncated files,
So that I always know what the tool is doing and whether the data I'm viewing might be incomplete.

**NFRs verified:** NFR6 (no crash on malformed input)

**Acceptance Criteria:**

**Given** an object expansion or collection page load taking more than 1 second
**When** the operation is in progress
**Then** a loading indicator is displayed in the TUI (FR28)

**Given** the loading indicator is shown
**When** the operation completes
**Then** the indicator disappears and the results are displayed

**Given** a truncated or corrupted hprof file was indexed with warnings
**When** the TUI session is active
**Then** a persistent status bar indicator shows that the file is incomplete (e.g., "Incomplete file — 94% indexed") throughout the entire session (FR30)

**Given** a non-fatal parsing error occurs during navigation (e.g., unresolved reference)
**When** the error is encountered
**Then** a warning is displayed in the TUI without crashing and navigation continues (FR29, NFR6)

**Given** multiple warnings have been collected during the session
**When** the user looks at the status bar
**Then** the most recent warning is visible, with an indication of total warning count

**Given** the engine is running with an active memory budget
**When** 15–30 seconds have elapsed since the last memory log
**Then** an INFO-level log line is emitted to stderr showing: current cache usage,
total budget, and the non-evictable skeleton baseline (object index + class metadata
held permanently in `HprofFile`), e.g.:
`[memory] cache 42 MB / 512 MB budget | skeleton 38 MB (non-evictable)`

## Epic 7: TUI UX & Interaction Design

The TUI has a consistent color theme (16 ANSI colors), a complete keyboard navigation map, and a
favorites/pin panel that appears conditionally when pins exist to support value comparison across threads.

### Story 7.1: Favorites Panel

As a user,
I want to pin values from any thread or object and compare them side-by-side in a favorites panel,
So that I can correlate data across threads without losing my place in the navigation.

**Acceptance Criteria:**

**Given** the user navigates to any value (variable, object field, stack frame)
**When** they press `f`
**Then** the item is pinned to the favorites list

**Given** at least one item is pinned
**When** the TUI renders
**Then** the favorites panel appears automatically on the right side of the layout, creating a split
view (threads/frames panel | favorites panel)

**Given** no items are pinned
**When** the TUI renders
**Then** the favorites panel is hidden — the full width is used by the main navigation panels

**Given** the favorites panel is visible
**When** I press `f` on an already-pinned item
**Then** the item is unpinned and removed from the favorites panel

**Given** the favorites panel is visible
**When** I press `F`
**Then** keyboard focus moves to the favorites panel; pressing `F` again (or `Esc`) returns focus to
the main panel

**Given** the favorites panel has focus
**When** I navigate with arrow keys and press `f` on a selected item
**Then** the item is unpinned

### Story 7.2: Theme System

As a developer,
I want a centralized `theme.rs` module in `hprof-tui` defining all colors using the 16-color ANSI
palette,
So that visual consistency is enforced across all widgets without scattered color constants.

**Acceptance Criteria:**

**Given** the `hprof-tui` crate
**When** I inspect `theme.rs`
**Then** it defines a `Theme` struct with named color roles: `thread_runnable`, `thread_waiting`,
`thread_blocked`, `thread_unknown`, `primitive_value`, `string_value`, `null_value`,
`expand_indicator`, `loading_indicator`, `error_indicator`, `warning`, `selection_bg`,
`selection_fg`, `border`, `status_bar_bg`

**Given** any widget in `hprof-tui` that renders colored output
**When** I inspect its source
**Then** it references colors via `Theme` — no inline `Color::*` literals scattered across widgets

**Given** the theme colors
**When** inspected
**Then** all colors use 16-color ANSI only (`Color::Red`, `Color::Green`, `Color::Yellow`,
`Color::Blue`, `Color::Magenta`, `Color::Cyan`, `Color::Gray`, `Color::DarkGray`, and their light
variants) — no `Color::Rgb(...)` or `Color::Indexed(...)`

### Story 7.3: Keyboard Navigation Map

As a user,
I want a consistent, complete keyboard map across all TUI panels,
So that I can navigate the entire tool without consulting documentation.

**Acceptance Criteria:**

**Given** any panel in the TUI
**When** I press `?`
**Then** a help overlay displays the full keymap

**Given** the keymap
**When** documented
**Then** it covers: `q` quit, `Esc` go back / close overlay, `Tab` cycle panel focus, `↑↓` move
selection, `PgUp/PgDn` scroll page, `Enter` expand/confirm, `f` pin/unpin favorite, `F` focus
favorites panel, `s` or `/` open search, `Esc` clear search

**Given** a key is pressed in a panel where it has no action
**When** the key event is received
**Then** nothing happens — no crash, no error message

**Given** I press `q` from any panel
**When** the quit event is processed
**Then** the TUI exits cleanly and the terminal is restored to its original state

## Epic 8: First Pass Performance Optimization

The system loads and indexes heap dumps significantly faster through optimized data structures, lazy string loading, and parallel heap segment parsing — reducing RustRover dump load time from ~30s to 10-15s. Target: 50-60% reduction in real load time.

### Story 8.0: Profiling Infrastructure

As a developer,
I want reproducible benchmarks and visual profiling tools for the first pass pipeline,
So that I can measure the exact impact of each optimization and identify remaining hotspots.

**NFRs verified:** NFR1 (indexing performance measurement)

**Acceptance Criteria:**

**Given** a real hprof file path in `HPROF_BENCH_FILE` env var
**When** I run `cargo bench --bench first_pass`
**Then** criterion produces benchmark results with statistical analysis comparing to the previous run

**Given** the `dev-profiling` feature flag is enabled
**When** I run `cargo run --features dev-profiling -- <file.hprof>`
**Then** a `trace.json` file is generated that can be opened in Perfetto UI showing spans for each first pass phase (record scan, heap extraction, segment filter build, thread cache)

**Given** no `HPROF_BENCH_FILE` env var is set
**When** I run `cargo test`
**Then** benchmark tests are skipped without failure

**Technical Notes:**
- Criterion benchmarks gated behind `HPROF_BENCH_FILE` env var (per architecture.md)
- `tracing-chrome` behind feature flag `dev-profiling` — not included in release builds
- Benchmark per component: first pass total, string parsing, heap extraction, segment filter build

### Story 8.1: FxHashMap, Pre-allocation & all_offsets Optimization

As a user,
I want faster heap dump loading through optimized data structures,
So that the indexing phase completes sooner with less memory pressure.

**NFRs improved:** NFR1 (indexing performance)

**Acceptance Criteria:**

**Given** the first pass uses FxHashMap for all integer-keyed maps
**When** I index a heap dump
**Then** the indexing time is measurably reduced vs std::HashMap (verified via Story 8.0 benchmarks)

**Given** the first pass pre-allocates HashMaps based on `file_size / 80`
**When** I index a heap dump
**Then** zero HashMap reallocations occur during the heap extraction phase

**Given** the temporary `all_offsets` storage uses a sorted `Vec<(u64, u64)>` instead of `HashMap<u64, u64>`
**When** `resolve_thread_transitive_offsets` looks up ~600-800 thread-related object offsets
**Then** all lookups succeed via binary search with identical results to the previous HashMap-based implementation

**Given** a heap dump with IDs that have common high bits (ZGC/Shenandoah pattern)
**When** indexed with FxHashMap
**Then** no pathological collision behavior — verified by a dedicated regression test

**Given** all existing tests
**When** I run `cargo test`
**Then** all tests pass with identical results to pre-optimization behavior

**Technical Notes:**
- Add `rustc-hash = "2"` dependency to `hprof-parser`
- Replace `HashMap` with `FxHashMap` in `precise.rs` for: `strings`, `classes`, `threads`, `stack_frames`, `stack_traces`, `java_frame_roots`, `class_dumps`, `thread_object_ids`, `class_names_by_id`, `instance_offsets`
- Replace `all_offsets: HashMap<u64, u64>` in `first_pass.rs` with `Vec<(u64, u64)>`, sort with `sort_unstable()` after heap scan, use `binary_search_by_key()` for lookups
- Pre-allocate: `strings` with `file_size / 300`, instance Vec with `file_size / 80`
- Keep `std::HashMap` for any string-keyed maps (FxHash is poor on long strings)
- Vec<(u64,u64)> sorted: ~80 MB vs ~120 MB for HashMap on 5M entries, cache-friendly sort

### Story 8.2: Lazy String References

As a user,
I want the indexing phase to skip eagerly loading all 130K+ string values,
So that the first pass is faster and uses less memory.

**NFRs improved:** NFR1 (indexing performance), NFR5 (memory usage)

**Acceptance Criteria:**

**Given** the first pass encounters a STRING record (tag 0x01)
**When** it is indexed
**Then** only `HprofStringRef { id, offset, len }` is stored — no string content is allocated

**Given** a component needs a string value (class name, method name, field name)
**When** it calls `resolve_string(ref)` on the HprofFile
**Then** the string is resolved on-demand from the mmap data with `from_utf8_lossy`

**Given** the first pass builds `class_names_by_id`
**When** it encounters LOAD_CLASS records
**Then** class names are resolved eagerly and stored as owned `String` in `class_names_by_id` (not lazy)

**Given** all existing tests
**When** I run `cargo test`
**Then** all tests pass with identical string values to pre-optimization behavior

**Technical Notes:**
- `HprofString { id, value: String }` → `HprofStringRef { id, offset: u64, len: u32 }` in `strings.rs`
- Add `resolve_string(&self, sref: &HprofStringRef) -> String` to `HprofFile`
- `class_names_by_id` stays eager — it's the natural cache for class/method names used in UI
- No LRU cache for strings — over-engineering per Red Team analysis
- 4 production call sites to adapt: `first_pass.rs:254`, `first_pass.rs:279`, `first_pass.rs:872`, `resolver.rs:32`
- String records appear before heap segments in hprof format — lazy resolve works during first pass
- Blast radius: 4 production call sites + ~15 test assertions

### Story 8.3: Parallel Heap Segment Parsing

As a user,
I want heap dump segments parsed in parallel across CPU cores,
So that multi-core machines index heap dumps proportionally faster.

**NFRs improved:** NFR1 (indexing performance — target 3-4x speedup on 8 cores)

**Acceptance Criteria:**

**Given** a heap dump with total heap segment size >= 32 MB
**When** the first pass reaches heap extraction
**Then** heap segments are parsed in parallel using rayon

**Given** a heap dump with total heap segment size < 32 MB
**When** the first pass reaches heap extraction
**Then** heap segments are parsed sequentially (no rayon overhead)

**Given** parallel heap parsing
**When** CLASS_DUMP sub-records (tag 0x20) are encountered
**Then** they are extracted in a sequential pre-pass before parallel extraction begins, so `class_dumps` is available as read-only shared state

**Given** parallel heap parsing with multiple workers
**When** workers produce object IDs for segment filters
**Then** IDs are collected in per-worker `Vec<u64>`, concatenated per 64 MiB segment, and built into BinaryFuse8 filters after all workers complete

**Given** parallel heap parsing with workers producing offset data
**When** results are merged
**Then** per-worker `Vec<(u64, u64)>` are concatenated (not HashMap-merged) and sorted once

**Given** a HEAP_DUMP_SEGMENT larger than 16 MB
**When** it is assigned to a worker
**Then** it may be sub-divided at sub-record boundaries for finer load balancing

**Given** all existing tests
**When** I run `cargo test`
**Then** all tests pass with identical indexing results to sequential parsing

**Technical Notes:**
- Parallelize by HEAP_DUMP_SEGMENT (tag 0x1C) — natural chunk boundaries in hprof format, already tracked in `heap_record_ranges`
- Sub-pass 1 (sequential): scan heap segments extracting only CLASS_DUMP (tag 0x20) + note sub-record offsets
- Sub-pass 2 (parallel): `heap_record_ranges.par_iter()` with per-worker local Vecs, no shared mutable state
- Merge: Vec `append` (O(1)) + single `sort_unstable` — no HashMap merge contention
- Segment filter coherence: collect IDs per 64 MiB segment boundary, build BinaryFuse8 after merge
- Minimum threshold: 32 MB total heap size to activate parallelism
- Sub-divide segments > 16 MB at sub-record boundaries for better work-stealing
