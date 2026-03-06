---
stepsCompleted: ['step-01-init', 'step-02-discovery', 'step-02b-vision', 'step-02c-executive-summary', 'step-03-success', 'step-04-journeys', 'step-05-domain', 'step-06-innovation', 'step-07-project-type', 'step-08-scoping', 'step-09-functional', 'step-10-nonfunctional', 'step-11-polish', 'step-12-complete']
inputDocuments: ['docs/brainstorming/brainstorming-session-2026-03-06-1000.md']
workflowType: 'prd'
documentCounts:
  briefs: 0
  research: 0
  brainstorming: 1
  projectDocs: 0
classification:
  projectType: 'desktop-tool'
  domain: 'developer-tooling-performance-forensics'
  complexity: 'medium-high'
  projectContext: 'greenfield'
  audience: 'internal-team, open-source-potential'
---

# Product Requirements Document - hprof-visualizer

**Author:** Florian
**Date:** 2026-03-06

## Executive Summary

hprof-visualizer is a lightweight desktop tool for exploring Java heap dump files (.hprof) of any size on standard developer machines. Existing tools (VisualVM, Eclipse MAT) load the entire heap into memory, making machines with 16-32 GB of RAM unusable for hours when analyzing production heap dumps exceeding 20 GB. hprof-visualizer solves this by using memory-mapped I/O and on-demand parsing — only the data the user is actively inspecting is loaded into memory.

The primary use case is forensic value inspection: navigating threads, stack frames, and local variables to examine object state at the time of the dump. The initial scope focuses on fast, low-resource access to specific values within large heap dumps.

Target users are the internal development team working with production heap dumps from clustering servers. The project is open to future open-source distribution.

## What Makes This Special

The core insight is that the hprof format is a sequence of independent records — full reconstruction of the object graph in memory is unnecessary for value inspection. A fast initial pass via mmap indexes structural metadata (threads, stack frames, class definitions) while individual objects are parsed lazily only when the user navigates to them. A 25 GB heap dump can be explored with a memory budget representing a fraction of the file size.

This approach inverts the cost model: instead of paying upfront (hours of loading, all RAM consumed), the cost is distributed — a few seconds of initial indexing, then milliseconds per navigation action, with the developer's machine remaining fully usable throughout.

## Project Classification

- **Type:** Desktop tool — analysis engine with TUI frontend (ratatui), decoupled via trait abstraction for future GUI migration (egui)
- **Domain:** Developer tooling — JVM performance forensics
- **Complexity:** Medium-high — binary format parsing, memory-mapped I/O, LRU cache management, data correctness critical for forensic use
- **Context:** Greenfield project, Rust (edition 2024)
- **Audience:** Internal team, open-source potential

## Success Criteria

### User Success

- Open any hprof file regardless of size (tested up to 100 GB) on a machine with 20 GB of available RAM
- Initial indexing pass completes within 5-10 minutes for a 70 GB file, with a visible progress bar showing speed and ETA
- Common navigation operations (list threads, open stack frame, display primitives) respond in under 1 second
- Heavy operations (expanding large collections of 500K+ entries) complete within 5 seconds
- The developer's machine remains fully usable during both indexing and navigation
- Corrupted or truncated files produce warnings, not crashes

### Business Success

- The tool replaces VisualVM for value inspection tasks in the author's daily workflow
- No adoption metrics required — this is a personal productivity tool with team sharing potential

### Technical Success

- Memory usage stays within configured budget (default: auto-calculated from available RAM, overridable via `--memory-limit`) regardless of heap dump size
- Correct parsing of hprof versions 1.0.1 and 1.0.2 with dynamic ID size (4/8 bytes)
- Unknown hprof record types are skipped gracefully without affecting navigation
- LRU eviction prevents memory pressure before OS swap occurs

### Measurable Outcomes

- End-to-end: a 70 GB production heap dump can be opened and a specific thread variable value located within 15 minutes total (indexing + navigation)
- Machine responsiveness (other applications) remains unaffected during heap analysis as measured by absence of OS swap pressure

