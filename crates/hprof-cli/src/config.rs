//! TOML configuration loading with a CWD-first, binary-directory-fallback
//! lookup strategy.
//!
//! The public entry point is [`load`]. Tests should call [`load_from`]
//! directly to inject explicit paths and avoid reading an ambient
//! `config.toml` from the real process working directory.

use std::path::Path;

/// Application-level configuration loaded from a `config.toml` file.
///
/// Unknown keys are silently ignored (serde default — satisfies AC6).
/// `Default` yields all-`None` fields, which acts as the silent built-in
/// default when no config file is found (AC3).
#[derive(Debug, Default, serde::Deserialize)]
pub struct AppConfig {
    pub memory_limit: Option<String>,
}

/// Loads configuration from the first `config.toml` found in the standard
/// lookup order: CWD → binary directory → built-in defaults.
///
/// `binary_path` should be the value of `std::env::current_exe()`.
pub fn load(binary_path: &Path) -> AppConfig {
    let cwd = std::env::current_dir().unwrap_or_default();
    load_from(&cwd, binary_path)
}

/// Testable core: resolves config with explicitly injected `cwd` and
/// `binary_path`.
///
/// Lookup order (early-return on first successful parse):
/// 1. `cwd/config.toml` (AC1)
/// 2. `parent(canonicalize(binary_path))/config.toml` (AC2)
/// 3. `AppConfig::default()` silently (AC3)
///
/// If a candidate file exists but contains malformed TOML, a warning is
/// printed to **stderr** and `AppConfig::default()` is returned (AC4).
pub(crate) fn load_from(cwd: &Path, binary_path: &Path) -> AppConfig {
    let bin_dir_config = {
        let resolved = binary_path
            .canonicalize()
            .unwrap_or_else(|_| binary_path.to_path_buf());
        resolved.parent().map(|p| p.join("config.toml"))
    };

    let candidates: &[&dyn Fn() -> Option<std::path::PathBuf>] = &[
        &|| Some(cwd.join("config.toml")),
        &|| bin_dir_config.clone(),
    ];

    for candidate_fn in candidates {
        let Some(path) = candidate_fn() else { continue };
        let content = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue, // file does not exist or unreadable — skip
        };
        return match toml::from_str::<AppConfig>(&content) {
            Ok(cfg) => cfg,
            Err(err) => {
                eprintln!("[warn] config: {}: {}", path.display(), err);
                AppConfig::default()
            }
        };
    }

    AppConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_config(dir: &std::path::Path, content: &str) {
        let mut f = std::fs::File::create(dir.join("config.toml")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn no_file_returns_defaults() {
        let cfg = load_from(
            std::path::Path::new("/nonexistent/cwd"),
            std::path::Path::new("/nonexistent/bin"),
        );
        assert!(cfg.memory_limit.is_none());
    }

    #[test]
    fn config_loaded_from_cwd() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), r#"memory_limit = "4G""#);
        let cfg = load_from(dir.path(), std::path::Path::new("/nonexistent/bin"));
        assert_eq!(cfg.memory_limit.as_deref(), Some("4G"));
    }

    #[test]
    fn config_loaded_from_binary_dir() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), r#"memory_limit = "4G""#);
        // bin path does not need to exist; canonicalize() fails → fallback to raw
        // → parent() = dir
        let cfg = load_from(
            std::path::Path::new("/nonexistent/cwd"),
            &dir.path().join("bin"),
        );
        assert_eq!(cfg.memory_limit.as_deref(), Some("4G"));
    }

    #[test]
    fn cwd_takes_priority_over_binary_dir() {
        let cwd_dir = tempfile::tempdir().unwrap();
        let bin_dir = tempfile::tempdir().unwrap();
        write_config(cwd_dir.path(), r#"memory_limit = "2G""#);
        write_config(bin_dir.path(), r#"memory_limit = "8G""#);
        let cfg = load_from(cwd_dir.path(), &bin_dir.path().join("bin"));
        assert_eq!(cfg.memory_limit.as_deref(), Some("2G"));
    }

    #[test]
    fn malformed_toml_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "not valid toml !!!");
        let cfg = load_from(dir.path(), std::path::Path::new("/nonexistent/bin"));
        assert!(cfg.memory_limit.is_none());
    }

    #[test]
    fn unknown_key_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "unknown_key = 42\nmemory_limit = \"2G\"");
        let cfg = load_from(dir.path(), std::path::Path::new("/nonexistent/bin"));
        assert_eq!(cfg.memory_limit.as_deref(), Some("2G"));
    }
}
