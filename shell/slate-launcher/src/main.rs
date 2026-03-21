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
// Linux: iced_layershell fullscreen overlay with compositor retry
// -----------------------------------------------------------------------

/// How many times to retry connecting to the Wayland compositor before
/// giving up. Each attempt is separated by COMPOSITOR_RETRY_DELAY_MS.
///
/// The launcher starts early in the arkhe boot sequence; the retry loop
/// absorbs the race between arkhd launching the service and niri completing
/// its Wayland socket setup.
#[cfg(target_os = "linux")]
const COMPOSITOR_CONNECT_RETRIES: u32 = 10;

/// Delay in milliseconds between compositor connection attempts.
#[cfg(target_os = "linux")]
const COMPOSITOR_RETRY_DELAY_MS: u64 = 500;

/// Fallback surface width for the initial layer-shell surface creation.
///
/// wgpu panics if either surface dimension is zero when the swap-chain is
/// configured. We supply a non-zero default here; the compositor replaces
/// it with the real output dimensions once the fullscreen anchored surface
/// is mapped.
#[cfg(target_os = "linux")]
const INITIAL_SURFACE_WIDTH: u32 = 1280;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("slate_launcher=debug,warn")
        .init();

    tracing::info!("starting slate-launcher (layershell)");

    use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
    use iced_layershell::settings::{LayerShellSettings, Settings};

    let layer_settings = LayerShellSettings {
        // Non-zero width prevents the wgpu swap-chain panic on initial surface
        // creation. The compositor overrides both dimensions once the surface
        // is anchored to all four edges.
        size: Some((INITIAL_SURFACE_WIDTH, 0)),
        anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
        layer: Layer::Overlay,
        keyboard_interactivity: KeyboardInteractivity::Exclusive,
        exclusive_zone: -1,
        ..LayerShellSettings::default()
    };

    let settings: Settings<()> = Settings {
        layer_settings,
        ..Settings::default()
    };

    connect_with_retry(settings)
}

/// Attempt to start the iced_layershell application, retrying if the
/// Wayland compositor is not yet available.
///
/// `ConnectError(NoCompositor)` is returned by iced_layershell when the
/// `WAYLAND_DISPLAY` socket does not yet exist or the compositor has not
/// finished global-registry setup. We sleep briefly between retries to
/// avoid a spin-loop while the compositor boots.
///
/// Any other error is treated as fatal and propagated immediately.
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

        match LauncherLayerShell::run(settings.clone()) {
            Ok(()) => return Ok(()),
            Err(e) => {
                let msg = e.to_string();
                // NoCompositor / ConnectError indicate the compositor socket
                // is not ready yet — safe to retry.
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
                    // A non-transient error (wgpu adapter missing, protocol
                    // mismatch, etc.): log clearly and fail fast.
                    tracing::error!("fatal error starting slate-launcher: {}", e);
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("compositor never became available")))
}

/// Thin wrapper that adapts `Launcher` to the `iced_layershell::Application`
/// trait, which differs from the standard `iced::Application` trait.
#[cfg(target_os = "linux")]
struct LauncherLayerShell(Launcher);

#[cfg(target_os = "linux")]
impl iced_layershell::Application for LauncherLayerShell {
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

#[cfg(target_os = "linux")]
impl TryInto<iced_layershell::actions::LayershellCustomActions> for Message {
    type Error = Self;
    fn try_into(self) -> Result<iced_layershell::actions::LayershellCustomActions, Self::Error> {
        Err(self)
    }
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

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn initial_surface_width_is_nonzero() {
        // Prevents the wgpu zero-size surface panic on initial layer-shell setup.
        assert!(
            super::INITIAL_SURFACE_WIDTH > 0,
            "INITIAL_SURFACE_WIDTH must be > 0"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn compositor_retry_constants_are_sane() {
        assert!(
            super::COMPOSITOR_CONNECT_RETRIES > 0,
            "must retry at least once"
        );
        assert!(
            super::COMPOSITOR_RETRY_DELAY_MS > 0,
            "delay must be positive to avoid spin-loop"
        );
        // Worst-case wait must be under 30 s so the service manager does not
        // kill us for taking too long to start.
        let max_wait_ms =
            u64::from(super::COMPOSITOR_CONNECT_RETRIES) * super::COMPOSITOR_RETRY_DELAY_MS;
        assert!(
            max_wait_ms < 30_000,
            "worst-case retry wait ({max_wait_ms}ms) should be under 30 s"
        );
    }
}
