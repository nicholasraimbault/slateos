/// Slate OS shared types.
///
/// This crate is the foundation of the Slate OS workspace. Every other crate
/// depends on it for palette types, D-Bus constants, settings schema, and
/// iced theme generation.
pub mod ai;
pub mod dbus;
pub mod icons;
pub mod layout;
pub mod notifications;
pub mod palette;
pub mod physics;
pub mod settings;
pub mod system;
pub mod theme;
pub mod toast;

// Re-exports for convenience
pub use icons::{resolve_icon, IconCache};
pub use palette::Palette;
pub use settings::Settings;
pub use toast::{ToastKind, ToastState};
