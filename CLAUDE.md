# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

hprof-visualizer is a Rust project for visualizing Java hprof heap dump files. It is in early development (skeleton stage).

## Build & Run Commands

```bash
cargo build           # Build the project
cargo run             # Run the binary
cargo test            # Run all tests
cargo test <name>     # Run a single test by name
cargo clippy          # Lint
cargo fmt             # Format code
cargo fmt -- --check  # Check formatting without modifying
```

## Technical Details

- **Language:** Rust (edition 2024)
- **Min Rust version:** Requires nightly or recent stable that supports edition 2024

## Environment Setup


# Coding Standards & AI instructions

## Git rules
- Do not add a co-authored
- Always check if files are ignored before adding them for staging

## Report document
- When asking for a report, always persist the result in a file preferably markdown in the folder docs/report at the project root.

## Review Output Rule
- Any code review report must be saved under `docs/code-review`.
- Any story review report must be saved under `docs/story-review`.
- Review filenames must be prefixed with `claude-`.
- If it's the code review of a story, add story-{storyId} in the name.

## Coding style
- Your most important job is to manage your own context. Always read any relevant files BEFORE planning changes.
- When updating documentation, keep updates concise and on point to prevent bloat.
- Write code following KISS, YAGNI, and DRY principles.
- When in doubt follow proven best practices for implementation.
- Do not commit to git without user approval.
- Always consider industry standard libraries/frameworks first over custom implementations.
- Never mock anything. Never use placeholders. Never omit code.
- Apply SOLID principles where relevant. Use modern framework features rather than reinventing solutions.
- Be brutally honest about whether an idea is good or bad.
- Make side effects explicit and minimal.
- Never go past 100 characters in a single line.
- Avoid functions going over 80 lines.

## Naming Conventions
- Structs & Traits: PascalCase (e.g., VoicePipeline)
- Functions/Methods: snake_case (e.g., process_audio)
- Constants: UPPER_SNAKE_CASE (e.g., MAX_AUDIO_SIZE)

## Documentation Requirements
- NO SUPERFLOUS COMMENTS that are equivalent to the function name, var name, or explicit code.
- Every module needs a docstring
- Module docstring use "//!" instead of "//"
- Surround code snippet in triple backtick
- Every public function or API needs a docstring
- Use markdown style in docstring
- Include type information in docstrings

## File Organization & Modularity
- Default to creating multiple small, focused files rather than large monolithic ones
- Each module should have a single responsibility and clear purpose
- Forbid files with multiples modules with different purpose
- Follow existing project structure and conventions - place files in appropriate directories. Create new directories and move files if deemed appropriate.
- Use well defined sub-directories to keep things organized and scalable
- Structure projects with clear folder hierarchies and consistent naming conventions

## Security First
- Never trust external inputs - validate or parse (generate validated type) everything at the boundaries
- Keep secrets in environment variables, never in code
- Log security events (login attempts, auth failures, rate limits, permission denials) but never log sensitive data (audio, conversation content, tokens, personal info)
- Authenticate users at the API gateway level - never trust client-side tokens
- Use Row Level Security (RLS) to enforce data isolation between users
- Design auth to work across all client types consistently
- Use secure authentication patterns for your platform
- Validate all authentication tokens server-side before creating sessions
- Sanitize all user inputs before storing or processing

## Error Handling
- Use specific exceptions over generic ones
- Always log errors with context
- Provide helpful error messages
- Fail securely - errors shouldn't reveal system internals

## Architecture Overview

### Architecture Principles
- **TDD**: All features developed test-first for maximum coverage

### TDD Cycle
1. **Red**: Write a failing test first
2. **Green**: Write minimal code to make test pass
3. **Refactor**: Improve code while keeping tests green
4. Always run full test suite before committing

### Testing Strategy
- **Unit tests**: Test domain logic in isolation
- **Integration tests**: Test ports/adapters integration
- **Acceptance tests**: Test complete workflows end-to-end
- **Cross-field validation TDD**: When a feature involves
  validation across multiple fields (e.g., right operand
  bounds depend on left indicator), test the full
  combinatorial matrix — not just each axis independently.
  Individual axis tests give false confidence.
- Aim for high code coverage (>90%) through TDD