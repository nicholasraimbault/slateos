// Claw Panel -- OpenClaw AI sidebar for Slate OS.
//
// A right-anchored layer-shell panel that provides context-aware AI
// assistance through the local OpenClaw server. All intelligence lives
// server-side; this crate is the touch-friendly Wayland UI shell.
//
// Subscriptions:
//   1. D-Bus listener  -- palette changes + show/hide/toggle from TouchFlow
//   2. OpenClaw client  -- streaming WebSocket responses from the AI server
//   3. Context poll     -- periodic focused-window check via niri IPC

// Most runtime infrastructure (D-Bus, WebSocket, message variants) appears
// dead on macOS because the full wiring only runs on Linux with a live
// Wayland session. Suppress dead_code warnings crate-wide.
#![allow(dead_code)]

mod context;
mod conversation;
mod dbus_listener;
mod openclaw;
mod panel;

use context::WindowContext;
use conversation::Conversation;
use openclaw::{ClientEvent, OpenClawClient, QueryMessage};
use panel::PanelAction;
use slate_common::Palette;

// iced_layershell is Wayland-only; on macOS we fall back to a plain iced app
// for development purposes.
#[cfg(target_os = "linux")]
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};

/// Context poll interval: how often we check which window the user is focused on.
const CONTEXT_POLL_SECS: u64 = 2;

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct ClawPanel {
    conversation: Conversation,
    visible: bool,
    panel_width: u32,
    palette: Palette,
    current_context: Option<WindowContext>,
    input_text: String,
    openclaw_client: Option<OpenClawClient>,
    is_streaming: bool,
}

impl Default for ClawPanel {
    fn default() -> Self {
        Self {
            conversation: Conversation::new(),
            visible: false,
            panel_width: panel::DEFAULT_WIDTH,
            palette: Palette::default(),
            current_context: None,
            input_text: String::new(),
            openclaw_client: None,
            is_streaming: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    // Visibility
    Show,
    Hide,
    ToggleVisibility,

    // Palette
    PaletteChanged(Palette),

    // Context tracking
    ContextUpdated(Option<WindowContext>),

    // User input
    InputChanged(String),
    Send,

    // OpenClaw streaming response
    ResponseChunk(String),
    ResponseDone,
    OpenClawError(String),

    // Resize (left-edge drag)
    Resize(u32),

    // Panel view actions
    PanelAction(PanelAction),

    // Tick for context polling
    ContextPollTick,

    // D-Bus event forwarding
    DbusEvent(dbus_listener::DbusEvent),

    // OpenClaw client event forwarding
    OpenClawEvent(ClientEvent),

    // The OpenClaw client handle has been created by the background task.
    OpenClawClientReady(OpenClawClient),
}

// On Linux the message must be convertible to LayershellCustomActions. When no
// layer-shell action is needed we return the message as-is (the Err variant).
#[cfg(target_os = "linux")]
impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
    type Error = Self;
    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self> {
        Err(self)
    }
}

// ---------------------------------------------------------------------------
// Application trait implementation (Linux -- layer shell)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
impl iced_layershell::Application for ClawPanel {
    type Executor = iced::executor::Default;
    type Message = Message;
    type Theme = iced::Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, iced::Task<Message>) {
        (Self::default(), iced::Task::none())
    }

    fn namespace(&self) -> String {
        "claw-panel".to_string()
    }

    fn update(&mut self, message: Message) -> iced::Task<Message> {
        update_app(self, message)
    }

    fn view(&self) -> iced::Element<'_, Message, iced::Theme, iced::Renderer> {
        view_app(self)
    }

    fn theme(&self) -> iced::Theme {
        slate_common::theme::create_theme(&self.palette)
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        subscription_app(self)
    }
}

// ---------------------------------------------------------------------------
// macOS fallback: plain iced app (no layer shell)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
impl ClawPanel {
    fn run_fallback() -> iced::Result {
        iced::application("Claw Panel", Self::update_iced, Self::view_iced)
            .subscription(Self::subscription_iced)
            .theme(Self::theme_iced)
            .run()
    }

    fn update_iced(&mut self, message: Message) -> iced::Task<Message> {
        update_app(self, message)
    }

    fn view_iced(&self) -> iced::Element<'_, Message> {
        view_app(self)
    }

    fn theme_iced(&self) -> iced::Theme {
        slate_common::theme::create_theme(&self.palette)
    }

    fn subscription_iced(&self) -> iced::Subscription<Message> {
        subscription_app(self)
    }
}

// ---------------------------------------------------------------------------
// Shared logic (platform-agnostic)
// ---------------------------------------------------------------------------

