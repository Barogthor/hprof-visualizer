//! Binary entry point, CLI argument parsing, and
//! frontend selection.
//!
//! After argument parsing the binary opens the hprof
//! file with a live progress bar, then launches the TUI.

use std::fmt;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use hprof_api::ParseProgressObserver;
use hprof_engine::NavigationEngine;
use indicatif::MultiProgress;

mod progress;

/// Visualize Java hprof heap dumps in the terminal.
#[derive(Parser, Debug)]
#[command(name = "hprof-visualizer")]
struct Cli {
    /// Path to the .hprof heap dump file.
    file: PathBuf,

    /// Memory budget override (e.g. "8G", "512M").
    ///
    /// Binary units: 1G = 1024^3. Supported suffixes:
    /// K, M, G, T (case-insensitive). Without this flag
    /// the budget is auto-calculated as 50% of total RAM.
    #[arg(long = "memory-limit")]
    memory_limit: Option<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr().lock(), "{err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), CliError> {
    #[cfg(feature = "dev-profiling")]
    let _guard = {
        use tracing_subscriber::prelude::*;
        use tracing_subscriber::{EnvFilter, fmt};

        let chrome_layer = {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .file("trace.json")
                .build();
            (layer, guard)
        };

        let file_appender = tracing_appender::rolling::never(".", "logs/hprof-debug.log");
        let file_layer = fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_target(true);

        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"));

        tracing_subscriber::registry()
            .with(filter)
            .with(chrome_layer.0)
            .with(file_layer)
            .init();
        tracing::debug!("=== hprof-debug logging active ===");
        chrome_layer.1
    };

    let cli = Cli::parse();

    let budget_bytes = cli
        .memory_limit
        .as_deref()
        .map(parse_memory_size)
        .transpose()
        .map_err(CliError::InvalidMemoryLimit)?;

    let path = &cli.file;
    let file_len = std::fs::metadata(path)
        .map_err(CliError::MetadataFailed)?
        .len();
    let mp = MultiProgress::new();
    let mut observer = progress::CliProgressObserver::new(&mp, file_len);

    let config = hprof_engine::EngineConfig { budget_bytes };
    let engine = hprof_engine::Engine::from_file_with_progress(
        path,
        &config,
        &mut observer as &mut dyn ParseProgressObserver,
    )
    .map_err(CliError::OpenFailed)?;
    observer.finish();

    for w in engine.warnings() {
        eprintln!("[warn] {w}");
    }

    hprof_tui::run_tui(engine, path.display().to_string()).map_err(CliError::TuiFailed)?;

    Ok(())
}

/// Parses a human-readable memory size string into bytes.
///
/// Supports suffixes `K`, `M`, `G`, `T` (case-insensitive,
/// binary: 1G = 1024^3). A plain number is treated as bytes.
///
/// Uses `checked_mul` for overflow safety.
fn parse_memory_size(s: &str) -> Result<u64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty memory size".to_string());
    }
    let (num_str, multiplier) = match s.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1024u64),
        Some(b'm' | b'M') => (&s[..s.len() - 1], 1024u64 * 1024),
        Some(b'g' | b'G') => (&s[..s.len() - 1], 1024u64 * 1024 * 1024),
        Some(b't' | b'T') => (&s[..s.len() - 1], 1024u64.pow(4)),
        _ => (s, 1u64),
    };
    let num: u64 = num_str
        .trim()
        .parse()
        .map_err(|e| format!("invalid number: {e}"))?;
    num.checked_mul(multiplier)
        .ok_or_else(|| format!("overflow: {s}"))
}

#[derive(Debug)]
enum CliError {
    InvalidMemoryLimit(String),
    MetadataFailed(std::io::Error),
    OpenFailed(hprof_engine::HprofError),
    TuiFailed(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMemoryLimit(msg) => {
                write!(f, "invalid --memory-limit: {msg}")
            }
            Self::MetadataFailed(err) => {
                write!(f, "failed to read file metadata: {err}")
            }
            Self::OpenFailed(err) => {
                write!(f, "failed to open heap dump: {err}")
            }
            Self::TuiFailed(err) => {
                write!(f, "TUI error: {err}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_memory_size tests ---

    #[test]
    fn parse_memory_size_kilobytes() {
        assert_eq!(parse_memory_size("100K").unwrap(), 102_400);
        assert_eq!(parse_memory_size("100k").unwrap(), 102_400);
    }

    #[test]
    fn parse_memory_size_megabytes() {
        assert_eq!(parse_memory_size("512M").unwrap(), 512 * 1024 * 1024);
        assert_eq!(parse_memory_size("512m").unwrap(), 512 * 1024 * 1024);
    }

    #[test]
    fn parse_memory_size_gigabytes() {
        assert_eq!(parse_memory_size("8G").unwrap(), 8 * 1024 * 1024 * 1024);
        assert_eq!(parse_memory_size("8g").unwrap(), 8 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_memory_size_terabytes() {
        assert_eq!(parse_memory_size("1T").unwrap(), 1024u64.pow(4));
    }

    #[test]
    fn parse_memory_size_plain_bytes() {
        assert_eq!(parse_memory_size("1048576").unwrap(), 1_048_576);
    }

    #[test]
    fn parse_memory_size_empty_is_error() {
        assert!(parse_memory_size("").is_err());
    }

    #[test]
    fn parse_memory_size_invalid_is_error() {
        assert!(parse_memory_size("abc").is_err());
        assert!(parse_memory_size("G").is_err());
    }

    #[test]
    fn parse_memory_size_overflow_is_error() {
        assert!(parse_memory_size("99999999999999T").is_err());
    }

    #[test]
    fn parse_memory_size_zero() {
        assert_eq!(parse_memory_size("0").unwrap(), 0);
        assert_eq!(parse_memory_size("0G").unwrap(), 0);
    }

    // --- Cli::try_parse_from tests ---

    #[test]
    fn cli_parse_file_only() {
        let cli = Cli::try_parse_from(["hprof-visualizer", "heap.hprof"]).unwrap();
        assert_eq!(cli.file, PathBuf::from("heap.hprof"));
        assert!(cli.memory_limit.is_none());
    }

    #[test]
    fn cli_parse_with_memory_limit() {
        let cli = Cli::try_parse_from(["hprof-visualizer", "--memory-limit", "8G", "heap.hprof"])
            .unwrap();
        assert_eq!(cli.file, PathBuf::from("heap.hprof"));
        assert_eq!(cli.memory_limit.as_deref(), Some("8G"));
    }

    #[test]
    fn cli_parse_missing_file_is_error() {
        assert!(Cli::try_parse_from(["hprof-visualizer"]).is_err());
    }

    #[test]
    fn cli_parse_extra_positional_is_error() {
        assert!(Cli::try_parse_from(["hprof-visualizer", "a.hprof", "b.hprof",]).is_err());
    }
}
