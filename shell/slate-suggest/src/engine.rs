/// Suggestion engine for the keyboard suggestion bar.
///
/// Combines history-based prefix matching with optional LLM completions
/// and static command suggestions. Scoring accounts for recency, frequency,
/// and prefix match quality.
use std::collections::HashMap;

/// A single suggestion to present in the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub text: String,
    pub source: SuggestionSource,
    pub score: f64,
}

/// Where a suggestion originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionSource {
    /// Matched from shell history.
    History,
    /// Completed by a local LLM.
    Llm,
    /// Hardcoded common commands.
    Static,
}

/// Hardcoded common commands shown when input is empty.
const STATIC_COMMANDS: &[&str] = &[
    "ls",
    "cd",
    "git status",
    "git log",
    "git diff",
    "cargo build",
    "cargo test",
    "cargo run",
    "pwd",
    "cat",
    "grep",
    "find",
];

/// Generate suggestions from shell history using prefix matching.
///
/// The `history` slice should be ordered most-recent-first (as returned
/// by [`crate::history::load_history`]). Scoring combines:
///   - **Recency** — newer entries score higher (position bonus).
///   - **Frequency** — commands that appear more often in history score higher.
///   - **Prefix match length** — longer input matches score higher.
///
/// Returns at most `limit` suggestions, sorted by descending score.
pub fn suggest_from_history(input: &str, history: &[String], limit: usize) -> Vec<Suggestion> {
    let input_lower = input.to_lowercase();

    if input_lower.is_empty() {
        return static_suggestions(limit);
    }

    // Count frequency of each command across the full history.
    let frequency = count_frequency(history);
    let max_freq = frequency.values().copied().max().unwrap_or(1) as f64;
    let history_len = history.len() as f64;

    let mut suggestions: Vec<Suggestion> = history
        .iter()
        .enumerate()
        .filter(|(_, cmd)| cmd.to_lowercase().starts_with(&input_lower))
        .map(|(index, cmd)| {
            let score = compute_score(
                index,
                history_len,
                *frequency.get(cmd.as_str()).unwrap_or(&1),
                max_freq,
                input.len(),
                cmd.len(),
            );
            Suggestion {
                text: cmd.clone(),
                source: SuggestionSource::History,
                score,
            }
        })
        .collect();

    // Sort by score descending, then alphabetically for stability.
    suggestions.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.text.cmp(&b.text))
    });

    // Deduplicate by text (history may still contain near-duplicates with
    // different casing after the case-insensitive prefix match).
    dedup_by_text(&mut suggestions);

    suggestions.truncate(limit);
    suggestions
}

/// Generate static suggestions for empty input.
pub fn static_suggestions(limit: usize) -> Vec<Suggestion> {
    STATIC_COMMANDS
        .iter()
        .take(limit)
        .enumerate()
        .map(|(i, cmd)| Suggestion {
            text: cmd.to_string(),
            source: SuggestionSource::Static,
            // Higher score for commands listed first
            score: 1.0 - (i as f64 * 0.01),
        })
        .collect()
}

/// Create a suggestion from an LLM completion.
///
/// The LLM suggestion gets a fixed high score so it appears prominently,
/// but below a very strong history match.
pub fn llm_suggestion(text: String) -> Suggestion {
    Suggestion {
        text,
        source: SuggestionSource::Llm,
        score: 0.85,
    }
}

