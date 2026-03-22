// Quick settings panel for the notification shade.
//
// Renders a tile grid (WiFi, Bluetooth, DND, Airplane mode, Auto-rotate, etc.)
// plus brightness and volume sliders. Tiles are palette-aware and show an
// active/inactive state. Hardware availability determines which tiles are shown
// at runtime; unavailable tiles are hidden rather than greyed-out.

use iced::widget::{button, column, container, row, slider, text, Column, Row};
use iced::{Alignment, Color, Element, Length, Theme};

use slate_common::Palette;

// ---------------------------------------------------------------------------
// Layout constants
// ---------------------------------------------------------------------------

const TILE_SIZE: f32 = 72.0;
const TILE_ICON_SIZE: f32 = 24.0;
const TILE_LABEL_SIZE: f32 = 11.0;
const TILE_SPACING: f32 = 8.0;
const SLIDER_PADDING: f32 = 8.0;
const SLIDER_LABEL_SIZE: f32 = 13.0;
const SECTION_SPACING: f32 = 16.0;
const GRID_COLUMNS: usize = 4;

// ---------------------------------------------------------------------------
// Quick settings tile definitions
// ---------------------------------------------------------------------------

/// Identifier for each quick-settings tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileKind {
    WiFi,
    Bluetooth,
    DoNotDisturb,
    AirplaneMode,
    AutoRotate,
    FlashLight,
    NightLight,
    Hotspot,
}

impl TileKind {
    /// Human-readable label shown beneath the tile icon.
    pub fn label(self) -> &'static str {
        match self {
            TileKind::WiFi => "Wi-Fi",
            TileKind::Bluetooth => "Bluetooth",
            TileKind::DoNotDisturb => "Do Not\nDisturb",
            TileKind::AirplaneMode => "Airplane",
            TileKind::AutoRotate => "Auto\nRotate",
            TileKind::FlashLight => "Flashlight",
            TileKind::NightLight => "Night\nLight",
            TileKind::Hotspot => "Hotspot",
        }
    }

    /// Unicode character used as an icon placeholder.
    ///
    /// In production these would be replaced by icon theme glyphs resolved
    /// via `slate_common::icons::resolve_icon`.
    pub fn icon(self) -> &'static str {
        match self {
            TileKind::WiFi => "📶",
            TileKind::Bluetooth => "🔵",
            TileKind::DoNotDisturb => "🔕",
            TileKind::AirplaneMode => "✈",
            TileKind::AutoRotate => "🔄",
            TileKind::FlashLight => "🔦",
            TileKind::NightLight => "🌙",
            TileKind::Hotspot => "📡",
        }
    }
}

// ---------------------------------------------------------------------------
// Tile state
// ---------------------------------------------------------------------------

/// Runtime state of a single quick-settings tile.
#[derive(Debug, Clone)]
pub struct Tile {
    pub kind: TileKind,
    /// Whether the feature is currently on.
    pub active: bool,
    /// Whether this hardware/feature is available on this device.
    pub available: bool,
}

impl Tile {
    /// Create a tile that is available and inactive.
    pub fn new(kind: TileKind) -> Self {
        Self {
            kind,
            active: false,
            available: true,
        }
    }

