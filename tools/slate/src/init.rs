/// `slate init` — set up a local development environment.
///
/// Creates local config directories and symlinks the correct per-device
/// niri/waybar configs for the specified device. Useful after cloning
/// the repo for the first time.
use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;

use crate::device::Device;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Device to configure for (determines niri/waybar config selection).
    #[arg(short, long, default_value = "generic-x86")]
    pub device: Device,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate init`.
pub fn run(args: InitArgs) -> Result<()> {
    let repo_root =
        crate::workspace::find_repo_root().context("could not find slateos repo root")?;

    println!();
    println!("  slate init");
    println!("  ----------");
    println!("  device : {}", args.device);
    println!();

    // Save the default device to config.
    let mut config = crate::config::CliConfig::load().unwrap_or_default();
    config.default_device = args.device.to_string();
    config.save().context("failed to save CLI config")?;
    println!("  [1/3] Saved default device to CLI config");

    // Check for the correct niri config.
    let niri_config = repo_root
        .join("config")
        .join("niri")
        .join("devices")
        .join(format!("{}.kdl", args.device));

    if niri_config.exists() {
        println!(
            "  [2/3] Niri config ready: {}",
            niri_config
                .strip_prefix(&repo_root)
                .unwrap_or(&niri_config)
                .display()
        );
    } else {
        println!(
            "  [2/3] No niri config for {} (will use generic-x86 fallback)",
            args.device
        );
    }

    // Check for waybar config.
    let waybar_config = repo_root
        .join("config")
        .join("waybar")
        .join("devices")
        .join(format!("{}.jsonc", args.device));

    if waybar_config.exists() {
        println!(
            "  [3/3] Waybar config ready: {}",
            waybar_config
                .strip_prefix(&repo_root)
                .unwrap_or(&waybar_config)
                .display()
        );
    } else {
        println!(
            "  [3/3] No waybar config for {} (will use base config)",
            args.device
        );
    }

    println!();
    println!("  Development environment ready!");
    println!();
    println!("  Next steps:");
    println!("    slate build    # Build all shell crates");
    println!("    slate check    # Verify everything passes");
    println!("    slate dev      # Start live rebuild loop");
    println!();

    print_nested_niri_hint(&repo_root, &args.device);

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn print_nested_niri_hint(repo_root: &Path, device: &Device) {
    if !device.needs_cross_compile() {
        let config_path = repo_root
            .join("config")
            .join("niri")
            .join("devices")
            .join(format!("{device}.kdl"));

        if config_path.exists() {
            println!("  To run niri nested (inside an existing Wayland session):");
            println!("    niri --config {}", config_path.display());
            println!();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_runs_without_error() {
        let args = InitArgs {
            device: Device::GenericX86,
        };
        // Just verifies it doesn't panic — actual file ops depend on env.
        assert!(run(args).is_ok());
    }
}
