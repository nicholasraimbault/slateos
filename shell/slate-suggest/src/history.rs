/// Shell history loading for suggestion bar.
///
/// Reads command history from bash, zsh, and fish shell history files,
/// deduplicates entries (most recent first), and returns them for use
/// by the suggestion engine.
use std::collections::HashSet;
use std::path::PathBuf;

/// Maximum number of unique history entries to keep.
const MAX_ENTRIES: usize = 1000;

/// Return the path to `~/.bash_history`.
fn bash_history_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".bash_history"))
}

/// Return the path to `~/.zsh_history`.
fn zsh_history_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".zsh_history"))
}

/// Return the path to `~/.local/share/fish/fish_history`.
fn fish_history_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".local/share/fish/fish_history"))
}

/// Parse a single line from bash history.
///
/// Bash history is plain text, one command per line.
pub fn parse_bash_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() <= 1 {
        return None;
    }
    Some(trimmed.to_string())
}

/// Parse a single line from zsh extended history format.
///
/// Zsh extended history lines look like: `: timestamp:0;command`
/// Plain zsh history is just the command per line.
pub fn parse_zsh_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Handle zsh extended format: `: timestamp:0;command`
    let command = if trimmed.starts_with(": ") {
        // Find the semicolon that separates metadata from command
        trimmed.find(';').map(|pos| &trimmed[pos + 1..])
    } else {
        Some(trimmed)
    };

    let command = command?.trim();
    if command.is_empty() || command.len() <= 1 {
        return None;
    }
    Some(command.to_string())
}

/// Parse fish history content and extract commands.
///
/// Fish history format uses YAML-like entries:
/// ```text
/// - cmd: some command
///   when: 1234567890
/// ```
pub fn parse_fish_history(content: &str) -> Vec<String> {
    content.lines().filter_map(parse_fish_line).collect()
}

/// Parse a single line from fish history.
///
/// Fish history command lines look like: `- cmd: command`
pub fn parse_fish_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let command = trimmed.strip_prefix("- cmd: ")?;
    let command = command.trim();
    if command.is_empty() || command.len() <= 1 {
        return None;
    }
    Some(command.to_string())
}

/// Read and parse a bash history file, returning commands in file order.
fn load_bash_history() -> Vec<String> {
    let path = match bash_history_path() {
        Some(p) => p,
        None => return Vec::new(),
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => content.lines().filter_map(parse_bash_line).collect(),
        Err(err) => {
            tracing::debug!("could not read bash history: {err}");
            Vec::new()
        }
    }
}

/// Read and parse a zsh history file, returning commands in file order.
fn load_zsh_history() -> Vec<String> {
    let path = match zsh_history_path() {
        Some(p) => p,
        None => return Vec::new(),
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => content.lines().filter_map(parse_zsh_line).collect(),
        Err(err) => {
            tracing::debug!("could not read zsh history: {err}");
            Vec::new()
        }
    }
}

/// Read and parse a fish history file, returning commands in file order.
fn load_fish_history() -> Vec<String> {
    let path = match fish_history_path() {
        Some(p) => p,
        None => return Vec::new(),
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => parse_fish_history(&content),
        Err(err) => {
            tracing::debug!("could not read fish history: {err}");
            Vec::new()
        }
    }
}

/// Load shell history from all supported shells.
///
/// Returns deduplicated commands with most recent first. Later entries
/// (which are more recent) take precedence during deduplication.
/// Limited to the last [`MAX_ENTRIES`] unique entries.
pub fn load_history() -> Vec<String> {
    let mut all_commands = Vec::new();

    // Load from each shell; order does not matter since we deduplicate
    // and entries later in the combined list are treated as more recent.
    all_commands.extend(load_bash_history());
    all_commands.extend(load_zsh_history());
    all_commands.extend(load_fish_history());

    deduplicate(all_commands)
}

/// Load history from raw content strings (for testing or custom sources).
pub fn load_history_from_parts(
    bash_content: Option<&str>,
    zsh_content: Option<&str>,
    fish_content: Option<&str>,
) -> Vec<String> {
    let mut all_commands = Vec::new();

    if let Some(content) = bash_content {
        all_commands.extend(content.lines().filter_map(parse_bash_line));
    }
    if let Some(content) = zsh_content {
        all_commands.extend(content.lines().filter_map(parse_zsh_line));
    }
    if let Some(content) = fish_content {
        all_commands.extend(parse_fish_history(content));
    }

    deduplicate(all_commands)
}

