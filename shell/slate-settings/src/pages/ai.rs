/// Rhea AI engine settings page.
///
/// Allows selecting a backend (None / Local / Claude / OpenAI / Ollama),
/// configuring backend-specific options, and viewing the live Rhea status.
use std::path::PathBuf;

use iced::widget::{column, container, row, text, text_input};
use iced::{Element, Length};

use slate_common::settings::RheaSettings;

// ---------------------------------------------------------------------------
// Model scanner
// ---------------------------------------------------------------------------

/// Scan for .gguf model files in `~/.config/slate/models/`.
pub fn scan_models() -> Vec<PathBuf> {
    let dir = std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/slate/models"))
        .unwrap_or_else(|_| PathBuf::from("/root/.config/slate/models"));
    let mut models = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("gguf"))
                .unwrap_or(false)
            {
                models.push(path);
            }
        }
    }
    models.sort();
    models
}

// ---------------------------------------------------------------------------
// D-Bus status fetch (Linux only)
// ---------------------------------------------------------------------------

/// Fetch Rhea's current status string from `org.slate.Rhea.GetStatus`.
/// Returns a human-readable label, guarded so the crate compiles on macOS.
#[cfg(target_os = "linux")]
pub async fn fetch_rhea_status() -> String {
    use zbus::Connection;
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(e) => return format!("D-Bus unavailable: {e}"),
    };
    let reply: Result<String, zbus::Error> = conn
        .call_method(
            Some("org.slate.Rhea"),
            "/org/slate/Rhea",
            Some("org.slate.Rhea"),
            "GetStatus",
            &(),
        )
        .await
        .and_then(|msg| msg.body().deserialize::<String>());
    match reply {
        Ok(json) => {
            // Extract backend/ready from {"backend":"local","ready":true} without serde_json.
            let backend = extract_json_str(&json, "backend").unwrap_or("unknown");
            let ready = json.contains("\"ready\":true");
            format!(
                "backend: {}, ready: {}",
                backend,
                if ready { "yes" } else { "no" }
            )
        }
        Err(e) => format!("status error: {e}"),
    }
}

/// No-op version for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub async fn fetch_rhea_status() -> String {
    "status unavailable (non-Linux)".to_string()
}

/// Extract a string value from `{"key":"value",...}` without pulling in serde_json.
#[cfg(target_os = "linux")]
fn extract_json_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(needle.as_str())? + needle.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AiMsg {
    BackendSelected(String),
    ModelSelected(PathBuf),
    IdleTimeoutChanged(f32),
    ClaudeApiKeyChanged(String),
    ClaudeModelChanged(String),
    OpenAiBaseUrlChanged(String),
    OpenAiApiKeyChanged(String),
    OpenAiModelChanged(String),
    OllamaBaseUrlChanged(String),
    OllamaModelChanged(String),
    ModelsScanned(Vec<PathBuf>),
    StatusLoaded(String),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(
    settings: &mut RheaSettings,
    models: &mut Vec<PathBuf>,
    msg: AiMsg,
) -> Option<iced::Task<AiMsg>> {
    match msg {
        AiMsg::BackendSelected(b) => {
            settings.backend = b;
            None
        }
        AiMsg::ModelSelected(path) => {
            settings.local.model = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            None
        }
        AiMsg::IdleTimeoutChanged(v) => {
            settings.local.idle_timeout_secs = v as u32;
            None
        }
        AiMsg::ClaudeApiKeyChanged(v) => {
            settings.claude.api_key_file = v;
            None
        }
        AiMsg::ClaudeModelChanged(v) => {
            settings.claude.model = v;
            None
        }
        AiMsg::OpenAiBaseUrlChanged(v) => {
            settings.openai.base_url = v;
            None
        }
        AiMsg::OpenAiApiKeyChanged(v) => {
            settings.openai.api_key_file = v;
            None
        }
        AiMsg::OpenAiModelChanged(v) => {
            settings.openai.model = v;
            None
        }
        AiMsg::OllamaBaseUrlChanged(v) => {
            settings.ollama.base_url = v;
            None
        }
        AiMsg::OllamaModelChanged(v) => {
            settings.ollama.model = v;
            None
        }
        AiMsg::ModelsScanned(found) => {
            *models = found;
            None
        }
        // StatusLoaded is ephemeral — handled in main.rs, not stored in settings.
        AiMsg::StatusLoaded(_) => None,
    }
}

// ---------------------------------------------------------------------------
// View helpers
// ---------------------------------------------------------------------------

fn backend_button<'a>(
    label: &'static str,
    value: &'static str,
    current: &str,
) -> Element<'a, AiMsg> {
    let display = if current == value {
        format!("[*] {label}")
    } else {
        label.to_string()
    };
    iced::widget::button(text(display).size(14))
        .on_press(AiMsg::BackendSelected(value.to_string()))
        .padding(8)
        .into()
}

