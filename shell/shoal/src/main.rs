/// Shoal -- the Wayland dock for Slate OS.
///
/// An iced + iced_layershell app anchored at the bottom of the screen with
/// macOS-style magnification, app icons, running indicators, and a context
/// menu triggered by right-click or long-press.
mod actions;
mod context_menu;
mod dbus_listener;
mod desktop;
mod dock;
mod magnification;
mod windows;

use std::collections::HashMap;
use std::time::Duration;

use iced::widget::{column, container, mouse_area, stack, Space};
use iced::{Element, Length, Subscription, Task, Theme};

use context_menu::{MenuAction, MenuState};
use slate_common::theme::create_theme;
use slate_common::toast::{ToastKind, ToastPosition, ToastState};
use slate_common::Palette;

use desktop::DesktopEntry;
#[cfg(test)]
use dock::DockIcon;
use dock::{build_dock_icons, view_dock};
use windows::RunningApp;

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
    /// Currently open context menu, if any.
    menu: Option<MenuState>,
    /// Toast notification overlay state.
    toast_state: ToastState,
    /// Active notification counts keyed by app_name from slate-notifyd.
    ///
    /// Apps with zero notifications are removed rather than stored as 0, so
    /// absence from the map and presence with count 0 are equivalent.
    notification_counts: HashMap<String, u32>,
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
    /// User selected an action from the context menu.
    MenuAction(MenuAction),
    /// Dismiss the context menu without taking action.
    MenuDismiss,
    /// A menu action completed; refresh the dock state.
    MenuActionDone,
    /// Pinned apps were reloaded after a pin/unpin operation.
    PinnedReloaded(Vec<DesktopEntry>),
    /// TouchFlow requested dock to show.
    Show,
    /// TouchFlow requested dock to hide.
    Hide,
    /// Toast expiry tick.
    ToastTick,
    /// Notification count changed for an app (app_name, count).
    NotificationCountChanged(String, u32),
    /// Batch of initial notification counts loaded on startup.
    InitialNotificationCounts(HashMap<String, u32>),
    /// D-Bus event forwarded from the dock listener.
    #[cfg(target_os = "linux")]
    DockDbusEvent(dbus_listener::DockDbusEvent),
}

// ---------------------------------------------------------------------------
// iced application
// ---------------------------------------------------------------------------