/// Deduplicate commands: keep the last occurrence of each command (most recent),
/// return in most-recent-first order, limited to [`MAX_ENTRIES`].
fn deduplicate(commands: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique: Vec<String> = Vec::new();

    // Iterate in reverse so the last (most recent) occurrence wins.
    for cmd in commands.into_iter().rev() {
        if seen.insert(cmd.clone()) {
            unique.push(cmd);
        }
    }

    // unique is already in most-recent-first order from the reverse iteration
    unique.truncate(MAX_ENTRIES);
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bash_line_simple() {
        assert_eq!(parse_bash_line("ls -la"), Some("ls -la".to_string()));
    }

    #[test]
    fn parse_bash_line_filters_single_char() {
        assert_eq!(parse_bash_line("x"), None);
        assert_eq!(parse_bash_line("a"), None);
    }

    #[test]
    fn parse_bash_line_filters_empty() {
        assert_eq!(parse_bash_line(""), None);
        assert_eq!(parse_bash_line("   "), None);
    }

    #[test]
    fn parse_bash_history_format() {
        let content = "ls\ncd /tmp\ngit status\npwd\n";
        let commands: Vec<String> = content.lines().filter_map(parse_bash_line).collect();
        assert_eq!(commands, vec!["ls", "cd /tmp", "git status", "pwd"]);
    }

    #[test]
    fn parse_zsh_line_plain() {
        assert_eq!(
            parse_zsh_line("git commit -m 'hello'"),
            Some("git commit -m 'hello'".to_string())
        );
    }

    #[test]
    fn parse_zsh_line_extended_format() {
        assert_eq!(
            parse_zsh_line(": 1700000000:0;cargo build"),
            Some("cargo build".to_string())
        );
    }

    #[test]
    fn parse_zsh_line_extended_with_spaces() {
        assert_eq!(
            parse_zsh_line(": 1700000000:0;git log --oneline"),
            Some("git log --oneline".to_string())
        );
    }

    #[test]
    fn parse_zsh_extended_history_format() {
        let content = ": 1700000001:0;ls -la\n: 1700000002:0;cd /home\n: 1700000003:0;git status\n";
        let commands: Vec<String> = content.lines().filter_map(parse_zsh_line).collect();
        assert_eq!(commands, vec!["ls -la", "cd /home", "git status"]);
    }

    #[test]
    fn parse_zsh_line_filters_single_char_command() {
        assert_eq!(parse_zsh_line(": 1700000000:0;x"), None);
    }

    #[test]
    fn parse_fish_line_valid() {
        assert_eq!(
            parse_fish_line("- cmd: cargo test"),
            Some("cargo test".to_string())
        );
    }

    #[test]
    fn parse_fish_line_non_cmd_line() {
        assert_eq!(parse_fish_line("  when: 1700000000"), None);
    }

    #[test]
    fn parse_fish_line_filters_single_char() {
        assert_eq!(parse_fish_line("- cmd: x"), None);
    }

    #[test]
    fn parse_fish_history_format() {
        let content = "\
- cmd: ls -la
  when: 1700000001
- cmd: cd /tmp
  when: 1700000002
- cmd: git status
  when: 1700000003
";
        let commands = parse_fish_history(content);
        assert_eq!(commands, vec!["ls -la", "cd /tmp", "git status"]);
    }

    #[test]
    fn deduplication_keeps_most_recent() {
        let commands = vec![
            "ls".to_string(),
            "cd /tmp".to_string(),
            "ls".to_string(), // more recent duplicate
        ];
        let result = deduplicate(commands);
        // "ls" should appear once, and since it was last (most recent), it
        // should come first in the most-recent-first output.
        assert_eq!(result, vec!["ls", "cd /tmp"]);
    }

    #[test]
    fn deduplication_respects_limit() {
        let commands: Vec<String> = (0..2000).map(|i| format!("command_{i}")).collect();
        let result = deduplicate(commands);
        assert_eq!(result.len(), MAX_ENTRIES);
        // Most recent commands should be first
        assert_eq!(result[0], "command_1999");
    }

    #[test]
    fn load_history_from_parts_combines_sources() {
        let bash = "ls\ngit status\n";
        let zsh = ": 1700000000:0;cargo build\n: 1700000001:0;ls\n";
        let fish =
            "- cmd: npm install\n  when: 1700000000\n- cmd: cargo build\n  when: 1700000001\n";

        let result = load_history_from_parts(Some(bash), Some(zsh), Some(fish));

        // "cargo build" from fish is more recent than from zsh, so it should
        // appear once. "ls" from zsh is more recent than from bash.
        assert!(result.contains(&"git status".to_string()));
        assert!(result.contains(&"cargo build".to_string()));
        assert!(result.contains(&"npm install".to_string()));
        assert!(result.contains(&"ls".to_string()));

        // No duplicates
        let unique_count = result.iter().collect::<HashSet<_>>().len();
        assert_eq!(unique_count, result.len());
    }

    #[test]
    fn load_history_from_parts_handles_empty() {
        let result = load_history_from_parts(None, None, None);
        assert!(result.is_empty());
    }
}
