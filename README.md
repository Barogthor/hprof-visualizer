# hprof-visualizer

[![CI](https://github.com/Barogthor/hprof-visualizer/actions/workflows/ci.yml/badge.svg)](https://github.com/Barogthor/hprof-visualizer/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-2024%20edition-orange)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-TUI-blue)](#)

`hprof-visualizer` is a Rust terminal tool for exploring large Java heap dumps (`.hprof`) without loading the full file into memory.

It is designed for fast forensic inspection on standard developer machines using:

- memory-mapped I/O (`mmap`)
- first-pass indexing with progress feedback
- lazy loading during navigation

## Screenshot


## Why this project

Traditional tools often become slow or memory-hungry on very large dumps. `hprof-visualizer` shifts work to indexed + on-demand reads so you can inspect the values you care about quickly while keeping your machine responsive.

## Current Features

- Open `.hprof` files from the CLI
- Run initial indexing with visible progress
- Navigate threads -> stack frames -> local variables
- Search threads with inline filter
- Expand objects lazily and load strings lazily
- Continue with warnings on partially corrupted/truncated dumps

## Workspace Architecture

This repository is a Cargo workspace with the following crates:

- `crates/hprof-cli`: `hprof-visualizer` binary entry point
- `crates/hprof-tui`: terminal UI (`ratatui` + `crossterm`)
- `crates/hprof-engine`: navigation and exploration engine
- `crates/hprof-parser`: low-level HPROF parser
- `crates/hprof-api`: shared types and traits

## Prerequisites

- Rust (recent stable toolchain)
- Cargo
- A TUI-compatible terminal

## Quick Start

Run directly with Cargo:

```bash
cargo run -p hprof-cli -- /path/to/heap.hprof
```

Build release binary:

```bash
cargo build --release -p hprof-cli
```

Binary output:

```text
target/release/hprof-visualizer
```

## TUI Controls

- `Up` / `Down`: move selection
- `Home` / `End`: jump to top / bottom
- `Enter`: open item / expand
- `Esc`: go back / cancel in-flight expansion
- `/`: activate search in thread list
- `Backspace`: remove one search character
- `q` or `Ctrl+C`: quit

## Development

Run checks and tests:

```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Run parser benchmark:

```bash
cargo bench -p hprof-parser --bench first_pass
```

Enable dev profiling (writes `trace.json`):

```bash
cargo run -p hprof-cli --features dev-profiling -- /path/to/heap.hprof
```

## Roadmap (high level)

- richer thread grouping and filtering
- circular reference handling improvements
- additional heap summaries and analysis views
- future GUI/scriptable workflows

## Contributing

Contributions are welcome.

1. Fork the repository
2. Create a feature branch
3. Run formatting, clippy, and tests
4. Open a pull request with a clear description

For larger changes, opening an issue first is recommended.

## License

Licensed under either of:

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
