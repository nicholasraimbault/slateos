/// Persistent notification history stored as daily TOML files.
///
/// Each day gets its own TOML file (YYYY-MM-DD.toml) in the history
/// directory. Files are append-only and human-readable. No automatic
/// deletion — the user controls retention.
///
/// Uses `std::fs` (not tokio::fs) because these functions are called from
/// zbus D-Bus handler threads that lack a tokio runtime context.
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use slate_common::notifications::Notification;

use crate::error::HistoryError;

// ---------------------------------------------------------------------------
// On-disk format
// ---------------------------------------------------------------------------

/// Wrapper for a day's worth of notifications in TOML.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DayFile {
    notifications: Vec<Notification>,
}

// ---------------------------------------------------------------------------
// HistoryWriter
// ---------------------------------------------------------------------------

/// Appends dismissed/expired notifications to daily history files.
pub struct HistoryWriter;

impl HistoryWriter {
    /// Append a notification to the appropriate daily TOML file.
    ///
    /// Creates the file if it does not exist. Appends to the existing
    /// notifications array if the file already has content.
    pub fn append(notification: &Notification, base_dir: &Path) -> Result<(), HistoryError> {
        std::fs::create_dir_all(base_dir)?;

        let date_str = notification.timestamp.format("%Y-%m-%d").to_string();
        let file_path = base_dir.join(format!("{date_str}.toml"));

        let mut day_file = if file_path.exists() {
            let content = std::fs::read_to_string(&file_path)?;
            toml::from_str::<DayFile>(&content)?
        } else {
            DayFile {
                notifications: Vec::new(),
            }
        };

        day_file.notifications.push(notification.clone());
        let content = toml::to_string_pretty(&day_file)?;
        std::fs::write(&file_path, content)?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HistoryReader
// ---------------------------------------------------------------------------

/// Reads notification history from daily TOML files.
pub struct HistoryReader;

impl HistoryReader {
    /// Read notifications from history since a given timestamp, up to a limit.
    ///
    /// Scans daily files from the `since` date forward, returning at most
    /// `limit` notifications sorted newest-first.
    pub fn read(
        base_dir: &Path,
        since: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<Notification>, HistoryError> {
        if !base_dir.exists() {
            return Ok(Vec::new());
        }

        let since_date = since.date_naive();
        let today = Utc::now().date_naive();

        let mut all: Vec<Notification> = Vec::new();

        let file_paths = Self::date_range_paths(base_dir, since_date, today);

        for path in &file_paths {
            if !path.exists() {
                continue;
            }
            let content = std::fs::read_to_string(path)?;
            let day_file: DayFile = toml::from_str(&content)?;
            for n in day_file.notifications {
                if n.timestamp >= since {
                    all.push(n);
                }
            }
        }

        // Sort newest first and truncate to limit
        all.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        all.truncate(limit);

        Ok(all)
    }

    /// Generate file paths for each date in range [start, end].
    fn date_range_paths(base_dir: &Path, start: NaiveDate, end: NaiveDate) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut current = start;
        while current <= end {
            let filename = format!("{}.toml", current.format("%Y-%m-%d"));
            paths.push(base_dir.join(filename));
            current = current.succ_opt().unwrap_or(current);
        }
        paths
    }

    /// Return the default history base directory.
    pub fn default_base_dir() -> Result<PathBuf, HistoryError> {
        let home = std::env::var("HOME").map_err(|_| HistoryError::NoBaseDir)?;
        Ok(PathBuf::from(home).join(".local/state/slate-notifyd/history"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn append_creates_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let n = Notification::new(1, "Firefox", "Tab", "Opened");

        HistoryWriter::append(&n, dir.path()).expect("append");

        let date_str = n.timestamp.format("%Y-%m-%d").to_string();
        let file_path = dir.path().join(format!("{date_str}.toml"));
        assert!(file_path.exists());
    }

    #[test]
    fn append_multiple_to_same_day() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let n1 = Notification::new(1, "Firefox", "Tab 1", "Opened");
        let n2 = Notification::new(2, "Firefox", "Tab 2", "Opened");

        HistoryWriter::append(&n1, dir.path()).expect("append 1");
        HistoryWriter::append(&n2, dir.path()).expect("append 2");

        let date_str = n1.timestamp.format("%Y-%m-%d").to_string();
        let file_path = dir.path().join(format!("{date_str}.toml"));
        let content = std::fs::read_to_string(file_path).expect("read");
        let day: DayFile = toml::from_str(&content).expect("parse");
        assert_eq!(day.notifications.len(), 2);
    }

    #[test]
    fn read_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let since = Utc::now() - Duration::hours(1);
        let result = HistoryReader::read(dir.path(), since, 100).expect("read");
        assert!(result.is_empty());
    }

    #[test]
    fn read_nonexistent_dir_returns_empty() {
        let since = Utc::now() - Duration::hours(1);
        let result =
            HistoryReader::read(Path::new("/nonexistent/history"), since, 100).expect("read");
        assert!(result.is_empty());
    }

    #[test]
    fn read_returns_notifications_since_timestamp() {
        let dir = tempfile::tempdir().expect("create temp dir");

        let now = Utc::now();
        let mut old = Notification::new(1, "App", "Old", "body");
        old.timestamp = now - Duration::hours(2);
        let mut recent = Notification::new(2, "App", "Recent", "body");
        recent.timestamp = now;

        HistoryWriter::append(&old, dir.path()).expect("append old");
        HistoryWriter::append(&recent, dir.path()).expect("append recent");

        // Read only last hour
        let since = now - Duration::hours(1);
        let result = HistoryReader::read(dir.path(), since, 100).expect("read");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].summary, "Recent");
    }