impl Shoal {
    fn new() -> (Self, Task<Message>) {
        let all_entries = desktop::load_desktop_entries();

        // Resolve the pinned list from settings (or defaults)
        let pinned_ids = actions::load_pinned_apps();
        let pinned = resolve_pinned(&pinned_ids, &all_entries);

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
                menu: None,
                toast_state: ToastState::new(ToastPosition::BottomCenter),
                notification_counts: HashMap::new(),
            },
            // Seed initial notification counts by fetching active notifications
            // from slate-notifyd on startup. The result is a pre-aggregated map
            // delivered as a single InitialNotificationCounts message.
            Task::perform(
                fetch_initial_notification_counts(),
                Message::InitialNotificationCounts,
            ),
        )
    }

    #[allow(dead_code)] // Used by macOS fallback entry point
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
                // Tapping while a menu is open just dismisses the menu
                if self.menu.is_some() {
                    self.menu = None;
                    return Task::none();
                }

                let is_running = self
                    .running
                    .iter()
                    .any(|r| r.app_id.eq_ignore_ascii_case(&desktop_id));

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
                self.open_menu(&desktop_id);
            }
            Message::MenuAction(action) => {
                // Capture the app name before clearing the menu for toast messages
                let app_name = self
                    .menu
                    .as_ref()
                    .map(|m| m.app_name.clone())
                    .unwrap_or_default();
                self.menu = None;

                // Show toast feedback for the action
                match &action {
                    MenuAction::KeepInDock(_) => {
                        self.toast_state
                            .push(format!("Pinned {app_name}"), ToastKind::Success);
                    }
                    MenuAction::RemoveFromDock(_) => {
                        self.toast_state
                            .push(format!("Removed {app_name}"), ToastKind::Info);
                    }
                    MenuAction::CloseWindow(_) | MenuAction::CloseAllWindows(_) => {
                        self.toast_state.push("App closed", ToastKind::Info);
                    }
                }

                return self.handle_menu_action(action);
            }
            Message::MenuDismiss => {
                self.menu = None;
            }
            Message::MenuActionDone => {
                // Reload pinned list from disk after pin/unpin changes
                let all = self.all_entries.clone();
                return Task::perform(
                    async move {
                        let ids = actions::load_pinned_apps();
                        resolve_pinned(&ids, &all)
                    },
                    Message::PinnedReloaded,
                );
            }
            Message::PinnedReloaded(new_pinned) => {
                self.pinned = new_pinned;
                // Also refresh running windows to update indicators
                return Task::perform(
                    async { windows::poll_running_apps().await.unwrap_or_default() },
                    Message::WindowsUpdated,
                );
            }
            Message::Show => {
                self.visible = true;
            }
            Message::Hide => {
                self.visible = false;
            }
            Message::ToastTick => {
                self.toast_state.tick();
            }
            Message::NotificationCountChanged(app_name, count) => {
                if app_name.is_empty() {
                    // Ignore the dummy startup message.
                } else if count > 0 {
                    self.notification_counts.insert(app_name, count);
                } else {
                    self.notification_counts.remove(&app_name);
                }
            }
            Message::InitialNotificationCounts(counts) => {
                // Replace the entire map with the freshly fetched state.
                self.notification_counts = counts;
            }
            #[cfg(target_os = "linux")]
            Message::DockDbusEvent(event) => {
                use dbus_listener::DockDbusEvent;
                match event {
                    DockDbusEvent::PaletteChanged(palette) => {
                        self.palette = palette;
                    }
                    DockDbusEvent::Show => {
                        self.visible = true;
                    }
                    DockDbusEvent::Hide => {
                        self.visible = false;
                    }
                    DockDbusEvent::NotificationCountChanged(app_name, count) => {
                        if count > 0 {
                            self.notification_counts.insert(app_name, count);
                        } else {
                            self.notification_counts.remove(&app_name);
                        }
                    }
                }
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
            &self.notification_counts,
            Message::AppTap,
            Message::AppLongPress,
        );

        let dock_container = container(dock)
            .width(Length::Fill)
            .height(Length::Fixed(dock::DOCK_HEIGHT as f32))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(dock::DOCK_HEIGHT as f32));

        // Toast overlay rendered above the dock bar
        let toast_overlay = self.toast_state.view(&self.palette);

        // Combine dock and toasts vertically (toasts above dock)
        let dock_with_toasts = column![toast_overlay, dock_container];

        // If a context menu is open, overlay it above the dock
        if let Some(ref menu_state) = self.menu {
            let menu_popup =
                context_menu::view_menu(menu_state, Message::MenuAction, Message::MenuDismiss);

            // Wrap the menu in a container positioned above the dock
            let menu_overlay = container(menu_popup)
                .width(Length::Shrink)
                .height(Length::Shrink)
                .center_x(Length::Fill)
                .align_bottom(Length::Fixed(dock::DOCK_HEIGHT as f32));

            // Use a stack to layer the menu above the dock. The dismiss
            // area covers the full surface so taps outside the menu close it.
            let dismiss_area: Element<'_, Message> = mouse_area(Space::new(
                Length::Fill,
                Length::Fixed(dock::DOCK_HEIGHT as f32),
            ))
            .on_press(Message::MenuDismiss)
            .into();

            stack![dismiss_area, dock_with_toasts, menu_overlay].into()
        } else {
            dock_with_toasts.into()
        }
    }

    fn theme(&self) -> Theme {
        create_theme(&self.palette)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs =
            vec![iced::time::every(Duration::from_secs(POLL_INTERVAL_SECS)).map(|_| Message::Tick)];

        // Toast expiry tick: only run while toasts are visible
        if !self.toast_state.is_empty() {
            subs.push(iced::time::every(Duration::from_millis(250)).map(|_| Message::ToastTick));
        }

        // D-Bus subscription: runs exactly once (stable ID) for the app lifetime.
        #[cfg(target_os = "linux")]
        subs.push(dbus_subscription());

        Subscription::batch(subs)
    }
}

// ---------------------------------------------------------------------------
// Context menu helpers
// ---------------------------------------------------------------------------

impl Shoal {
    /// Open a context menu for the given app.
    fn open_menu(&mut self, desktop_id: &str) {
        let is_running = self
            .running
            .iter()
            .any(|r| r.app_id.eq_ignore_ascii_case(desktop_id));

        let is_pinned = self
            .pinned
            .iter()
            .any(|e| e.desktop_id.eq_ignore_ascii_case(desktop_id));

        // Find the human-readable name
        let app_name = self
            .all_entries
            .iter()
            .chain(self.pinned.iter())
            .find(|e| e.desktop_id.eq_ignore_ascii_case(desktop_id))
            .map(|e| e.name.clone())
            .unwrap_or_else(|| desktop_id.to_string());

        self.menu = Some(MenuState {
            desktop_id: desktop_id.to_string(),
            app_name,
            is_running,
            is_pinned,
        });
    }

