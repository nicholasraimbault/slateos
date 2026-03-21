// Recent apps tracking and persistence for the launcher.
//
// Tracks which apps the user launches with timestamps so the launcher can
// display a "Recent" section above the full app grid. Persisted to a TOML
// file so recent apps survive restarts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Maximum number of recent apps to retain and display.
pub const MAX_RECENT: usize = 8;

/// A single record of a launched app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentEntry {
    /// The desktop_id of the launched application.
    pub desktop_id: String,
    /// Unix timestamp (seconds since epoch) of the last launch.
    pub last_launched: u64,
}

/// The on-disk format: a list of recent entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RecentApps {
    /// Ordered list of recently launched apps (most recent first).
    pub entries: Vec<RecentEntry>,
}

impl RecentApps {
    /// Return the default file path for the recent-apps store.
    ///
    /// Uses `$XDG_CONFIG_HOME/slate/recent-apps.toml`, falling back to
    /// `~/.config/slate/recent-apps.toml` when the env var is unset.
    pub fn default_path() -> PathBuf {
        let config_home = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                PathBuf::from(home).join(".config")
            });
        config_home.join("slate").join("recent-apps.toml")
    }

    /// Load recent apps from a TOML file, returning an empty list on any error.
    ///
    /// Errors are logged but not propagated because a missing or corrupt file
    /// should never block the launcher from starting.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<RecentApps>(&content) {
                Ok(recent) => {
                    tracing::debug!("loaded {} recent apps from {}", recent.entries.len(), path.display());
                    recent
                }
                Err(err) => {
                    tracing::warn!("failed to parse recent-apps TOML: {err}");
                    Self::default()
                }
            },
            Err(_) => {
                tracing::debug!("no recent-apps file at {}, starting fresh", path.display());
                Self::default()
            }
        }
    }

    /// Save recent apps to a TOML file, creating parent directories if needed.
    ///
    /// Errors are logged but not propagated because failing to persist recent
    /// apps should never crash the launcher.
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                tracing::warn!("cannot create config dir {}: {err}", parent.display());
                return;
            }
        }

        match toml::to_string_pretty(self) {
            Ok(content) => {
                if let Err(err) = std::fs::write(path, content) {
                    tracing::warn!("failed to write recent-apps: {err}");
                }
            }
            Err(err) => {
                tracing::warn!("failed to serialise recent-apps: {err}");
            }
        }
    }

    /// Record a launch of the given desktop_id.
    ///
    /// Moves the app to the front of the list (most recent) and trims the
    /// list to `MAX_RECENT` entries.
    pub fn record_launch(&mut self, desktop_id: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Remove any existing entry for this app so it moves to the front.
        self.entries.retain(|e| e.desktop_id != desktop_id);

        self.entries.insert(
            0,
            RecentEntry {
                desktop_id: desktop_id.to_string(),
                last_launched: now,
            },
        );

        // Keep only the most recent entries.
        self.entries.truncate(MAX_RECENT);
    }

    /// Return the desktop_ids of recent apps, most-recent first.
    pub fn recent_ids(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.desktop_id.as_str()).collect()
    }

    /// Build a set of recent desktop_ids for O(1) membership checks.
    pub fn recent_id_set(&self) -> HashMap<&str, ()> {
        self.entries.iter().map(|e| (e.desktop_id.as_str(), ())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_launch_adds_to_front() {
        let mut recent = RecentApps::default();
        recent.record_launch("firefox");
        recent.record_launch("nautilus");

        assert_eq!(recent.entries[0].desktop_id, "nautilus");
        assert_eq!(recent.entries[1].desktop_id, "firefox");
    }

    #[test]
    fn record_launch_deduplicates() {
        let mut recent = RecentApps::default();
        recent.record_launch("firefox");
        recent.record_launch("nautilus");
        recent.record_launch("firefox");

        assert_eq!(recent.entries.len(), 2);
        assert_eq!(recent.entries[0].desktop_id, "firefox");
        assert_eq!(recent.entries[1].desktop_id, "nautilus");
    }

    #[test]
    fn record_launch_truncates_at_max() {
        let mut recent = RecentApps::default();
        for i in 0..MAX_RECENT + 5 {
            recent.record_launch(&format!("app-{i}"));
        }

        assert_eq!(recent.entries.len(), MAX_RECENT);
    }

    #[test]
    fn recent_ids_returns_ordered() {
        let mut recent = RecentApps::default();
        recent.record_launch("a");
        recent.record_launch("b");
        recent.record_launch("c");

        let ids = recent.recent_ids();
        assert_eq!(ids, vec!["c", "b", "a"]);
    }

    #[test]
    fn recent_id_set_contains_all() {
        let mut recent = RecentApps::default();
        recent.record_launch("firefox");
        recent.record_launch("nautilus");

        let set = recent.recent_id_set();
        assert!(set.contains_key("firefox"));
        assert!(set.contains_key("nautilus"));
        assert!(!set.contains_key("unknown"));
    }

    #[test]
    fn serialization_round_trip() {
        let mut recent = RecentApps::default();
        recent.record_launch("firefox");
        recent.record_launch("nautilus");

        let toml_str = toml::to_string_pretty(&recent).expect("serialise");
        let back: RecentApps = toml::from_str(&toml_str).expect("deserialise");
        assert_eq!(recent, back);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("slate-launcher-test-recent");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("recent-apps.toml");

        let mut recent = RecentApps::default();
        recent.record_launch("firefox");
        recent.record_launch("nautilus");

        recent.save(&path);
        let loaded = RecentApps::load(&path);
        assert_eq!(recent, loaded);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let loaded = RecentApps::load(Path::new("/nonexistent/recent-apps.toml"));
        assert_eq!(loaded, RecentApps::default());
    }

    #[test]
    fn default_path_ends_with_expected_filename() {
        let path = RecentApps::default_path();
        assert!(
            path.ends_with("slate/recent-apps.toml"),
            "expected path ending with slate/recent-apps.toml, got: {path:?}"
        );
    }

    #[test]
    fn empty_recent_apps_has_no_ids() {
        let recent = RecentApps::default();
        assert!(recent.recent_ids().is_empty());
        assert!(recent.recent_id_set().is_empty());
    }
}
