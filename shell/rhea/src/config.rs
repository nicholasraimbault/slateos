/// Rhea configuration loaded from the `[rhea]` section of settings.toml.
///
/// Wraps `RheaSettings` from slate-common and provides derived helpers so
/// the rest of the crate never has to import slate-common settings directly.
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use slate_common::settings::{RheaSettings, Settings};

// ---------------------------------------------------------------------------
// BackendKind
// ---------------------------------------------------------------------------

/// Which backend Rhea should use for AI requests.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendKind {
    Local,
    Cloud,
}

// ---------------------------------------------------------------------------
// RheaConfig
// ---------------------------------------------------------------------------

/// Derived config for the Rhea daemon, loaded from settings.toml.
#[derive(Debug, Clone)]
pub struct RheaConfig {
    pub backend: BackendKind,

    // Local backend
    pub local_model_path: PathBuf,
    pub local_idle_timeout: Duration,

    // Cloud backend (OpenAI-compatible)
    pub cloud_api_key_file: PathBuf,
    pub cloud_endpoint: String,
    pub cloud_model: String,
}

impl RheaConfig {
    /// Load `RheaConfig` from the given settings.toml path.
    ///
    /// Falls back to defaults when values are absent or empty so the daemon
    /// can start with a minimal settings file.
    pub fn load(path: &Path) -> Result<Self> {
        let settings = Settings::load(path).context("failed to load settings.toml")?;
        Ok(Self::from_rhea_settings(&settings.rhea))
    }

    /// Convert `RheaSettings` (from slate-common) into a `RheaConfig`.
    pub fn from_rhea_settings(s: &RheaSettings) -> Self {
        let backend = if s.backend == "local" {
            BackendKind::Local
        } else {
            // "claude", "openai", "ollama" all use the same OpenAI-compatible HTTP path.
            BackendKind::Cloud
        };

        // Derive the cloud endpoint and API key file from whichever backend is active.
        let (cloud_api_key_file, cloud_endpoint, cloud_model) = match s.backend.as_str() {
            "claude" => (
                PathBuf::from(&s.claude.api_key_file),
                // Claude uses a non-standard base URL; we target the messages endpoint
                // via an OpenAI-compatible shim or the native endpoint.
                "https://api.anthropic.com/v1".to_string(),
                s.claude.model.clone(),
            ),
            "ollama" => (
                PathBuf::new(), // Ollama has no API key
                s.ollama.base_url.clone(),
                s.ollama.model.clone(),
            ),
            _ => {
                // "openai" or any unknown value — use OpenAI-compatible settings
                (
                    PathBuf::from(&s.openai.api_key_file),
                    s.openai.base_url.clone(),
                    s.openai.model.clone(),
                )
            }
        };

        let local_model_path =
            PathBuf::from(format!("/usr/share/rhea/models/{}.gguf", s.local.model));

        Self {
            backend,
            local_model_path,
            local_idle_timeout: Duration::from_secs(u64::from(s.local.idle_timeout_secs)),
            cloud_api_key_file,
            cloud_endpoint,
            cloud_model,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use slate_common::settings::{
        RheaClaudeSettings, RheaLocalSettings, RheaOllamaSettings, RheaOpenAiSettings,
    };

    fn default_rhea_settings() -> RheaSettings {
        RheaSettings::default()
    }

    #[test]
    fn default_backend_is_local() {
        let cfg = RheaConfig::from_rhea_settings(&default_rhea_settings());
        assert_eq!(cfg.backend, BackendKind::Local);
    }

    #[test]
    fn openai_backend_selects_cloud() {
        let mut s = default_rhea_settings();
        s.backend = "openai".to_string();
        s.openai = RheaOpenAiSettings {
            api_key_file: "/run/secrets/openai_key".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
        };
        let cfg = RheaConfig::from_rhea_settings(&s);
        assert_eq!(cfg.backend, BackendKind::Cloud);
        assert_eq!(
            cfg.cloud_api_key_file,
            PathBuf::from("/run/secrets/openai_key")
        );
        assert_eq!(cfg.cloud_model, "gpt-4o-mini");
    }

    #[test]
    fn claude_backend_selects_cloud_with_anthropic_url() {
        let mut s = default_rhea_settings();
        s.backend = "claude".to_string();
        s.claude = RheaClaudeSettings {
            api_key_file: "/run/secrets/claude_key".to_string(),
            model: "claude-sonnet-4-6".to_string(),
        };
        let cfg = RheaConfig::from_rhea_settings(&s);
        assert_eq!(cfg.backend, BackendKind::Cloud);
        assert!(cfg.cloud_endpoint.contains("anthropic.com"));
        assert_eq!(cfg.cloud_model, "claude-sonnet-4-6");
    }

    #[test]
    fn ollama_backend_has_no_api_key() {
        let mut s = default_rhea_settings();
        s.backend = "ollama".to_string();
        s.ollama = RheaOllamaSettings {
            base_url: "http://localhost:11434".to_string(),
            model: "llama3.2:3b".to_string(),
        };
        let cfg = RheaConfig::from_rhea_settings(&s);
        assert_eq!(cfg.backend, BackendKind::Cloud);
        assert_eq!(cfg.cloud_api_key_file, PathBuf::new());
    }

    #[test]
    fn idle_timeout_derived_from_settings() {
        let mut s = default_rhea_settings();
        s.local = RheaLocalSettings {
            model: "gemma-2-2b-Q4_K_M".to_string(),
            idle_timeout_secs: 120,
            whisper_model: "base".to_string(),
        };
        let cfg = RheaConfig::from_rhea_settings(&s);
        assert_eq!(cfg.local_idle_timeout, Duration::from_secs(120));
    }

    #[test]
    fn local_model_path_includes_model_name() {
        let mut s = default_rhea_settings();
        s.local.model = "my-custom-model".to_string();
        let cfg = RheaConfig::from_rhea_settings(&s);
        assert!(cfg
            .local_model_path
            .to_str()
            .unwrap()
            .contains("my-custom-model"));
    }

    #[test]
    fn load_from_missing_file_returns_error() {
        let result = RheaConfig::load(Path::new("/nonexistent/settings.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn load_from_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let settings = slate_common::settings::Settings::default();
        settings.save(&path).unwrap();
        let cfg = RheaConfig::load(&path).unwrap();
        assert_eq!(cfg.backend, BackendKind::Local);
    }
}
