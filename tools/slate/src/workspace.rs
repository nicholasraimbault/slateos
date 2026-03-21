/// Shared workspace utilities for the slate CLI.
use std::path::PathBuf;

use tracing::debug;

/// Walk up from the current directory to find the slateos workspace root.
///
/// Identifies the root by the presence of `Cargo.toml` containing the
/// `[workspace]` key.
pub fn find_repo_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            if let Ok(contents) = std::fs::read_to_string(&candidate) {
                if contents.contains("[workspace]") {
                    debug!(root = %dir.display(), "found workspace root");
                    return Some(dir);
                }
            }
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_repo_root_from_slateos_dir() {
        // When run from inside the slateos repo, this should find the root.
        let root = find_repo_root();
        assert!(root.is_some(), "should find workspace root");
        let root = root.unwrap();
        assert!(root.join("Cargo.toml").exists());
    }
}
