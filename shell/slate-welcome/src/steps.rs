// First-boot wizard steps.
//
// Each step is a self-contained screen with its own state, view, and
// update logic. The parent app drives navigation between steps.

use std::path::{Path, PathBuf};

use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

// ---------------------------------------------------------------------------
// Step enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Wifi,
    Wallpaper,
    Pin,
    Done,
}

impl Step {
    pub fn next(self) -> Step {
        match self {
            Step::Welcome => Step::Wifi,
            Step::Wifi => Step::Wallpaper,
            Step::Wallpaper => Step::Pin,
            Step::Pin => Step::Done,
            Step::Done => Step::Done,
        }
    }

    pub fn prev(self) -> Step {
        match self {
            Step::Welcome => Step::Welcome,
            Step::Wifi => Step::Welcome,
            Step::Wallpaper => Step::Wifi,
            Step::Pin => Step::Wallpaper,
            Step::Done => Step::Pin,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Step::Welcome => 0,
            Step::Wifi => 1,
            Step::Wallpaper => 2,
            Step::Pin => 3,
            Step::Done => 4,
        }
    }

    pub fn count() -> usize {
        5
    }
}

// ---------------------------------------------------------------------------
// Step actions (messages from step views)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum StepAction {
    // WiFi
    WifiSsidInput(String),
    WifiPasswordInput(String),
    WifiConnect,
    WifiConnectResult(Result<String, String>),
    WifiSkip,

    // Wallpaper
    WallpaperSelected(PathBuf),

    // PIN
    PinInput(String),
    PinConfirmInput(String),
    PinSet,
    PinResult(Result<String, String>),
}

// ---------------------------------------------------------------------------
// Step state (stored inline in the Step enum would be complex, use a
// separate struct that the parent owns alongside the Step)
// ---------------------------------------------------------------------------

// For simplicity, we store ephemeral step state in the Step enum itself
// by extending it. But since Step is Copy, we keep state external.
// The parent main.rs owns Step (which step we're on) and the step
// functions use closures over local state. For the wizard's simple
// needs, we embed mutable state in StepAction responses and let the
// parent track it.
//
// Actually, let's keep it simple: each step's view just takes &Step
// and returns elements. Ephemeral state (text inputs) is tracked
// here in module-level statics... no, that's bad.
//
// Cleanest approach: Step becomes a struct-enum with embedded state.
// But it needs to be PartialEq for tests. Let's use a companion struct.

/// Mutable state for all wizard steps. Lives in the parent app.
#[derive(Debug, Default, Clone)]
pub struct StepState {
    // WiFi
    pub wifi_ssid: String,
    pub wifi_password: String,
    pub wifi_status: Option<Result<String, String>>,

    // Wallpaper
    pub wallpaper_selected: Option<PathBuf>,
    pub wallpaper_images: Vec<PathBuf>,

