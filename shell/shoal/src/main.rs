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

/// Fallback width for the layer-shell surface when the compositor has not yet
/// assigned a size. A non-zero value is required because wgpu panics if either
/// dimension is zero when configuring the swap-chain surface. The compositor
/// overrides this with the real output width once the surface is mapped.
#[cfg(target_os = "linux")]
const INITIAL_SURFACE_WIDTH: u32 = 800;

/// How many times to retry connecting to the Wayland compositor before giving
/// up. Each attempt is separated by COMPOSITOR_RETRY_DELAY_MS.
#[cfg(target_os = "linux")]
const COMPOSITOR_CONNECT_RETRIES: u32 = 10;

/// Delay in milliseconds between compositor connection attempts.
#[cfg(target_os = "linux")]
const COMPOSITOR_RETRY_DELAY_MS: u64 = 500;

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
            Message::AppTap,
            Message::AppLongPress,
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
///
/// Retries connecting to the compositor up to COMPOSITOR_CONNECT_RETRIES times
/// with a short delay between attempts, because shoal starts early in the boot
/// sequence and the compositor may not be ready immediately.
///
/// The initial surface width must be non-zero: wgpu panics when asked to
/// configure a swap-chain surface with a zero dimension. We supply a sensible
/// fallback; the compositor replaces it with the real output width once the
/// layer-shell surface is mapped.
#[cfg(target_os = "linux")]
fn run_app() -> anyhow::Result<()> {
    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};
    use iced_layershell::Application as _;

    let layer_settings = LayerShellSettings {
        // Width must be non-zero to avoid a wgpu swap-chain panic on surface
        // creation. The compositor resizes it to the full output width once the
        // anchored surface is mapped (Left | Right anchoring enables stretching).
        size: Some((INITIAL_SURFACE_WIDTH, dock::DOCK_HEIGHT)),
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

    // Retry the compositor connection to handle the race between arkhd
    // starting shoal and niri completing its Wayland socket setup.
    connect_with_retry(settings)
}

/// Attempt to start the iced_layershell application, retrying if the
/// compositor is not yet available.
///
/// `ConnectError(NoCompositor)` is the specific error returned by
/// iced_layershell when the WAYLAND_DISPLAY socket does not exist or the
/// compositor has not yet set up its global registry. We sleep briefly between
/// retries so we do not spin-loop and waste CPU while the compositor boots.
#[cfg(target_os = "linux")]
fn connect_with_retry(settings: iced_layershell::settings::Settings<()>) -> anyhow::Result<()> {
    use iced_layershell::Application as _;

    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 1..=COMPOSITOR_CONNECT_RETRIES {
        tracing::info!(
            "connecting to Wayland compositor (attempt {}/{})",
            attempt,
            COMPOSITOR_CONNECT_RETRIES
        );

        match ShoalLayerShell::run(settings.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let msg = e.to_string();
                // NoCompositor means the Wayland socket is not yet available;
                // all other errors are not transient and should fail immediately.
                if msg.contains("NoCompositor") || msg.contains("ConnectError") {
                    tracing::warn!(
                        "compositor not ready (attempt {}/{}): {}",
                        attempt,
                        COMPOSITOR_CONNECT_RETRIES,
                        e
                    );
                    last_err = Some(anyhow::anyhow!("{}", e));
                    std::thread::sleep(std::time::Duration::from_millis(COMPOSITOR_RETRY_DELAY_MS));
                } else {
                    // A non-transient error (e.g. wgpu no adapter, protocol
                    // mismatch): log it clearly and fail fast.
                    tracing::error!("fatal error starting shoal: {}", e);
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("compositor never became available")))
}

/// Wrapper that implements iced_layershell::Application trait.
/// The lib.rs Application trait is self-contained (does not extend
/// iced_runtime::Program) and provides its own run() method.
#[cfg(target_os = "linux")]
#[derive(Clone)]
struct ShoalLayerShell {
    inner: Shoal,
}

// Shoal must be Clone so ShoalLayerShell can be Clone (required by the retry
// loop which calls run() more than once on the same settings value).
#[cfg(target_os = "linux")]
impl Clone for Shoal {
    fn clone(&self) -> Self {
        Self {
            pinned: self.pinned.clone(),
            all_entries: self.all_entries.clone(),
            running: self.running.clone(),
            palette: self.palette.clone(),
            visible: self.visible,
            touch_x: self.touch_x,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Linux-only constants are only available on Linux, so gate these tests.
    #[cfg(target_os = "linux")]
    #[test]
    fn initial_surface_width_is_nonzero() {
        // Ensures the constant we pass to layer-shell never triggers the wgpu
        // zero-size surface panic.
        assert!(
            INITIAL_SURFACE_WIDTH > 0,
            "INITIAL_SURFACE_WIDTH must be > 0 to avoid wgpu surface panic"
        );
    }

    #[test]
    fn dock_height_is_nonzero() {
        assert!(
            dock::DOCK_HEIGHT > 0,
            "DOCK_HEIGHT must be > 0 to avoid wgpu surface panic"
        );
    }

    #[test]
    fn shoal_new_returns_empty_running_list() {
        let (state, _task) = Shoal::new();
        assert!(
            state.running.is_empty(),
            "running list should be empty on startup"
        );
    }

    #[test]
    fn shoal_starts_visible() {
        let (state, _task) = Shoal::new();
        assert!(state.visible, "dock should be visible by default");
    }

    #[test]
    fn shoal_update_hide_sets_invisible() {
        let (mut state, _) = Shoal::new();
        let _ = state.update(Message::Hide);
        assert!(!state.visible);
    }

    #[test]
    fn shoal_update_show_sets_visible() {
        let (mut state, _) = Shoal::new();
        let _ = state.update(Message::Hide);
        let _ = state.update(Message::Show);
        assert!(state.visible);
    }

    #[test]
    fn shoal_update_touch_move_stores_x() {
        let (mut state, _) = Shoal::new();
        let _ = state.update(Message::TouchMove(Some(123.0)));
        assert_eq!(state.touch_x, Some(123.0));
        let _ = state.update(Message::TouchMove(None));
        assert_eq!(state.touch_x, None);
    }

    #[test]
    fn shoal_update_palette_changed_stores_palette() {
        let (mut state, _) = Shoal::new();
        let new_palette = Palette::default();
        let _ = state.update(Message::PaletteChanged(new_palette.clone()));
        // Palette::default is the same value — just confirm no panic
        let _ = state.theme();
    }

    #[test]
    fn shoal_update_windows_updated_stores_list() {
        let (mut state, _) = Shoal::new();
        let apps = vec![windows::RunningApp {
            app_id: "firefox".into(),
            title: "Firefox".into(),
            is_focused: true,
        }];
        let _ = state.update(Message::WindowsUpdated(apps));
        assert_eq!(state.running.len(), 1);
    }

    /// Retry constants are only defined on Linux; gate accordingly.
    #[cfg(target_os = "linux")]
    #[test]
    fn compositor_retry_constants_are_sane() {
        assert!(COMPOSITOR_CONNECT_RETRIES > 0, "must retry at least once");
        assert!(
            COMPOSITOR_RETRY_DELAY_MS > 0,
            "delay must be positive to avoid spin-loop"
        );
        // Total worst-case wait time should be under 30 seconds so the service
        // manager does not kill us for being slow to start.
        let max_wait_ms = u64::from(COMPOSITOR_CONNECT_RETRIES) * COMPOSITOR_RETRY_DELAY_MS;
        assert!(
            max_wait_ms < 30_000,
            "worst-case retry wait ({max_wait_ms}ms) should be under 30 s"
        );
    }
}
