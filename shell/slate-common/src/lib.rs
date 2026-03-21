/// Slate OS shared types.
///
/// This crate is the foundation of the Slate OS workspace. Every other crate
/// depends on it for palette types, D-Bus constants, settings schema, and
/// iced theme generation.
pub mod dbus;
pub mod layout;
pub mod palette;
pub mod settings;
pub mod theme;

// Re-exports for convenience
pub use palette::Palette;
pub use settings::Settings;
