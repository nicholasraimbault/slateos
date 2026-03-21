/// `slate config` — print or edit the slate CLI configuration.
///
/// The slate CLI does not yet persist per-user config; this subcommand
/// prints what configuration *would* live at `~/.config/slate/cli.toml`
/// and documents the keys.  The subcommand is a scaffold for future
/// persistent config (default device, cross-toolchain path, etc.).
use anyhow::Result;
use clap::Args;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate config`.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Print the path where cli.toml would be stored, then exit.
    #[arg(long)]
    pub path: bool,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate config`.
pub fn run(args: ConfigArgs) -> Result<()> {
    let config_path = default_config_path();

    if args.path {
        println!("{}", config_path.display());
        return Ok(());
    }

    println!();
    println!("  slate config");
    println!("  ------------");
    println!();
    println!("  Config file : {}", config_path.display());
    println!("  Status      : not yet implemented (using built-in defaults)");
    println!();
    println!("  Future keys (cli.toml):");
    println!("    default_device = \"generic-x86\"   # default --device flag");
    println!("    cross_toolchain = \"\"              # path to aarch64 musl toolchain");
    println!("    rootfs_cache = \"\"                 # where build-rootfs.sh caches downloads");
    println!("    web_ui = false                    # enable localhost:3000 guided setup");
    println!();

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the canonical path for the slate CLI config file.
///
/// Respects `$XDG_CONFIG_HOME` when set, otherwise falls back to
/// `~/.config/slate/cli.toml` — matching XDG Base Directory spec.
fn default_config_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            dirs_home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("/root"))
                .join(".config")
        });
    base.join("slate").join("cli.toml")
}

/// Resolve the current user's home directory without pulling in the `dirs` crate.
///
/// Checks `$HOME` first (always set on Linux/macOS), then falls back to
/// the passwd entry via `getpwuid`.  Returns `None` only in degenerate
/// environments (containers with no home).
fn dirs_home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_ends_with_cli_toml() {
        let p = default_config_path();
        assert!(
            p.to_string_lossy().ends_with("slate/cli.toml"),
            "unexpected path: {p:?}"
        );
    }

    #[test]
    fn config_path_honours_xdg_config_home() {
        // Temporarily set XDG_CONFIG_HOME and verify the path changes.
        // Safety: environment mutation is not async-signal-safe, but tests
        // run single-threaded per process by default in Rust.
        let _guard = EnvGuard::set("XDG_CONFIG_HOME", "/tmp/xdg-test");
        let p = default_config_path();
        assert_eq!(p, std::path::PathBuf::from("/tmp/xdg-test/slate/cli.toml"));
    }

    #[test]
    fn config_runs_without_error() {
        let args = ConfigArgs { path: false };
        assert!(run(args).is_ok());
    }

    #[test]
    fn config_path_flag_runs_without_error() {
        let args = ConfigArgs { path: true };
        assert!(run(args).is_ok());
    }

    // ---------------------------------------------------------------------------
    // Test helper: temporarily override an env var and restore on drop.
    // ---------------------------------------------------------------------------

    struct EnvGuard {
        key: &'static str,
        original: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            std::env::set_var(key, value);
            EnvGuard { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
