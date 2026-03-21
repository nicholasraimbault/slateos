// .desktop file discovery and parsing for the app launcher.
//
// Scans XDG_DATA_DIRS for .desktop files and produces a sorted list of
// launchable applications. This is intentionally independent of Shoal's
// implementation — both crates are standalone binaries.

use std::path::{Path, PathBuf};

/// A launchable application parsed from a .desktop file.
#[derive(Debug, Clone, PartialEq)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub desktop_id: String,
    pub keywords: Vec<String>,
}

/// Return the list of directories to scan for .desktop files.
///
/// Uses `XDG_DATA_DIRS` when set, otherwise falls back to the FreeDesktop
/// default (`/usr/local/share:/usr/share`). Each directory gets
/// `/applications` appended.
pub fn desktop_dirs() -> Vec<PathBuf> {
    let raw = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());

    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(|dir| PathBuf::from(dir).join("applications"))
        .collect()
}

/// Parse a single .desktop file into an `AppEntry`.
///
/// Returns `None` when the file is not a visible, launchable application
/// (e.g. `NoDisplay=true`, `Type!=Application`, or missing `Name`/`Exec`).
pub fn parse_desktop_file(path: &Path) -> Option<AppEntry> {
    let content = std::fs::read_to_string(path).ok()?;
    parse_desktop_content(&content, path)
}

/// Parse the textual content of a .desktop file. Separated from I/O so
/// we can unit-test without touching the filesystem.
pub fn parse_desktop_content(content: &str, path: &Path) -> Option<AppEntry> {
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut icon = String::new();
    let mut keywords = Vec::new();
    let mut entry_type: Option<String> = None;
    let mut no_display = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let line = line.trim();

        // Section headers
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "Name" => name = Some(value.to_string()),
                "Exec" => {
                    // Strip field codes (%f, %F, %u, %U, etc.)
                    let cleaned = strip_field_codes(value);
                    exec = Some(cleaned);
                }
                "Icon" => icon = value.to_string(),
                "Type" => entry_type = Some(value.to_string()),
                "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
                "Keywords" => {
                    keywords = value
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    // Must be Type=Application (or unset, which we treat as Application)
    if let Some(ref t) = entry_type {
        if t != "Application" {
            return None;
        }
    }

    if no_display {
        return None;
    }

    let name = name?;
    let exec = exec?;

    let desktop_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    Some(AppEntry {
        name,
        exec,
        icon,
        desktop_id,
        keywords,
    })
}

/// Remove .desktop Exec field codes like %f, %F, %u, %U, etc.
fn strip_field_codes(exec: &str) -> String {
    let mut result = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            // Skip the next character (the field code letter)
            chars.next();
        } else {
            result.push(ch);
        }
    }

    // Collapse any double spaces left behind and trim
    let collapsed: String = result.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
}

/// Discover all launchable apps, sorted alphabetically by name.
pub fn discover_apps() -> Vec<AppEntry> {
    let mut apps = Vec::new();

    for dir in desktop_dirs() {
        if !dir.is_dir() {
            continue;
        }

        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("cannot read {}: {err}", dir.display());
                continue;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }

            if let Some(app) = parse_desktop_file(&path) {
                apps.push(app);
            }
        }
    }

    // Deduplicate by desktop_id (keep first occurrence)
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.desktop_id.clone()));

    // Sort alphabetically by name (case-insensitive)
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    apps
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DESKTOP: &str = "\
[Desktop Entry]
Name=Firefox
Exec=firefox %u
Icon=firefox
Type=Application
Keywords=browser;web;internet;
";

    #[test]
    fn parse_sample_desktop_file_produces_correct_entry() {
        let path = Path::new("/usr/share/applications/firefox.desktop");
        let entry = parse_desktop_content(SAMPLE_DESKTOP, path).expect("should parse");

        assert_eq!(entry.name, "Firefox");
        assert_eq!(entry.exec, "firefox");
        assert_eq!(entry.icon, "firefox");
        assert_eq!(entry.desktop_id, "firefox");
        assert_eq!(entry.keywords, vec!["browser", "web", "internet"]);
    }

    #[test]
    fn no_display_entry_is_skipped() {
        let content = "\
[Desktop Entry]
Name=Hidden
Exec=hidden
Type=Application
NoDisplay=true
";
        let result = parse_desktop_content(content, Path::new("hidden.desktop"));
        assert!(result.is_none());
    }

    #[test]
    fn non_application_type_is_skipped() {
        let content = "\
[Desktop Entry]
Name=Some Link
Exec=xdg-open http://example.com
Type=Link
";
        let result = parse_desktop_content(content, Path::new("link.desktop"));
        assert!(result.is_none());
    }

    #[test]
    fn missing_name_returns_none() {
        let content = "\
[Desktop Entry]
Exec=something
Type=Application
";
        let result = parse_desktop_content(content, Path::new("no-name.desktop"));
        assert!(result.is_none());
    }

    #[test]
    fn missing_exec_returns_none() {
        let content = "\
[Desktop Entry]
Name=No Exec
Type=Application
";
        let result = parse_desktop_content(content, Path::new("no-exec.desktop"));
        assert!(result.is_none());
    }

    #[test]
    fn strip_field_codes_removes_percent_codes() {
        assert_eq!(strip_field_codes("firefox %u"), "firefox");
        assert_eq!(strip_field_codes("nautilus %F"), "nautilus");
        assert_eq!(
            strip_field_codes("code --new-window %F"),
            "code --new-window"
        );
        assert_eq!(strip_field_codes("no-codes"), "no-codes");
    }

    #[test]
    fn desktop_dirs_returns_non_empty() {
        // Even without XDG_DATA_DIRS, the fallback should produce paths
        let dirs = desktop_dirs();
        assert!(!dirs.is_empty());
    }

    #[test]
    fn keywords_empty_when_missing() {
        let content = "\
[Desktop Entry]
Name=Simple
Exec=simple
Type=Application
";
        let entry =
            parse_desktop_content(content, Path::new("simple.desktop")).expect("should parse");
        assert!(entry.keywords.is_empty());
    }

    #[test]
    fn only_desktop_entry_section_is_parsed() {
        let content = "\
[Desktop Entry]
Name=RealApp
Exec=realapp
Type=Application

[Desktop Action NewWindow]
Name=Overridden
Exec=overridden
";
        let entry =
            parse_desktop_content(content, Path::new("multi.desktop")).expect("should parse");
        assert_eq!(entry.name, "RealApp");
        assert_eq!(entry.exec, "realapp");
    }
}