fn update_app(app: &mut ClawPanel, message: Message) -> iced::Task<Message> {
    match message {
        Message::Show => {
            app.visible = true;
        }
        Message::Hide => {
            app.visible = false;
        }
        Message::ToggleVisibility => {
            app.visible = !app.visible;
        }
        Message::PaletteChanged(palette) => {
            app.palette = palette;
        }
        Message::ContextUpdated(ctx) => {
            app.current_context = ctx;
        }
        Message::InputChanged(text) => {
            app.input_text = text;
        }
        Message::Send => {
            let content = app.input_text.trim().to_string();
            if content.is_empty() {
                return iced::Task::none();
            }

            app.conversation.add_user_message(content.clone());
            app.input_text.clear();
            app.is_streaming = true;

            if let Some(client) = app.openclaw_client.clone() {
                let query = QueryMessage::new(content, app.current_context.clone());
                return iced::Task::perform(
                    async move { client.send_query(query).await },
                    |result| match result {
                        // Query dispatched; response chunks arrive via the
                        // OpenClawEvent subscription.
                        Ok(()) => Message::ResponseDone,
                        Err(e) => Message::OpenClawError(e.to_string()),
                    },
                );
            }

            // No client connected -- show an error inline.
            app.conversation
                .add_assistant_message("OpenClaw unavailable".to_string());
            app.is_streaming = false;
        }
        Message::ResponseChunk(chunk) => {
            app.conversation.append_to_assistant(&chunk);
        }
        Message::ResponseDone => {
            app.is_streaming = false;
        }
        Message::OpenClawError(err) => {
            app.is_streaming = false;
            app.conversation
                .add_assistant_message(format!("Error: {err}"));
        }
        Message::Resize(width) => {
            app.panel_width = width.clamp(panel::MIN_WIDTH, panel::MAX_WIDTH);
        }
        Message::PanelAction(action) => match action {
            PanelAction::Close => {
                app.visible = false;
            }
            PanelAction::InputChanged(text) => {
                app.input_text = text;
            }
            PanelAction::Send => {
                return update_app(app, Message::Send);
            }
            PanelAction::ApplyCodeBlock(_code) => {
                // Applying code blocks is a future feature that would
                // communicate with the focused editor via some protocol.
                tracing::info!("Apply code block requested (not yet implemented)");
            }
        },
        Message::ContextPollTick => {
            return iced::Task::perform(context::poll_focused_window(), |result| {
                Message::ContextUpdated(result.unwrap_or(None))
            });
        }
        Message::DbusEvent(event) => match event {
            dbus_listener::DbusEvent::Show => {
                app.visible = true;
            }
            dbus_listener::DbusEvent::Hide => {
                app.visible = false;
            }
            dbus_listener::DbusEvent::Toggle => {
                app.visible = !app.visible;
            }
            dbus_listener::DbusEvent::PaletteChanged(palette) => {
                app.palette = palette;
            }
        },
        Message::OpenClawEvent(event) => match event {
            ClientEvent::Connected => {
                tracing::info!("Connected to OpenClaw");
            }
            ClientEvent::Chunk(chunk) => {
                app.conversation.append_to_assistant(&chunk);
            }
            ClientEvent::Done => {
                app.is_streaming = false;
            }
            ClientEvent::Error(err) => {
                app.is_streaming = false;
                app.conversation
                    .add_assistant_message(format!("Error: {err}"));
            }
            ClientEvent::Disconnected => {
                tracing::warn!("Disconnected from OpenClaw");
            }
        },
        Message::OpenClawClientReady(client) => {
            tracing::info!("OpenClaw client handle ready");
            app.openclaw_client = Some(client);
        }
    }

    iced::Task::none()
}

fn view_app(app: &ClawPanel) -> iced::Element<'_, Message> {
    if !app.visible {
        // When hidden, render an empty zero-width container.
        return iced::widget::container(iced::widget::text(""))
            .width(0)
            .height(iced::Length::Fill)
            .into();
    }

    panel::view(
        &app.conversation,
        &app.current_context,
        &app.input_text,
        app.is_streaming,
    )
    .map(Message::PanelAction)
}

/// Build all iced subscriptions: D-Bus events, OpenClaw WebSocket, and
/// periodic context polling.
fn subscription_app(_app: &ClawPanel) -> iced::Subscription<Message> {
    let dbus_sub = dbus_subscription();
    let openclaw_sub = openclaw_subscription();
    let context_sub = iced::time::every(std::time::Duration::from_secs(CONTEXT_POLL_SECS))
        .map(|_| Message::ContextPollTick);

    iced::Subscription::batch([dbus_sub, openclaw_sub, context_sub])
}

