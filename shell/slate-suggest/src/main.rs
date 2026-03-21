// Many items are only used on Linux (layer-shell path) or in tests.
#![allow(dead_code)]

// slate-suggest — Keyboard suggestion bar for Slate OS.
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

use anyhow::Result;
use slate_common::Palette;

use crate::bar::BarMessage;
use crate::engine::{llm_suggestion, merge_suggestions, suggest_from_history, Suggestion};

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
    /// Whether LLM suggestions are enabled.
    llm_enabled: bool,
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
    /// An LLM completion arrived.
    LlmResult(Option<String>),
    /// Periodic tick for input polling.
    Tick,
}

impl SuggestBar {
    fn new() -> Self {
        let history = history::load_history();
        let palette = Palette::default();
        let suggestions = suggest_from_history("", &history, 8);

        Self {
            current_input: String::new(),
            suggestions,
            history,
            palette,
            llm_enabled: false,
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::ChipTapped(text) => {
                if let Err(err) = inject::inject_text(&text) {
                    tracing::error!("failed to inject text: {err:#}");
                }
            }
            Message::InputChanged(input) => {
                if input != self.current_input {
                    self.current_input = input;
                    self.refresh_suggestions();
                }
            }
            Message::PaletteChanged(palette) => {
                self.palette = palette;
            }
            Message::LlmResult(completion) => {
                let llm_sugg = completion.map(llm_suggestion);
                self.suggestions = merge_suggestions(
                    suggest_from_history(&self.current_input, &self.history, 7),
                    llm_sugg,
                    8,
                );
            }
            Message::Tick => {
                // In a full implementation this would poll the focused
                // window's input context. For now it is a no-op placeholder.
            }
        }
    }

    fn refresh_suggestions(&mut self) {
        self.suggestions = suggest_from_history(&self.current_input, &self.history, 8);
    }

    fn view(&self) -> iced::Element<'_, Message> {
        bar::view(&self.suggestions, &self.palette).map(|msg| match msg {
            BarMessage::ChipTapped(text) => Message::ChipTapped(text),
        })
    }
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
            SuggestBar::update(self, message);
            iced::Task::none()
        }

        fn view(&self) -> iced::Element<'_, Message, Self::Theme, iced::Renderer> {
            SuggestBar::view(self)
        }

        fn theme(&self) -> iced::Theme {
            slate_common::theme::create_theme(&self.palette)
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

    // Print suggestions for demonstration
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
