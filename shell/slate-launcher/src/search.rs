// Fuzzy search for the app launcher.
//
// Provides a simple but effective fuzzy matching algorithm that checks whether
// all characters in the query appear in order in the target. Scoring favours
// matches at word boundaries, at the start of the string, and where matched
// characters are close together.

use crate::apps::AppEntry;

/// Attempt a fuzzy match of `query` against `target`.
///
/// Returns `Some(score)` if every character in `query` appears (in order,
/// case-insensitive) in `target`, or `None` if the query does not match.
///
/// Higher scores indicate better matches. Scoring heuristics:
/// - +10 for each character matched at the start of a word boundary
/// - +15 for a match at position 0 (start of string)
/// - +5 base per matched character
/// - penalty of -1 for each gap between consecutive matched positions
pub fn fuzzy_match(query: &str, target: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }

    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    let target_lower: Vec<char> = target.to_lowercase().chars().collect();
    let target_chars: Vec<char> = target.chars().collect();

    if query_lower.len() > target_lower.len() {
        return None;
    }

    // Exact match gets a large bonus
    if query.to_lowercase() == target.to_lowercase() {
        return Some(1000);
    }

    let mut score: i32 = 0;
    let mut qi = 0; // index into query
    let mut last_match_pos: Option<usize> = None;

    for (ti, &tc) in target_lower.iter().enumerate() {
        if qi < query_lower.len() && tc == query_lower[qi] {
            // Base score per matched character
            score += 5;

            // Bonus: match at start of string
            if ti == 0 {
                score += 15;
            }

            // Bonus: match at word boundary (start, or preceded by space/punctuation)
            if ti == 0
                || !target_chars[ti - 1].is_alphanumeric()
                || (target_chars[ti - 1].is_lowercase() && target_chars[ti].is_uppercase())
            {
                score += 10;
            }

            // Penalty: gap between consecutive matches
            if let Some(prev) = last_match_pos {
                let gap = ti - prev - 1;
                score -= gap as i32;
            }

            last_match_pos = Some(ti);
            qi += 1;
        }
    }

    // All query characters must have matched
    if qi < query_lower.len() {
        return None;
    }

    // Floor at 1 so a valid match never returns 0 (which looks like "empty query")
    Some(score.max(1) as u32)
}

/// Determine whether `query` is a shell command (starts with `>`).
pub fn is_shell_command(query: &str) -> bool {
    query.starts_with('>')
}

/// Extract the shell command from a `>` prefixed query.
pub fn shell_command(query: &str) -> Option<&str> {
    if is_shell_command(query) {
        let cmd = query[1..].trim_start();
        if cmd.is_empty() {
            None
        } else {
            Some(cmd)
        }
    } else {
        None
    }
}

/// Search apps by fuzzy matching the query against app names and keywords.
///
/// Returns references to matching apps sorted by score (best first).
/// If the query is empty, returns all apps in their original order.
pub fn search_apps<'a>(query: &str, apps: &'a [AppEntry]) -> Vec<&'a AppEntry> {
    if query.is_empty() {
        return apps.iter().collect();
    }

    let mut scored: Vec<(&AppEntry, u32)> = apps
        .iter()
        .filter_map(|app| {
            // Try matching against name first
            let name_score = fuzzy_match(query, &app.name);

            // Also try matching against keywords
            let keyword_score = app
                .keywords
                .iter()
                .filter_map(|kw| fuzzy_match(query, kw))
                .max();

            // Also try matching against desktop_id
            let id_score = fuzzy_match(query, &app.desktop_id);

            // Take the best score across all fields
            let best = [name_score, keyword_score, id_score]
                .into_iter()
                .flatten()
                .max()?;

            Some((app, best))
        })
        .collect();

    // Sort by score descending
    scored.sort_by(|a, b| b.1.cmp(&a.1));

    scored.into_iter().map(|(app, _)| app).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app(name: &str, keywords: &[&str]) -> AppEntry {
        AppEntry {
            name: name.to_string(),
            exec: name.to_lowercase(),
            icon: String::new(),
            desktop_id: name.to_lowercase().replace(' ', "-"),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn exact_match_scores_highest() {
        let exact = fuzzy_match("Firefox", "Firefox").expect("should match");
        let partial = fuzzy_match("fire", "Firefox").expect("should match");

        assert!(
            exact > partial,
            "exact ({exact}) should beat partial ({partial})"
        );
    }

    #[test]
    fn fuzzy_ff_matches_firefox() {
        let score = fuzzy_match("ff", "Firefox");
        assert!(score.is_some(), "ff should match Firefox");
    }

    #[test]
    fn shell_command_prefix_detected() {
        assert!(is_shell_command(">ls -la"));
        assert!(is_shell_command(">"));
        assert!(!is_shell_command("ls -la"));
        assert!(!is_shell_command(""));
    }

    #[test]
    fn shell_command_extraction() {
        assert_eq!(shell_command(">ls -la"), Some("ls -la"));
        assert_eq!(shell_command("> echo hello"), Some("echo hello"));
        assert_eq!(shell_command(">"), None);
        assert_eq!(shell_command("> "), None);
        assert_eq!(shell_command("ls"), None);
    }

    #[test]
    fn no_match_returns_none() {
        let score = fuzzy_match("xyz", "Firefox");
        assert!(score.is_none());
    }

    #[test]
    fn empty_query_matches_everything() {
        let score = fuzzy_match("", "anything");
        assert_eq!(score, Some(0));
    }

    #[test]
    fn case_insensitive_matching() {
        let score = fuzzy_match("firefox", "Firefox");
        assert!(score.is_some());
    }

    #[test]
    fn search_apps_returns_all_on_empty_query() {
        let apps = vec![make_app("Zulu", &[]), make_app("Alpha", &[])];
        let results = search_apps("", &apps);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_apps_filters_by_name() {
        let apps = vec![
            make_app("Firefox", &["browser"]),
            make_app("Files", &["nautilus"]),
            make_app("Calculator", &["math"]),
        ];

        let results = search_apps("fire", &apps);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Firefox");
    }

    #[test]
    fn search_apps_matches_keywords() {
        let apps = vec![
            make_app("Firefox", &["browser", "web"]),
            make_app("Nautilus", &["files", "manager"]),
        ];

        let results = search_apps("browser", &apps);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Firefox");
    }

    #[test]
    fn query_longer_than_target_returns_none() {
        assert!(fuzzy_match("a very long query", "ab").is_none());
    }

    #[test]
    fn word_boundary_bonus_produces_higher_score() {
        // "te" matching "Text Editor" (word boundaries) vs "testy" (consecutive)
        let boundary = fuzzy_match("te", "Text Editor").expect("should match");
        let inner = fuzzy_match("te", "abctefgh").expect("should match");

        assert!(
            boundary > inner,
            "word-boundary match ({boundary}) should beat inner match ({inner})"
        );
    }
}
