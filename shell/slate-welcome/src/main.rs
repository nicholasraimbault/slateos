// slate-welcome — first-boot setup wizard for Slate OS.
//
// Runs once on first login. Walks the user through:
//   1. Welcome screen
//   2. WiFi connection
//   3. Wallpaper selection
//   4. PIN setup
//   5. Done — marks first boot complete
//
// After completion, writes ~/.config/slate/.welcome-done so it never
// runs again. The arkhe service checks for this file before launching.

mod steps;

use iced::widget::{button, column, container, text};
use iced::{Alignment, Element, Length, Task};

use slate_common::theme::create_theme;
use slate_common::Palette;
use steps::{Step, StepAction};

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct Welcome {
    step: Step,
    palette: Palette,
}

impl Default for Welcome {
    fn default() -> Self {
        Self {
            step: Step::Welcome,
            palette: Palette::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    Next,
    Back,
    Step(StepAction),
    Finished,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

fn update(app: &mut Welcome, message: Message) -> Task<Message> {
    match message {
        Message::Next => {
            app.step = app.step.next();
            if app.step == Step::Done {
                return Task::perform(async { mark_complete().await }, |_| Message::Finished);
            }
        }
        Message::Back => {
            app.step = app.step.prev();
        }
        Message::Step(action) => {
            if let Some(task) = steps::update(&mut app.step, action) {
                return task.map(Message::Step);
            }
        }
        Message::Finished => {
            tracing::info!("first-boot setup complete");
            std::process::exit(0);
        }
    }
    Task::none()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn view(app: &Welcome) -> Element<'_, Message> {
    let step_content = steps::view(&app.step).map(Message::Step);

    let nav = match app.step {
        Step::Welcome => container(
            button(text("Get Started").size(16))
                .on_press(Message::Next)
                .padding(12),
        )
        .center_x(Length::Fill),
        Step::Done => container(
            button(text("Start Using SlateOS").size(16))
                .on_press(Message::Next)
                .padding(12),
        )
        .center_x(Length::Fill),
        _ => {
            let back = button(text("Back").size(14))
                .on_press(Message::Back)
                .padding(10);
            let next = button(text("Next").size(14))
                .on_press(Message::Next)
                .padding(10);
            let skip = button(text("Skip").size(14))
                .on_press(Message::Next)
                .padding(10)
                .style(button::secondary);

            container(
                iced::widget::row![back, skip, next]
                    .spacing(12)
                    .align_y(Alignment::Center),
            )
            .center_x(Length::Fill)
        }
    };

    let progress = text(format!(
        "Step {} of {}",
        app.step.index() + 1,
        Step::count()
    ))
    .size(12)
    .color(iced::Color::from_rgba(1.0, 1.0, 1.0, 0.4));

    let layout = column![step_content, nav, progress]
        .spacing(24)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .padding(40);

    container(layout)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Mark first-boot as complete so the wizard doesn't run again.
async fn mark_complete() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let marker = std::path::PathBuf::from(home).join(".config/slate/.welcome-done");
    if let Some(parent) = marker.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&marker, "done\n").await;
    tracing::info!("wrote {}", marker.display());
}

/// Check if first-boot setup has already been completed.
fn already_complete() -> bool {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::Path::new(&home)
        .join(".config/slate/.welcome-done")
        .exists()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("slate_welcome=info")
        .init();

    if already_complete() {
        tracing::info!("first-boot already complete, exiting");
        return Ok(());
    }

    tracing::info!("starting first-boot wizard");

    iced::application(|_: &Welcome| "SlateOS Setup".to_string(), update, view)
        .theme(|app: &Welcome| create_theme(&app.palette))
        .window_size(iced::Size::new(600.0, 700.0))
        .run_with(|| (Welcome::default(), Task::none()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_starts_at_welcome() {
        let app = Welcome::default();
        assert_eq!(app.step, Step::Welcome);
    }

    #[test]
    fn already_complete_returns_false_for_missing_file() {
        std::env::set_var("HOME", "/nonexistent-slate-welcome-test");
        assert!(!already_complete());
    }

    #[test]
    fn next_advances_step() {
        let mut app = Welcome::default();
        let _ = update(&mut app, Message::Next);
        assert_eq!(app.step, Step::Wifi);
    }

    #[test]
    fn back_from_welcome_stays() {
        let mut app = Welcome::default();
        let _ = update(&mut app, Message::Back);
        assert_eq!(app.step, Step::Welcome);
    }

    #[test]
    fn step_count_matches_variants() {
        assert_eq!(Step::count(), 5);
    }
}
