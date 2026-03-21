/// Shoal — the Wayland dock for Slate OS.
///
/// An iced + iced_layershell app anchored at the bottom of the screen with
/// macOS-style magnification, app icons, and running indicators.
mod actions;
mod dbus_listener;
mod desktop;
mod dock;
mod magnification;
mod windows;

use iced::widget::{container, Space};
use iced::{Element, Length, Subscription, Task, Theme};

use slate_common::theme::create_theme;
use slate_common::Palette;

use desktop::DesktopEntry;
use dock::{build_dock_icons, view_dock};
use windows::RunningApp;

/// Default pinned app desktop IDs (must match filenames without .desktop).
const DEFAULT_PINNED: &[&str] = &["Alacritty", "firefox", "org.gnome.Nautilus"];

/// Window poll interval in seconds.
const POLL_INTERVAL_SECS: u64 = 2;

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct Shoal {
    /// Desktop entries for pinned favourites.
    pinned: Vec<DesktopEntry>,
    /// All discovered desktop entries (for matching running apps to icons).
    all_entries: Vec<DesktopEntry>,
    /// Currently running applications from Niri.
    running: Vec<RunningApp>,
    /// Current system palette for theming.
    palette: Palette,
    /// Whether the dock is visible (can be toggled by TouchFlow).
    visible: bool,
    /// Current touch/cursor X position for magnification (None = no touch).
    touch_x: Option<f64>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)] // Some variants are only constructed by D-Bus listeners on Linux
enum Message {
    /// Periodic timer: re-poll running windows.
    Tick,
    /// Running window list updated.
    WindowsUpdated(Vec<RunningApp>),
    /// System palette changed via D-Bus.
    PaletteChanged(Palette),
    /// Touch/cursor moved horizontally over the dock.
    TouchMove(Option<f64>),
    /// User tapped an app icon.
    AppTap(String),
    /// User long-pressed an app icon (context menu).
    AppLongPress(String),
    /// TouchFlow requested dock to show.
    Show,
    /// TouchFlow requested dock to hide.
    Hide,
}

// ---------------------------------------------------------------------------
// iced application
// ---------------------------------------------------------------------------

impl Shoal {
    fn new() -> (Self, Task<Message>) {
        let all_entries = desktop::load_desktop_entries();

        // Resolve pinned desktop IDs to full entries
        let pinned: Vec<DesktopEntry> = DEFAULT_PINNED
            .iter()
            .filter_map(|id| {
                all_entries
                    .iter()
                    .find(|e| e.desktop_id.eq_ignore_ascii_case(id))
                    .cloned()
                    .or_else(|| {
                        // Create a stub entry so the dock slot is visible
                        Some(DesktopEntry {
                            name: id.to_string(),
                            exec: id.to_lowercase(),
                            icon: String::new(),
                            desktop_id: id.to_string(),
                        })
                    })
            })
            .collect();

        tracing::info!(
            "loaded {} desktop entries, {} pinned",
            all_entries.len(),
            pinned.len()
        );

        (
            Self {
                pinned,
                all_entries,
                running: Vec::new(),
                palette: Palette::default(),
                visible: true,
                touch_x: None,
            },
            Task::none(),
        )
    }

    fn title(&self) -> String {
        "Shoal".to_string()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick => {
                return Task::perform(
                    async { windows::poll_running_apps().await.unwrap_or_default() },
                    Message::WindowsUpdated,
                );
            }
            Message::WindowsUpdated(apps) => {
                self.running = apps;
            }
            Message::PaletteChanged(palette) => {
                self.palette = palette;
            }
            Message::TouchMove(x) => {
                self.touch_x = x;
            }
            Message::AppTap(desktop_id) => {
                let is_running = self
                    .running
                    .iter()
                    .any(|r| r.app_id.eq_ignore_ascii_case(&desktop_id));

                // Find the entry to launch or focus
                if let Some(entry) = self
                    .all_entries
                    .iter()
                    .chain(self.pinned.iter())
                    .find(|e| e.desktop_id.eq_ignore_ascii_case(&desktop_id))
                    .cloned()
                {
                    return Task::perform(
                        async move { actions::activate_app(&entry, is_running).await },
                        |_| Message::Tick,
                    );
                }
            }
            Message::AppLongPress(desktop_id) => {
                // Close the app if running
                let is_running = self
                    .running
                    .iter()
                    .any(|r| r.app_id.eq_ignore_ascii_case(&desktop_id));

                if is_running {
                    let id = desktop_id.clone();
                    return Task::perform(async move { actions::close_app(&id).await }, |_| {
                        Message::Tick
                    });
                }
            }
            Message::Show => {
                self.visible = true;
            }
            Message::Hide => {
                self.visible = false;
            }
        }

        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if !self.visible {
            return Space::new(Length::Fill, Length::Fixed(0.0)).into();
        }

