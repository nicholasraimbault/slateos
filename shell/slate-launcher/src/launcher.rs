// Launcher UI — the main iced view for the fullscreen app grid overlay.
//
// On Linux this uses iced_layershell to display as a fullscreen overlay on the
// Wayland compositor's overlay layer with exclusive keyboard interactivity.
// On other platforms (macOS dev builds) it falls back to a plain iced window.

use crate::apps::AppEntry;
use crate::dbus_listener::DbusEvent;
use crate::search;

use slate_common::layout::{self, LayoutParams};
use slate_common::theme;
use slate_common::Palette;

use iced::widget::{column, container, row, scrollable, text, text_input, Column, Row};
use iced::{Element, Length, Subscription, Task, Theme};

/// Physical screen dimensions and scale, used to compute the adaptive layout.
///
/// These are tracked in the launcher state so that the column count can be
/// recalculated if the window is resized (e.g. orientation change).
#[derive(Debug, Clone, Copy)]
struct ScreenInfo {
    width: u32,
    height: u32,
    scale: f32,
}

impl Default for ScreenInfo {
    /// Sensible tablet-class defaults so the layout is usable before the
    /// compositor sends real dimensions.
    fn default() -> Self {
        Self {
            width: 2560,
            height: 1600,
            scale: 2.0,
        }
    }
}

/// Top-level launcher state.
pub struct Launcher {
    /// All discovered apps (loaded once at startup).
    apps: Vec<AppEntry>,
    /// Current search query.
    search_query: String,
    /// Whether the launcher overlay is visible.
    visible: bool,
    /// Current system palette.
    palette: Palette,
    /// Physical screen info used to derive layout parameters.
    screen_info: ScreenInfo,
    /// Adaptive layout parameters derived from screen size.
    layout: LayoutParams,
    /// Channel receiver for D-Bus events.
    dbus_rx: Option<tokio::sync::mpsc::UnboundedReceiver<DbusEvent>>,
    /// Channel sender for D-Bus events (kept alive so we can pass it).
    dbus_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
}

/// Messages for the iced update loop.
#[derive(Debug, Clone)]
pub enum Message {
    /// Show the launcher overlay.
    Show,
    /// Hide the launcher overlay.
    Hide,
    /// Toggle visibility.
    Toggle,
    /// The search input changed.
    SearchChanged(String),
    /// User tapped an app at the given index in the filtered list.
    LaunchApp(usize),
    /// System palette was updated.
    PaletteChanged(Palette),
    /// The output/window size changed; update adaptive layout.
    ScreenSizeChanged { width: u32, height: u32, scale: f32 },
    /// A keyboard event occurred.
    KeyPress(iced::keyboard::Key),
    /// D-Bus event received.
    DbusEvent(DbusEvent),
    /// No-op (used for subscriptions that don't produce events).
    Noop,
}

impl Launcher {
    /// Create a new launcher with the given app list.
    pub fn new() -> (Self, Task<Message>) {
        let apps = crate::apps::discover_apps();
        tracing::info!("discovered {} apps", apps.len());

        let (dbus_tx, dbus_rx) = tokio::sync::mpsc::unbounded_channel();

        let screen_info = ScreenInfo::default();
        let layout_params =
            layout::compute_layout(screen_info.width, screen_info.height, screen_info.scale);

        let launcher = Self {
            apps,
            search_query: String::new(),
            visible: false,
            palette: Palette::default(),
            screen_info,
            layout: layout_params,
            dbus_rx: Some(dbus_rx),
            dbus_tx,
        };

        (launcher, Task::none())
    }

    /// The iced title.
    pub fn title(&self) -> String {
        "Slate Launcher".to_string()
    }

