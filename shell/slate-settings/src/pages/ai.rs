/// AI & LLM settings page.
///
/// Controls the llama-server service via a flag file and provides
/// model selection, endpoint URL, and API key inputs.
use std::path::PathBuf;

use iced::widget::{column, container, text, text_input, toggler};
use iced::{Element, Length};

use slate_common::settings::RheaSettings;

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
    settings: &mut RheaSettings,
    models: &mut Vec<PathBuf>,
    msg: AiMsg,
) -> Option<iced::Task<AiMsg>> {
    match msg {
        AiMsg::EnabledToggled(v) => {
            // Map the toggle to backend selection: "local" when enabled, "none" when disabled.
            settings.backend = if v {
                "local".to_string()
            } else {
                "none".to_string()
            };
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
            // Store the filename (without path) as the local model name.
            let name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            settings.local.model = name;
            None
        }
        AiMsg::EndpointChanged(url) => {
            settings.openai.base_url = url;
            None
        }
        AiMsg::ApiKeyChanged(key) => {
            settings.openai.api_key_file = key;
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

pub fn view<'a>(settings: &'a RheaSettings, models: &'a [PathBuf]) -> Element<'a, AiMsg> {
    let mut items: Vec<Element<'a, AiMsg>> = Vec::new();

    items.push(text("AI & LLM").size(24).into());

    let is_enabled = settings.backend != "none";
    items.push(
        toggler(is_enabled)
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
            let stem = model
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let is_active = settings.local.model == stem;
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
        text_input("https://api.example.com/v1", &settings.openai.base_url)
            .on_input(AiMsg::EndpointChanged)
            .padding(8)
            .into(),
    );

    // API key file (secure field)
    items.push(text("API Key File:").size(16).into());
    items.push(
        text_input("path/to/key-file", &settings.openai.api_key_file)
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
    fn endpoint_empty_clears_url() {
        let mut s = RheaSettings::default();
        let mut models = Vec::new();
        update(&mut s, &mut models, AiMsg::EndpointChanged(String::new()));
        assert!(s.openai.base_url.is_empty());
    }

    #[test]
    fn endpoint_non_empty_sets_url() {
        let mut s = RheaSettings::default();
        let mut models = Vec::new();
        update(
            &mut s,
            &mut models,
            AiMsg::EndpointChanged("https://example.com".to_string()),
        );
        assert_eq!(s.openai.base_url, "https://example.com");
    }

    #[test]
    fn api_key_empty_clears_file() {
        let mut s = RheaSettings::default();
        let mut models = Vec::new();
        update(&mut s, &mut models, AiMsg::ApiKeyChanged(String::new()));
        assert!(s.openai.api_key_file.is_empty());
    }
}