/// Merge history suggestions with an optional LLM suggestion.
///
/// The LLM suggestion is inserted into the list by score, and the total
/// count is capped at `limit`.
pub fn merge_suggestions(
    mut history_suggestions: Vec<Suggestion>,
    llm: Option<Suggestion>,
    limit: usize,
) -> Vec<Suggestion> {
    if let Some(llm_sugg) = llm {
        // Avoid duplicating a suggestion the history already provides.
        let already_present = history_suggestions.iter().any(|s| s.text == llm_sugg.text);

        if !already_present {
            history_suggestions.push(llm_sugg);
            history_suggestions.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
    }

    history_suggestions.truncate(limit);
    history_suggestions
}

/// Count how many times each command appears in history.
fn count_frequency(history: &[String]) -> HashMap<&str, usize> {
    let mut freq = HashMap::new();
    for cmd in history {
        *freq.entry(cmd.as_str()).or_insert(0) += 1;
    }
    freq
}

/// Compute a suggestion score from multiple factors.
///
/// - `index`: position in history (0 = most recent)
/// - `history_len`: total number of entries
/// - `freq`: how many times this command appears
/// - `max_freq`: the highest frequency in the history
/// - `input_len`: length of the user's typed prefix
/// - `cmd_len`: length of the matched command
fn compute_score(
    index: usize,
    history_len: f64,
    freq: usize,
    max_freq: f64,
    input_len: usize,
    cmd_len: usize,
) -> f64 {
    // Recency: 1.0 for most recent, decays towards 0.0
    let recency = if history_len > 0.0 {
        1.0 - (index as f64 / history_len)
    } else {
        0.5
    };

    // Frequency: normalised to 0.0-1.0
    let frequency = freq as f64 / max_freq;

    // Prefix coverage: how much of the command the user has already typed.
    // Shorter completions (closer to what the user typed) score slightly higher.
    let coverage = if cmd_len > 0 {
        input_len as f64 / cmd_len as f64
    } else {
        0.0
    };

    // Weighted combination
    recency * 0.5 + frequency * 0.3 + coverage * 0.2
}

/// Remove duplicate suggestions by text, keeping the first (highest-scored).
fn dedup_by_text(suggestions: &mut Vec<Suggestion>) {
    let mut seen = std::collections::HashSet::new();
    suggestions.retain(|s| seen.insert(s.text.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_history() -> Vec<String> {
        // Most recent first
        vec![
            "git status".to_string(),
            "cargo test".to_string(),
            "git log --oneline".to_string(),
            "cargo build".to_string(),
            "git status".to_string(), // duplicate
            "ls -la".to_string(),
            "cd /tmp".to_string(),
            "git commit -m 'fix'".to_string(),
        ]
    }

    #[test]
    fn prefix_match_returns_correct_suggestions() {
        let history = sample_history();
        let results = suggest_from_history("git", &history, 10);

        // All results should start with "git"
        for s in &results {
            assert!(
                s.text.starts_with("git"),
                "expected prefix 'git', got {:?}",
                s.text
            );
        }

        // Should find all git commands
        let texts: Vec<&str> = results.iter().map(|s| s.text.as_str()).collect();
        assert!(texts.contains(&"git status"));
        assert!(texts.contains(&"git log --oneline"));
        assert!(texts.contains(&"git commit -m 'fix'"));
    }

    #[test]
    fn empty_input_returns_static_suggestions() {
        let history = sample_history();
        let results = suggest_from_history("", &history, 5);

        assert_eq!(results.len(), 5);
        for s in &results {
            assert_eq!(s.source, SuggestionSource::Static);
        }
    }

    #[test]
    fn suggestions_sorted_by_score() {
        let history = sample_history();
        let results = suggest_from_history("git", &history, 10);

        // Verify descending score order
        for window in results.windows(2) {
            assert!(
                window[0].score >= window[1].score,
                "scores not in descending order: {} >= {} failed",
                window[0].score,
                window[1].score,
            );
        }
    }

    #[test]
    fn limit_is_respected() {
        let history = sample_history();
        let results = suggest_from_history("git", &history, 2);
        assert!(results.len() <= 2);
    }

    #[test]
    fn no_match_returns_empty() {
        let history = sample_history();
        let results = suggest_from_history("zzz_nonexistent", &history, 10);
        assert!(results.is_empty());
    }

    #[test]
    fn case_insensitive_prefix_match() {
        let history = vec!["Git Status".to_string(), "git log".to_string()];
        let results = suggest_from_history("git", &history, 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn static_suggestions_have_descending_scores() {
        let statics = static_suggestions(10);
        for window in statics.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn llm_suggestion_has_correct_source() {
        let s = llm_suggestion("git push origin main".to_string());
        assert_eq!(s.source, SuggestionSource::Llm);
        assert_eq!(s.text, "git push origin main");
    }

    #[test]
    fn merge_inserts_llm_suggestion() {
        let history_suggs = vec![
            Suggestion {
                text: "git status".to_string(),
                source: SuggestionSource::History,
                score: 0.9,
            },
            Suggestion {
                text: "git log".to_string(),
                source: SuggestionSource::History,
                score: 0.7,
            },
        ];
        let llm = llm_suggestion("git push".to_string());
        let merged = merge_suggestions(history_suggs, Some(llm), 10);

        assert_eq!(merged.len(), 3);
        // LLM suggestion (0.85) should be between 0.9 and 0.7
        assert_eq!(merged[1].text, "git push");
        assert_eq!(merged[1].source, SuggestionSource::Llm);
    }

    #[test]
    fn merge_deduplicates_llm_with_history() {
        let history_suggs = vec![Suggestion {
            text: "git status".to_string(),
            source: SuggestionSource::History,
            score: 0.9,
        }];
        let llm = llm_suggestion("git status".to_string());
        let merged = merge_suggestions(history_suggs, Some(llm), 10);

        // Should not have two "git status" entries
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn merge_without_llm() {
        let history_suggs = vec![Suggestion {
            text: "ls".to_string(),
            source: SuggestionSource::History,
            score: 0.9,
        }];
        let merged = merge_suggestions(history_suggs.clone(), None, 10);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn recency_boosts_newer_commands() {
        // "cargo test" is index 0 (most recent), "cargo build" is index 1
        let history = vec!["cargo test".to_string(), "cargo build".to_string()];
        let results = suggest_from_history("cargo", &history, 10);

        assert!(results.len() >= 2);
        let test_score = results
            .iter()
            .find(|s| s.text == "cargo test")
            .unwrap()
            .score;
        let build_score = results
            .iter()
            .find(|s| s.text == "cargo build")
            .unwrap()
            .score;
        assert!(
            test_score > build_score,
            "more recent 'cargo test' ({test_score}) should score higher than 'cargo build' ({build_score})"
        );
    }

    #[test]
    fn frequency_boosts_repeated_commands() {
        // "git status" appears 3 times, "git log" appears once
        let history = vec![
            "git status".to_string(),
            "git log".to_string(),
            "git status".to_string(),
            "git status".to_string(),
        ];
        let results = suggest_from_history("git", &history, 10);

        let status_score = results
            .iter()
            .find(|s| s.text == "git status")
            .unwrap()
            .score;
        let log_score = results.iter().find(|s| s.text == "git log").unwrap().score;
        assert!(
            status_score > log_score,
            "frequent 'git status' ({status_score}) should score higher than 'git log' ({log_score})"
        );
    }
}
