//! Splash screen printed to stderr at startup, before any progress bars.
//!
//! All output goes to `stderr` via `eprint!` to avoid contaminating the
//! raw-mode stdout buffer used by the TUI (`CrosstermBackend::new(io::stdout())`).

/// Builds the full splash string including ASCII-art banner, version line,
/// tagline, and a trailing newline.
///
/// Pure function — no I/O side effects.
///
/// Returns `String` containing:
/// - ASCII-art tool name `hprof-visualizer`
/// - version from `env!("CARGO_PKG_VERSION")`
/// - tagline "Java heap dump visualizer"
/// - trailing `\n` so progress bars start on a fresh line
pub fn build_splash() -> String {
    let version = env!("CARGO_PKG_VERSION");
    // Inner width: 2 leading spaces + 43 (max art line) + 2 trailing = 47.
    const IW: usize = 47;
    let border = format!("+{}+\n", "-".repeat(IW));

    let art: &[&str] = &[
        " _                    __          _   ",
        "| |__  _ __  _ __ ___/ _| __  ___(_)____",
        "| '_ \\| '_ \\| '__/ _ \\ |  \\ \\ / /| |_  /",
        "| | | | |_) | | | (_)| |   \\ V / | |/ /",
        "|_| |_| .__/|_|  \\___/_|    \\_/  |_/___|",
    ];
    let version_line = format!("      |_|   hprof-visualizer  v{version}");

    let mut out = border.clone();
    for line in art {
        out.push_str(&format!("|  {line:<43}  |\n"));
    }
    out.push_str(&format!("|  {version_line:<43}  |\n"));
    out.push_str(&border);
    out.push_str("  Java heap dump visualizer\n");
    out
}

/// Prints the splash screen to stderr.
pub fn print_splash() {
    eprint!("{}", build_splash());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_splash_contains_version() {
        let splash = build_splash();
        assert!(
            splash.contains(env!("CARGO_PKG_VERSION")),
            "splash must contain the crate version"
        );
    }

    #[test]
    fn build_splash_contains_tool_name() {
        let splash = build_splash();
        assert!(
            splash.contains("hprof-visualizer"),
            "splash must contain 'hprof-visualizer'"
        );
    }

    #[test]
    fn build_splash_max_line_width() {
        let splash = build_splash();
        for (i, line) in splash.lines().enumerate() {
            assert!(
                line.len() <= 80,
                "line {} exceeds 80 chars ({} chars): {line:?}",
                i + 1,
                line.len()
            );
        }
    }

    #[test]
    fn build_splash_ends_with_newline() {
        let splash = build_splash();
        assert!(splash.ends_with('\n'), "splash must end with a newline");
    }
}
