/// Desktop file parser for Shoal.
///
/// Reads `.desktop` files from standard XDG directories and extracts the
/// fields needed by the dock: name, exec command, icon name, and desktop ID.
/// Malformed entries are silently skipped with a warning log.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed representation of a `.desktop` file entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopEntry {
    /// Human-readable application name.
    pub name: String,
    /// Command to launch the application.
    pub exec: String,
    /// Icon name or path for the application.
    pub icon: String,
    /// Filename without the `.desktop` extension, used as a stable identifier.
    pub desktop_id: String,
}

/// Load all valid desktop entries from standard XDG data directories.
///
/// Searches `/usr/share/applications/` and `~/.local/share/applications/`
/// (plus any directories listed in `$XDG_DATA_DIRS`). Entries with
/// `NoDisplay=true` are excluded.
pub fn load_desktop_entries() -> Vec<DesktopEntry> {
    let dirs = desktop_dirs();
    let mut entries = Vec::new();

    for dir in &dirs {
        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                tracing::debug!("skipping directory {}: {e}", dir.display());
                continue;
            }
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }

            match parse_desktop_file(&path) {
                Ok(Some(de)) => entries.push(de),
                Ok(None) => {} // NoDisplay=true or missing required fields
                Err(e) => {
                    tracing::warn!("skipping {}: {e}", path.display());
                }
            }
        }
    }

    entries
}

/// Return the list of directories to scan for `.desktop` files.
fn desktop_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // XDG_DATA_DIRS (colon-separated), falling back to /usr/share
    if let Ok(xdg) = std::env::var("XDG_DATA_DIRS") {
        for part in xdg.split(':') {
            let p = PathBuf::from(part).join("applications");
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        }
    } else {
        dirs.push(PathBuf::from("/usr/share/applications"));
        dirs.push(PathBuf::from("/usr/local/share/applications"));
    }

    // User-local applications directory
    if let Ok(home) = std::env::var("HOME") {
        let local = PathBuf::from(home).join(".local/share/applications");
        if !dirs.contains(&local) {
            dirs.push(local);
        }
    }

    dirs
}

/// Parse a single `.desktop` file, returning `None` if the entry should be
/// hidden (`NoDisplay=true`) or is missing required fields.
fn parse_desktop_file(path: &Path) -> anyhow::Result<Option<DesktopEntry>> {
    let content = std::fs::read_to_string(path)?;
    parse_desktop_content(&content, path)
}

/// Parse the textual content of a `.desktop` file. The `path` is used only
/// to derive the `desktop_id`.
fn parse_desktop_content(content: &str, path: &Path) -> anyhow::Result<Option<DesktopEntry>> {
    let fields = parse_desktop_fields(content);

    // Skip entries explicitly hidden from menus
    if fields.get("NoDisplay").map(|v| v.as_str()) == Some("true") {
        return Ok(None);
    }

    let name = match fields.get("Name") {
        Some(n) if !n.is_empty() => n.clone(),
        _ => return Ok(None),
    };

    let exec = match fields.get("Exec") {
        Some(e) if !e.is_empty() => clean_exec(e),
        _ => return Ok(None),
    };

    let icon = fields.get("Icon").cloned().unwrap_or_default();

    let desktop_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    if desktop_id.is_empty() {
        return Ok(None);
    }

    Ok(Some(DesktopEntry {
        name,
        exec,
        icon,
        desktop_id,
    }))
}

/// Extract key=value pairs from the `[Desktop Entry]` section.
/// Ignores lines outside that section, comments, and malformed lines.
fn parse_desktop_fields(content: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let mut in_section = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') {
            in_section = trimmed == "[Desktop Entry]";
            continue;
        }

        if !in_section || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            fields.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    fields
}