fn view_local_section<'a>(settings: &'a RheaSettings, models: &'a [PathBuf]) -> Element<'a, AiMsg> {
    let mut items: Vec<Element<'a, AiMsg>> = vec![text("Local model").size(16).into()];
    if models.is_empty() {
        items.push(
            text("No .gguf models found in ~/.config/slate/models/")
                .size(12)
                .into(),
        );
    } else {
        for model in models {
            let name = model
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let stem = model
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let label = if settings.local.model == stem {
                format!("[*] {name}")
            } else {
                name
            };
            items.push(
                iced::widget::button(text(label).size(12))
                    .on_press(AiMsg::ModelSelected(model.clone()))
                    .width(Length::Fill)
                    .padding(6)
                    .into(),
            );
        }
    }
    items.push(
        text(format!(
            "Idle timeout: {}s",
            settings.local.idle_timeout_secs
        ))
        .size(14)
        .into(),
    );
    items.push(
        iced::widget::slider(
            10.0..=300.0,
            settings.local.idle_timeout_secs as f32,
            AiMsg::IdleTimeoutChanged,
        )
        .step(10.0)
        .into(),
    );
    column(items).spacing(8).into()
}

fn view_claude_section<'a>(settings: &'a RheaSettings) -> Element<'a, AiMsg> {
    column(vec![
        text("Claude API").size(16).into(),
        text("API Key File:").size(14).into(),
        text_input("path/to/anthropic-key", &settings.claude.api_key_file)
            .on_input(AiMsg::ClaudeApiKeyChanged)
            .secure(true)
            .padding(8)
            .into(),
        text("Model:").size(14).into(),
        text_input("claude-sonnet-4-6", &settings.claude.model)
            .on_input(AiMsg::ClaudeModelChanged)
            .padding(8)
            .into(),
    ])
    .spacing(8)
    .into()
}

fn view_openai_section<'a>(settings: &'a RheaSettings) -> Element<'a, AiMsg> {
    column(vec![
        text("OpenAI-compatible API").size(16).into(),
        text("Base URL:").size(14).into(),
        text_input("https://api.openai.com/v1", &settings.openai.base_url)
            .on_input(AiMsg::OpenAiBaseUrlChanged)
            .padding(8)
            .into(),
        text("API Key File:").size(14).into(),
        text_input("path/to/openai-key", &settings.openai.api_key_file)
            .on_input(AiMsg::OpenAiApiKeyChanged)
            .secure(true)
            .padding(8)
            .into(),
        text("Model:").size(14).into(),
        text_input("gpt-4o-mini", &settings.openai.model)
            .on_input(AiMsg::OpenAiModelChanged)
            .padding(8)
            .into(),
    ])
    .spacing(8)
    .into()
}

