// D-Bus interface constants and service definitions for Slate OS.
//
// Every crate that communicates over D-Bus imports these constants so all
// components agree on bus names, object paths, and interface names.

// ---------------------------------------------------------------------------
// Palette daemon (slate-palette)
// ---------------------------------------------------------------------------
pub const PALETTE_INTERFACE: &str = "org.slate.Palette";
pub const PALETTE_PATH: &str = "/org/slate/Palette";
pub const PALETTE_BUS_NAME: &str = "org.slate.Palette";

// ---------------------------------------------------------------------------
// Gesture daemon (touchflow)
// ---------------------------------------------------------------------------
pub const TOUCHFLOW_INTERFACE: &str = "org.slate.TouchFlow";
pub const TOUCHFLOW_PATH: &str = "/org/slate/TouchFlow";
pub const TOUCHFLOW_BUS_NAME: &str = "org.slate.TouchFlow";

// ---------------------------------------------------------------------------
// Dock (shoal)
// ---------------------------------------------------------------------------
pub const DOCK_INTERFACE: &str = "org.slate.Dock";
pub const DOCK_PATH: &str = "/org/slate/Dock";

// ---------------------------------------------------------------------------
// Launcher (slate-launcher)
// ---------------------------------------------------------------------------
pub const LAUNCHER_INTERFACE: &str = "org.slate.Launcher";
pub const LAUNCHER_PATH: &str = "/org/slate/Launcher";

// ---------------------------------------------------------------------------
// AI sidebar (claw-panel)
// ---------------------------------------------------------------------------
pub const CLAW_INTERFACE: &str = "org.slate.Claw";
pub const CLAW_PATH: &str = "/org/slate/Claw";

// ---------------------------------------------------------------------------
// Suggestion bar (slate-suggest)
// ---------------------------------------------------------------------------
pub const SUGGEST_INTERFACE: &str = "org.slate.Suggest";
pub const SUGGEST_PATH: &str = "/org/slate/Suggest";

// ---------------------------------------------------------------------------
// Settings app (slate-settings)
// ---------------------------------------------------------------------------
pub const SETTINGS_INTERFACE: &str = "org.slate.Settings";
pub const SETTINGS_PATH: &str = "/org/slate/Settings";

// ---------------------------------------------------------------------------
// Rhea AI engine
// ---------------------------------------------------------------------------
pub const RHEA_INTERFACE: &str = "org.slate.Rhea";
pub const RHEA_PATH: &str = "/org/slate/Rhea";
pub const RHEA_BUS_NAME: &str = "org.slate.Rhea";

// ---------------------------------------------------------------------------
// Notification daemon (slate-notifyd)
// ---------------------------------------------------------------------------
pub const NOTIFICATIONS_INTERFACE: &str = "org.slate.Notifications";
pub const NOTIFICATIONS_PATH: &str = "/org/slate/Notifications";
pub const NOTIFICATIONS_BUS_NAME: &str = "org.slate.Notifications";

// ---------------------------------------------------------------------------
// Notification shade (slate-shade)
// ---------------------------------------------------------------------------
pub const SHADE_INTERFACE: &str = "org.slate.Shade";
pub const SHADE_PATH: &str = "/org/slate/Shade";

// ---------------------------------------------------------------------------
// PaletteService — zbus interface for broadcasting palette changes
// ---------------------------------------------------------------------------

/// D-Bus service that holds the current palette and emits a signal on change.
/// The palette is serialised as TOML so consumers can deserialise into
/// `slate_common::Palette` without depending on a binary format.
pub struct PaletteService {
    pub palette_toml: String,
}

#[zbus::interface(name = "org.slate.Palette")]
impl PaletteService {
    /// The current palette as a TOML string.
    #[zbus(property)]
    async fn palette_toml(&self) -> &str {
        &self.palette_toml
    }

    /// Signal emitted whenever the palette changes.
    #[zbus(signal)]
    async fn changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        palette_toml: &str,
    ) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_well_formed() {
        // All interface names must start with "org.slate."
        for iface in [
            PALETTE_INTERFACE,
            TOUCHFLOW_INTERFACE,
            DOCK_INTERFACE,
            LAUNCHER_INTERFACE,
            CLAW_INTERFACE,
            SUGGEST_INTERFACE,
            SETTINGS_INTERFACE,
            RHEA_INTERFACE,
            NOTIFICATIONS_INTERFACE,
            SHADE_INTERFACE,
        ] {
            assert!(
                iface.starts_with("org.slate."),
                "{iface} should start with org.slate."
            );
        }

        // All paths must start with "/org/slate/"
        for path in [
            PALETTE_PATH,
            TOUCHFLOW_PATH,
            DOCK_PATH,
            LAUNCHER_PATH,
            CLAW_PATH,
            SUGGEST_PATH,
            SETTINGS_PATH,
            RHEA_PATH,
            NOTIFICATIONS_PATH,
            SHADE_PATH,
        ] {
            assert!(
                path.starts_with("/org/slate/"),
                "{path} should start with /org/slate/"
            );
        }
    }

    #[test]
    fn palette_bus_name_matches_interface() {
        assert_eq!(PALETTE_BUS_NAME, PALETTE_INTERFACE);
    }

    #[test]
    fn rhea_bus_name_matches_interface() {
        assert_eq!(RHEA_BUS_NAME, RHEA_INTERFACE);
    }

    #[test]
    fn notifications_bus_name_matches_interface() {
        assert_eq!(NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE);
    }

    #[test]
    fn touchflow_bus_name_matches_interface() {
        assert_eq!(TOUCHFLOW_BUS_NAME, TOUCHFLOW_INTERFACE);
    }
}
