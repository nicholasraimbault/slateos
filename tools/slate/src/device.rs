/// Device catalogue for SlateOS.
///
/// Centralises every known target so all subcommands share the same list
/// and the same Rust → cargo-target mapping.  Adding a new device is a
/// single-line change here.
use std::fmt;

use clap::ValueEnum;

// ---------------------------------------------------------------------------
// Device enum
// ---------------------------------------------------------------------------

/// Known SlateOS target devices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum Device {
    /// Google Pixel Tablet (Tensor G2) — primary target.
    PixelTablet,
    /// Google Pixel phones (Tensor) — secondary target.
    PixelPhone,
    /// Generic x86-64 desktop or laptop — tertiary target.
    #[default]
    GenericX86,
    /// ONN 11 Tablet Pro (Snapdragon 685) — legacy/experimental.
    OnnTablet,
}

impl Device {
    /// All known devices, in priority order.
    #[cfg(test)]
    pub const ALL: &'static [Device] = &[
        Device::PixelTablet,
        Device::PixelPhone,
        Device::GenericX86,
        Device::OnnTablet,
    ];

    /// The Rust cross-compilation target triple for this device.
    pub fn cargo_target(self) -> &'static str {
        match self {
            Device::PixelTablet => "aarch64-unknown-linux-musl",
            Device::PixelPhone => "aarch64-unknown-linux-musl",
            Device::GenericX86 => "x86_64-unknown-linux-musl",
            Device::OnnTablet => "aarch64-unknown-linux-musl",
        }
    }

    /// Whether this device requires cross-compilation from a typical x86 host.
    pub fn needs_cross_compile(self) -> bool {
        self.cargo_target().starts_with("aarch64")
    }

    /// Human-readable device description.
    #[cfg(test)]
    pub fn description(self) -> &'static str {
        match self {
            Device::PixelTablet => "Google Pixel Tablet (Tensor G2) — primary target",
            Device::PixelPhone => "Google Pixel Phone (Tensor) — secondary target",
            Device::GenericX86 => "Generic x86-64 desktop/laptop — tertiary target",
            Device::OnnTablet => "ONN 11 Tablet Pro (Snapdragon 685) — legacy/experimental",
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Device::PixelTablet => write!(f, "pixel-tablet"),
            Device::PixelPhone => write!(f, "pixel-phone"),
            Device::GenericX86 => write!(f, "generic-x86"),
            Device::OnnTablet => write!(f, "onn-tablet"),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_devices_have_cargo_target() {
        for device in Device::ALL {
            let target = device.cargo_target();
            assert!(!target.is_empty(), "{device} has empty cargo target");
        }
    }

    #[test]
    fn generic_x86_does_not_need_cross_compile() {
        assert!(!Device::GenericX86.needs_cross_compile());
    }

    #[test]
    fn aarch64_devices_need_cross_compile() {
        assert!(Device::PixelTablet.needs_cross_compile());
        assert!(Device::PixelPhone.needs_cross_compile());
        assert!(Device::OnnTablet.needs_cross_compile());
    }

    #[test]
    fn default_device_is_generic_x86() {
        assert_eq!(Device::default(), Device::GenericX86);
    }

    #[test]
    fn display_matches_clap_value_enum_names() {
        // Ensures Display output is consistent for UX messages.
        assert_eq!(Device::GenericX86.to_string(), "generic-x86");
        assert_eq!(Device::PixelTablet.to_string(), "pixel-tablet");
        assert_eq!(Device::OnnTablet.to_string(), "onn-tablet");
    }
}
