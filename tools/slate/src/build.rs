/// `slate build` — compile the SlateOS shell crates.
///
/// For generic-x86 the host toolchain is used directly; aarch64 targets
/// use LLVM/Clang cross-compilation via the `cross` module.  The
/// subcommand drives `cargo build --workspace` in the slateos repo root
/// so every shell crate is compiled in one pass.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use tracing::{debug, error, info, warn};

use crate::config::CliConfig;
use crate::cross;
use crate::device::Device;

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

/// Build profile passed to cargo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Profile {
    /// Optimised release build (default).
    #[default]
    Release,
    /// Unoptimised debug build (fast compile, slow binary).
    Debug,
}

impl Profile {
    /// The cargo flag for this profile, if any.
    fn cargo_flag(self) -> Option<&'static str> {
        match self {
            Profile::Release => Some("--release"),
            Profile::Debug => None,
        }
    }

    /// The subdirectory inside `target/<triple>/` where outputs land.
    fn target_subdir(self) -> &'static str {
        match self {
            Profile::Release => "release",
            Profile::Debug => "debug",
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Release => write!(f, "release"),
            Profile::Debug => write!(f, "debug"),
        }
    }
}

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Target device to build for.
    #[arg(short, long, default_value = "generic-x86")]
    pub device: Device,

    /// Build profile.
    #[arg(short, long, default_value = "release")]
    pub profile: Profile,

    /// Build only a specific crate (e.g. `shoal`, `touchflow`).
    /// Omit to build the entire workspace.
    #[arg(short, long)]
    pub crate_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Shell crates (source of truth for status reporting)
// ---------------------------------------------------------------------------