    /// Handle a message and return a command.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Show => {
                self.visible = true;
                self.search_query.clear();
                Task::none()
            }
            Message::Hide => {
                self.visible = false;
                self.search_query.clear();
                Task::none()
            }
            Message::Toggle => {
                if self.visible {
                    self.update(Message::Hide)
                } else {
                    self.update(Message::Show)
                }
            }
            Message::SearchChanged(query) => {
                self.search_query = query;
                Task::none()
            }
            Message::LaunchApp(index) => {
                let filtered = self.filtered_apps();
                if let Some(app) = filtered.get(index) {
                    launch_exec(&app.exec);
                }
                self.visible = false;
                self.search_query.clear();
                Task::none()
            }
            Message::PaletteChanged(palette) => {
                self.palette = palette;
                Task::none()
            }
            Message::ScreenSizeChanged {
                width,
                height,
                scale,
            } => {
                self.screen_info = ScreenInfo {
                    width,
                    height,
                    scale,
                };
                self.layout = layout::compute_layout(width, height, scale);
                tracing::debug!(
                    "screen size changed: {}x{} @ {:.1}x → {} launcher columns",
                    width,
                    height,
                    scale,
                    self.layout.launcher_columns
                );
                Task::none()
            }
            Message::KeyPress(key) => {
                if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                    self.visible = false;
                    self.search_query.clear();
                }
                Task::none()
            }
            Message::DbusEvent(event) => match event {
                DbusEvent::Show => self.update(Message::Show),
                DbusEvent::Hide => self.update(Message::Hide),
                DbusEvent::Toggle => self.update(Message::Toggle),
                DbusEvent::PaletteChanged(toml_str) => {
                    if let Ok(palette) = toml::from_str::<Palette>(&toml_str) {
                        self.palette = palette;
                    } else {
                        tracing::warn!("failed to parse palette TOML from D-Bus signal");
                    }
                    Task::none()
                }
            },
            Message::Noop => Task::none(),
        }
    }

    /// Build the view.
    pub fn view(&self) -> Element<'_, Message> {
        if !self.visible {
            // When hidden, render a minimal invisible element
            return container(text(""))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let is_shell_mode = search::is_shell_command(&self.search_query);

        // Search bar
        let search_placeholder = if is_shell_mode {
            "Run command..."
        } else {
            "Search apps..."
        };

        let search_prefix = if is_shell_mode {
            text("Run: ").size(18)
        } else {
            text("").size(18)
        };

        let search_bar = row![
            search_prefix,
            text_input(search_placeholder, &self.search_query)
                .on_input(Message::SearchChanged)
                .size(18)
                .padding(12)
                .width(Length::Fill),
        ]
        .spacing(8)
        .padding(20)
        .align_y(iced::Alignment::Center);

        // Adaptive column count from the layout system. Clamped to at least 1
        // so the grid is always usable even before a ScreenSizeChanged message
        // has arrived from the compositor.
        let columns = (self.layout.launcher_columns as usize).max(1);
        let icon_size = self.layout.launcher_icon_size as f32;
        let gap = self.layout.launcher_gap as f32;

        // App grid — build directly from the filtered list to avoid lifetime issues.
        let filtered = self.filtered_apps();

        let grid_column: Column<Message> = if filtered.is_empty() {
            // Empty state: show a friendly message centred in the grid area.
            let empty_label = if self.apps.is_empty() {
                "No apps installed"
            } else {
                "No results"
            };

            Column::new()
                .spacing(gap)
                .padding(20.0)
                .push(
                    container(text(empty_label).size(18))
                        .width(Length::Fill)
                        .center_x(Length::Fill),
                )
        } else {
            filtered.chunks(columns).enumerate().fold(
                Column::new().spacing(gap).padding(20.0),
                |col, (row_idx, apps_chunk)| {
                    let row_widget: Row<Message> = apps_chunk.iter().enumerate().fold(
                        Row::new().spacing(gap),
                        |r, (col_idx, app)| {
                            let global_idx = row_idx * columns + col_idx;

                            let cell = iced::widget::button(
                                column![
                                    // Icon placeholder (a rounded square; real icon
                                    // lookup via the icon theme is future work).
                                    container(
                                        text(app.icon_placeholder()).size(icon_size * 0.5),
                                    )
                                    .width(Length::Fixed(icon_size))
                                    .height(Length::Fixed(icon_size))
                                    .center_x(Length::Fixed(icon_size))
                                    .center_y(Length::Fixed(icon_size)),
                                    text(&app.name).size(12).center(),
                                ]
                                .spacing(4)
                                .align_x(iced::Alignment::Center),
                            )
                            .on_press(Message::LaunchApp(global_idx))
                            .padding(8)
                            .width(Length::FillPortion(1));

                            r.push(cell)
                        },
                    );

                    // Pad the row with empty space if it has fewer than `columns` items
                    // so cells stay the same width as full rows.
                    let row_widget = if apps_chunk.len() < columns {
                        let empty_cells = columns - apps_chunk.len();
                        (0..empty_cells).fold(row_widget, |r, _| {
                            r.push(
                                container(text(""))
                                    .width(Length::FillPortion(1))
                                    .height(Length::Shrink),
                            )
                        })
                    } else {
                        row_widget
                    };

                    col.push(row_widget)
                },
            )
        };

        let content = column![search_bar, scrollable(grid_column).height(Length::Fill),]
            .width(Length::Fill)
            .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }

    /// Build the iced theme from the current palette.
    pub fn theme(&self) -> Theme {
        theme::create_theme(&self.palette)
    }

    /// Build subscriptions: D-Bus listener + keyboard events.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::keyboard::on_key_press(|key, _modifiers| {
            if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                Some(Message::KeyPress(key))
            } else {
                None
            }
        })
    }

    /// Get the D-Bus sender for passing to the listener task.
    pub fn dbus_sender(&self) -> tokio::sync::mpsc::UnboundedSender<DbusEvent> {
        self.dbus_tx.clone()
    }

    /// Take the D-Bus receiver (can only be called once).
    pub fn take_dbus_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<DbusEvent>> {
        self.dbus_rx.take()
    }

    // -- Private helpers --

    /// Get the currently filtered app list based on the search query.
    fn filtered_apps(&self) -> Vec<&AppEntry> {
        if search::is_shell_command(&self.search_query) {
            // In shell mode, don't filter apps
            Vec::new()
        } else {
            search::search_apps(&self.search_query, &self.apps)
        }
    }
}