    /// Dispatch a context menu action to the appropriate async operation.
    fn handle_menu_action(&self, action: MenuAction) -> Task<Message> {
        match action {
            MenuAction::CloseWindow(id) => {
                Task::perform(async move { actions::close_app(&id).await }, |_| {
                    Message::MenuActionDone
                })
            }
            MenuAction::CloseAllWindows(id) => {
                Task::perform(async move { actions::close_app(&id).await }, |_| {
                    Message::MenuActionDone
                })
            }
            MenuAction::KeepInDock(id) => {
                Task::perform(async move { actions::pin_app(&id).await }, |_| {
                    Message::MenuActionDone
                })
            }
            MenuAction::RemoveFromDock(id) => {
                Task::perform(async move { actions::unpin_app(&id).await }, |_| {
                    Message::MenuActionDone
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pinned app resolution
// ---------------------------------------------------------------------------

/// Resolve a list of desktop IDs into full DesktopEntry values.
///
/// Missing entries get a stub so the dock slot is visible even if the app
/// is not installed on this system.
fn resolve_pinned(ids: &[String], all_entries: &[DesktopEntry]) -> Vec<DesktopEntry> {
    ids.iter()
        .filter_map(|id| {
            all_entries
                .iter()
                .find(|e| e.desktop_id.eq_ignore_ascii_case(id))
                .cloned()
                .or_else(|| {
                    Some(DesktopEntry {
                        name: id.to_string(),
                        exec: id.to_lowercase(),
                        icon: String::new(),
                        desktop_id: id.to_string(),
                    })
                })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// DockIcon test helper
// ---------------------------------------------------------------------------

/// Find a DockIcon by desktop ID from an iterator of icons.
///
/// Used by tests to verify icon state after context menu operations.
#[cfg(test)]
fn icon_for_menu<'a>(
    mut icons: impl Iterator<Item = &'a DockIcon>,
    desktop_id: &str,
) -> Option<&'a DockIcon> {
    icons.find(|i| i.entry.desktop_id.eq_ignore_ascii_case(desktop_id))
}

// ---------------------------------------------------------------------------
// D-Bus helpers
// ---------------------------------------------------------------------------

/// Fetch active notifications from slate-notifyd and aggregate counts per app.
///
/// This runs once on startup to pre-populate badge counts before any
/// GroupChanged signals arrive. On non-Linux targets it returns an empty map.
async fn fetch_initial_notification_counts() -> HashMap<String, u32> {
    #[cfg(not(target_os = "linux"))]
    {
        HashMap::new()
    }
    #[cfg(target_os = "linux")]
    {
        fetch_initial_notification_counts_linux().await
    }
}

#[cfg(target_os = "linux")]
async fn fetch_initial_notification_counts_linux() -> HashMap<String, u32> {
    use slate_common::dbus::{NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH};
    use slate_common::notifications::Notification;

    let conn = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("shoal: could not connect to session bus for initial counts: {e}");
            return HashMap::new();
        }
    };

    let proxy = match zbus::Proxy::new(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("shoal: could not create notifications proxy: {e}");
            return HashMap::new();
        }
    };

    let toml_str: String = match proxy.call("GetActive", &()).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("shoal: GetActive call failed: {e}");
            return HashMap::new();
        }
    };

    // The response is a TOML document with a top-level `notifications` array.
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        notifications: Vec<Notification>,
    }

    let wrapper: Wrapper = match toml::from_str(&toml_str) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("shoal: failed to parse GetActive TOML: {e}");
            return HashMap::new();
        }
    };

    let mut counts: HashMap<String, u32> = HashMap::new();
    for n in wrapper.notifications {
        *counts.entry(n.app_name.clone()).or_insert(0) += 1;
    }
    counts
}