/// All shell crates in the workspace, in dependency order.
pub const SHELL_CRATES: &[&str] = &[
    "slate-common",
    "touchflow",
    "shoal",
    "slate-launcher",
    "claw-panel",
    "slate-palette",
    "slate-suggest",
    "slate-settings",
    "slate-power",
];

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate build`.
pub fn run(args: BuildArgs) -> Result<()> {
    let repo_root =
        crate::workspace::find_repo_root().context("could not find slateos repo root")?;

    info!(
        device = %args.device,
        profile = %args.profile,
        "starting build"
    );

    print_header(&args);

    // Resolve cross-compilation environment when targeting aarch64.
    let cross_env = if args.device.needs_cross_compile() {
        let cli_config = CliConfig::load().unwrap_or_default();
        let target = args.device.cargo_target();

        match cross::setup_cross_env(target, &cli_config.cross_toolchain) {
            Some(env) => {
                info!(target, "cross-compiling with LLVM/Clang toolchain");
                Some(env)
            }
            None => {
                warn!(
                    "cross-compilation toolchain not available for {}",
                    args.device
                );
                warn!("falling back to native host toolchain — binaries will not run on target");
                None
            }
        }
    } else {
        None
    };

    let status = invoke_cargo(&repo_root, &args, cross_env.as_ref())?;

    if status.success() {
        let out_dir = output_dir(&repo_root, &args);
        print_success(&args, &out_dir);
        Ok(())
    } else {
        let code = status.code().unwrap_or(-1);
        error!(exit_code = code, "cargo exited with non-zero status");
        bail!("build failed (cargo exit code {code})");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Print a human-readable build header before invoking cargo.
fn print_header(args: &BuildArgs) {
    println!();
    println!("  slate build");
    println!("  -----------");
    println!("  device  : {}", args.device);
    println!("  target  : {}", args.device.cargo_target());
    println!("  profile : {}", args.profile);
    if let Some(name) = &args.crate_name {
        println!("  crate   : {name}");
    } else {
        println!("  crates  : {} shell crates", SHELL_CRATES.len());
    }
    println!();
}

/// Invoke `cargo build --workspace` with the right flags for the given args.
///
/// When `cross_env` is `Some`, the cross-compilation environment variables
/// are injected into the cargo process.  When `None` and the device needs
/// cross-compilation, the build falls back to the native host toolchain.
///
/// Streams cargo's stdout/stderr directly to the terminal so the user sees
/// live progress output.
fn invoke_cargo(
    repo_root: &Path,
    args: &BuildArgs,
    cross_env: Option<&HashMap<String, String>>,
) -> Result<ExitStatus> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build");

    if let Some(name) = &args.crate_name {
        cmd.arg("-p").arg(name);
    } else {
        cmd.arg("--workspace");
    }

    if let Some(flag) = args.profile.cargo_flag() {
        cmd.arg(flag);
    }

    // For generic-x86 we don't pass --target so cargo uses the host default.
    // For other devices we pass the explicit musl triple.
    if args.device.needs_cross_compile() {
        cmd.arg("--target").arg(args.device.cargo_target());
    }

    // Inject cross-compilation env vars when the toolchain is available.
    if let Some(env) = cross_env {
        for (key, value) in env {
            debug!(key, value, "setting cross-compile env var");
            cmd.env(key, value);
        }
    }

    cmd.current_dir(repo_root);

    debug!(command = ?cmd, "invoking cargo");
    let start = Instant::now();

    let status = cmd
        .status()
        .context("failed to execute cargo — is it installed?")?;

    let elapsed = start.elapsed();
    debug!(elapsed_secs = elapsed.as_secs_f32(), "cargo finished");

    Ok(status)
}

/// Compute the directory where built binaries land.
fn output_dir(repo_root: &Path, args: &BuildArgs) -> PathBuf {
    if args.device.needs_cross_compile() {
        repo_root
            .join("target")
            .join(args.device.cargo_target())
            .join(args.profile.target_subdir())
    } else {
        repo_root.join("target").join(args.profile.target_subdir())
    }
}

/// Print a success summary listing the built binary names.
fn print_success(args: &BuildArgs, out_dir: &Path) {
    println!();
    println!("  Build complete!");
    println!();
    println!("  device  : {}", args.device);
    println!("  profile : {}", args.profile);
    println!("  output  : {}", out_dir.display());
    println!();
    println!("  Binaries built:");

    // Shell crates that produce a binary (not slate-common which is a lib).
    let binaries = [
        "touchflow",
        "shoal",
        "slate-launcher",
        "claw-panel",
        "slate-palette",
        "slate-suggest",
        "slate-settings",
        "slate-power",
    ];

    for name in binaries {
        let bin = out_dir.join(name);
        let marker = if bin.exists() { "  " } else { "  (expected)" };
        println!("    {marker}{name}");
    }

    println!();
    println!("  Next steps:");
    println!(
        "    slate flash --device {}   (once flash is implemented)",
        args.device
    );
    println!();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_release_has_release_flag() {
        assert_eq!(Profile::Release.cargo_flag(), Some("--release"));
    }

    #[test]
    fn profile_debug_has_no_flag() {
        assert!(Profile::Debug.cargo_flag().is_none());
    }

    #[test]
    fn profile_target_subdirs() {
        assert_eq!(Profile::Release.target_subdir(), "release");
        assert_eq!(Profile::Debug.target_subdir(), "debug");
    }

    #[test]
    fn output_dir_generic_x86_release() {
        let root = PathBuf::from("/repo");
        let args = BuildArgs {
            device: Device::GenericX86,
            profile: Profile::Release,
            crate_name: None,
        };
        let dir = output_dir(&root, &args);
        assert_eq!(dir, PathBuf::from("/repo/target/release"));
    }

    #[test]
    fn output_dir_pixel_tablet_release() {
        let root = PathBuf::from("/repo");
        let args = BuildArgs {
            device: Device::PixelTablet,
            profile: Profile::Release,
            crate_name: None,
        };
        let dir = output_dir(&root, &args);
        assert_eq!(
            dir,
            PathBuf::from("/repo/target/aarch64-unknown-linux-musl/release")
        );
    }

    #[test]
    fn shell_crates_list_is_non_empty() {
        assert!(!SHELL_CRATES.is_empty());
    }

    #[test]
    fn shell_crates_includes_slate_common() {
        assert!(SHELL_CRATES.contains(&"slate-common"));
    }
}