/// Strip desktop-entry field codes (%f, %F, %u, %U, etc.) from an Exec value
/// so the command can be launched directly.
fn clean_exec(exec: &str) -> String {
    let mut result = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            // Skip the field code character that follows
            chars.next();
        } else {
            result.push(ch);
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_desktop() -> &'static str {
        "[Desktop Entry]\n\
         Name=Firefox\n\
         Exec=firefox %u\n\
         Icon=firefox\n\
         Type=Application\n\
         Categories=Network;WebBrowser;\n"
    }

    #[test]
    fn parse_valid_desktop_entry() {
        let path = PathBuf::from("/tmp/firefox.desktop");
        let result = parse_desktop_content(sample_desktop(), &path)
            .expect("parse should not error")
            .expect("should return Some");

        assert_eq!(result.name, "Firefox");
        assert_eq!(result.exec, "firefox");
        assert_eq!(result.icon, "firefox");
        assert_eq!(result.desktop_id, "firefox");
    }

    #[test]
    fn parse_nodisplay_entry_returns_none() {
        let content = "[Desktop Entry]\n\
                        Name=Hidden App\n\
                        Exec=hidden\n\
                        Icon=hidden\n\
                        NoDisplay=true\n";
        let path = PathBuf::from("/tmp/hidden.desktop");
        let result = parse_desktop_content(content, &path).expect("parse should not error");
        assert!(result.is_none());
    }

    #[test]
    fn parse_missing_name_returns_none() {
        let content = "[Desktop Entry]\n\
                        Exec=something\n\
                        Icon=something\n";
        let path = PathBuf::from("/tmp/noname.desktop");
        let result = parse_desktop_content(content, &path).expect("parse should not error");
        assert!(result.is_none());
    }

    #[test]
    fn parse_missing_exec_returns_none() {
        let content = "[Desktop Entry]\n\
                        Name=No Exec App\n\
                        Icon=something\n";
        let path = PathBuf::from("/tmp/noexec.desktop");
        let result = parse_desktop_content(content, &path).expect("parse should not error");
        assert!(result.is_none());
    }

    #[test]
    fn parse_malformed_desktop_no_panic() {
        // Completely invalid content
        let content = "this is not a desktop file at all\n\
                        garbage = in = here\n\
                        ====\n";
        let path = PathBuf::from("/tmp/malformed.desktop");
        let result = parse_desktop_content(content, &path).expect("should not error");
        // Missing [Desktop Entry] section means no fields found
        assert!(result.is_none());
    }

    #[test]
    fn parse_empty_file_no_panic() {
        let path = PathBuf::from("/tmp/empty.desktop");
        let result = parse_desktop_content("", &path).expect("should not error");
        assert!(result.is_none());
    }

    #[test]
    fn clean_exec_strips_field_codes() {
        assert_eq!(clean_exec("firefox %u"), "firefox");
        assert_eq!(clean_exec("code %F --new-window"), "code  --new-window");
        assert_eq!(clean_exec("app"), "app");
        assert_eq!(clean_exec("app %f %u %U"), "app");
    }

    #[test]
    fn parse_ignores_other_sections() {
        let content = "[Desktop Entry]\n\
                        Name=App\n\
                        Exec=app\n\
                        Icon=app\n\
                        \n\
                        [Desktop Action NewWindow]\n\
                        Name=New Window\n\
                        Exec=app --new-window\n";
        let path = PathBuf::from("/tmp/multi-section.desktop");
        let result = parse_desktop_content(content, &path)
            .expect("parse should not error")
            .expect("should return Some");

        // Should only pick up the [Desktop Entry] values
        assert_eq!(result.name, "App");
        assert_eq!(result.exec, "app");
    }

    #[test]
    fn parse_with_extra_whitespace() {
        let content = "[Desktop Entry]\n\
                        Name = Spaced App \n\
                        Exec = spaced-app \n\
                        Icon = spaced-icon \n";
        let path = PathBuf::from("/tmp/spaced.desktop");
        let result = parse_desktop_content(content, &path)
            .expect("parse should not error")
            .expect("should return Some");

        assert_eq!(result.name, "Spaced App");
        assert_eq!(result.exec, "spaced-app");
        assert_eq!(result.icon, "spaced-icon");
    }

    #[test]
    fn parse_missing_icon_gives_empty_string() {
        let content = "[Desktop Entry]\n\
                        Name=No Icon\n\
                        Exec=noicon\n";
        let path = PathBuf::from("/tmp/noicon.desktop");
        let result = parse_desktop_content(content, &path)
            .expect("parse should not error")
            .expect("should return Some");

        assert_eq!(result.icon, "");
    }
}