    /// Create a tile that is unavailable on this hardware.
    pub fn unavailable(kind: TileKind) -> Self {
        Self {
            kind,
            active: false,
            available: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Quick settings state
// ---------------------------------------------------------------------------

/// Full quick-settings state: tile grid plus brightness and volume sliders.
#[derive(Debug, Clone)]
pub struct QuickSettingsState {
    pub tiles: Vec<Tile>,
    /// Brightness level [0.0, 1.0].
    pub brightness: f32,
    /// System volume [0.0, 1.0].
    pub volume: f32,
}

impl Default for QuickSettingsState {
    fn default() -> Self {
        Self {
            tiles: default_tiles(),
            brightness: 0.7,
            volume: 0.5,
        }
    }
}

impl QuickSettingsState {
    /// Toggle the active state of a tile by kind.
    pub fn toggle_tile(&mut self, kind: TileKind) {
        if let Some(tile) = self.tiles.iter_mut().find(|t| t.kind == kind) {
            if tile.available {
                tile.active = !tile.active;
            }
        }
    }

    /// Set brightness to a clamped [0, 1] value.
    pub fn set_brightness(&mut self, value: f32) {
        self.brightness = value.clamp(0.0, 1.0);
    }

    /// Set volume to a clamped [0, 1] value.
    pub fn set_volume(&mut self, value: f32) {
        self.volume = value.clamp(0.0, 1.0);
    }

    /// Return tiles that are visible (available on this hardware).
    pub fn visible_tiles(&self) -> impl Iterator<Item = &Tile> {
        self.tiles.iter().filter(|t| t.available)
    }
}

/// Build the default set of tiles present on all supported devices.
///
/// Tiles that require specific hardware (flashlight, hotspot) are included
/// with `available = true` so they can be individually disabled at runtime
/// based on capability detection.
pub fn default_tiles() -> Vec<Tile> {
    vec![
        Tile::new(TileKind::WiFi),
        Tile::new(TileKind::Bluetooth),
        Tile::new(TileKind::DoNotDisturb),
        Tile::new(TileKind::AirplaneMode),
        Tile::new(TileKind::AutoRotate),
        Tile::new(TileKind::FlashLight),
        Tile::new(TileKind::NightLight),
        Tile::new(TileKind::Hotspot),
    ]
}

// ---------------------------------------------------------------------------
// Messages produced by the quick settings view
// ---------------------------------------------------------------------------

/// Actions the user can trigger from the quick settings panel.
#[derive(Debug, Clone)]
pub enum QsAction {
    /// Toggle a tile on or off.
    TileToggled(TileKind),
    /// Brightness slider moved.
    BrightnessChanged(f32),
    /// Volume slider moved.
    VolumeChanged(f32),
}

// ---------------------------------------------------------------------------
// View functions
// ---------------------------------------------------------------------------

/// Render the full quick settings panel.
pub fn view_quick_settings<'a>(
    state: &'a QuickSettingsState,
    palette: &Palette,
) -> Element<'a, QsAction> {
    let mut col = Column::new().spacing(SECTION_SPACING);

    // Tile grid
    col = col.push(view_tile_grid(state, palette));

    // Brightness slider
    col = col.push(view_slider(
        "☀",
        "Brightness",
        state.brightness,
        QsAction::BrightnessChanged,
        palette,
    ));

    // Volume slider
    col = col.push(view_slider(
        "🔊",
        "Volume",
        state.volume,
        QsAction::VolumeChanged,
        palette,
    ));

    container(col)
        .padding(SLIDER_PADDING)
        .width(Length::Fill)
        .into()
}

/// Render the tile grid. Unavailable tiles are hidden.
fn view_tile_grid<'a>(state: &'a QuickSettingsState, palette: &Palette) -> Element<'a, QsAction> {
    let visible: Vec<&Tile> = state.visible_tiles().collect();
    let mut rows = Column::new().spacing(TILE_SPACING);
    let chunks = visible.chunks(GRID_COLUMNS);

    // Build rows of up to GRID_COLUMNS tiles each.
    for chunk in chunks {
        let mut tile_row = Row::new().spacing(TILE_SPACING);
        for tile in chunk {
            tile_row = tile_row.push(view_tile(tile, palette));
        }
        // Pad incomplete last row with blank spacers so the grid is uniform.
        for _ in chunk.len()..GRID_COLUMNS {
            tile_row = tile_row.push(iced::widget::Space::new(
                Length::Fixed(TILE_SIZE),
                Length::Fixed(TILE_SIZE),
            ));
        }
        rows = rows.push(tile_row);
    }

    rows.into()
}

/// Render a single quick-settings tile.
fn view_tile<'a>(tile: &'a Tile, palette: &Palette) -> Element<'a, QsAction> {
    let bg = tile_bg(tile.active, palette);
    let fg = if tile.active {
        Palette::color_to_iced(palette.primary)
    } else {
        Palette::color_to_iced(palette.neutral)
    };

    let content = column![
        text(tile.kind.icon()).size(TILE_ICON_SIZE).color(fg),
        text(tile.kind.label())
            .size(TILE_LABEL_SIZE)
            .color(fg)
            .align_x(iced::alignment::Horizontal::Center),
    ]
    .align_x(Alignment::Center)
    .spacing(4.0);

    let kind = tile.kind;
    button(
        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
    )
    .on_press(QsAction::TileToggled(kind))
    .width(Length::Fixed(TILE_SIZE))
    .height(Length::Fixed(TILE_SIZE))
    .style(move |_theme: &Theme, _status| button::Style {
        background: Some(iced::Background::Color(bg)),
        border: iced::Border {
            radius: 12.0.into(),
            ..Default::default()
        },
        text_color: fg,
        shadow: iced::Shadow::default(),
    })
    .into()
}

/// Render a labelled horizontal slider (brightness or volume).
fn view_slider<'a, F>(
    icon: &'static str,
    label: &'static str,
    value: f32,
    on_change: F,
    palette: &Palette,
) -> Element<'a, QsAction>
where
    F: 'a + Fn(f32) -> QsAction,
{
    let label_color = Palette::color_to_iced(palette.neutral);
    row![
        text(icon).size(SLIDER_LABEL_SIZE).color(label_color),
        slider(0.0..=1.0, value, on_change)
            .step(0.01)
            .width(Length::Fill),
        text(label).size(SLIDER_LABEL_SIZE).color(label_color),
    ]
    .align_y(Alignment::Center)
    .spacing(SLIDER_PADDING)
    .padding(SLIDER_PADDING)
    .into()
}

