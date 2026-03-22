/// Settings schema for Slate OS.
///
/// The canonical `settings.toml` is read/written by slate-settings and consumed
/// by every other component. This module defines the schema, defaults, and
/// file I/O helpers.
use std::path::Path;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("failed to read settings file: {0}")]
    Read(#[from] std::io::Error),

    #[error("failed to parse settings TOML: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("failed to serialise settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

// ---------------------------------------------------------------------------
// Top-level settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub display: DisplaySettings,
    pub wallpaper: WallpaperSettings,
    pub dock: DockSettings,
    pub gestures: GestureSettings,
    pub keyboard: KeyboardSettings,
    pub rhea: RheaSettings,
    pub notifications: NotificationSettings,
}

impl Settings {
    /// Load settings from a TOML file at `path`.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        let content = std::fs::read_to_string(path)?;
        let settings: Settings = toml::from_str(&content)?;
        Ok(settings)
    }

    /// Save settings to a TOML file at `path`, creating parent dirs if needed.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(SettingsError::Read)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sub-sections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplaySettings {
    /// UI scale factor (e.g. 1.5 for HiDPI tablets).
    pub scale_factor: f64,
    /// Lock auto-rotation when true.
    pub rotation_lock: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            scale_factor: 1.5,
            rotation_lock: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperSettings {
    /// Absolute path to the current wallpaper image.
    pub path: String,
}

impl Default for WallpaperSettings {
    fn default() -> Self {
        Self {
            path: "/usr/share/backgrounds/slate-default.jpg".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockSettings {
    /// Auto-hide the dock when an app is focused.
    pub auto_hide: bool,
    /// Icon size in logical pixels.
    pub icon_size: u32,
    /// Desktop IDs of pinned dock apps, in display order.
    #[serde(default = "default_pinned_apps")]
    pub pinned_apps: Vec<String>,
}

/// Fallback pinned apps when the field is absent from an older settings file.
fn default_pinned_apps() -> Vec<String> {
    vec![
        "Alacritty".to_string(),
        "firefox".to_string(),
        "org.gnome.Nautilus".to_string(),
    ]
}

impl Default for DockSettings {
    fn default() -> Self {
        Self {
            auto_hide: false,
            icon_size: 44,
            pinned_apps: default_pinned_apps(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GestureSettings {
    /// Master toggle for touch gestures.
    pub enabled: bool,
    /// Sensitivity multiplier (0.5 – 2.0).
    pub sensitivity: f64,
    /// Size of the edge swipe detection zone in logical pixels.
    pub edge_size: u32,
}

impl Default for GestureSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            sensitivity: 1.0,
            edge_size: 50,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyboardSettings {
    /// Split the on-screen keyboard for thumb typing.
    pub split_mode: bool,
    /// Show word suggestions above the keyboard.
    pub suggestions: bool,
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            split_mode: false,
            suggestions: true,
        }
    }
}

/// Top-level Rhea AI engine settings (replaces old AiSettings).
///
/// Supports multiple backends: local llama.cpp, Claude API, OpenAI-compatible,
/// and Ollama. The `backend` field selects which one is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RheaSettings {
    /// Which backend to use: "local", "claude", "openai", "ollama".
    pub backend: String,
    /// Whether Rhea proactively offers suggestions.
    pub proactive: bool,
    /// Local llama.cpp backend settings.
    pub local: RheaLocalSettings,
    /// Anthropic Claude API settings.
    pub claude: RheaClaudeSettings,
    /// OpenAI-compatible API settings.
    pub openai: RheaOpenAiSettings,
    /// Ollama API settings.
    pub ollama: RheaOllamaSettings,
}

impl Default for RheaSettings {
    fn default() -> Self {
        Self {
            backend: "local".to_string(),
            proactive: false,
            local: RheaLocalSettings::default(),
            claude: RheaClaudeSettings::default(),
            openai: RheaOpenAiSettings::default(),
            ollama: RheaOllamaSettings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RheaLocalSettings {
    /// GGUF model filename (resolved from models dir).
    pub model: String,
    /// Seconds of idle before unloading the model from RAM.
    pub idle_timeout_secs: u32,
    /// Whisper model size for speech-to-text.
    pub whisper_model: String,
}

impl Default for RheaLocalSettings {
    fn default() -> Self {
        Self {
            model: "gemma-2-2b-Q4_K_M".to_string(),
            idle_timeout_secs: 30,
            whisper_model: "base".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RheaClaudeSettings {
    /// Path to a file containing the API key (not the key itself).
    pub api_key_file: String,
    /// Claude model to use.
    pub model: String,
}

impl Default for RheaClaudeSettings {
    fn default() -> Self {
        Self {
            api_key_file: String::new(),
            model: "claude-sonnet-4-6".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RheaOpenAiSettings {
    /// Path to a file containing the API key.
    pub api_key_file: String,
    /// Base URL of the OpenAI-compatible API.
    pub base_url: String,
    /// Model name.
    pub model: String,
}

impl Default for RheaOpenAiSettings {
    fn default() -> Self {
        Self {
            api_key_file: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RheaOllamaSettings {
    /// Base URL of the Ollama API.
    pub base_url: String,
    /// Model name.
    pub model: String,
}

impl Default for RheaOllamaSettings {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434".to_string(),
            model: "llama3.2:3b".to_string(),
        }
    }
}

/// Returns `true` — used as the serde default for boolean "enabled" fields.
fn default_true() -> bool {
    true
}

/// Notification display preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationSettings {
    /// Do-not-disturb mode suppresses heads-up banners.
    pub dnd: bool,
    /// How long heads-up notifications remain visible.
    pub heads_up_duration_secs: u32,
    /// Play a sound when a non-silent notification arrives.
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            dnd: false,
            heads_up_duration_secs: 5,
            sound_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn settings_serialization_round_trip() {
        let settings = Settings::default();
        let toml_str = toml::to_string_pretty(&settings).expect("serialise");
        let back: Settings = toml::from_str(&toml_str).expect("deserialise");
        assert_eq!(settings, back);
    }

    #[test]
    fn default_settings_produce_valid_toml() {
        let settings = Settings::default();
        let toml_str = toml::to_string_pretty(&settings).expect("serialise");
        // Should be parseable without error
        let _: Settings = toml::from_str(&toml_str).expect("round-trip must succeed");
        // Should contain every section header
        assert!(toml_str.contains("[display]"));
        assert!(toml_str.contains("[wallpaper]"));
        assert!(toml_str.contains("[dock]"));
        assert!(toml_str.contains("[gestures]"));
        assert!(toml_str.contains("[keyboard]"));
        assert!(toml_str.contains("[rhea]"));
        assert!(toml_str.contains("[notifications]"));
    }

    #[test]
    fn settings_load_and_save() {
        let dir = std::env::temp_dir().join("slate-common-test-settings");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("settings.toml");

        let settings = Settings {
            display: DisplaySettings {
                scale_factor: 2.0,
                rotation_lock: false,
            },
            ..Settings::default()
        };

        settings.save(&path).expect("save");
        let loaded = Settings::load(&path).expect("load");
        assert_eq!(settings, loaded);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_load_invalid_toml_returns_error() {
        let dir = std::env::temp_dir().join("slate-common-test-bad-settings");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"this is not valid { toml").unwrap();

        let result = Settings::load(&path);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_load_missing_file_returns_error() {
        let result = Settings::load(Path::new("/nonexistent/settings.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn gesture_sensitivity_default_in_range() {
        let g = GestureSettings::default();
        assert!((0.5..=2.0).contains(&g.sensitivity));
    }
}