## User Journeys

### Journey 1: Targeted Value Inspection (Primary - Success Path)

**Persona:** Florian, Java developer on a clustering team. A production server is behaving unexpectedly — cluster candidates aren't matching the expected configuration. He needs to inspect the actual values in the heap dump to understand what the JVM saw at the time of the dump.

**Opening Scene:** Florian has a 65 GB heap dump file from a production server sitting on his dev machine (32 GB RAM). Opening it in VisualVM would consume all his RAM and take hours. He launches hprof-visualizer from the terminal with the file path.

**Rising Action:** A progress bar appears — "Indexing: 12 GB/65 GB — 2.1 GB/s — ETA 4:10". His IDE and browser remain responsive in the background. After ~5 minutes, the TUI displays a list of threads. He types a quick search to jump to the thread he's looking for — it highlights instantly among the 300+ threads. He hits Enter. The stack frames appear instantly. He navigates to the frame he's interested in, expands the local variables. The clustering candidate object appears with its fields inline — primitives show their values immediately, complex objects show their type and an "expand" indicator.

**Climax:** He expands the candidate's configuration map. The entries load in under a second. He finds the value he was looking for — the configuration parameter that explains the unexpected behavior. His machine never stuttered.

**Resolution:** Total time from launch to answer: ~7 minutes. He closes the tool, his machine is exactly as responsive as before. No swap file cleanup, no killed processes, no reboot needed.

### Journey 2: Corrupted Heap Dump (Primary - Edge Case)

**Persona:** Same Florian. This time the heap dump was captured during an OOM kill — the file is truncated at 80% of its expected size.

**Opening Scene:** He launches hprof-visualizer on the truncated file. The indexing pass begins normally.

**Rising Action:** At ~80% progress, the parser encounters unexpected end-of-file. Instead of crashing, a warning appears: "File appears truncated at offset 52.1 GB — indexed 94% of records successfully." The TUI loads with the available data.

**Climax:** The thread he needs was captured before the truncation point. He navigates normally and finds his values. A small indicator in the status bar reminds him that the file is incomplete.

**Resolution:** He got his answer despite the corrupted file. He notes which threads/data might be missing from the last 6% but it doesn't affect his investigation.

### Journey 3: Spot-Checking a Large Collection (Primary - Heavy Case)

**Persona:** Same Florian. He's inspecting a thread that holds a reference to a `ConcurrentHashMap` with 500K entries — the full cluster candidate registry.

**Opening Scene:** He navigates to the thread and stack frame as usual — instant response. He sees the `ConcurrentHashMap` field with an indicator: "ConcurrentHashMap (524,288 entries)".

**Rising Action:** He expands the map. A brief loading indicator appears — "Loading entries 1-1000 of 524,288". The first page renders in ~2 seconds. He inspects the first few entries to understand the data structure and values. Then he uses Page Down to jump ahead several pages and spot-checks another entry to compare.

**Climax:** The values are consistent between the entries he checked at the beginning and further down the list. The clustering configuration matches what he expected — the issue lies elsewhere. Memory usage remains stable — the LRU has already evicted the pages he scrolled past.

**Resolution:** He confirmed the registry state in under a minute of browsing. He never had to load all 500K entries — only the pages he actually viewed.

These journeys map directly to the functional requirements defined below — each capability in the FR list traces back to a concrete user need surfaced by these scenarios.

## Desktop Tool Specific Requirements

### Project-Type Overview

hprof-visualizer is an interactive TUI application launched from the terminal. No scriptable/batch mode in MVP — the tool is always interactive.

### Command Structure

```
hprof-visualizer <file.hprof> [options]
```

**CLI Options (MVP):**
- `<file.hprof>` — path to the heap dump file (required, positional)
- `--memory-limit <size>` — override memory budget (e.g., `--memory-limit 8G`)
- `--config <path>` — path to config file (default: see lookup order below)