fn view_ollama_section<'a>(settings: &'a RheaSettings) -> Element<'a, AiMsg> {
    column(vec![
        text("Ollama").size(16).into(),
        text("Base URL:").size(14).into(),
        text_input("http://localhost:11434", &settings.ollama.base_url)
            .on_input(AiMsg::OllamaBaseUrlChanged)
            .padding(8)
            .into(),
        text("Model:").size(14).into(),
        text_input("llama3.2:3b", &settings.ollama.model)
            .on_input(AiMsg::OllamaModelChanged)
            .padding(8)
            .into(),
    ])
    .spacing(8)
    .into()
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view<'a>(
    settings: &'a RheaSettings,
    models: &'a [PathBuf],
    rhea_status: &'a str,
) -> Element<'a, AiMsg> {
    let backend_row = row![
        backend_button("None", "none", &settings.backend),
        backend_button("Local", "local", &settings.backend),
        backend_button("Claude", "claude", &settings.backend),
        backend_button("OpenAI", "openai", &settings.backend),
        backend_button("Ollama", "ollama", &settings.backend),
    ]
    .spacing(6);

    let mut items: Vec<Element<'a, AiMsg>> = vec![
        text("Rhea").size(24).into(),
        text(format!("Status: {rhea_status}")).size(13).into(),
        backend_row.into(),
    ];

    match settings.backend.as_str() {
        "local" => items.push(view_local_section(settings, models)),
        "claude" => items.push(view_claude_section(settings)),
        "openai" => items.push(view_openai_section(settings)),
        "ollama" => items.push(view_ollama_section(settings)),
        _ => {}
    }

    container(column(items).spacing(12).padding(20).width(Length::Fill))
        .width(Length::Fill)
        .into()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> RheaSettings {
        RheaSettings::default()
    }

    #[test]
    fn backend_selected_updates_backend() {
        let mut s = settings();
        let mut m = vec![];
        update(&mut s, &mut m, AiMsg::BackendSelected("claude".to_string()));
        assert_eq!(s.backend, "claude");
    }

    #[test]
    fn backend_none_sets_none() {
        let mut s = settings();
        let mut m = vec![];
        update(&mut s, &mut m, AiMsg::BackendSelected("none".to_string()));
        assert_eq!(s.backend, "none");
    }

    #[test]
    fn backend_ollama_sets_ollama() {
        let mut s = settings();
        let mut m = vec![];
        update(&mut s, &mut m, AiMsg::BackendSelected("ollama".to_string()));
        assert_eq!(s.backend, "ollama");
    }

    #[test]
    fn idle_timeout_rounds_to_u32() {
        let mut s = settings();
        let mut m = vec![];
        update(&mut s, &mut m, AiMsg::IdleTimeoutChanged(120.7));
        assert_eq!(s.local.idle_timeout_secs, 120);
    }

    #[test]
    fn claude_api_key_changed() {
        let mut s = settings();
        let mut m = vec![];
        update(
            &mut s,
            &mut m,
            AiMsg::ClaudeApiKeyChanged("/home/user/.config/slate/claude-key".to_string()),
        );
        assert_eq!(s.claude.api_key_file, "/home/user/.config/slate/claude-key");
    }

    #[test]
    fn claude_model_changed() {
        let mut s = settings();
        let mut m = vec![];
        update(
            &mut s,
            &mut m,
            AiMsg::ClaudeModelChanged("claude-opus-4".to_string()),
        );
        assert_eq!(s.claude.model, "claude-opus-4");
    }

    #[test]
    fn openai_fields_update() {
        let mut s = settings();
        let mut m = vec![];
        update(
            &mut s,
            &mut m,
            AiMsg::OpenAiBaseUrlChanged("https://custom.api.com/v1".to_string()),
        );
        assert_eq!(s.openai.base_url, "https://custom.api.com/v1");
        update(
            &mut s,
            &mut m,
            AiMsg::OpenAiApiKeyChanged("/keys/openai".to_string()),
        );
        assert_eq!(s.openai.api_key_file, "/keys/openai");
        update(
            &mut s,
            &mut m,
            AiMsg::OpenAiModelChanged("gpt-4o".to_string()),
        );
        assert_eq!(s.openai.model, "gpt-4o");
    }

    #[test]
    fn ollama_fields_update() {
        let mut s = settings();
        let mut m = vec![];
        update(
            &mut s,
            &mut m,
            AiMsg::OllamaBaseUrlChanged("http://remote:11434".to_string()),
        );
        assert_eq!(s.ollama.base_url, "http://remote:11434");
        update(
            &mut s,
            &mut m,
            AiMsg::OllamaModelChanged("mistral:7b".to_string()),
        );
        assert_eq!(s.ollama.model, "mistral:7b");
    }

    #[test]
    fn models_scanned_replaces_list() {
        let mut s = settings();
        let mut models = vec![];
        let new_models = vec![
            PathBuf::from("/models/a.gguf"),
            PathBuf::from("/models/b.gguf"),
        ];
        update(
            &mut s,
            &mut models,
            AiMsg::ModelsScanned(new_models.clone()),
        );
        assert_eq!(models, new_models);
    }

    #[test]
    fn model_selected_uses_stem() {
        let mut s = settings();
        let mut m = vec![];
        update(
            &mut s,
            &mut m,
            AiMsg::ModelSelected(PathBuf::from("/models/gemma-2b.gguf")),
        );
        assert_eq!(s.local.model, "gemma-2b");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn extract_json_str_works() {
        let json = r#"{"backend":"local","ready":true}"#;
        assert_eq!(extract_json_str(json, "backend"), Some("local"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn extract_json_str_missing_key_returns_none() {
        let json = r#"{"backend":"local","ready":true}"#;
        assert_eq!(extract_json_str(json, "model"), None);
    }
}