// ---------------------------------------------------------------------------
// Style helpers
// ---------------------------------------------------------------------------

/// Background colour for an active/inactive tile.
fn tile_bg(active: bool, palette: &Palette) -> Color {
    if active {
        let p = palette.primary;
        Color::from_rgba8(p[0], p[1], p[2], 0.85)
    } else {
        let s = palette.surface;
        let base = Palette::color_to_iced(s);
        Color {
            r: (base.r + 0.10).min(1.0),
            g: (base.g + 0.10).min(1.0),
            b: (base.b + 0.10).min(1.0),
            a: 0.80,
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
    fn default_tiles_all_available() {
        let tiles = default_tiles();
        assert!(!tiles.is_empty());
        for tile in &tiles {
            assert!(
                tile.available,
                "tile {:?} should be available by default",
                tile.kind
            );
        }
    }

    #[test]
    fn default_tiles_all_inactive() {
        let tiles = default_tiles();
        for tile in &tiles {
            assert!(
                !tile.active,
                "tile {:?} should be inactive by default",
                tile.kind
            );
        }
    }

    #[test]
    fn quick_settings_default_values() {
        let qs = QuickSettingsState::default();
        assert!((qs.brightness - 0.7).abs() < 1e-6);
        assert!((qs.volume - 0.5).abs() < 1e-6);
        assert!(!qs.tiles.is_empty());
    }

    #[test]
    fn toggle_tile_flips_active() {
        let mut qs = QuickSettingsState::default();
        let wifi = TileKind::WiFi;
        assert!(!qs.tiles.iter().find(|t| t.kind == wifi).unwrap().active);
        qs.toggle_tile(wifi);
        assert!(qs.tiles.iter().find(|t| t.kind == wifi).unwrap().active);
        qs.toggle_tile(wifi);
        assert!(!qs.tiles.iter().find(|t| t.kind == wifi).unwrap().active);
    }

    #[test]
    fn toggle_unavailable_tile_does_nothing() {
        let mut qs = QuickSettingsState::default();
        // Mark flashlight unavailable.
        if let Some(t) = qs.tiles.iter_mut().find(|t| t.kind == TileKind::FlashLight) {
            t.available = false;
        }
        qs.toggle_tile(TileKind::FlashLight);
        assert!(
            !qs.tiles
                .iter()
                .find(|t| t.kind == TileKind::FlashLight)
                .unwrap()
                .active
        );
    }

    #[test]
    fn set_brightness_clamps_to_range() {
        let mut qs = QuickSettingsState::default();
        qs.set_brightness(2.0);
        assert!((qs.brightness - 1.0).abs() < 1e-6);
        qs.set_brightness(-1.0);
        assert!((qs.brightness - 0.0).abs() < 1e-6);
        qs.set_brightness(0.42);
        assert!((qs.brightness - 0.42).abs() < 1e-4);
    }

    #[test]
    fn set_volume_clamps_to_range() {
        let mut qs = QuickSettingsState::default();
        qs.set_volume(99.0);
        assert!((qs.volume - 1.0).abs() < 1e-6);
        qs.set_volume(-5.0);
        assert!((qs.volume - 0.0).abs() < 1e-6);
    }

    #[test]
    fn visible_tiles_excludes_unavailable() {
        let mut qs = QuickSettingsState::default();
        // Make hotspot unavailable.
        if let Some(t) = qs.tiles.iter_mut().find(|t| t.kind == TileKind::Hotspot) {
            t.available = false;
        }
        let visible: Vec<_> = qs.visible_tiles().collect();
        assert!(visible.iter().all(|t| t.kind != TileKind::Hotspot));
    }

    #[test]
    fn tile_kind_labels_are_nonempty() {
        for kind in [
            TileKind::WiFi,
            TileKind::Bluetooth,
            TileKind::DoNotDisturb,
            TileKind::AirplaneMode,
            TileKind::AutoRotate,
            TileKind::FlashLight,
            TileKind::NightLight,
            TileKind::Hotspot,
        ] {
            assert!(!kind.label().is_empty());
            assert!(!kind.icon().is_empty());
        }
    }

    #[test]
    fn qs_action_is_debug_clone() {
        let action = QsAction::TileToggled(TileKind::WiFi);
        let _cloned = action.clone();
        let _debug = format!("{action:?}");
    }

    #[test]
    fn view_quick_settings_produces_element() {
        let qs = QuickSettingsState::default();
        let palette = Palette::default();
        let _el: Element<'_, QsAction> = view_quick_settings(&qs, &palette);
    }
}