### Configuration

**Config file:** `config.toml` with the following lookup order:
1. Current working directory (`./config.toml`)
2. Next to the binary (fallback)

**Configurable settings:**
- `memory_limit` — default memory budget (overridable via CLI flag)
- Additional settings added as needed during development

**Precedence:** CLI flags > config file > built-in defaults.

**Error handling:** Missing config file uses defaults silently. Malformed config file logs a warning and falls back to defaults.

### Implementation Considerations

- **Interactive only (MVP):** No stdout/pipe mode, no JSON export, no scriptable commands
- **Scriptable mode (backlog):** Planned for future phases
- **Config file format:** TOML for simplicity and Rust ecosystem alignment

## Product Scope & Phased Development

### MVP Strategy

**MVP Approach:** Problem-solving MVP — the minimum feature set that replaces VisualVM for targeted value inspection on large heap dumps. One user type, one workflow, zero ceremony.

**Resource Requirements:** Solo developer (Florian), Rust expertise, familiarity with hprof format.

**Validation asset:** A real-world heap dump file provided by the author for benchmarking and integration testing.

### Phase 1: MVP

**Core User Journey Supported:** Targeted value inspection — open file, index, navigate threads, inspect values.

- Hprof parser: header, versions 1.0.1/1.0.2, dynamic ID size (4/8 bytes), unknown records skipped
- Structural strings (class/method names) loaded eagerly; value strings (object content) loaded lazily
- First pass indexing via mmap with progress bar (speed + ETA), including Bloom filter construction per segment
- Bloom filter per segment for fast object resolution
- Thread-centric navigation: threads, stack frames, local variables, values
- Quick jump-to/search by thread name
- Inline display of primitives/nulls, lazy expansion for complex objects
- Human-readable Java types (`HashMap` instead of `Ljava/util/HashMap;`)
- Lazy pagination of large collections in batches
- Page Up/Down and arrow key navigation
- LRU eviction by subtree with configurable memory budget
- Memory budget: auto-calculated at launch (50% of available RAM), overridable via `--memory-limit` or config file
- Tolerant parsing: truncated/corrupted files produce warnings, not crashes
- TUI frontend (ratatui + crossterm)
- Config file (TOML) with CLI flag overrides
- Favorites/pin panel with conditional split view for value comparison (clustering use case)

### Phase 2: Growth

- Thread search/filter and automatic grouping by thread pool prefix
- Circular reference detection during expansion
- Heap summary: instance count per class, total size, top N largest objects
- Dynamic memory pressure detection (adaptive budget based on OS state)

### Phase 3: Expansion

- GUI frontend migration (egui) via trait abstraction
- Scriptable extraction mode
- Memory leak analysis (dominator tree, retention paths)
- GC roots statistics, classloader leak detection
- Instance size distribution histogram
- Real-time statistics during scan

### Risk Mitigation Strategy

**Technical Risks:**
- *First pass performance on 70+ GB files:* Prototype and benchmark mmap sequential read speed early, including Bloom filter construction overhead. Target: 5-10 minutes on 70 GB. If too slow, explore parallelized first pass or partial indexing.
- *Object resolution depth:* Bloom filter per segment in MVP mitigates scan cost. Each object lookup hits a small segment instead of the full file.
- *Memory budget:* Fixed budget calculated at startup (50% available RAM). LRU eviction triggers when approaching budget. No dynamic OS pressure detection in MVP — predictable and testable.

**Resource Risks:**
- Solo developer project — MVP scope is intentionally minimal to remain achievable. Growth features are independent and can be added incrementally.

## Functional Requirements

### File Loading & Initialization