/// Launch an application by its Exec command.
///
/// Uses `std::process::Command` which passes arguments as a vector (no shell
/// interpolation), avoiding command injection risks.
fn launch_exec(exec: &str) {
    tracing::info!("launching: {exec}");

    // Split on whitespace for argv
    let parts: Vec<&str> = exec.split_whitespace().collect();
    if parts.is_empty() {
        tracing::warn!("empty exec command");
        return;
    }

    let program = parts[0];
    let args = &parts[1..];

    match std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_child) => {
            tracing::info!("spawned {program}");
        }
        Err(err) => {
            tracing::error!("failed to spawn {program}: {err}");
        }
    }
}

/// Launch a shell command in a terminal emulator.
///
/// Uses `std::process::Command` with explicit argv — the command string is
/// passed as a single argument to `sh -c`, which is intentional: the user
/// typed this command in the launcher's shell-command mode.
pub fn launch_shell_command(cmd: &str) {
    tracing::info!("running shell command: {cmd}");

    match std::process::Command::new("alacritty")
        .args(["-e", "sh", "-c", cmd])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => tracing::info!("launched shell command in alacritty"),
        Err(err) => tracing::error!("failed to launch shell command: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_initial_state() {
        let (launcher, _) = Launcher::new();
        assert!(!launcher.visible);
        assert!(launcher.search_query.is_empty());
    }

    #[test]
    fn show_sets_visible() {
        let (mut launcher, _) = Launcher::new();
        let _ = launcher.update(Message::Show);
        assert!(launcher.visible);
    }

    #[test]
    fn hide_clears_visibility_and_query() {
        let (mut launcher, _) = Launcher::new();
        let _ = launcher.update(Message::Show);
        let _ = launcher.update(Message::SearchChanged("test".to_string()));
        let _ = launcher.update(Message::Hide);

        assert!(!launcher.visible);
        assert!(launcher.search_query.is_empty());
    }

    #[test]
    fn toggle_flips_visibility() {
        let (mut launcher, _) = Launcher::new();
        assert!(!launcher.visible);

        let _ = launcher.update(Message::Toggle);
        assert!(launcher.visible);

        let _ = launcher.update(Message::Toggle);
        assert!(!launcher.visible);
    }

    #[test]
    fn search_changed_updates_query() {
        let (mut launcher, _) = Launcher::new();
        let _ = launcher.update(Message::SearchChanged("firefox".to_string()));
        assert_eq!(launcher.search_query, "firefox");
    }

    #[test]
    fn escape_key_hides_launcher() {
        let (mut launcher, _) = Launcher::new();
        let _ = launcher.update(Message::Show);
        assert!(launcher.visible);

        let _ = launcher.update(Message::KeyPress(iced::keyboard::Key::Named(
            iced::keyboard::key::Named::Escape,
        )));
        assert!(!launcher.visible);
    }

    #[test]
    fn palette_changed_updates_palette() {
        let (mut launcher, _) = Launcher::new();
        let new_palette = Palette {
            primary: [255, 0, 0, 255],
            ..Palette::default()
        };
        let _ = launcher.update(Message::PaletteChanged(new_palette.clone()));
        assert_eq!(launcher.palette, new_palette);
    }

    #[test]
    fn title_returns_expected() {
        let (launcher, _) = Launcher::new();
        assert_eq!(launcher.title(), "Slate Launcher");
    }

    #[test]
    fn screen_size_changed_updates_layout() {
        let (mut launcher, _) = Launcher::new();

        // Simulate a phone-class screen: 1080×2400 at 2.5× → logical width 432 → phone → 3 cols
        let _ = launcher.update(Message::ScreenSizeChanged {
            width: 1080,
            height: 2400,
            scale: 2.5,
        });

        assert_eq!(
            launcher.layout.launcher_columns, 3,
            "phone should get 3 launcher columns"
        );
    }

    #[test]
    fn screen_size_changed_tablet_layout() {
        let (mut launcher, _) = Launcher::new();

        // Mid-width tablet: 1800×1200 at 2.0× → logical width 900 → tablet → 6 cols
        // (900 >= 900 threshold but < 1200, so the 6-column branch fires)
        let _ = launcher.update(Message::ScreenSizeChanged {
            width: 1800,
            height: 1200,
            scale: 2.0,
        });

        assert_eq!(
            launcher.layout.launcher_columns, 6,
            "mid-width tablet (logical 900) should get 6 launcher columns"
        );
    }

    #[test]
    fn initial_layout_is_usable() {
        // The default ScreenInfo produces a valid layout (non-zero columns).
        let (launcher, _) = Launcher::new();
        assert!(
            launcher.layout.launcher_columns > 0,
            "initial layout must have at least one column"
        );
    }
}
