// Slate Launcher — fullscreen app launcher for Slate OS.
//
// Displays a searchable grid of all installed applications as a Wayland
// layer-shell overlay. Triggered by a 4-finger pinch gesture (via D-Bus
// from TouchFlow).
//
// On Linux: uses iced_layershell for a proper compositor overlay.
// On macOS: falls back to a plain iced window (for development only).

// Many public items in submodules are used only on Linux (layer-shell path)
// or only via D-Bus at runtime; suppress dead-code warnings for the macOS
// dev-build fallback.
#[allow(dead_code)]
mod apps;
#[allow(dead_code)]
mod dbus_listener;
#[allow(dead_code)]
mod grid;
#[allow(dead_code)]
mod launcher;
#[allow(dead_code)]
mod search;

use launcher::Launcher;
#[cfg(target_os = "linux")]
use launcher::Message;

// -----------------------------------------------------------------------
// Linux: iced_layershell fullscreen overlay
// -----------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use iced_layershell::actions::LayershellCustomActionsWithIdAndInfo;
    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};
    use iced_layershell::Application;

    tracing_subscriber::fmt()
        .with_env_filter("slate_launcher=debug,warn")
        .init();

    tracing::info!("starting slate-launcher (layershell)");

    let layer_settings = LayerShellSettings {
        size: None, // fullscreen
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        layer: Layer::Overlay,
        keyboard_interactivity: KeyboardInteractivity::Exclusive,
        exclusive_zone: -1,
        ..LayerShellSettings::default()
    };

    let settings = Settings {
        layer_settings,
        ..Settings::default()
    };

    // iced_layershell uses its own application trait; we wrap our Launcher.
    struct LayerLauncher(Launcher);

    impl iced_layershell::Application for LayerLauncher {
        type Message = Message;
        type Theme = iced::Theme;
        type Executor = iced::executor::Default;
        type Flags = ();

        fn new(_flags: ()) -> (Self, iced::Task<Self::Message>) {
            let (inner, task) = Launcher::new();
            (Self(inner), task)
        }

        fn namespace(&self) -> String {
            self.0.title()
        }

        fn update(&mut self, message: Self::Message) -> iced::Task<Self::Message> {
            self.0.update(message)
        }

        fn view(&self) -> iced::Element<'_, Self::Message, Self::Theme, iced::Renderer> {
            self.0.view()
        }

        fn theme(&self) -> Self::Theme {
            self.0.theme()
        }

        fn subscription(&self) -> iced::Subscription<Self::Message> {
            self.0.subscription()
        }
    }

    impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
        type Error = Self;
        fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self::Error> {
            Err(self)
        }
    }

    LayerLauncher::run(settings)?;
    Ok(())
}

// -----------------------------------------------------------------------
// macOS / other: plain iced window (dev fallback)
// -----------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("starting slate-launcher (plain window — non-Linux fallback)");

    iced::application(Launcher::title, Launcher::update, Launcher::view)
        .subscription(Launcher::subscription)
        .theme(Launcher::theme)
        .run_with(Launcher::new)?;

    Ok(())
}
