/// `slate info` — print a comprehensive system and project summary.
///
/// Combines build status, device info, service count, and config state
/// into a single diagnostic output. Useful for debugging and support.
use anyhow::Result;
use clap::Args;

use crate::build::SHELL_CRATES;
use crate::config::CliConfig;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate info`.
#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Output machine-readable format (one key=value per line).
    #[arg(long)]
    pub raw: bool,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate info`.
pub fn run(args: InfoArgs) -> Result<()> {
    let repo_root = crate::workspace::find_repo_root();
    let config = CliConfig::load().unwrap_or_default();

    let version = env!("CARGO_PKG_VERSION");
    let rust_version = rust_version();
    let crate_count = SHELL_CRATES.len();
    let service_count = count_services(&repo_root);
    let has_repo = repo_root.is_some();

    if args.raw {
        println!("slate_version={version}");
        println!("rust_version={rust_version}");
        println!("shell_crates={crate_count}");
        println!("services={service_count}");
        println!("default_device={}", config.default_device);
        println!("has_repo={has_repo}");
        if let Some(root) = &repo_root {
            println!("repo_root={}", root.display());
        }
        return Ok(());
    }

    println!();
    println!("  slate info");
    println!("  ----------");
    println!();
    println!("  Slate CLI     : v{version}");
    println!("  Rust          : {rust_version}");
    println!("  Shell crates  : {crate_count}");
    println!("  Services      : {service_count} (base)");
    println!("  Default device: {}", config.default_device);

    if let Some(root) = &repo_root {
        println!("  Repo root     : {}", root.display());

        let git_branch = git_branch(root);
        let git_dirty = git_dirty(root);
        println!("  Git branch    : {git_branch}");
        if git_dirty {
            println!("  Git status    : dirty (uncommitted changes)");
        } else {
            println!("  Git status    : clean");
        }
    } else {
        println!("  Repo root     : (not found)");
    }

    if !config.cross_toolchain.is_empty() {
        println!("  Cross toolchain: {}", config.cross_toolchain);
    }

    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn rust_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn count_services(repo_root: &Option<std::path::PathBuf>) -> usize {
    let Some(root) = repo_root else {
        return 0;
    };
    let base_dir = root.join("services").join("base");
    std::fs::read_dir(base_dir)
        .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).count())
        .unwrap_or(0)
}

fn git_branch(repo_root: &std::path::Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn git_dirty(repo_root: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_runs_without_error() {
        let args = InfoArgs { raw: false };
        assert!(run(args).is_ok());
    }

    #[test]
    fn info_raw_runs_without_error() {
        let args = InfoArgs { raw: true };
        assert!(run(args).is_ok());
    }

    #[test]
    fn rust_version_returns_something() {
        let v = rust_version();
        assert!(!v.is_empty());
    }
}