/// D-Bus subscription that spawns all dock listeners and forwards events.
///
/// Uses a stable subscription ID so iced creates it exactly once.
#[cfg(target_os = "linux")]
fn dbus_subscription() -> Subscription<Message> {
    use iced::futures::SinkExt;

    let stream = iced::stream::channel(50, |mut output| async move {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<dbus_listener::DockDbusEvent>();

        // Spawn one task per signal source. If one fails the others continue.
        let tx_palette = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = dbus_listener::listen_palette(tx_palette).await {
                tracing::warn!("shoal: palette listener ended: {e}");
            }
        });

        let tx_dock = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = dbus_listener::listen_dock_signals(tx_dock).await {
                tracing::warn!("shoal: dock signal listener ended: {e}");
            }
        });

        let tx_notif = tx;
        tokio::spawn(async move {
            if let Err(e) = dbus_listener::listen_notification_counts(tx_notif).await {
                tracing::warn!("shoal: notification count listener ended: {e}");
            }
        });

        loop {
            match rx.recv().await {
                Some(event) => {
                    let msg = Message::DockDbusEvent(event);
                    if output.send(msg).await.is_err() {
                        tracing::warn!("shoal: D-Bus subscription channel closed");
                        break;
                    }
                }
                None => {
                    tracing::warn!("shoal: D-Bus event channel closed unexpectedly");
                    std::future::pending::<()>().await;
                }
            }
        }
    });

    Subscription::run_with_id("shoal-dbus", stream)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("shoal=info")
        .init();

    tracing::info!("starting Shoal dock");

    run_app()
}

