/// `slate dev` — start a development loop with cargo watch.
///
/// Wraps `cargo watch` to rebuild on source changes. Falls back to
/// a manual `cargo build --workspace` if cargo-watch is not installed.
use std::process::Command;

use anyhow::{Context, Result};
use clap::Args;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate dev`.
#[derive(Debug, Args)]
pub struct DevArgs {
    /// Only watch a specific crate.
    #[arg(short, long)]
    pub crate_name: Option<String>,

    /// Run tests on change instead of building.
    #[arg(long)]
    pub test: bool,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate dev`.
pub fn run(args: DevArgs) -> Result<()> {
    let repo_root =
        crate::workspace::find_repo_root().context("could not find slateos repo root")?;

    let scope = args.crate_name.as_deref().unwrap_or("workspace");
    let action = if args.test { "test" } else { "build" };

    println!();
    println!("  slate dev ({scope})");
    println!("  {}", "-".repeat(14 + scope.len()));
    println!("  action : {action} on change");
    println!("  scope  : {scope}");
    println!();

    // Try cargo-watch first (better UX with debouncing + clear).
    if has_cargo_watch() {
        info!("using cargo-watch for live rebuild");
        run_cargo_watch(&repo_root, &args)
    } else {
        warn!("cargo-watch not installed — running a single build instead");
        warn!("install it with: cargo install cargo-watch");
        run_single_build(&repo_root, &args)
    }
}

// ---------------------------------------------------------------------------
// Implementations
// ---------------------------------------------------------------------------

fn run_cargo_watch(
    repo_root: &std::path::Path,
    args: &DevArgs,
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg("watch");
    cmd.arg("-c"); // Clear screen between runs.

    let action = if args.test { "test" } else { "build" };

    if let Some(name) = &args.crate_name {
        cmd.arg("-x").arg(format!("{action} -p {name}"));
    } else {
        cmd.arg("-x").arg(format!("{action} --workspace"));
    }

    cmd.current_dir(repo_root);

    println!("  Watching for changes... (Ctrl+C to stop)");
    println!();

    let status = cmd.status().context("failed to run cargo watch")?;

    if !status.success() {
        // cargo-watch exits non-zero when the user presses Ctrl+C, which is fine.
        info!("cargo watch exited");
    }

    Ok(())
}

fn run_single_build(
    repo_root: &std::path::Path,
    args: &DevArgs,
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    let action = if args.test { "test" } else { "build" };
    cmd.arg(action);

    if let Some(name) = &args.crate_name {
        cmd.arg("-p").arg(name);
    } else {
        cmd.arg("--workspace");
    }

    cmd.current_dir(repo_root);

    let status = cmd
        .status()
        .with_context(|| format!("failed to run cargo {action}"))?;

    if status.success() {
        println!("  Build complete.");
    } else {
        println!("  Build failed.");
    }

    Ok(())
}

fn has_cargo_watch() -> bool {
    Command::new("cargo")
        .args(["watch", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_cargo_watch_does_not_panic() {
        // Just verify it runs without crashing, regardless of result.
        let _ = has_cargo_watch();
    }
}