    // PIN
    pub pin_input: String,
    pub pin_confirm: String,
    pub pin_status: Option<Result<String, String>>,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(step: &mut Step, action: StepAction) -> Option<iced::Task<StepAction>> {
    match action {
        StepAction::WifiSsidInput(_) | StepAction::WifiPasswordInput(_) | StepAction::WifiSkip => {
            // These are handled by the parent via StepState
            None
        }

        StepAction::WifiConnect => {
            // Would spawn nmcli connect — stubbed for now
            Some(iced::Task::perform(
                async { Ok::<String, String>("Connected".to_string()) },
                StepAction::WifiConnectResult,
            ))
        }

        StepAction::WifiConnectResult(_) => None,

        StepAction::WallpaperSelected(_) => None,

        StepAction::PinInput(_) | StepAction::PinConfirmInput(_) => None,

        StepAction::PinSet => {
            // Would hash and save — stubbed for now
            Some(iced::Task::perform(
                async { Ok::<String, String>("PIN set".to_string()) },
                StepAction::PinResult,
            ))
        }

        StepAction::PinResult(ref result) => {
            if result.is_ok() {
                *step = Step::Done;
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

pub fn view(step: &Step) -> Element<'_, StepAction> {
    match step {
        Step::Welcome => view_welcome(),
        Step::Wifi => view_wifi(),
        Step::Wallpaper => view_wallpaper(),
        Step::Pin => view_pin(),
        Step::Done => view_done(),
    }
}

fn view_welcome<'a>() -> Element<'a, StepAction> {
    let content = column![
        text("Welcome to SlateOS").size(32),
        text("A touch-first Linux experience").size(16),
        text("").size(8),
        text("Let's set up your device in a few quick steps.").size(14),
    ]
    .spacing(12)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn view_wifi<'a>() -> Element<'a, StepAction> {
    let content = column![
        text("Connect to WiFi").size(24),
        text("You can skip this and connect later in Settings.").size(14),
        text("").size(8),
        text("WiFi scanning requires NetworkManager (nmcli).").size(12),
        text("Connect manually after setup if needed.").size(12),
    ]
    .spacing(12)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn view_wallpaper<'a>() -> Element<'a, StepAction> {
    // Scan for wallpapers
    let dirs = wallpaper_dirs();
    let mut images: Vec<PathBuf> = Vec::new();
    for dir in &dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if is_image_file(&path) {
                    images.push(path);
                }
            }
        }
    }

    let items: Vec<Element<'_, StepAction>> = images
        .into_iter()
        .map(|path| {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("wallpaper")
                .to_string();
            let p = path.clone();
            button(text(name).size(14))
                .on_press(StepAction::WallpaperSelected(p))
                .padding(10)
                .width(Length::Fill)
                .into()
        })
        .collect();

    let list = if items.is_empty() {
        column![text("No wallpapers found.").size(14)]
    } else {
        column(items).spacing(4)
    };

    let content = column![
        text("Choose a Wallpaper").size(24),
        text("This sets your wallpaper and generates your color theme.").size(14),
        scrollable(list).height(Length::Fixed(300.0)),
    ]
    .spacing(12)
    .width(Length::Fill);

    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn view_pin<'a>() -> Element<'a, StepAction> {
    let content = column![
        text("Set a PIN").size(24),
        text("4-8 digits to lock your device. You can skip this.").size(14),
        text("").size(8),
        text_input("Enter PIN (4-8 digits)", "")
            .on_input(StepAction::PinInput)
            .secure(true)
            .width(Length::Fixed(250.0)),
        text_input("Confirm PIN", "")
            .on_input(StepAction::PinConfirmInput)
            .secure(true)
            .width(Length::Fixed(250.0)),
        button(text("Set PIN").size(14))
            .on_press(StepAction::PinSet)
            .padding(10),
    ]
    .spacing(12)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn view_done<'a>() -> Element<'a, StepAction> {
    let content = column![
        text("You're all set!").size(32),
        text("").size(8),
        text("SlateOS is ready to use.").size(16),
        text("Swipe up from the bottom for the dock.").size(14),
        text("Swipe down from the top for notifications.").size(14),
        text("Three-finger swipe to switch workspaces.").size(14),
    ]
    .spacing(8)
    .align_x(Alignment::Center)
    .width(Length::Fill);

    container(content)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Wallpaper helpers (same logic as slate-settings)
// ---------------------------------------------------------------------------

fn wallpaper_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".config/slate/wallpapers"));
    }
    dirs.push(PathBuf::from("/usr/share/backgrounds"));
    dirs
}

fn is_image_file(path: &Path) -> bool {
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTS.contains(&ext.to_lowercase().as_str()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_navigation_forward() {
        assert_eq!(Step::Welcome.next(), Step::Wifi);
        assert_eq!(Step::Wifi.next(), Step::Wallpaper);
        assert_eq!(Step::Wallpaper.next(), Step::Pin);
        assert_eq!(Step::Pin.next(), Step::Done);
        assert_eq!(Step::Done.next(), Step::Done);
    }

    #[test]
    fn step_navigation_backward() {
        assert_eq!(Step::Welcome.prev(), Step::Welcome);
        assert_eq!(Step::Wifi.prev(), Step::Welcome);
        assert_eq!(Step::Wallpaper.prev(), Step::Wifi);
        assert_eq!(Step::Pin.prev(), Step::Wallpaper);
        assert_eq!(Step::Done.prev(), Step::Pin);
    }

    #[test]
    fn step_index_matches_order() {
        assert_eq!(Step::Welcome.index(), 0);
        assert_eq!(Step::Wifi.index(), 1);
        assert_eq!(Step::Wallpaper.index(), 2);
        assert_eq!(Step::Pin.index(), 3);
        assert_eq!(Step::Done.index(), 4);
    }

    #[test]
    fn image_file_detection() {
        assert!(is_image_file(Path::new("photo.jpg")));
        assert!(is_image_file(Path::new("photo.PNG")));
        assert!(is_image_file(Path::new("photo.webp")));
        assert!(!is_image_file(Path::new("readme.txt")));
        assert!(!is_image_file(Path::new("no-extension")));
    }
}
