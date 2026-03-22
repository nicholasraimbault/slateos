// Claw Panel -- AI sidebar for SlateOS.
//
// A right-anchored layer-shell panel that provides context-aware AI
// assistance through the Rhea AI engine (accessed over D-Bus). All
// intelligence lives in Rhea; this crate is the touch-friendly Wayland UI.
//
// Subscriptions:
//   1. D-Bus listener  -- palette changes + show/hide/toggle from TouchFlow
//                         + streaming Rhea CompletionChunk/Done/Error signals
//   2. Context poll    -- periodic focused-window check via niri IPC
// Most runtime infrastructure (D-Bus, message variants) appears dead on macOS
// because the full wiring only runs on Linux with a live Wayland session.
// Suppress dead_code warnings crate-wide.
#![allow(dead_code)]

mod clipboard;
mod context;
mod conversation;
mod dbus_listener;
mod panel;
mod toast;

use context::WindowContext;
use conversation::Conversation;
use panel::PanelAction;
use slate_common::Palette;
use toast::ToastState;

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
    is_streaming: bool,
    toast_state: ToastState,
    /// Name of the active Rhea backend (e.g. "local", "cloud"). Empty until
    /// we receive a BackendChanged signal or query GetStatus on startup.
    rhea_backend: String,
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
            is_streaming: false,
            toast_state: ToastState::new(),
            rhea_backend: String::new(),
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

    // Rhea D-Bus send result (fires once CompleteStream call returns or fails)
    RheaSendResult(Result<(), String>),

    // Resize (left-edge drag)
    Resize(u32),

    // Panel view actions
    PanelAction(PanelAction),

    // Tick for context polling
    ContextPollTick,

    // D-Bus event forwarding (includes Rhea streaming signals)
    DbusEvent(dbus_listener::DbusEvent),

    // Clipboard copy result (success message or error string)
    ClipboardResult(Result<String, String>),

    // Toast auto-dismiss tick
    ToastTick,
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

            // Invoke Rhea.CompleteStream over D-Bus. Streaming chunks arrive
            // via the DbusEvent subscription (RheaChunk/RheaDone/RheaError).
            return iced::Task::perform(call_rhea_complete_stream(content), |result| {
                Message::RheaSendResult(result)
            });
        }
        Message::RheaSendResult(result) => {
            if let Err(e) = result {
                // The D-Bus call itself failed before any chunks arrived.
                app.is_streaming = false;
                app.conversation
                    .add_assistant_message(format!("Rhea unavailable: {e}"));
            }
            // On Ok(()), streaming chunks will arrive via DbusEvent.
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
            PanelAction::ApplyCodeBlock(code) => {
                let context = app.current_context.clone();
                return iced::Task::perform(
                    apply_code_block(code, context),
                    Message::ClipboardResult,
                );
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
            dbus_listener::DbusEvent::RheaChunk(chunk) => {
                app.conversation.append_to_assistant(&chunk);
            }
            dbus_listener::DbusEvent::RheaDone => {
                app.is_streaming = false;
            }
            dbus_listener::DbusEvent::RheaError(err) => {
                app.is_streaming = false;
                app.conversation
                    .add_assistant_message(format!("Error: {err}"));
            }
            dbus_listener::DbusEvent::RheaBackendChanged(name) => {
                tracing::info!("Rhea backend changed to: {name}");
                app.rhea_backend = name;
            }
        },
        Message::ClipboardResult(result) => match result {
            Ok(msg) => {
                app.toast_state.show_success(msg);
            }
            Err(err) => {
                tracing::warn!("clipboard operation failed: {err}");
                app.toast_state.show_error(err);
            }
        },
        Message::ToastTick => {
            app.toast_state.tick();
        }
    }

    iced::Task::none()
}

/// Copy a code block to clipboard and optionally inject into a focused terminal.
///
/// Returns `Ok(message)` with a user-facing success string, or `Err(message)`
/// with a user-facing error string. Both variants are suitable for displaying
/// in a toast notification.
async fn apply_code_block(code: String, context: Option<WindowContext>) -> Result<String, String> {
    // Always copy to clipboard first.
    clipboard::copy_to_clipboard(&code)
        .await
        .map_err(|e| format!("Copy failed: {e}"))?;

    // If the focused window is a terminal, also inject via wtype.
    if let Some(ref ctx) = context {
        if clipboard::is_terminal_window(&ctx.app_id, &ctx.title) {
            match clipboard::inject_text_wtype(&code).await {
                Ok(()) => {
                    tracing::info!("code block copied and injected into terminal");
                    return Ok("Copied & pasted!".to_string());
                }
                Err(e) => {
                    // wtype failed but clipboard copy succeeded -- still a
                    // partial success.
                    tracing::warn!("wtype injection failed: {e}");
                    return Ok("Copied! (paste failed)".to_string());
                }
            }
        }
    }

    tracing::info!("code block copied to clipboard");
    Ok("Copied!".to_string())
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
        &app.toast_state,
        app.palette.surface,
        &app.rhea_backend,
    )
    .map(Message::PanelAction)
}

/// Build all iced subscriptions: D-Bus events (including Rhea AI signals),
/// periodic context polling, and toast auto-dismiss.
fn subscription_app(app: &ClawPanel) -> iced::Subscription<Message> {
    let dbus_sub = dbus_subscription();
    let context_sub = iced::time::every(std::time::Duration::from_secs(CONTEXT_POLL_SECS))
        .map(|_| Message::ContextPollTick);

    let mut subs = vec![dbus_sub, context_sub];

    // Only tick the toast timer when a toast is visible, to avoid
    // unnecessary wakeups when the panel is idle.
    if app.toast_state.is_visible() {
        let toast_sub =
            iced::time::every(std::time::Duration::from_millis(250)).map(|_| Message::ToastTick);
        subs.push(toast_sub);
    }

    iced::Subscription::batch(subs)
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

/// Call `org.slate.Rhea.CompleteStream` over D-Bus with the user's prompt.
///
/// The actual response text arrives as `CompletionChunk` D-Bus signals which
/// are picked up by `dbus_listener::watch_rhea` and forwarded as
/// `DbusEvent::RheaChunk` messages. This function only initiates the call and
/// returns once the method returns (or errors).
///
/// A system context string is intentionally left empty here because Rhea
/// gathers shell context internally (focused window, clipboard, etc.).
#[cfg(target_os = "linux")]
async fn call_rhea_complete_stream(prompt: String) -> Result<(), String> {
    use slate_common::dbus::{RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH};

    let connection = zbus::Connection::session()
        .await
        .map_err(|e| e.to_string())?;

    let proxy = zbus::Proxy::new(&connection, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE)
        .await
        .map_err(|e| e.to_string())?;

    // Empty system prompt -- Rhea supplies its own default.
    proxy
        .call_method("CompleteStream", &(prompt.as_str(), ""))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Non-Linux stub: always returns an error so the UI can show a message.
#[cfg(not(target_os = "linux"))]
async fn call_rhea_complete_stream(_prompt: String) -> Result<(), String> {
    Err("Rhea D-Bus is only available on Linux".to_string())
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
mod tests;
