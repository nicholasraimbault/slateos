/// Cross-compilation environment setup for aarch64 targets.
///
/// Validates that the required rustup target and LLVM/Clang toolchain are
/// available, then returns the environment variables needed for
/// `cargo build --target aarch64-unknown-linux-musl`.  If the toolchain
/// is missing the functions warn and return `None` so the caller can fall
/// back to the native host toolchain.
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use tracing::{debug, info, warn};

/// The target triple used for all aarch64 SlateOS builds.
#[cfg(test)]
const AARCH64_TARGET: &str = "aarch64-unknown-linux-musl";

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of [`check_cross_toolchain`].
///
/// When `ready` is true the host has everything needed to cross-compile.
/// When false, `instructions` contains human-readable setup steps.
#[derive(Debug)]
pub struct ToolchainStatus {
    /// Whether the cross-compilation toolchain is fully available.
    pub ready: bool,
    /// Human-readable instructions for fixing any missing pieces.
    pub instructions: Vec<String>,
}

/// Check whether the host can cross-compile for `target`.
///
/// Validates:
/// 1. The rustup target is installed.
/// 2. A suitable C compiler (clang) is reachable.
///
/// Returns a [`ToolchainStatus`] so the caller can decide whether to
/// proceed or fall back.
pub fn check_cross_toolchain(target: &str) -> ToolchainStatus {
    let mut instructions: Vec<String> = Vec::new();
    let mut ready = true;

    // 1. rustup target
    if !is_rustup_target_installed(target) {
        ready = false;
        instructions.push(format!(
            "Install the Rust target:  rustup target add {target}"
        ));
    }

    // 2. C compiler (clang preferred on Chimera/musl)
    if !has_clang() {
        ready = false;
        instructions.push(
            "Install clang (LLVM):  e.g. `apk add clang` or your distro's package".to_string(),
        );
    }

    if ready {
        info!(target, "cross-compilation toolchain is ready");
    } else {
        warn!(target, "cross-compilation toolchain is incomplete");
    }

    ToolchainStatus {
        ready,
        instructions,
    }
}

/// Build a map of environment variables required for cross-compiling to
/// `target` via LLVM/Clang.
///
/// If `custom_toolchain` is non-empty, the path is used as a sysroot prefix
/// (set via `slate config --set-cross-toolchain /path`).
///
/// Returns `None` when the toolchain is not ready (after printing warnings
/// and setup instructions).
pub fn setup_cross_env(target: &str, custom_toolchain: &str) -> Option<HashMap<String, String>> {
    let status = check_cross_toolchain(target);

    if !status.ready {
        print_setup_instructions(&status.instructions);
        return None;
    }

    let mut env = HashMap::new();

    // Cargo linker override — tell rustc to use clang as the linker for
    // the aarch64-musl target.  The env var name is derived from the target
    // triple with hyphens replaced by underscores and upper-cased.
    let linker_var = cargo_linker_env_var(target);
    env.insert(linker_var, "clang".to_string());

    // CC / AR for C code compiled as part of -sys crates.
    env.insert(
        "CC_aarch64_unknown_linux_musl".to_string(),
        "clang".to_string(),
    );
    env.insert(
        "AR_aarch64_unknown_linux_musl".to_string(),
        "llvm-ar".to_string(),
    );

    // Clang needs to know the target triple when cross-compiling.
    env.insert(
        "CFLAGS_aarch64_unknown_linux_musl".to_string(),
        format!("--target={target}"),
    );

    // If a custom sysroot was configured, pass it through.
    if !custom_toolchain.is_empty() {
        let sysroot = PathBuf::from(custom_toolchain);
        if sysroot.is_dir() {
            let flag = format!("--target={target} --sysroot={}", sysroot.display());
            env.insert("CFLAGS_aarch64_unknown_linux_musl".to_string(), flag);
            debug!(sysroot = %sysroot.display(), "using custom cross-toolchain sysroot");
        } else {
            warn!(
                path = %sysroot.display(),
                "configured cross_toolchain path does not exist, ignoring"
            );
        }
    }

    debug!(?env, "cross-compilation environment");
    Some(env)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check whether `target` appears in `rustup target list --installed`.
fn is_rustup_target_installed(target: &str) -> bool {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let installed = stdout.lines().any(|line| line.trim() == target);
            debug!(target, installed, "checked rustup target");
            installed
        }
        Err(e) => {
            debug!(error = %e, "failed to run rustup — assuming target is missing");
            false
        }
    }
}

