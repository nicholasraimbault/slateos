/// D-Bus broadcast for palette changes.
///
/// Registers the `PaletteService` on the session bus so other Slate OS
/// components can subscribe to palette updates. On each change we update
/// the property and emit the `changed` signal.
use anyhow::{Context, Result};
use slate_common::dbus::{PaletteService, PALETTE_BUS_NAME, PALETTE_INTERFACE, PALETTE_PATH};
use slate_common::Palette;
use zbus::connection::Builder;
use zbus::Connection;

/// Build a D-Bus session connection with the `PaletteService` served at
/// the well-known name and object path from `slate_common::dbus`.
pub async fn create_connection(initial_palette: &Palette) -> Result<Connection> {
    let toml_str = toml::to_string(initial_palette).context("serialise initial palette")?;
    let service = PaletteService {
        palette_toml: toml_str,
    };

    let connection = Builder::session()
        .context("open session bus")?
        .name(PALETTE_BUS_NAME)
        .context("request bus name")?
        .serve_at(PALETTE_PATH, service)
        .context("serve PaletteService")?
        .build()
        .await
        .context("build D-Bus connection")?;

    tracing::info!("D-Bus: serving {PALETTE_BUS_NAME} at {PALETTE_PATH}");
    Ok(connection)
}

/// Broadcast a palette change over D-Bus.
///
/// Updates the `palette_toml` property on the object server and emits the
/// `changed` signal so subscribers get notified.
pub async fn broadcast_palette(connection: &Connection, palette: &Palette) -> Result<()> {
    let toml_str = toml::to_string(palette).context("serialise palette for broadcast")?;

    let object_server = connection.object_server();
    let iface_ref = object_server
        .interface::<_, PaletteService>(PALETTE_PATH)
        .await
        .context("get PaletteService interface ref")?;

    // Update the property value.
    {
        let mut iface = iface_ref.get_mut().await;
        iface.palette_toml = toml_str.clone();
    }

    // Emit the "changed" signal manually via the low-level SignalEmitter API.
    // The zbus #[zbus(signal)] macro generates a crate-private function, so
    // from a different crate we emit the signal directly.
    iface_ref
        .signal_emitter()
        .emit(PALETTE_INTERFACE, "changed", &(&toml_str,))
        .await
        .context("emit changed signal")?;

    tracing::info!("broadcast palette change over D-Bus");
    Ok(())
}

#[cfg(test)]
mod tests {
    use slate_common::dbus::PaletteService;
    use slate_common::Palette;

    #[test]
    fn palette_service_can_be_constructed() {
        let palette = Palette::default();
        let toml_str = toml::to_string(&palette).unwrap();
        let service = PaletteService {
            palette_toml: toml_str.clone(),
        };
        assert_eq!(service.palette_toml, toml_str);
    }

    #[test]
    fn palette_service_toml_round_trips() {
        let palette = Palette {
            primary: [200, 100, 50, 255],
            secondary: [150, 120, 90, 255],
            surface: [10, 10, 15, 255],
            container: [20, 20, 30, 255],
            neutral: [220, 220, 225, 255],
        };
        let toml_str = toml::to_string(&palette).unwrap();
        let service = PaletteService {
            palette_toml: toml_str,
        };
        let back: Palette = toml::from_str(&service.palette_toml).unwrap();
        assert_eq!(palette, back);
    }
}
