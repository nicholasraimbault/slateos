// Many items are only used on Linux (layer-shell path) or in tests.
#![allow(dead_code)]

// slate-suggest -- Keyboard suggestion bar for Slate OS.
//
// Presents a horizontal row of contextual command completions above the
// on-screen keyboard. Sources: shell history (bash/zsh/fish) and an
// optional local LLM. Tapping a chip injects the command into the
// focused application via wtype.

mod bar;
mod dbus_listener;
mod engine;
mod history;
mod inject;
mod llm;

use std::time::Duration;

use anyhow::Result;
use slate_common::Palette;

use crate::bar::BarMessage;
use crate::dbus_listener::SuggestDbusEvent;
use crate::engine::{llm_suggestion, merge_suggestions, suggest_from_history, Suggestion};

/// How often the Tick subscription fires to poll input context (milliseconds).
const TICK_INTERVAL_MS: u64 = 300;

/// Maximum number of suggestions shown in the bar.
const MAX_SUGGESTIONS: usize = 8;

/// Application state for the suggestion bar.
struct SuggestBar {
    /// Text currently being typed in the focused window.
    current_input: String,
    /// Computed suggestions for the current input.
    suggestions: Vec<Suggestion>,
    /// Shell command history, most recent first.
    history: Vec<String>,
    /// Current system palette for theming.
    palette: Palette,
    /// Whether the suggestion bar is visible.
    visible: bool,
    /// Whether LLM suggestions are enabled.
    llm_enabled: bool,
    /// Whether an LLM query is currently in-flight (prevents stacking).
    llm_pending: bool,
    /// The input text that the in-flight LLM query was issued for.
    /// Used to discard stale results when the user has typed ahead.
    llm_query_input: String,
}

/// Messages that drive the iced application.
#[derive(Debug, Clone)]
enum Message {
    /// A suggestion chip was tapped.
    ChipTapped(String),
    /// The current input text changed (from polling or D-Bus).
    InputChanged(String),
    /// The system palette was updated.
    PaletteChanged(Palette),
    /// An LLM completion arrived for the given input prefix.
    LlmResult {
        input: String,
        completion: Option<String>,
    },
    /// Periodic tick for input polling.
    Tick,
    /// D-Bus event forwarded from the subscription.
    DbusEvent(SuggestDbusEvent),
    /// The bar should become visible.
    Show,
    /// The bar should hide.
    Hide,
}

impl SuggestBar {
    fn new() -> Self {
        let history = history::load_history();
        let palette = Palette::default();
        let suggestions = suggest_from_history("", &history, MAX_SUGGESTIONS);

        Self {
            current_input: String::new(),
            suggestions,
            history,
            palette,
            visible: true,
            llm_enabled: true,
            llm_pending: false,
            llm_query_input: String::new(),
        }
    }

    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::ChipTapped(text) => {
                if let Err(err) = inject::inject_text(&text) {
                    tracing::error!("failed to inject text: {err:#}");
                }
            }
            Message::InputChanged(input) => {
                if input != self.current_input {
                    self.current_input = input.clone();
                    self.refresh_suggestions();

                    // Fire an async LLM query when enabled and input is non-empty.
                    // Only one query is in-flight at a time to avoid flooding.
                    if self.llm_enabled && !input.trim().is_empty() && !self.llm_pending {
                        self.llm_pending = true;
                        self.llm_query_input = input.clone();
                        return iced::Task::perform(
                            async move { (input.clone(), llm::query_llm(&input).await) },
                            |(input, completion)| Message::LlmResult { input, completion },
                        );
                    }
                }
            }
            Message::PaletteChanged(palette) => {
                self.palette = palette;
            }
            Message::LlmResult { input, completion } => {
                self.llm_pending = false;

                // Only apply the result if the user hasn't typed ahead past
                // the query that produced this completion.
                if input == self.current_input {
                    let llm_sugg = completion.map(llm_suggestion);
                    self.suggestions = merge_suggestions(
                        suggest_from_history(
                            &self.current_input,
                            &self.history,
                            MAX_SUGGESTIONS - 1,
                        ),
                        llm_sugg,
                        MAX_SUGGESTIONS,
                    );
                }
            }
            Message::Tick => {
                // Poll input context via Niri IPC. On non-Linux platforms
                // or when niri is unavailable, this is a graceful no-op.
                return iced::Task::perform(poll_input_context(), Message::InputChanged);
            }
            Message::DbusEvent(event) => match event {
                SuggestDbusEvent::PaletteChanged(palette) => {
                    self.palette = palette;
                }
                SuggestDbusEvent::Show => {
                    self.visible = true;
                }
                SuggestDbusEvent::Hide => {
                    self.visible = false;
                }
                SuggestDbusEvent::InputContext(text) => {
                    if text != self.current_input {
                        self.current_input = text.clone();
                        self.refresh_suggestions();

                        if self.llm_enabled && !text.trim().is_empty() && !self.llm_pending {
                            self.llm_pending = true;
                            self.llm_query_input = text.clone();
                            return iced::Task::perform(
                                async move { (text.clone(), llm::query_llm(&text).await) },
                                |(input, completion)| Message::LlmResult { input, completion },
                            );
                        }
                    }
                }
            },
            Message::Show => {
                self.visible = true;
            }
            Message::Hide => {
                self.visible = false;
            }
        }

        iced::Task::none()
    }

    fn refresh_suggestions(&mut self) {
        self.suggestions =
            suggest_from_history(&self.current_input, &self.history, MAX_SUGGESTIONS);
    }

    fn view(&self) -> iced::Element<'_, Message> {
        if !self.visible {
            return iced::widget::Space::new(iced::Length::Fill, iced::Length::Fixed(0.0)).into();
        }

        bar::view(&self.suggestions, &self.palette).map(|msg| match msg {
            BarMessage::ChipTapped(text) => Message::ChipTapped(text),
        })
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let tick =
            iced::time::every(Duration::from_millis(TICK_INTERVAL_MS)).map(|_| Message::Tick);

        let dbus = dbus_subscription();

        iced::Subscription::batch([tick, dbus])
    }

    fn theme(&self) -> iced::Theme {
        slate_common::theme::create_theme(&self.palette)
    }
}