/// Check whether `clang` is available on `$PATH`.
fn has_clang() -> bool {
    let result = Command::new("clang").arg("--version").output();
    let available = result.is_ok_and(|o| o.status.success());
    debug!(available, "checked for clang");
    available
}

/// Derive the `CARGO_TARGET_<TRIPLE>_LINKER` env var name from a target
/// triple.  Cargo expects the triple in SCREAMING_SNAKE_CASE.
fn cargo_linker_env_var(target: &str) -> String {
    let screaming = target.to_uppercase().replace('-', "_");
    format!("CARGO_TARGET_{screaming}_LINKER")
}

/// Print human-readable instructions for setting up cross-compilation.
fn print_setup_instructions(instructions: &[String]) {
    println!();
    println!("  Cross-compilation setup required");
    println!("  --------------------------------");
    for (i, instruction) in instructions.iter().enumerate() {
        println!("  {}. {instruction}", i + 1);
    }
    println!();
    println!("  After setup, run `slate build --device <device>` again.");
    println!("  For a custom sysroot:  slate config --set-cross-toolchain /path/to/sysroot");
    println!();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_linker_env_var_aarch64() {
        assert_eq!(
            cargo_linker_env_var(AARCH64_TARGET),
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER"
        );
    }

    #[test]
    fn cargo_linker_env_var_x86() {
        assert_eq!(
            cargo_linker_env_var("x86_64-unknown-linux-musl"),
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER"
        );
    }

    #[test]
    fn check_cross_toolchain_returns_status() {
        // We can't guarantee the target is installed in CI, but the
        // function must not panic regardless.
        let status = check_cross_toolchain(AARCH64_TARGET);
        // ready may be true or false depending on the host — just check
        // that the struct is well-formed.
        if !status.ready {
            assert!(
                !status.instructions.is_empty(),
                "incomplete toolchain should provide instructions"
            );
        }
    }

    #[test]
    fn setup_cross_env_returns_some_when_ready() {
        // Only assert shape when the toolchain happens to be installed.
        let status = check_cross_toolchain(AARCH64_TARGET);
        if status.ready {
            let env = setup_cross_env(AARCH64_TARGET, "");
            assert!(env.is_some());
            let env = env.unwrap();
            assert!(env.contains_key("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER"));
            assert!(env.contains_key("CC_aarch64_unknown_linux_musl"));
            assert!(env.contains_key("AR_aarch64_unknown_linux_musl"));
        }
    }

    #[test]
    fn setup_cross_env_with_nonexistent_sysroot_still_works() {
        // A bogus sysroot path should be ignored (warned) but not crash.
        let status = check_cross_toolchain(AARCH64_TARGET);
        if status.ready {
            let env = setup_cross_env(AARCH64_TARGET, "/nonexistent/sysroot");
            assert!(env.is_some());
        }
    }

    #[test]
    fn print_setup_instructions_does_not_panic() {
        let instructions = vec![
            "Install the Rust target:  rustup target add aarch64-unknown-linux-musl".to_string(),
            "Install clang".to_string(),
        ];
        // Should print without panicking.
        print_setup_instructions(&instructions);
    }

    #[test]
    fn toolchain_status_debug_format() {
        let status = ToolchainStatus {
            ready: false,
            instructions: vec!["test instruction".to_string()],
        };
        let debug_str = format!("{status:?}");
        assert!(debug_str.contains("ready: false"));
    }
}
