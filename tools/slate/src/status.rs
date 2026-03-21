/// `slate status` — show build state and available shell crates.
///
/// Walks the workspace `target/` directory to determine which binaries
/// have been built and when.  Does not run cargo — purely an inspection
/// of on-disk artefacts.
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use clap::Args;
use tracing::debug;

use crate::build::SHELL_CRATES;
use crate::device::Device;

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Arguments for `slate status`.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Device target to show status for.
    #[arg(short, long, default_value = "generic-x86")]
    pub device: Device,
}

// ---------------------------------------------------------------------------
// Crate status
// ---------------------------------------------------------------------------

/// Build state for a single crate.
#[derive(Debug)]
struct CrateStatus {
    /// Whether this crate is a library (no standalone binary output).
    is_library: bool,
    /// mtime of the release binary, if it exists.
    built_at: Option<SystemTime>,
}

impl CrateStatus {
    fn new(name: &'static str, out_dir: &std::path::Path) -> Self {
        // slate-common is a library crate; it produces no top-level binary.
        let is_library = name == "slate-common";

        let built_at = if is_library {
            None
        } else {
            let bin = out_dir.join(name);
            debug!(path = %bin.display(), "checking binary");
            bin.metadata().ok().and_then(|m| m.modified().ok())
        };

        CrateStatus {
            is_library,
            built_at,
        }
    }

    /// Whether a build artefact exists for this crate.
    fn is_built(&self) -> bool {
        self.is_library || self.built_at.is_some()
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Execute `slate status`.
pub fn run(args: StatusArgs) -> Result<()> {
    let repo_root = crate::workspace::find_repo_root();

    println!();
    println!("  slate status");
    println!("  ------------");

    print_device_section(&args.device);
    print_crate_section(&repo_root, &args.device);
    print_build_hint(&args.device, repo_root.is_some());

    Ok(())
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

fn print_device_section(device: &Device) {
    println!();
    println!("  Target device");
    println!("    device  : {device}");
    println!("    triple  : {}", device.cargo_target());
    println!(
        "    cross   : {}",
        if device.needs_cross_compile() {
            "yes (not yet implemented)"
        } else {
            "no (native)"
        }
    );
}

fn print_crate_section(repo_root: &Option<PathBuf>, device: &Device) {
    println!();
    println!("  Shell crates");

    let Some(root) = repo_root else {
        println!("    (workspace root not found — run slate from inside the slateos repo)");
        return;
    };

    // Check both release and debug output directories.
    let release_dir = out_dir(root, device, "release");
    let debug_dir = out_dir(root, device, "debug");

    for &name in SHELL_CRATES {
        let rel = CrateStatus::new(name, &release_dir);
        let dbg = CrateStatus::new(name, &debug_dir);

        let state = format_state(&rel, &dbg);
        println!("    {name:<20} {state}");
    }
}

fn print_build_hint(device: &Device, have_root: bool) {
    println!();

    if have_root {
        println!("  Run `slate build --device {device}` to compile all shell crates.");
    } else {
        println!("  Change into the slateos repository, then run `slate build`.");
    }
    println!();
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn out_dir(root: &std::path::Path, device: &Device, profile: &str) -> PathBuf {
    if device.needs_cross_compile() {
        root.join("target")
            .join(device.cargo_target())
            .join(profile)
    } else {
        root.join("target").join(profile)
    }
}

fn format_state(rel: &CrateStatus, dbg: &CrateStatus) -> String {
    if rel.is_library {
        return "(library)".to_string();
    }

    match (rel.is_built(), dbg.is_built()) {
        (true, true) => format!(
            "built  [release: {}, debug: {}]",
            format_mtime(rel.built_at),
            format_mtime(dbg.built_at)
        ),
        (true, false) => format!("built  [release: {}]", format_mtime(rel.built_at)),
        (false, true) => format!("built  [debug: {}]", format_mtime(dbg.built_at)),
        (false, false) => "not built".to_string(),
    }
}

fn format_mtime(mtime: Option<SystemTime>) -> String {
    let Some(t) = mtime else {
        return "unknown".to_string();
    };

    // Format as duration since build (e.g. "5m ago", "2h ago", "3d ago").
    match SystemTime::now().duration_since(t) {
        Ok(age) => {
            let secs = age.as_secs();
            if secs < 60 {
                format!("{secs}s ago")
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else {
                format!("{}d ago", secs / 86400)
            }
        }
        Err(_) => "just now".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slate_common_is_library_in_status() {
        // Use a path that definitely doesn't exist so built_at is None.
        let nonexistent = PathBuf::from("/nonexistent/target/release");
        let s = CrateStatus::new("slate-common", &nonexistent);
        assert!(s.is_library);
        assert!(s.built_at.is_none());
        // Library crates always report as "built" (no binary to check).
        assert!(s.is_built());
    }

    #[test]
    fn binary_crate_not_built_when_path_missing() {
        let nonexistent = PathBuf::from("/nonexistent/target/release");
        let s = CrateStatus::new("touchflow", &nonexistent);
        assert!(!s.is_library);
        assert!(!s.is_built());
    }

    #[test]
    fn format_mtime_recent() {
        // A mtime 30 seconds in the past should render as "30s ago".
        let t = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(30))
            .unwrap();
        let rendered = format_mtime(Some(t));
        assert!(rendered.ends_with("s ago"), "got: {rendered}");
    }

    #[test]
    fn format_mtime_minutes() {
        let t = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(120))
            .unwrap();
        let rendered = format_mtime(Some(t));
        assert!(rendered.ends_with("m ago"), "got: {rendered}");
    }

    #[test]
    fn format_mtime_none() {
        assert_eq!(format_mtime(None), "unknown");
    }

    #[test]
    fn out_dir_native() {
        let root = PathBuf::from("/repo");
        let dir = out_dir(&root, &Device::GenericX86, "release");
        assert_eq!(dir, PathBuf::from("/repo/target/release"));
    }

    #[test]
    fn out_dir_cross() {
        let root = PathBuf::from("/repo");
        let dir = out_dir(&root, &Device::PixelTablet, "release");
        assert_eq!(
            dir,
            PathBuf::from("/repo/target/aarch64-unknown-linux-musl/release")
        );
    }
}
