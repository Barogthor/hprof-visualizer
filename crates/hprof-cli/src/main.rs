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
use hprof_tui::keymap::KeymapPreset;
use indicatif::MultiProgress;

mod config;
mod progress;
mod splash;

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

    /// Keyboard layout preset. Accepted values: `azerty`, `qwerty`.
    /// Overrides the `keymap` setting in config.toml.
    #[arg(long = "keymap")]
    keymap: Option<String>,
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

    splash::print_splash();

    let binary_path = std::env::current_exe().unwrap_or_default();
    let app_config = config::load(&binary_path);

    let cli = Cli::parse();

    let effective_memory_limit: Option<&str> = cli
        .memory_limit
        .as_deref()
        .or(app_config.memory_limit.as_deref());

    let budget_bytes = match effective_memory_limit {
        None => None,
        Some(val) => {
            let source = if cli.memory_limit.is_some() {
                "--memory-limit"
            } else {
                "config file memory_limit"
            };
            Some(
                parse_memory_size(val)
                    .map_err(|e| CliError::InvalidMemoryLimit(format!("{source}: {e}")))?,
            )
        }
    };

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

    let keymap_str = resolve_keymap(cli.keymap.as_deref(), app_config.keymap.as_deref());
    let keymap_preset = keymap_str
        .parse::<KeymapPreset>()
        .map_err(CliError::InvalidKeymap)?;
    let keymap = keymap_preset.build();

    hprof_tui::run_tui(engine, path.display().to_string(), keymap).map_err(CliError::TuiFailed)?;

    Ok(())
}

/// Resolves the effective keymap preset name using CLI > config > default precedence.
///
/// Returns the first non-`None` value among `cli`, `config`, or `"azerty"`.
fn resolve_keymap<'a>(cli: Option<&'a str>, config: Option<&'a str>) -> &'a str {
    cli.or(config).unwrap_or("azerty")
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
    InvalidKeymap(String),
    MetadataFailed(std::io::Error),
    OpenFailed(hprof_engine::HprofError),
    TuiFailed(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMemoryLimit(msg) => {
                write!(f, "invalid memory limit: {msg}")
            }
            Self::InvalidKeymap(msg) => {
                write!(f, "invalid keymap: {msg}")
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
        assert!(cli.keymap.is_none());
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

    // --- CLI / config precedence tests (AC5) ---

    fn resolve_effective<'a>(
        cli_val: Option<&'a str>,
        config_val: Option<&'a str>,
    ) -> Option<&'a str> {
        cli_val.or(config_val)
    }

    #[test]
    fn cli_overrides_config_memory_limit() {
        assert_eq!(resolve_effective(Some("8G"), Some("4G")), Some("8G"));
    }

    #[test]
    fn config_used_when_cli_absent() {
        assert_eq!(resolve_effective(None, Some("4G")), Some("4G"));
    }

    #[test]
    fn both_absent_is_none() {
        assert_eq!(resolve_effective(None, None), None);
    }

    #[test]
    fn config_bad_value_error_message_names_source() {
        // Simulate: cli=None, config=Some("not-a-size")
        let cli_val: Option<&str> = None;
        let config_val: Option<&str> = Some("not-a-size");
        let effective = resolve_effective(cli_val, config_val);
        let source = if cli_val.is_some() {
            "--memory-limit"
        } else {
            "config file memory_limit"
        };
        let err_msg = parse_memory_size(effective.unwrap())
            .map_err(|e| format!("{source}: {e}"))
            .unwrap_err();
        assert!(
            err_msg.contains("config file"),
            "expected 'config file' in error, got: {err_msg}"
        );
    }

    #[test]
    fn cli_parse_with_keymap_flag() {
        let cli =
            Cli::try_parse_from(["hprof-visualizer", "--keymap", "qwerty", "heap.hprof"]).unwrap();
        assert_eq!(cli.keymap.as_deref(), Some("qwerty"));
    }

    #[test]
    fn cli_keymap_azerty_parses_to_preset() {
        "azerty"
            .parse::<KeymapPreset>()
            .expect("azerty must be a valid preset");
    }

    #[test]
    fn cli_keymap_bogus_is_rejected() {
        assert!("bogus".parse::<KeymapPreset>().is_err());
    }

    #[test]
    fn cli_keymap_overrides_config_keymap() {
        assert_eq!(
            super::resolve_keymap(Some("qwerty"), Some("azerty")),
            "qwerty"
        );
    }

    #[test]
    fn config_keymap_used_when_cli_absent() {
        assert_eq!(super::resolve_keymap(None, Some("azerty")), "azerty");
    }

    #[test]
    fn both_absent_defaults_to_azerty() {
        assert_eq!(super::resolve_keymap(None, None), "azerty");
    }

    #[test]
    fn cli_memory_limit_wires_to_engine_config_budget() {
        // AC2: --memory-limit 8G → EngineConfig budget = 8 GiB
        let cli = Cli::try_parse_from(["hprof-visualizer", "--memory-limit", "8G", "heap.hprof"])
            .unwrap();
        let budget_bytes = cli
            .memory_limit
            .as_deref()
            .map(parse_memory_size)
            .transpose()
            .unwrap();
        let config = hprof_engine::EngineConfig { budget_bytes };
        assert_eq!(config.effective_budget(), 8 * 1024 * 1024 * 1024);
    }
}
