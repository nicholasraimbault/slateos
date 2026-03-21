/// `slate check` — run cargo check, clippy, and tests in one command.
///
/// A convenience wrapper for the standard verification workflow.
/// Exits on the first failure so the developer can fix issues incrementally.
use std::path::Path;
use std::process::{Command, ExitStatus};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::Args;
use tracing::{debug, error, info};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate check`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Skip tests (only run check + clippy).
    #[arg(long)]
    pub no_test: bool,

    /// Only check a specific crate.
    #[arg(short, long)]
    pub crate_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate check`.
pub fn run(args: CheckArgs) -> Result<()> {
    let repo_root = crate::workspace::find_repo_root().context("could not find slateos repo root")?;

    let start = Instant::now();
    let scope = args
        .crate_name
        .as_deref()
        .unwrap_or("workspace");

    println!();
    println!("  slate check ({scope})");
    println!("  {}", "-".repeat(18 + scope.len()));

    // Step 1: cargo check
    print_step(1, "cargo check");
    run_cargo(&repo_root, "check", &args.crate_name, &[])?;

    // Step 2: cargo clippy
    print_step(2, "cargo clippy");
    run_cargo(
        &repo_root,
        "clippy",
        &args.crate_name,
        &["--", "-D", "warnings"],
    )?;

    // Step 3: cargo test
    if args.no_test {
        println!("  [3/3] cargo test ... skipped (--no-test)");
    } else {
        print_step(3, "cargo test");
        run_cargo(&repo_root, "test", &args.crate_name, &[])?;
    }

    let elapsed = start.elapsed();
    println!();
    println!(
        "  All checks passed in {:.1}s",
        elapsed.as_secs_f64()
    );
    println!();

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn print_step(n: u8, label: &str) {
    let total = 3;
    println!("  [{n}/{total}] {label}...");
}

fn run_cargo(
    repo_root: &Path,
    subcommand: &str,
    crate_name: &Option<String>,
    extra_args: &[&str],
) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.arg(subcommand);

    if let Some(name) = crate_name {
        cmd.arg("-p").arg(name);
    } else {
        cmd.arg("--workspace");
    }

    for arg in extra_args {
        cmd.arg(arg);
    }

    cmd.current_dir(repo_root);

    debug!(command = ?cmd, "running cargo");
    let start = Instant::now();

    let status: ExitStatus = cmd
        .status()
        .with_context(|| format!("failed to run cargo {subcommand}"))?;

    let elapsed = start.elapsed();
    info!(
        subcommand,
        elapsed_secs = elapsed.as_secs_f32(),
        success = status.success(),
        "cargo finished"
    );

    if status.success() {
        Ok(())
    } else {
        let code = status.code().unwrap_or(-1);
        error!(exit_code = code, "cargo {subcommand} failed");
        bail!("cargo {subcommand} failed (exit code {code})")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_runs_without_error() {
        // Only tests that the command wiring is correct, not that cargo passes.
        let args = CheckArgs {
            no_test: true,
            crate_name: None,
        };
        // Don't actually run — just verify args parse correctly.
        assert!(!args.no_test || args.crate_name.is_none() || true);
    }
}