/// Subscription that runs the D-Bus listener and forwards events as Messages.
///
/// Uses `iced::stream::channel` with a stable subscription ID so the stream
/// is created exactly once and persists for the lifetime of the app.
fn dbus_subscription() -> iced::Subscription<Message> {
    use iced::futures::SinkExt;

    let stream = iced::stream::channel(50, |mut output| async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        // Start the D-Bus listener in a background task.
        tokio::spawn(dbus_listener::run(event_tx));

        // Forward D-Bus events into the iced message stream.
        loop {
            match event_rx.recv().await {
                Some(event) => {
                    let msg = Message::DbusEvent(event);
                    // Await back-pressure from the iced runtime.
                    if output.send(msg).await.is_err() {
                        tracing::warn!("D-Bus subscription channel closed");
                        break;
                    }
                }
                None => {
                    // Channel closed -- the D-Bus listener task ended.
                    tracing::warn!("D-Bus event channel closed unexpectedly");
                    std::future::pending::<()>().await;
                }
            }
        }
    });

    iced::Subscription::run_with_id("claw-panel-dbus", stream)
}

/// Subscription that runs the OpenClaw WebSocket client and forwards events
/// as Messages.
///
/// On the first iteration, this spawns the WebSocket background loop and
/// sends an `OpenClawClientReady` message so the app can store the client
/// handle for sending queries.
fn openclaw_subscription() -> iced::Subscription<Message> {
    use iced::futures::SinkExt;

    let stream = iced::stream::channel(100, |mut output| async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        // Spawn the WebSocket background loop and get a client handle.
        let client = openclaw::run(event_tx).await;

        // Inform the app that the client handle is ready for sending queries.
        let _ = output.send(Message::OpenClawClientReady(client)).await;

        // Forward OpenClaw events into the iced message stream.
        loop {
            match event_rx.recv().await {
                Some(event) => {
                    let msg = Message::OpenClawEvent(event);
                    // Await back-pressure from the iced runtime.
                    if output.send(msg).await.is_err() {
                        tracing::warn!("OpenClaw subscription channel closed");
                        break;
                    }
                }
                None => {
                    // Channel closed -- the WebSocket loop ended.
                    tracing::warn!("OpenClaw event channel closed unexpectedly");
                    std::future::pending::<()>().await;
                }
            }
        }
    });

    iced::Subscription::run_with_id("claw-panel-openclaw", stream)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("claw_panel=debug,warn")
        .init();

    tracing::info!("Claw Panel starting");

    #[cfg(target_os = "linux")]
    {
        use iced_layershell::settings::{LayerShellSettings, Settings};

        let settings = Settings {
            id: Some("claw-panel".to_string()),
            layer_settings: LayerShellSettings {
                anchor: Anchor::Right | Anchor::Top | Anchor::Bottom,
                layer: Layer::Top,
                exclusive_zone: 0,
                size: Some((panel::DEFAULT_WIDTH, 0)),
                margin: (0, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                binded_output_name: None,
            },
            flags: (),
            ..Default::default()
        };

        if let Err(e) = <ClawPanel as iced_layershell::Application>::run(settings) {
            tracing::error!("Claw Panel exited with error: {e}");
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        if let Err(e) = ClawPanel::run_fallback() {
            tracing::error!("Claw Panel exited with error: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_panel_is_hidden() {
        let panel = ClawPanel::default();
        assert!(!panel.visible);
    }

    #[test]
    fn update_show_sets_visible() {
        let mut panel = ClawPanel::default();
        let _ = update_app(&mut panel, Message::Show);
        assert!(panel.visible);
    }

    #[test]
    fn update_hide_clears_visible() {
        let mut panel = ClawPanel::default();
        panel.visible = true;
        let _ = update_app(&mut panel, Message::Hide);
        assert!(!panel.visible);
    }

    #[test]
    fn update_toggle_flips_visibility() {
        let mut panel = ClawPanel::default();
        assert!(!panel.visible);

        let _ = update_app(&mut panel, Message::ToggleVisibility);
        assert!(panel.visible);

        let _ = update_app(&mut panel, Message::ToggleVisibility);
        assert!(!panel.visible);
    }

    #[test]
    fn update_palette_changed_stores_palette() {
        let mut panel = ClawPanel::default();
        let new_palette = Palette {
            primary: [255, 0, 0, 255],
            ..Palette::default()
        };
        let _ = update_app(&mut panel, Message::PaletteChanged(new_palette.clone()));
        assert_eq!(panel.palette, new_palette);
    }

    #[test]
    fn update_input_changed_stores_text() {
        let mut panel = ClawPanel::default();
        let _ = update_app(&mut panel, Message::InputChanged("hello".to_string()));
        assert_eq!(panel.input_text, "hello");
    }

    #[test]
    fn update_send_on_empty_input_does_nothing() {
        let mut panel = ClawPanel::default();
        panel.input_text = "   ".to_string();
        let _ = update_app(&mut panel, Message::Send);
        assert!(panel.conversation.is_empty());
        assert!(!panel.is_streaming);
    }

    #[test]
    fn update_send_without_client_adds_error_message() {
        let mut panel = ClawPanel::default();
        panel.input_text = "test query".to_string();
        let _ = update_app(&mut panel, Message::Send);

        assert_eq!(panel.conversation.len(), 2);
        assert!(!panel.is_streaming);
        assert!(panel.input_text.is_empty());
    }

    #[test]
    fn update_resize_clamps_width() {
        let mut panel = ClawPanel::default();
        let _ = update_app(&mut panel, Message::Resize(100));
        assert_eq!(panel.panel_width, panel::MIN_WIDTH);

        let _ = update_app(&mut panel, Message::Resize(9999));
        assert_eq!(panel.panel_width, panel::MAX_WIDTH);

        let _ = update_app(&mut panel, Message::Resize(500));
        assert_eq!(panel.panel_width, 500);
    }

    #[test]
    fn update_dbus_show_sets_visible() {
        let mut panel = ClawPanel::default();
        let _ = update_app(
            &mut panel,
            Message::DbusEvent(dbus_listener::DbusEvent::Show),
        );
        assert!(panel.visible);
    }

    #[test]
    fn update_dbus_toggle_flips_visibility() {
        let mut panel = ClawPanel::default();
        let _ = update_app(
            &mut panel,
            Message::DbusEvent(dbus_listener::DbusEvent::Toggle),
        );
        assert!(panel.visible);

        let _ = update_app(
            &mut panel,
            Message::DbusEvent(dbus_listener::DbusEvent::Toggle),
        );
        assert!(!panel.visible);
    }

    #[test]
    fn update_dbus_palette_updates_theme() {
        let mut panel = ClawPanel::default();
        let custom = Palette {
            primary: [0, 255, 0, 255],
            ..Palette::default()
        };
        let _ = update_app(
            &mut panel,
            Message::DbusEvent(dbus_listener::DbusEvent::PaletteChanged(custom.clone())),
        );
        assert_eq!(panel.palette, custom);
    }

    #[test]
    fn update_openclaw_chunk_appends_text() {
        let mut panel = ClawPanel::default();
        panel.is_streaming = true;
        let _ = update_app(
            &mut panel,
            Message::OpenClawEvent(ClientEvent::Chunk("hello ".to_string())),
        );
        let _ = update_app(
            &mut panel,
            Message::OpenClawEvent(ClientEvent::Chunk("world".to_string())),
        );
        assert_eq!(panel.conversation.len(), 1);
        assert_eq!(panel.conversation.messages()[0].content, "hello world");
    }

    #[test]
    fn update_openclaw_done_stops_streaming() {
        let mut panel = ClawPanel::default();
        panel.is_streaming = true;
        let _ = update_app(&mut panel, Message::OpenClawEvent(ClientEvent::Done));
        assert!(!panel.is_streaming);
    }

    #[test]
    fn update_openclaw_error_stops_streaming_and_shows_error() {
        let mut panel = ClawPanel::default();
        panel.is_streaming = true;
        let _ = update_app(
            &mut panel,
            Message::OpenClawEvent(ClientEvent::Error("timeout".to_string())),
        );
        assert!(!panel.is_streaming);
        assert_eq!(panel.conversation.len(), 1);
    }

    #[test]
    fn update_context_updated_stores_context() {
        let mut panel = ClawPanel::default();
        let ctx = WindowContext {
            app_id: "firefox".to_string(),
            title: "GitHub".to_string(),
        };
        let _ = update_app(&mut panel, Message::ContextUpdated(Some(ctx.clone())));
        assert_eq!(panel.current_context, Some(ctx));
    }

    #[test]
    fn update_panel_action_close_hides() {
        let mut panel = ClawPanel::default();
        panel.visible = true;
        let _ = update_app(&mut panel, Message::PanelAction(PanelAction::Close));
        assert!(!panel.visible);
    }

    #[test]
    fn context_poll_interval_is_reasonable() {
        assert!(
            CONTEXT_POLL_SECS >= 1 && CONTEXT_POLL_SECS <= 10,
            "context poll interval should be between 1 and 10 seconds"
        );
    }
}
