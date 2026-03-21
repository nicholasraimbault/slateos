// Claw Panel — OpenClaw AI sidebar for Slate OS.
//
// A right-anchored layer-shell panel that provides context-aware AI
// assistance through the local OpenClaw server. All intelligence lives
// server-side; this crate is the touch-friendly Wayland UI shell.

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
    // Channel receivers held as Options so they can be .take()-en into
    // subscriptions without requiring mutability in view().
    dbus_rx: Option<tokio::sync::mpsc::UnboundedReceiver<dbus_listener::DbusEvent>>,
    openclaw_rx: Option<tokio::sync::mpsc::UnboundedReceiver<ClientEvent>>,
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
            dbus_rx: None,
            openclaw_rx: None,
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
// Application trait implementation (Linux — layer shell)
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

            // No client connected — show an error inline.
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

fn subscription_app(_app: &ClawPanel) -> iced::Subscription<Message> {
    // Context polling: every 1 second.
    iced::time::every(std::time::Duration::from_secs(1)).map(|_| Message::ContextPollTick)
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
