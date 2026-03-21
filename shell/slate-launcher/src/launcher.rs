// Launcher UI -- the main iced application state for the fullscreen app grid.
//
// Manages app discovery, recent-app tracking, launch feedback animation state,
// search filtering, and D-Bus integration. The actual Element tree is built by
// the `view` module to keep this file focused on state and update logic.

use crate::apps::AppEntry;
use crate::dbus_listener::DbusEvent;
use crate::feedback::LaunchFeedback;
use crate::recent::RecentApps;
use crate::search;
use crate::view;

use slate_common::layout::{self, LayoutParams};
use slate_common::theme;
use slate_common::Palette;

use iced::widget::{container, text};
use iced::{Element, Length, Subscription, Task, Theme};

use std::path::PathBuf;

/// Physical screen dimensions and scale for adaptive layout computation.
#[derive(Debug, Clone, Copy)]
struct ScreenInfo {
    width: u32,
    height: u32,
    scale: f32,
}

impl Default for ScreenInfo {
    fn default() -> Self {
        Self { width: 2560, height: 1600, scale: 2.0 }
    }
}

/// Top-level launcher state.
pub struct Launcher {
    apps: Vec<AppEntry>,
    search_query: String,
    visible: bool,
    palette: Palette,
    screen_info: ScreenInfo,
    layout: LayoutParams,
    dbus_rx: Option<tokio::sync::mpsc::UnboundedReceiver<DbusEvent>>,
    dbus_tx: tokio::sync::mpsc::UnboundedSender<DbusEvent>,
    /// Recently launched apps, persisted across restarts.
    recent: RecentApps,
    recent_path: PathBuf,
    /// Transient launch-feedback state for the tap-pulse animation.
    feedback: LaunchFeedback,
}

/// Messages for the iced update loop.
#[derive(Debug, Clone)]
pub enum Message {
    Show,
    Hide,
    Toggle,
    SearchChanged(String),
    /// User tapped an app at the given index in the combined (recent + main) list.
    LaunchApp(usize),
    PaletteChanged(Palette),
    ScreenSizeChanged { width: u32, height: u32, scale: f32 },
    KeyPress(iced::keyboard::Key),
    DbusEvent(DbusEvent),
    Noop,
}

impl Launcher {
    /// Create a new launcher, discovering apps and loading recent-apps state.
    pub fn new() -> (Self, Task<Message>) {
        let apps = crate::apps::discover_apps();
        tracing::info!("discovered {} apps", apps.len());

        let (dbus_tx, dbus_rx) = tokio::sync::mpsc::unbounded_channel();
        let screen_info = ScreenInfo::default();
        let layout_params =
            layout::compute_layout(screen_info.width, screen_info.height, screen_info.scale);

        let recent_path = RecentApps::default_path();
        let recent = RecentApps::load(&recent_path);
        tracing::info!("loaded {} recent apps", recent.entries.len());

        let launcher = Self {
            apps,
            search_query: String::new(),
            visible: false,
            palette: Palette::default(),
            screen_info,
            layout: layout_params,
            dbus_rx: Some(dbus_rx),
            dbus_tx,
            recent,
            recent_path,
            feedback: LaunchFeedback::new(),
        };
        (launcher, Task::none())
    }

    pub fn title(&self) -> String {
        "Slate Launcher".to_string()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Show => {
                self.visible = true;
                self.search_query.clear();
                self.feedback.clear();
                Task::none()
            }
            Message::Hide => {
                self.visible = false;
                self.search_query.clear();
                self.feedback.clear();
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
                self.handle_launch(index);
                Task::none()
            }
            Message::PaletteChanged(palette) => {
                self.palette = palette;
                Task::none()
            }
            Message::ScreenSizeChanged { width, height, scale } => {
                self.screen_info = ScreenInfo { width, height, scale };
                self.layout = layout::compute_layout(width, height, scale);
                tracing::debug!(
                    "screen size changed: {}x{} @ {:.1}x -> {} launcher columns",
                    width, height, scale, self.layout.launcher_columns
                );
                Task::none()
            }
            Message::KeyPress(key) => {
                if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                    self.visible = false;
                    self.search_query.clear();
                    self.feedback.clear();
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

    pub fn view(&self) -> Element<'_, Message> {
        if !self.visible {
            return container(text(""))
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }
        let is_shell_mode = search::is_shell_command(&self.search_query);
        view::build_view(
            &self.search_query, &self.apps, &self.recent,
            &self.layout, &self.feedback, is_shell_mode,
        )
    }

    pub fn theme(&self) -> Theme {
        theme::create_theme(&self.palette)
    }

    /// Keyboard Escape closes the launcher.
    pub fn subscription(&self) -> Subscription<Message> {
        iced::keyboard::on_key_press(|key, _modifiers| {
            if key == iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape) {
                Some(Message::KeyPress(key))
            } else {
                None
            }
        })
    }