/// Create an iced subscription that forwards D-Bus events into Messages.
///
/// Uses iced::subscription::channel to spawn background tasks that listen
/// for palette changes and visibility signals on the session bus. On
/// non-Linux platforms this returns an empty subscription.
fn dbus_subscription() -> iced::Subscription<Message> {
    #[cfg(target_os = "linux")]
    {
        iced::subscription::channel("slate-suggest-dbus", 16, |mut output| async move {
            use iced::futures::SinkExt;

            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            // Spawn separate listeners for palette and visibility signals.
            let palette_tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = dbus_listener::listen_palette(palette_tx).await {
                    tracing::warn!("palette D-Bus listener failed: {e}");
                }
            });

            let visibility_tx = tx;
            tokio::spawn(async move {
                if let Err(e) = dbus_listener::listen_visibility_signals(visibility_tx).await {
                    tracing::warn!("visibility D-Bus listener failed: {e}");
                }
            });

            // Forward events from the mpsc channel into the iced subscription.
            loop {
                match rx.recv().await {
                    Some(event) => {
                        if output.send(Message::DbusEvent(event)).await.is_err() {
                            tracing::debug!("D-Bus subscription channel closed");
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("all D-Bus senders dropped");
                        break;
                    }
                }
            }

            // Satisfy the never-returning future type by pending forever.
            std::future::pending::<()>().await;
            unreachable!();
        })
    }

    #[cfg(not(target_os = "linux"))]
    {
        iced::Subscription::none()
    }
}

/// Poll the focused window for the current input context.
///
/// Queries niri for the focused window's title, which in terminal
/// emulators typically includes the partial command being typed.
/// Returns the extracted command prefix or an empty string if
/// unavailable.
async fn poll_input_context() -> String {
    // On non-Linux or when niri is unavailable, return empty gracefully.
    poll_input_context_inner().await.unwrap_or_default()
}

