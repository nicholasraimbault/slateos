/// AI & LLM settings page.
///
/// Controls the llama-server service via a flag file and provides
/// model selection, endpoint URL, and API key inputs.
use std::path::PathBuf;

use iced::widget::{column, container, text, text_input, toggler};
use iced::{Element, Length};

use slate_common::settings::AiSettings;

// ---------------------------------------------------------------------------
// LLM flag file
// ---------------------------------------------------------------------------

/// Path to the flag file that controls whether llama-server starts.
/// The arkhe service checks for this file to decide whether to start.
pub fn llm_flag_path() -> PathBuf {
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".config/slate/llm-enabled"))
        .unwrap_or_else(|_| PathBuf::from("/root/.config/slate/llm-enabled"))
}

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
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum AiMsg {
    EnabledToggled(bool),
    ModelSelected(PathBuf),
    EndpointChanged(String),
    ApiKeyChanged(String),
    ModelsScanned(Vec<PathBuf>),
    ServiceResult(Result<String, String>),
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

pub fn update(
    settings: &mut AiSettings,
    models: &mut Vec<PathBuf>,
    msg: AiMsg,
) -> Option<iced::Task<AiMsg>> {
    match msg {
        AiMsg::EnabledToggled(v) => {
            settings.enabled = v;
            // Create or remove the flag file
            let flag = llm_flag_path();
            if v {
                if let Some(parent) = flag.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&flag, "");
            } else {
                let _ = std::fs::remove_file(&flag);
            }
            // Start or stop the service
            let action = if v { "start" } else { "stop" };
            let task = iced::Task::perform(
                async move { crate::settings_io::arkhe_service_ctl(action, "llama-server").await },
                AiMsg::ServiceResult,
            );
            Some(task)
        }
        AiMsg::ModelSelected(path) => {
            settings.model_path = Some(path.to_string_lossy().to_string());
            None
        }
        AiMsg::EndpointChanged(url) => {
            settings.endpoint = if url.is_empty() { None } else { Some(url) };
            None
        }
        AiMsg::ApiKeyChanged(key) => {
            settings.api_key = if key.is_empty() { None } else { Some(key) };
            None
        }
        AiMsg::ModelsScanned(found) => {
            *models = found;
            None
        }
        AiMsg::ServiceResult(result) => {
            match result {
                Ok(msg) => tracing::info!("llama-server: {msg}"),
                Err(msg) => tracing::warn!("llama-server: {msg}"),
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

pub fn view<'a>(settings: &'a AiSettings, models: &'a [PathBuf]) -> Element<'a, AiMsg> {
    let mut items: Vec<Element<'a, AiMsg>> = Vec::new();

    items.push(text("AI & LLM").size(24).into());

    items.push(
        toggler(settings.enabled)
            .label("Enable local LLM")
            .on_toggle(AiMsg::EnabledToggled)
            .into(),
    );

    // Model list
    items.push(text("Models:").size(16).into());
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
            let is_active = settings
                .model_path
                .as_deref()
                .map(|p| p == model.to_string_lossy().as_ref())
                .unwrap_or(false);
            let label = if is_active {
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

    // Endpoint URL
    items.push(text("Endpoint URL:").size(16).into());
    items.push(
        text_input(
            "https://api.example.com/v1",
            settings.endpoint.as_deref().unwrap_or(""),
        )
        .on_input(AiMsg::EndpointChanged)
        .padding(8)
        .into(),
    );

    // API key (secure field)
    items.push(text("API Key:").size(16).into());
    items.push(
        text_input("sk-...", settings.api_key.as_deref().unwrap_or(""))
            .on_input(AiMsg::ApiKeyChanged)
            .secure(true)
            .padding(8)
            .into(),
    );

    let content = column(items).spacing(12).padding(20).width(Length::Fill);

    container(content).width(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_flag_file_path_is_correct() {
        let path = llm_flag_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with(".config/slate/llm-enabled"),
            "unexpected path: {path_str}"
        );
    }

    #[test]
    fn endpoint_empty_becomes_none() {
        let mut s = AiSettings::default();
        let mut models = Vec::new();
        update(&mut s, &mut models, AiMsg::EndpointChanged(String::new()));
        assert!(s.endpoint.is_none());
    }

    #[test]
    fn endpoint_non_empty_becomes_some() {
        let mut s = AiSettings::default();
        let mut models = Vec::new();
        update(
            &mut s,
            &mut models,
            AiMsg::EndpointChanged("https://example.com".to_string()),
        );
        assert_eq!(s.endpoint.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn api_key_empty_becomes_none() {
        let mut s = AiSettings::default();
        let mut models = Vec::new();
        update(&mut s, &mut models, AiMsg::ApiKeyChanged(String::new()));
        assert!(s.api_key.is_none());
    }
}