    pub fn dbus_sender(&self) -> tokio::sync::mpsc::UnboundedSender<DbusEvent> {
        self.dbus_tx.clone()
    }

    pub fn take_dbus_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<DbusEvent>> {
        self.dbus_rx.take()
    }

    // -- Private helpers --

    /// Handle a launch by index in the combined [recent..main] list.
    /// Records the launch, triggers feedback, spawns the process, hides launcher.
    fn handle_launch(&mut self, index: usize) {
        // Clone the app data before mutating self (satisfies borrow checker).
        let (desktop_id, exec_cmd) = {
            let (recent_refs, main_refs) = self.split_recent_and_main();
            let combined_len = recent_refs.len() + main_refs.len();
            let app = if index < recent_refs.len() {
                recent_refs.get(index).copied()
            } else {
                main_refs.get(index - recent_refs.len()).copied()
            };
            match app {
                Some(a) => (a.desktop_id.clone(), a.exec.clone()),
                None => {
                    tracing::warn!("LaunchApp index {index} out of range ({combined_len})");
                    self.visible = false;
                    self.search_query.clear();
                    return;
                }
            }
        };
        self.feedback.trigger(&desktop_id);
        self.recent.record_launch(&desktop_id);
        self.recent.save(&self.recent_path);
        launch_exec(&exec_cmd);
        self.visible = false;
        self.search_query.clear();
    }

    /// Split apps into (recent, main) lists. During search the recent
    /// section is empty to avoid confusion.
    fn split_recent_and_main(&self) -> (Vec<&AppEntry>, Vec<&AppEntry>) {
        let filtered = self.filtered_apps();
        if !self.search_query.is_empty() {
            return (Vec::new(), filtered);
        }
        let recent_set = self.recent.recent_id_set();
        let recent_ids = self.recent.recent_ids();
        let recent_refs: Vec<&AppEntry> = recent_ids
            .iter()
            .filter_map(|id| self.apps.iter().find(|a| a.desktop_id == *id))
            .collect();
        let main_refs: Vec<&AppEntry> = filtered
            .into_iter()
            .filter(|a| !recent_set.contains_key(a.desktop_id.as_str()))
            .collect();
        (recent_refs, main_refs)
    }

    fn filtered_apps(&self) -> Vec<&AppEntry> {
        if search::is_shell_command(&self.search_query) {
            Vec::new()
        } else {
            search::search_apps(&self.search_query, &self.apps)
        }
    }
}

/// Launch an application by its Exec command using argv (no shell interpolation).
fn launch_exec(exec: &str) {
    tracing::info!("launching: {exec}");
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
        Ok(_child) => tracing::info!("spawned {program}"),
        Err(err) => tracing::error!("failed to spawn {program}: {err}"),
    }
}

/// Launch a shell command in a terminal emulator.
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
        let _ = launcher.update(Message::ScreenSizeChanged {
            width: 1080, height: 2400, scale: 2.5,
        });
        assert_eq!(launcher.layout.launcher_columns, 3);
    }

    #[test]
    fn screen_size_changed_tablet_layout() {
        let (mut launcher, _) = Launcher::new();
        let _ = launcher.update(Message::ScreenSizeChanged {
            width: 1800, height: 1200, scale: 2.0,
        });
        assert_eq!(launcher.layout.launcher_columns, 6);
    }

    #[test]
    fn initial_layout_is_usable() {
        let (launcher, _) = Launcher::new();
        assert!(launcher.layout.launcher_columns > 0);
    }

    #[test]
    fn split_recent_and_main_empty_when_searching() {
        let (mut launcher, _) = Launcher::new();
        launcher.recent.record_launch("firefox");
        let _ = launcher.update(Message::SearchChanged("fire".to_string()));
        let (recent, _main) = launcher.split_recent_and_main();
        assert!(recent.is_empty(), "recent section should be hidden during search");
    }

    #[test]
    fn feedback_is_cleared_on_hide() {
        let (mut launcher, _) = Launcher::new();
        launcher.feedback.trigger("firefox");
        let _ = launcher.update(Message::Hide);
        assert!(!launcher.feedback.is_active());
    }

    #[test]
    fn feedback_is_cleared_on_show() {
        let (mut launcher, _) = Launcher::new();
        launcher.feedback.trigger("firefox");
        let _ = launcher.update(Message::Show);
        assert!(!launcher.feedback.is_active());
    }
}