/// Inner implementation that returns Result for clean error propagation.
///
/// Uses `niri msg focused-window` to read the active window title. Terminal
/// emulators (Alacritty, foot) put the shell prompt + partial command in
/// their title, which we can use as a lightweight input context signal
/// without needing accessibility APIs.
async fn poll_input_context_inner() -> Result<String> {
    let output = tokio::process::Command::new("niri")
        .args(["msg", "focused-window"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(String::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The focused-window output includes a "title:" line. Parse the last
    // component after common shell prompts ($ or >) as the partial command.
    let title = extract_title_from_niri_output(&stdout);
    let command = extract_command_from_title(&title);

    Ok(command)
}

/// Extract the window title from `niri msg focused-window` output.
///
/// The output format includes lines like `title: "some title here"`.
/// We extract the quoted value.
fn extract_title_from_niri_output(output: &str) -> String {
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("title:") {
            let title = rest.trim().trim_matches('"');
            return title.to_string();
        }
    }
    String::new()
}

/// Extract a partial command from a terminal window title.
///
/// Terminal emulators often set their title to include the prompt and
/// command being typed. We look for common prompt patterns (ending in
/// $ or >) and return everything after the last prompt character.
fn extract_command_from_title(title: &str) -> String {
    // Look for the last shell prompt marker ($ or >) in the title.
    // Everything after it is likely the partial command.
    if let Some(pos) = title.rfind('$') {
        let after = title[pos + 1..].trim();
        return after.to_string();
    }
    if let Some(pos) = title.rfind('>') {
        let after = title[pos + 1..].trim();
        return after.to_string();
    }

    // No prompt marker found; the title may not be from a terminal.
    String::new()
}

// ---------------------------------------------------------------------------
// Layer-shell entry point (Linux / Wayland only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn run_layershell() -> Result<()> {
    use iced_layershell::reexport::{Anchor, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};
    use iced_layershell::Application;

    impl Application for SuggestBar {
        type Message = Message;
        type Flags = ();
        type Theme = iced::Theme;
        type Executor = iced::executor::Default;

        fn new(_flags: ()) -> (Self, iced::Task<Message>) {
            (SuggestBar::new(), iced::Task::none())
        }

        fn namespace(&self) -> String {
            "slate-suggest".to_string()
        }

        fn update(&mut self, message: Message) -> iced::Task<Message> {
            SuggestBar::update(self, message)
        }

        fn view(&self) -> iced::Element<'_, Message, Self::Theme, iced::Renderer> {
            SuggestBar::view(self)
        }

        fn theme(&self) -> iced::Theme {
            SuggestBar::theme(self)
        }

        fn subscription(&self) -> iced::Subscription<Message> {
            SuggestBar::subscription(self)
        }
    }

    impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
        type Error = Self;
        fn try_into(
            self,
        ) -> Result<iced_layershell::actions::LayershellCustomActions, Self::Error> {
            Err(self)
        }
    }

    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some((0, bar::BAR_HEIGHT as u32)),
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            layer: Layer::Overlay,
            exclusive_zone: bar::BAR_HEIGHT as i32,
            ..LayerShellSettings::default()
        },
        ..Settings::default()
    };

    SuggestBar::run(settings).map_err(|e| anyhow::anyhow!("iced_layershell error: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Fallback entry point (macOS / non-Wayland development)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
fn run_fallback() -> Result<()> {
    tracing::info!("iced_layershell not available on this platform, running in headless mode");

    let app = SuggestBar::new();
    tracing::info!(
        "loaded {} history entries, {} initial suggestions",
        app.history.len(),
        app.suggestions.len()
    );

    for s in &app.suggestions {
        tracing::info!(
            "  suggestion: {:?} (score={:.3}, source={:?})",
            s.text,
            s.score,
            s.source
        );
    }

    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    tracing::info!("slate-suggest starting");

    #[cfg(target_os = "linux")]
    {
        run_layershell()?;
    }

    #[cfg(not(target_os = "linux"))]
    {
        run_fallback()?;
    }

    tracing::info!("slate-suggest stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::SuggestionSource;

    #[test]
    fn suggest_bar_new_has_default_state() {
        let bar = SuggestBar::new();
        assert!(bar.current_input.is_empty());
        assert!(bar.visible);
        assert!(bar.llm_enabled);
        assert!(!bar.llm_pending);
    }

    #[test]
    fn suggest_bar_update_chip_tapped() {
        let mut bar = SuggestBar::new();
        // ChipTapped invokes wtype which is not available in tests,
        // but the error path should not panic.
        let _ = bar.update(Message::ChipTapped("ls".to_string()));
    }

    #[test]
    fn suggest_bar_update_input_changed() {
        let mut bar = SuggestBar::new();
        let _ = bar.update(Message::InputChanged("git".to_string()));
        assert_eq!(bar.current_input, "git");
        // All suggestions should start with "git" (or be empty if no history matches).
        for s in &bar.suggestions {
            if s.source == SuggestionSource::History {
                assert!(
                    s.text.to_lowercase().starts_with("git"),
                    "expected prefix 'git', got {:?}",
                    s.text
                );
            }
        }
    }

    #[test]
    fn suggest_bar_update_palette_changed() {
        let mut bar = SuggestBar::new();
        let new_palette = Palette {
            primary: [255, 0, 0, 255],
            ..Palette::default()
        };
        let _ = bar.update(Message::PaletteChanged(new_palette.clone()));
        assert_eq!(bar.palette.primary, [255, 0, 0, 255]);
    }

    #[test]
    fn suggest_bar_update_show_hide() {
        let mut bar = SuggestBar::new();
        let _ = bar.update(Message::Hide);
        assert!(!bar.visible);
        let _ = bar.update(Message::Show);
        assert!(bar.visible);
    }

    #[test]
    fn suggest_bar_update_llm_result_applied() {
        let mut bar = SuggestBar::new();
        bar.current_input = "git".to_string();
        let _ = bar.update(Message::LlmResult {
            input: "git".to_string(),
            completion: Some("git push origin main".to_string()),
        });
        let has_llm = bar
            .suggestions
            .iter()
            .any(|s| s.source == SuggestionSource::Llm);
        assert!(has_llm, "LLM suggestion should be present");
    }

    #[test]
    fn suggest_bar_update_llm_result_stale_discarded() {
        let mut bar = SuggestBar::new();
        // User typed ahead past the LLM query
        bar.current_input = "cargo".to_string();
        let _ = bar.update(Message::LlmResult {
            input: "git".to_string(),
            completion: Some("git push".to_string()),
        });
        // The stale result for "git" should not have added an LLM suggestion
        // because current_input is now "cargo".
        let has_llm = bar
            .suggestions
            .iter()
            .any(|s| s.source == SuggestionSource::Llm);
        assert!(!has_llm, "stale LLM result should be discarded");
    }

    #[test]
    fn suggest_bar_update_dbus_palette_event() {
        let mut bar = SuggestBar::new();
        let palette = Palette {
            primary: [0, 255, 0, 255],
            ..Palette::default()
        };
        let _ = bar.update(Message::DbusEvent(SuggestDbusEvent::PaletteChanged(
            palette,
        )));
        assert_eq!(bar.palette.primary, [0, 255, 0, 255]);
    }

    #[test]
    fn suggest_bar_update_dbus_show_hide() {
        let mut bar = SuggestBar::new();
        let _ = bar.update(Message::DbusEvent(SuggestDbusEvent::Hide));
        assert!(!bar.visible);
        let _ = bar.update(Message::DbusEvent(SuggestDbusEvent::Show));
        assert!(bar.visible);
    }

    #[test]
    fn suggest_bar_update_dbus_input_context() {
        let mut bar = SuggestBar::new();
        let _ = bar.update(Message::DbusEvent(SuggestDbusEvent::InputContext(
            "cargo".to_string(),
        )));
        assert_eq!(bar.current_input, "cargo");
    }

    #[test]
    fn extract_title_from_niri_output_parses_title() {
        let output = "app_id: \"Alacritty\"\ntitle: \"user@host:~ $ git st\"\n";
        let title = extract_title_from_niri_output(output);
        assert_eq!(title, "user@host:~ $ git st");
    }

    #[test]
    fn extract_title_from_niri_output_empty() {
        let title = extract_title_from_niri_output("");
        assert!(title.is_empty());
    }

    #[test]
    fn extract_title_from_niri_output_no_title_line() {
        let output = "app_id: \"firefox\"\n";
        let title = extract_title_from_niri_output(output);
        assert!(title.is_empty());
    }

    #[test]
    fn extract_command_from_title_dollar_prompt() {
        let command = extract_command_from_title("user@host:~/Projects $ cargo build");
        assert_eq!(command, "cargo build");
    }

    #[test]
    fn extract_command_from_title_angle_prompt() {
        let command = extract_command_from_title("fish> git status");
        assert_eq!(command, "git status");
    }

    #[test]
    fn extract_command_from_title_no_prompt() {
        let command = extract_command_from_title("Firefox");
        assert!(command.is_empty());
    }

    #[test]
    fn extract_command_from_title_empty() {
        let command = extract_command_from_title("");
        assert!(command.is_empty());
    }

    #[test]
    fn tick_interval_is_reasonable() {
        assert!(TICK_INTERVAL_MS >= 100, "too fast, would waste CPU");
        assert!(TICK_INTERVAL_MS <= 1000, "too slow, suggestions would lag");
    }

    #[test]
    fn max_suggestions_is_reasonable() {
        assert!(MAX_SUGGESTIONS >= 4);
        assert!(MAX_SUGGESTIONS <= 20);
    }
}