#[cfg(not(target_os = "linux"))]
fn run_app() -> anyhow::Result<()> {
    iced::application(Shoal::title, Shoal::update, Shoal::view)
        .subscription(Shoal::subscription)
        .theme(Shoal::theme)
        .window_size(iced::Size::new(600.0, dock::DOCK_HEIGHT as f32))
        .run_with(Shoal::new)?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn run_app() -> anyhow::Result<()> {
    connect_with_retry()
}

#[cfg(target_os = "linux")]
fn make_layer_settings() -> iced_layershell::settings::Settings<()> {
    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};

    Settings {
        layer_settings: LayerShellSettings {
            size: Some((INITIAL_SURFACE_WIDTH, dock::DOCK_HEIGHT)),
            anchor: Anchor::Bottom | Anchor::Left | Anchor::Right,
            exclusive_zone: dock::DOCK_HEIGHT as i32,
            layer: Layer::Top,
            keyboard_interactivity: KeyboardInteractivity::None,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(target_os = "linux")]
fn connect_with_retry() -> anyhow::Result<()> {
    use iced_layershell::Application as _;

    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 1..=COMPOSITOR_CONNECT_RETRIES {
        tracing::info!(
            "connecting to Wayland compositor (attempt {}/{})",
            attempt,
            COMPOSITOR_CONNECT_RETRIES
        );

        match ShoalLayerShell::run(make_layer_settings()) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let msg = e.to_string();
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
                    tracing::error!("fatal error starting shoal: {}", e);
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("compositor never became available")))
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct ShoalLayerShell {
    inner: Shoal,
}

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
            menu: self.menu.clone(),
            toast_state: self.toast_state.clone(),
            notification_counts: self.notification_counts.clone(),
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

    #[cfg(target_os = "linux")]
    #[test]
    fn initial_surface_width_is_nonzero() {
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
    fn shoal_starts_with_no_menu() {
        let (state, _task) = Shoal::new();
        assert!(state.menu.is_none(), "no menu should be open at startup");
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

    #[test]
    fn shoal_long_press_opens_menu() {
        let (mut state, _) = Shoal::new();
        // Add a running app so the menu has items
        state.running = vec![RunningApp {
            app_id: "firefox".into(),
            title: "Firefox".into(),
            is_focused: true,
        }];
        let _ = state.update(Message::AppLongPress("firefox".into()));
        assert!(state.menu.is_some(), "menu should open on long-press");
        let menu = state.menu.as_ref().expect("menu is Some");
        assert_eq!(menu.desktop_id, "firefox");
        assert!(menu.is_running);
    }

    #[test]
    fn shoal_menu_dismiss_closes_menu() {
        let (mut state, _) = Shoal::new();
        let _ = state.update(Message::AppLongPress("firefox".into()));
        assert!(state.menu.is_some());
        let _ = state.update(Message::MenuDismiss);
        assert!(state.menu.is_none(), "menu should close on dismiss");
    }

    #[test]
    fn shoal_tap_while_menu_open_dismisses() {
        let (mut state, _) = Shoal::new();
        let _ = state.update(Message::AppLongPress("firefox".into()));
        assert!(state.menu.is_some());
        let _ = state.update(Message::AppTap("firefox".into()));
        assert!(
            state.menu.is_none(),
            "tap while menu is open should dismiss"
        );
    }

    #[test]
    fn resolve_pinned_with_known_entries() {
        let entries = vec![DesktopEntry {
            name: "Firefox".into(),
            exec: "firefox".into(),
            icon: "firefox".into(),
            desktop_id: "firefox".into(),
        }];
        let ids = vec!["firefox".to_string()];
        let resolved = resolve_pinned(&ids, &entries);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].name, "Firefox");
    }

    #[test]
    fn resolve_pinned_creates_stub_for_missing() {
        let entries: Vec<DesktopEntry> = vec![];
        let ids = vec!["missing-app".to_string()];
        let resolved = resolve_pinned(&ids, &entries);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].desktop_id, "missing-app");
    }

    #[test]
    fn icon_for_menu_finds_matching_icon() {
        let icons = vec![DockIcon {
            entry: DesktopEntry {
                name: "Firefox".into(),
                exec: "firefox".into(),
                icon: "firefox".into(),
                desktop_id: "firefox".into(),
            },
            is_running: true,
            is_focused: false,
            is_pinned: true,
            scale: 1.0,
        }];
        let found = icon_for_menu(icons.iter(), "firefox");
        assert!(found.is_some());
        assert!(found.expect("found").is_pinned);
    }

    #[test]
    fn icon_for_menu_returns_none_for_missing() {
        let icons: Vec<DockIcon> = vec![];
        assert!(icon_for_menu(icons.iter(), "firefox").is_none());
    }

    #[test]
    fn shoal_starts_with_empty_toasts() {
        let (state, _) = Shoal::new();
        assert!(state.toast_state.is_empty(), "no toasts on startup");
    }

    #[test]
    fn shoal_pin_action_pushes_success_toast() {
        let (mut state, _) = Shoal::new();
        // Set up a running app with a matching desktop entry for name lookup
        state.all_entries.push(DesktopEntry {
            name: "Firefox".into(),
            exec: "firefox".into(),
            icon: "firefox".into(),
            desktop_id: "firefox".into(),
        });
        state.running = vec![RunningApp {
            app_id: "firefox".into(),
            title: "Firefox".into(),
            is_focused: true,
        }];
        let _ = state.update(Message::AppLongPress("firefox".into()));
        // Trigger a KeepInDock action
        let _ = state.update(Message::MenuAction(MenuAction::KeepInDock(
            "firefox".into(),
        )));
        assert_eq!(state.toast_state.len(), 1);
        assert_eq!(state.toast_state.toasts()[0].message(), "Pinned Firefox");
        assert_eq!(state.toast_state.toasts()[0].kind(), ToastKind::Success);
    }

    #[test]
    fn shoal_unpin_action_pushes_info_toast() {
        let (mut state, _) = Shoal::new();
        // Open a menu for a pinned app
        state.running = vec![RunningApp {
            app_id: "Alacritty".into(),
            title: "Alacritty".into(),
            is_focused: false,
        }];
        let _ = state.update(Message::AppLongPress("Alacritty".into()));
        let _ = state.update(Message::MenuAction(MenuAction::RemoveFromDock(
            "Alacritty".into(),
        )));
        assert_eq!(state.toast_state.len(), 1);
        assert_eq!(state.toast_state.toasts()[0].kind(), ToastKind::Info);
    }

    #[test]
    fn shoal_close_action_pushes_info_toast() {
        let (mut state, _) = Shoal::new();
        state.running = vec![RunningApp {
            app_id: "firefox".into(),
            title: "Firefox".into(),
            is_focused: true,
        }];
        let _ = state.update(Message::AppLongPress("firefox".into()));
        let _ = state.update(Message::MenuAction(MenuAction::CloseWindow(
            "firefox".into(),
        )));
        assert_eq!(state.toast_state.len(), 1);
        assert_eq!(state.toast_state.toasts()[0].message(), "App closed");
    }

    #[test]
    fn shoal_toast_tick_does_not_panic_when_empty() {
        let (mut state, _) = Shoal::new();
        let _ = state.update(Message::ToastTick);
        assert!(state.toast_state.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn compositor_retry_constants_are_sane() {
        assert!(COMPOSITOR_CONNECT_RETRIES > 0, "must retry at least once");
        assert!(
            COMPOSITOR_RETRY_DELAY_MS > 0,
            "delay must be positive to avoid spin-loop"
        );
        let max_wait_ms = u64::from(COMPOSITOR_CONNECT_RETRIES) * COMPOSITOR_RETRY_DELAY_MS;
        assert!(
            max_wait_ms < 30_000,
            "worst-case retry wait ({max_wait_ms}ms) should be under 30 s"
        );
    }
}
