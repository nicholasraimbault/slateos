/// Stub D-Bus wrappers for system hardware services.
///
/// These return stub values (`Ok(false)` / `Ok(())`) because real hardware
/// integration depends on device-specific drivers that are only available on
/// target hardware. Each function documents what the real implementation
/// will do once the hardware layer is ready.

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SystemError {
    #[error("D-Bus call failed: {0}")]
    Dbus(String),

    #[error("hardware not available: {0}")]
    Unavailable(String),
}

// ---------------------------------------------------------------------------
// WiFi
// ---------------------------------------------------------------------------

/// Check whether WiFi is currently enabled.
///
/// Real implementation will query NetworkManager or iwd over D-Bus.
pub async fn wifi_enabled() -> Result<bool, SystemError> {
    Ok(false)
}

/// Toggle WiFi on or off.
///
/// Real implementation will call NetworkManager/iwd to enable/disable
/// the wireless radio.
pub async fn set_wifi_enabled(_enabled: bool) -> Result<(), SystemError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Bluetooth
// ---------------------------------------------------------------------------

/// Check whether Bluetooth is currently enabled.
///
/// Real implementation will query bluez over D-Bus.
pub async fn bluetooth_enabled() -> Result<bool, SystemError> {
    Ok(false)
}

/// Toggle Bluetooth on or off.
///
/// Real implementation will call bluez to power the adapter on/off.
pub async fn set_bluetooth_enabled(_enabled: bool) -> Result<(), SystemError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------

/// Get the current audio volume as a fraction in [0.0, 1.0].
///
/// Real implementation will query PipeWire/PulseAudio over D-Bus.
pub async fn get_volume() -> Result<f32, SystemError> {
    Ok(0.0)
}

/// Set the audio volume as a fraction in [0.0, 1.0].
///
/// Real implementation will call PipeWire/PulseAudio to set the
/// default sink volume.
pub async fn set_volume(_level: f32) -> Result<(), SystemError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Display
// ---------------------------------------------------------------------------

/// Get the current display brightness as a fraction in [0.0, 1.0].
///
/// Real implementation will read from sysfs backlight or call logind.
pub async fn get_brightness() -> Result<f32, SystemError> {
    Ok(0.0)
}

/// Set the display brightness as a fraction in [0.0, 1.0].
///
/// Real implementation will write to sysfs backlight or call logind.
pub async fn set_brightness(_level: f32) -> Result<(), SystemError> {
    Ok(())
}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

/// Check whether the device is currently on AC power.
///
/// Real implementation will query UPower over D-Bus.
pub async fn on_ac_power() -> Result<bool, SystemError> {
    Ok(false)
}

/// Get the battery charge percentage in [0, 100], or None if no battery.
///
/// Real implementation will query UPower for battery state.
pub async fn battery_percent() -> Result<Option<u8>, SystemError> {
    Ok(None)
}

// ---------------------------------------------------------------------------
// Connectivity
// ---------------------------------------------------------------------------

/// Check whether the device has an active network connection.
///
/// Real implementation will query NetworkManager connectivity state.
pub async fn is_connected() -> Result<bool, SystemError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wifi_enabled_stub_returns_false() {
        assert_eq!(wifi_enabled().await.expect("should not error"), false);
    }

    #[tokio::test]
    async fn set_wifi_enabled_stub_succeeds() {
        set_wifi_enabled(true).await.expect("should not error");
        set_wifi_enabled(false).await.expect("should not error");
    }

    #[tokio::test]
    async fn bluetooth_enabled_stub_returns_false() {
        assert_eq!(bluetooth_enabled().await.expect("should not error"), false);
    }

    #[tokio::test]
    async fn set_bluetooth_enabled_stub_succeeds() {
        set_bluetooth_enabled(true).await.expect("should not error");
    }

    #[tokio::test]
    async fn get_volume_stub_returns_zero() {
        let vol = get_volume().await.expect("should not error");
        assert!((vol - 0.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn set_volume_stub_succeeds() {
        set_volume(0.5).await.expect("should not error");
    }

    #[tokio::test]
    async fn get_brightness_stub_returns_zero() {
        let brightness = get_brightness().await.expect("should not error");
        assert!((brightness - 0.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn set_brightness_stub_succeeds() {
        set_brightness(0.75).await.expect("should not error");
    }

    #[tokio::test]
    async fn on_ac_power_stub_returns_false() {
        assert_eq!(on_ac_power().await.expect("should not error"), false);
    }

    #[tokio::test]
    async fn battery_percent_stub_returns_none() {
        assert_eq!(battery_percent().await.expect("should not error"), None);
    }

    #[tokio::test]
    async fn is_connected_stub_returns_false() {
        assert_eq!(is_connected().await.expect("should not error"), false);
    }
}
