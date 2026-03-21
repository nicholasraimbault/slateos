/// Slate Settings — the settings app for Slate OS.
///
/// A standard iced window (not layer-shell) that reads/writes
/// `~/.config/slate/settings.toml` and emits D-Bus signals so
/// components update live.
mod navigation;
mod notifier;
mod pages;
mod settings_io;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use iced::widget::{container, row};
use iced::{Element, Length, Subscription, Task, Theme};

use slate_common::theme::create_theme;
use slate_common::{Palette, Settings};

use navigation::Page;
use pages::{about, ai, display, dock, fex, gestures, keyboard, network, wallpaper};

/// Debounce delay: wait this long after last change before saving.
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct SettingsApp {
    /// Current page displayed on the right.
    current_page: Page,
    /// The settings struct (persisted to TOML).
    settings: Settings,
    /// System palette for theming.
    palette: Palette,
    /// Discovered wallpaper images.
    wallpaper_images: Vec<PathBuf>,
    /// Discovered AI model files.
    ai_models: Vec<PathBuf>,
    /// FEX state (not in Settings, managed via system commands).
    fex_state: fex::FexState,
    /// Network state (not in Settings, managed via system commands).
    network_state: network::NetworkState,
    /// About info.
    about_info: about::AboutInfo,
    /// Brightness state (ephemeral, read from sysfs).
    brightness: display::BrightnessState,
    /// Timestamp of last settings mutation (for debounced save).
    last_change: Option<Instant>,
    /// Which section was last changed (for D-Bus signal).
    pending_section: Option<String>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    /// Navigate to a different page.
    ChangePage(Page),
    /// Display settings changed.
    Display(display::DisplayMsg),
    /// Wallpaper settings changed.
    Wallpaper(wallpaper::WallpaperMsg),
    /// Dock settings changed.
    Dock(dock::DockMsg),
    /// Gesture settings changed.
    Gesture(gestures::GestureMsg),
    /// Keyboard settings changed.
    Keyboard(keyboard::KeyboardMsg),
    /// AI settings changed.
    Ai(ai::AiMsg),
    /// FEX settings changed.
    Fex(fex::FexMsg),
    /// Network settings changed.
    Network(network::NetworkMsg),
    /// About info loaded.
    About(about::AboutMsg),
    /// Debounce timer fired: save settings now.
    SaveNow,
    /// Save completed.
    Saved,
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

impl SettingsApp {
    fn new() -> (Self, Task<Message>) {
        let settings = settings_io::load_settings();
        let wallpaper_images = wallpaper::scan_wallpapers();
        let ai_models = ai::scan_models();

        let app = Self {
            current_page: Page::Display,
            settings,
            palette: Palette::default(),
            wallpaper_images,
            ai_models,
            fex_state: fex::FexState::default(),
            network_state: network::NetworkState::default(),
            about_info: about::AboutInfo::default(),
            brightness: display::BrightnessState::default(),
            last_change: None,
            pending_section: None,
        };

        // Kick off background info gathering
        let about_task = Task::perform(async { about::gather_info().await }, |info| {
            Message::About(about::AboutMsg::InfoLoaded(info))
        });

        let fex_task = Task::perform(async { fex::check_fex_status().await }, |state| {
            Message::Fex(fex::FexMsg::StatusChecked(state))
        });

        let brightness_task = Task::perform(async { display::read_brightness().await }, |state| {
            Message::Display(display::DisplayMsg::BrightnessLoaded(state))
        });

        (app, Task::batch([about_task, fex_task, brightness_task]))
    }

    fn title(&self) -> String {
        "Slate Settings".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ChangePage(page) => {
                self.current_page = page;
                // Refresh data when navigating to certain pages
                match page {
                    Page::Wallpaper => {
                        return Task::perform(
                            async {
                                tokio::task::spawn_blocking(wallpaper::scan_wallpapers)
                                    .await
                                    .unwrap_or_default()
                            },
                            |images| Message::Wallpaper(wallpaper::WallpaperMsg::Refreshed(images)),
                        );
                    }
                    Page::Network => {
                        return Task::perform(async { network::scan_wifi().await }, |nets| {
                            Message::Network(network::NetworkMsg::WifiScanned(nets))
                        });
                    }
                    Page::Ai => {
                        return Task::perform(
                            async {
                                tokio::task::spawn_blocking(ai::scan_models)
                                    .await
                                    .unwrap_or_default()
                            },
                            |models| Message::Ai(ai::AiMsg::ModelsScanned(models)),
                        );
                    }
                    Page::Fex => {
                        return Task::perform(async { fex::check_fex_status().await }, |s| {
                            Message::Fex(fex::FexMsg::StatusChecked(s))
                        });
                    }
                    Page::Display => {
                        return Task::perform(
                            async { display::read_brightness().await },
                            |state| Message::Display(display::DisplayMsg::BrightnessLoaded(state)),
                        );
                    }
                    _ => {}
                }
            }

            Message::Display(msg) => {
                // Brightness messages do not require a settings save
                let needs_save = matches!(
                    msg,
                    display::DisplayMsg::ScaleChanged(_)
                        | display::DisplayMsg::RotationLockToggled(_)
                );
                if let Some(task) =
                    display::update(&mut self.settings.display, &mut self.brightness, msg)
                {
                    return task.map(Message::Display);
                }
                if needs_save {
                    self.mark_changed("display");
                }
            }

            Message::Wallpaper(msg) => {
                if let Some(task) = wallpaper::update(
                    &mut self.settings.wallpaper,
                    &mut self.wallpaper_images,
                    msg,
                ) {
                    return task.map(Message::Wallpaper);
                }
                self.mark_changed("wallpaper");
            }

            Message::Dock(msg) => {
                dock::update(&mut self.settings.dock, msg);
                self.mark_changed("dock");
            }

            Message::Gesture(msg) => {
                gestures::update(&mut self.settings.gestures, msg);
                self.mark_changed("gestures");
            }

            Message::Keyboard(msg) => {
                keyboard::update(&mut self.settings.keyboard, msg);
                self.mark_changed("keyboard");
            }

            Message::Ai(msg) => {
                if let Some(task) = ai::update(&mut self.settings.ai, &mut self.ai_models, msg) {
                    return task.map(Message::Ai);
                }
                self.mark_changed("ai");
            }

            Message::Fex(msg) => {
                if let Some(task) = fex::update(&mut self.fex_state, msg) {
                    return task.map(Message::Fex);
                }
                // FEX is not in settings.toml, no save needed
            }

            Message::Network(msg) => {
                if let Some(task) = network::update(&mut self.network_state, msg) {
                    return task.map(Message::Network);
                }
                // Network is not in settings.toml, no save needed
            }

            Message::About(about::AboutMsg::InfoLoaded(info)) => {
                self.about_info = info;
            }

            Message::SaveNow => {
                if self.last_change.is_some() {
                    let settings = self.settings.clone();
                    let section = self
                        .pending_section
                        .take()
                        .unwrap_or_else(|| "unknown".to_string());
                    self.last_change = None;
                    return Task::perform(
                        async move {
                            settings_io::save_and_notify(settings, section).await;
                        },
                        |()| Message::Saved,
                    );
                }
            }

            Message::Saved => {
                tracing::debug!("settings saved successfully");
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let sidebar = navigation::view_sidebar(self.current_page, Message::ChangePage);

        let content: Element<'_, Message> = match self.current_page {
            Page::Display => {
                display::view(&self.settings.display, &self.brightness).map(Message::Display)
            }
            Page::Wallpaper => wallpaper::view(&self.settings.wallpaper, &self.wallpaper_images)
                .map(Message::Wallpaper),
            Page::Dock => dock::view(&self.settings.dock).map(Message::Dock),
            Page::Gestures => gestures::view(&self.settings.gestures).map(Message::Gesture),
            Page::Keyboard => keyboard::view(&self.settings.keyboard).map(Message::Keyboard),
            Page::Ai => ai::view(&self.settings.ai, &self.ai_models).map(Message::Ai),
            Page::Fex => fex::view(&self.fex_state).map(Message::Fex),
            Page::Network => network::view(&self.network_state).map(Message::Network),
            Page::About => about::view(&self.about_info).map(Message::About),
        };

        let page_container = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(8);

        row![sidebar, page_container]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn theme(&self) -> Theme {
        create_theme(&self.palette)
    }

    fn subscription(&self) -> Subscription<Message> {
        // Debounced save: if there is a pending change, fire SaveNow after SAVE_DEBOUNCE
        if self.last_change.is_some() {
            iced::time::every(SAVE_DEBOUNCE).map(|_| Message::SaveNow)
        } else {
            Subscription::none()
        }
    }

    /// Mark that settings were changed; starts the debounce timer.
    fn mark_changed(&mut self, section: &str) {
        self.last_change = Some(Instant::now());
        self.pending_section = Some(section.to_string());
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("slate_settings=info")
        .init();

    tracing::info!("starting Slate Settings");

    iced::application(SettingsApp::title, SettingsApp::update, SettingsApp::view)
        .subscription(SettingsApp::subscription)
        .theme(SettingsApp::theme)
        .window_size(iced::Size::new(800.0, 600.0))
        .run_with(SettingsApp::new)?;

    Ok(())
}