- FR1: User can open an hprof file by providing its path as a CLI argument
- FR2: System can parse hprof file headers to detect format version (1.0.1, 1.0.2) and ID size (4/8 bytes)
- FR3: System can access the hprof file for read-only access without loading it into RAM
- FR4: System can perform a single sequential pass of the file, extracting structural metadata (threads, stack frames, class definitions)
- FR5: System can construct fast-lookup indexes per file segment during the indexing pass
- FR6: System can load structural strings (class/method names) eagerly during indexing
- FR7: System can skip unknown hprof record types without interruption
- FR8: System can index a truncated or corrupted file and report the percentage of records successfully indexed

### Navigation & Exploration

- FR9: User can view the list of all threads captured in the heap dump
- FR10: User can jump to a specific thread by typing a substring that matches the thread name
- FR11: User can select a thread and view its stack frames
- FR12: User can select a stack frame and view its local variables
- FR13: User can expand a complex object to view its fields
- FR14: User can navigate nested objects by expanding fields recursively
- FR15: User can scroll through lists using Page Up/Down for page-by-page navigation and arrow keys for line-by-line movement

### Data Display & Formatting

- FR16: System can display primitive values and nulls inline without requiring expansion
- FR17: System can display Java types in human-readable format (`HashMap` instead of `Ljava/util/HashMap;`)
- FR18: System can display collection sizes as indicators before expansion (e.g., "ConcurrentHashMap (524,288 entries)")
- FR19: System can load value strings (object String content) lazily, only when the user navigates to them

### Large Collection Handling

- FR20: System can paginate collections exceeding 1000 entries, loading entries in batches of 1000
- FR21: User can scroll through paginated collections with automatic batch loading when reaching page boundary
- FR22: System can resolve object references using segment-level indexes to locate objects without scanning the full file

### Memory Management

- FR23: System can auto-calculate a memory budget at launch based on 50% of available RAM
- FR24: User can override the memory budget via `--memory-limit` CLI flag (e.g., `8G`, `512M`) or config file
- FR25: System can evict parsed object subtrees from memory using LRU policy when memory usage exceeds 80% of the budget
- FR26: System can evict data without affecting the user's ability to re-navigate to evicted content (re-parsed on demand with the same latency as initial parse)

### Error Handling & Feedback

- FR27: System can display a progress bar during indexing showing speed and ETA
- FR28: System can display loading indicators during operations exceeding 1 second (collection expansion, object resolution)
- FR29: System can display warnings for truncated/corrupted files without crashing
- FR30: System can display a persistent status bar indicator throughout the session when operating on an incomplete file

### Configuration

- FR31: User can configure default settings (memory_limit and additional parameters) via a TOML config file
- FR32: System can locate the config file using lookup order: working directory first, next to binary as fallback
- FR33: System can apply precedence rules: CLI flags override config file, config file overrides built-in defaults
- FR34: System can operate with missing config file using built-in defaults silently
- FR35: System can warn and fall back to defaults when config file is malformed

## Non-Functional Requirements

### Performance

- NFR1: Initial indexing pass completes within 10 minutes for a 70 GB file on SSD/NVMe storage
- NFR2: Common navigation operations (list threads, open stack frame, display primitives) respond in under 1 second wall clock time from user action to display update, on a file with 300+ threads
- NFR3: Expanding collections with 500K+ entries completes within 5 seconds wall clock time from expand action to full page render
- NFR4: Batch loading of paginated collection pages completes without visible UI freeze (event loop yields within 16ms)
- NFR5: Memory usage never exceeds the configured budget during normal operation

### Reliability

- NFR6: The tool never crashes on malformed, truncated, or corrupted hprof input — all parsing errors result in warnings with graceful degradation
- NFR7: Displayed values are byte-accurate representations of the data in the hprof file — no silent data corruption from parsing errors
- NFR8: LRU eviction and re-parsing produces identical results to the original parse — data integrity is preserved across eviction cycles
- NFR9: The source hprof file is never modified (read-only access enforced at OS level)

### Portability

- NFR10: The tool runs on Linux, macOS, and Windows (platforms supported by ratatui + crossterm + memmap2)
- NFR11: No runtime dependencies beyond the compiled binary and optional config file