        let (pinned_icons, dynamic_icons) =
            build_dock_icons(&self.pinned, &self.running, &self.all_entries, self.touch_x);

        let dock = view_dock(
            &pinned_icons,
            &dynamic_icons,
            |id| Message::AppTap(id),
            |id| Message::AppLongPress(id),
        );

        container(dock)
            .width(Length::Fill)
            .height(Length::Fixed(dock::DOCK_HEIGHT as f32))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(dock::DOCK_HEIGHT as f32))
            .into()
    }

    fn theme(&self) -> Theme {
        create_theme(&self.palette)
    }

    fn subscription(&self) -> Subscription<Message> {
        // Poll running windows every POLL_INTERVAL_SECS
        iced::time::every(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).map(|_| Message::Tick)
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("shoal=info")
        .init();

    tracing::info!("starting Shoal dock");

    // On Linux with Wayland, use iced_layershell for proper dock behaviour.
    // On other platforms (macOS dev), fall back to a regular iced window.
    run_app()
}

/// Run the iced application. On Linux this would use iced_layershell for
/// layer-shell anchoring; on macOS we use a standard window for development.
#[cfg(not(target_os = "linux"))]
fn run_app() -> anyhow::Result<()> {
    iced::application(Shoal::title, Shoal::update, Shoal::view)
        .subscription(Shoal::subscription)
        .theme(Shoal::theme)
        .window_size(iced::Size::new(600.0, dock::DOCK_HEIGHT as f32))
        .run_with(Shoal::new)?;

    Ok(())
}

/// On Linux, use iced_layershell for Wayland layer-shell support.
/// This anchors the dock at the bottom of the screen with an exclusive zone.
#[cfg(target_os = "linux")]
fn run_app() -> anyhow::Result<()> {
    use iced_layershell::Application as _;
    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};

    let layer_settings = LayerShellSettings {
        size: Some((0, dock::DOCK_HEIGHT)),
        anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
        exclusive_zone: dock::DOCK_HEIGHT as i32,
        layer: Layer::Top,
        keyboard_interactivity: KeyboardInteractivity::None,
        ..Default::default()
    };

    let settings: Settings<()> = Settings {
        layer_settings,
        ..Default::default()
    };

    ShoalLayerShell::run(settings)?;

    Ok(())
}

/// Wrapper that implements iced_layershell::Application trait.
/// The lib.rs Application trait is self-contained (does not extend
/// iced_runtime::Program) and provides its own run() method.
#[cfg(target_os = "linux")]
struct ShoalLayerShell {
    inner: Shoal,
}

#[cfg(target_os = "linux")]
impl iced_layershell::Application for ShoalLayerShell {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let (inner, task) = Shoal::new();
        (Self { inner }, task)
    }

    fn namespace(&self) -> String {
        "shoal".to_string()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        self.inner.update(message)
    }

    fn view(&self) -> Element<'_, Self::Message, Self::Theme, iced::Renderer> {
        self.inner.view()
    }

    fn theme(&self) -> Self::Theme {
        self.inner.theme()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        self.inner.subscription()
    }
}

#[cfg(target_os = "linux")]
impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
    type Error = Self;

    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self::Error> {
        Err(self)
    }
}
