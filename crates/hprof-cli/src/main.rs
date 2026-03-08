//! Binary entry point, `clap` CLI argument parsing, TOML config loading,
//! memory budget calculation, and frontend selection.
//!
//! After argument parsing the binary opens the hprof file with a live
//! progress bar, then launches the TUI.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use hprof_engine::NavigationEngine;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(env::args_os()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(std::io::stderr().lock(), "{err}");
            ExitCode::FAILURE
        }
    }
}

fn run<I>(args: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let path = parse_hprof_path(args)?;
    let file_len = std::fs::metadata(&path)
        .map_err(CliError::MetadataFailed)?
        .len();
    let mp = hprof_tui::progress::new_multi_progress();
    let mut reporter = hprof_tui::progress::ProgressReporter::new(&mp, file_len);
    let mut name_reporter = hprof_tui::progress::NameProgressReporter::new(&mp);

    let config = hprof_engine::EngineConfig;
    let engine = hprof_engine::Engine::from_file_with_progress(
        &path,
        &config,
        |bytes| reporter.on_bytes_processed(bytes),
        |done, total| name_reporter.on_name_resolved(done, total),
    )
    .map_err(CliError::OpenFailed)?;
    name_reporter.finish();
    let (elapsed, total_bytes) = reporter.elapsed_summary();
    let speed = total_bytes as f64 / elapsed.as_secs_f64() / 1e9;
    println!("Loaded in {elapsed:.1?} ({speed:.2} GB/s)");

    for w in engine.warnings() {
        eprintln!("[warn] {w}");
    }

    hprof_tui::run_tui(engine, path.display().to_string()).map_err(CliError::TuiFailed)?;

    Ok(())
}

fn parse_hprof_path<I>(args: I) -> Result<PathBuf, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let program_name = args
        .next()
        .unwrap_or_else(|| OsString::from("hprof-visualizer"));

    let Some(path) = args.next() else {
        return Err(CliError::Usage(program_name));
    };

    if args.next().is_some() {
        return Err(CliError::Usage(program_name));
    }

    Ok(PathBuf::from(path))
}

#[derive(Debug)]
enum CliError {
    Usage(OsString),
    MetadataFailed(std::io::Error),
    OpenFailed(hprof_engine::HprofError),
    TuiFailed(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(program_name) => {
                let executable = Path::new(program_name).display();
                write!(f, "usage: {executable} <heap.hprof>")
            }
            Self::MetadataFailed(err) => write!(f, "failed to read file metadata: {err}"),
            Self::OpenFailed(err) => write!(f, "failed to open heap dump: {err}"),
            Self::TuiFailed(err) => write!(f, "TUI error: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    #[test]
    fn parse_hprof_path_requires_exactly_one_argument() {
        let missing = parse_hprof_path(os_args(&["hprof-visualizer"]));
        assert!(matches!(missing, Err(CliError::Usage(_))));

        let extra = parse_hprof_path(os_args(&["hprof-visualizer", "a.hprof", "b.hprof"]));
        assert!(matches!(extra, Err(CliError::Usage(_))));
    }

    #[test]
    fn parse_hprof_path_accepts_single_path_argument() {
        let parsed = parse_hprof_path(os_args(&["hprof-visualizer", "heap.hprof"])).unwrap();
        assert_eq!(parsed, PathBuf::from("heap.hprof"));
    }

    #[test]
    fn run_returns_metadata_failed_for_missing_path() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let missing_path = tmp.path().to_string_lossy().to_string();
        drop(tmp);

        let result = run(os_args(&["hprof-visualizer", &missing_path]));
        assert!(matches!(result, Err(CliError::MetadataFailed(_))));
    }
}
