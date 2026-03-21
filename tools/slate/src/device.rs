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
    /// Google Pixel Fold (Tensor G2) — secondary target.
    PixelFold,
    /// Framework Laptop 12 (x86, touchscreen) — dev machine.
    Framework12,
    /// Generic x86-64 desktop or laptop — tertiary target.
    #[default]
    GenericX86,
}

impl Device {
    /// All known devices, in priority order.
    #[cfg(test)]
    pub const ALL: &'static [Device] = &[
        Device::PixelTablet,
        Device::PixelPhone,
        Device::PixelFold,
        Device::Framework12,
        Device::GenericX86,
    ];

    /// The Rust cross-compilation target triple for this device.
    pub fn cargo_target(self) -> &'static str {
        match self {
            Device::PixelTablet | Device::PixelPhone | Device::PixelFold => {
                "aarch64-unknown-linux-musl"
            }
            Device::Framework12 | Device::GenericX86 => "x86_64-unknown-linux-musl",
        }
    }

    /// Whether this device requires cross-compilation from a typical x86 host.
    pub fn needs_cross_compile(self) -> bool {
        self.cargo_target().starts_with("aarch64")
    }

    /// Whether this device has a touchscreen.
    #[cfg(test)]
    pub fn has_touch(self) -> bool {
        match self {
            Device::PixelTablet | Device::PixelPhone | Device::PixelFold | Device::Framework12 => {
                true
            }
            Device::GenericX86 => false,
        }
    }
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Device::PixelTablet => write!(f, "pixel-tablet"),
            Device::PixelPhone => write!(f, "pixel-phone"),
            Device::PixelFold => write!(f, "pixel-fold"),
            Device::Framework12 => write!(f, "framework-12"),
            Device::GenericX86 => write!(f, "generic-x86"),
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
        assert!(Device::PixelFold.needs_cross_compile());
    }

    #[test]
    fn x86_devices_do_not_need_cross_compile() {
        assert!(!Device::Framework12.needs_cross_compile());
    }

    #[test]
    fn touch_devices() {
        assert!(Device::PixelTablet.has_touch());
        assert!(Device::PixelFold.has_touch());
        assert!(Device::Framework12.has_touch());
        assert!(!Device::GenericX86.has_touch());
    }

    #[test]
    fn default_device_is_generic_x86() {
        assert_eq!(Device::default(), Device::GenericX86);
    }

    #[test]
    fn display_matches_clap_value_enum_names() {
        assert_eq!(Device::GenericX86.to_string(), "generic-x86");
        assert_eq!(Device::PixelTablet.to_string(), "pixel-tablet");
        assert_eq!(Device::PixelFold.to_string(), "pixel-fold");
        assert_eq!(Device::Framework12.to_string(), "framework-12");
    }
}