    #[test]
    fn read_respects_limit() {
        let dir = tempfile::tempdir().expect("create temp dir");

        for i in 0..10 {
            let n = Notification::new(i, "App", &format!("Notif {i}"), "body");
            HistoryWriter::append(&n, dir.path()).expect("append");
        }

        let since = Utc::now() - Duration::hours(1);
        let result = HistoryReader::read(dir.path(), since, 3).expect("read");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn read_returns_newest_first() {
        let dir = tempfile::tempdir().expect("create temp dir");

        let now = Utc::now();
        let mut n1 = Notification::new(1, "App", "Older", "body");
        n1.timestamp = now - Duration::seconds(10);
        let mut n2 = Notification::new(2, "App", "Newer", "body");
        n2.timestamp = now;

        HistoryWriter::append(&n1, dir.path()).expect("append");
        HistoryWriter::append(&n2, dir.path()).expect("append");

        let since = now - Duration::hours(1);
        let result = HistoryReader::read(dir.path(), since, 100).expect("read");
        assert_eq!(result[0].summary, "Newer");
        assert_eq!(result[1].summary, "Older");
    }

    #[test]
    fn default_base_dir_contains_history() {
        if std::env::var("HOME").is_ok() {
            let path = HistoryReader::default_base_dir().expect("base dir");
            let path_str = path.to_string_lossy();
            assert!(path_str.contains("slate-notifyd"));
            assert!(path_str.contains("history"));
        }
    }

    #[test]
    fn date_range_paths_single_day() {
        let base = Path::new("/tmp/history");
        let date = NaiveDate::from_ymd_opt(2026, 3, 22).expect("valid date");
        let paths = HistoryReader::date_range_paths(base, date, date);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_string_lossy(), "/tmp/history/2026-03-22.toml");
    }

    #[test]
    fn date_range_paths_multiple_days() {
        let base = Path::new("/tmp/history");
        let start = NaiveDate::from_ymd_opt(2026, 3, 20).expect("valid date");
        let end = NaiveDate::from_ymd_opt(2026, 3, 22).expect("valid date");
        let paths = HistoryReader::date_range_paths(base, start, end);
        assert_eq!(paths.len(), 3);
    }
}
