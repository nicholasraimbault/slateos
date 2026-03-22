/// Cloud AI backend (OpenAI-compatible HTTP API).
///
/// Supports Claude API, OpenAI, and Ollama — all expose the same
/// `/v1/chat/completions` format so one client handles all of them.
/// The API key is read from a file path rather than being stored inline.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use slate_common::ai::{
    AiBackend, AiError, ChatMessage, Classification, CompletionRequest, CompletionResponse, Intent,
};

use crate::config::RheaConfig;

// ---------------------------------------------------------------------------
// Wire types (OpenAI chat completions format)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WireMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: WireMessage,
}

// ---------------------------------------------------------------------------
// CloudBackend
// ---------------------------------------------------------------------------

/// HTTP client for any OpenAI-compatible `/v1/chat/completions` endpoint.
pub struct CloudBackend {
    client: reqwest::Client,
    /// Full URL for completions, e.g. `https://api.openai.com/v1/chat/completions`.
    endpoint: String,
    /// Model identifier to send in every request.
    model: String,
    /// Optional bearer token read from disk at creation time.
    api_key: Option<String>,
}

impl CloudBackend {
    /// Create a backend from the given Rhea config.
    ///
    /// Reads the API key from the configured file path if the path is non-empty.
    /// Missing key files produce a warning but do not fail — Ollama has no key.
    pub async fn from_config(config: &RheaConfig) -> anyhow::Result<Self> {
        let api_key = if config.cloud_api_key_file.as_os_str().is_empty() {
            None
        } else {
            match tokio::fs::read_to_string(&config.cloud_api_key_file).await {
                Ok(key) => Some(key.trim().to_string()),
                Err(e) => {
                    warn!(path = ?config.cloud_api_key_file, "could not read API key file: {e}");
                    None
                }
            }
        };

        let endpoint = format!(
            "{}/chat/completions",
            config.cloud_endpoint.trim_end_matches('/')
        );

        Ok(Self {
            client: reqwest::Client::new(),
            endpoint,
            model: config.cloud_model.clone(),
            api_key,
        })
    }

    /// Build and send a chat completion request, returning the assistant text.
    async fn chat(
        &self,
        messages: Vec<WireMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<String, AiError> {
        let body = ChatRequest {
            model: self.model.clone(),
            messages,
            max_tokens,
            temperature,
        };

        debug!(endpoint = %self.endpoint, model = %self.model, "sending chat request");

        // Anthropic's API uses `x-api-key` instead of the OAuth2 `Authorization: Bearer` header.
        let mut req = self.client.post(&self.endpoint).json(&body);
        if let Some(key) = &self.api_key {
            req = if self.endpoint.contains("anthropic") {
                req.header("x-api-key", key)
            } else {
                req.bearer_auth(key)
            };
        }

        let resp = req
            .send()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AiError::Request(format!("HTTP {status}: {body}")));
        }

        let parsed: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AiError::InvalidResponse(e.to_string()))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| AiError::InvalidResponse("empty choices array".to_string()))
    }
}

#[async_trait]
impl AiBackend for CloudBackend {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError> {
        let mut messages = Vec::new();

        if let Some(system) = &request.system {
            messages.push(WireMessage {
                role: "system".to_string(),
                content: system.clone(),
            });
        }

        // Inject shell context into a system message when provided.
        if let Some(ctx) = &request.context {
            let mut ctx_lines = Vec::new();
            if let Some(win) = &ctx.focused_window {
                ctx_lines.push(format!("Focused window: {win}"));
            }
            if let Some(clip) = &ctx.clipboard {
                ctx_lines.push(format!("Clipboard: {clip}"));
            }
            if !ctx.recent_notifications.is_empty() {
                ctx_lines.push(format!(
                    "Recent notifications: {}",
                    ctx.recent_notifications.join("; ")
                ));
            }
            if !ctx_lines.is_empty() {
                messages.push(WireMessage {
                    role: "system".to_string(),
                    content: ctx_lines.join("\n"),
                });
            }
        }

        messages.push(WireMessage {
            role: "user".to_string(),
            content: request.prompt.clone(),
        });

        let text = self
            .chat(messages, request.max_tokens, request.temperature)
            .await?;
        Ok(CompletionResponse { text })
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<Classification, AiError> {
        let categories_list = categories.join(", ");
        let prompt = format!(
            "Classify the following text into exactly one of these categories: {categories_list}.\n\
             Respond with ONLY a JSON object like {{\"category\": \"<category>\", \"confidence\": 0.9}}.\n\nText: {text}"
        );
        let messages = vec![WireMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        let raw = self.chat(messages, Some(64), Some(0.0)).await?;

        #[derive(Deserialize)]
        struct ClassifyResult {
            category: String,
            confidence: f32,
        }

        let result: ClassifyResult = serde_json::from_str(raw.trim()).map_err(|e| {
            AiError::InvalidResponse(format!("classify parse error: {e} — raw: {raw}"))
        })?;

        Ok(Classification::new(result.category, result.confidence))
    }

    async fn summarize(&self, text: &str, max_words: u32) -> Result<String, AiError> {
        let prompt = format!(
            "Summarize the following text in at most {max_words} words. Respond with only the summary.\n\n{text}"
        );
        let messages = vec![WireMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        self.chat(messages, Some(max_words * 2), Some(0.3)).await
    }

    async fn suggest_replies(&self, messages: &[ChatMessage]) -> Result<Vec<String>, AiError> {
        let history: Vec<WireMessage> = messages
            .iter()
            .map(|m| WireMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let mut wire_msgs = history;
        wire_msgs.push(WireMessage {
            role: "user".to_string(),
            content: "Suggest 3 concise reply options for this conversation. \
                      Respond with ONLY a JSON array of strings, e.g. [\"Reply 1\",\"Reply 2\",\"Reply 3\"].".to_string(),
        });

        let raw = self.chat(wire_msgs, Some(256), Some(0.7)).await?;
        let replies: Vec<String> = serde_json::from_str(raw.trim()).map_err(|e| {
            AiError::InvalidResponse(format!("suggest_replies parse error: {e} — raw: {raw}"))
        })?;
        Ok(replies)
    }

    async fn detect_intent(&self, text: &str) -> Result<Intent, AiError> {
        let prompt = format!(
            "Detect the user's intent from the following text. \
             Respond with ONLY a JSON object with a \"kind\" field that is one of: \
             \"SystemControl\", \"AppLaunch\", \"Query\", \"Unknown\". \
             For SystemControl include an \"action\" field (one of: ToggleWifi, ToggleBluetooth, SetBrightness, SetVolume, ToggleDnd, LaunchSettings) and a \"value\" field. \
             For AppLaunch include an \"app\" field. \
             For Query include a \"query\" field. \
             Text: {text}"
        );
        let messages = vec![WireMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        let raw = self.chat(messages, Some(128), Some(0.0)).await?;

        #[derive(Deserialize)]
        struct IntentResult {
            kind: String,
            action: Option<String>,
            #[serde(default)]
            value: serde_json::Value,
            app: Option<String>,
            query: Option<String>,
        }

        let result: IntentResult = serde_json::from_str(raw.trim()).map_err(|e| {
            AiError::InvalidResponse(format!("detect_intent parse error: {e} — raw: {raw}"))
        })?;

        let intent = match result.kind.as_str() {
            "AppLaunch" => Intent::AppLaunch(result.app.unwrap_or_default()),
            "Query" => Intent::Query(result.query.unwrap_or_default()),
            "SystemControl" => {
                use slate_common::ai::SystemAction;
                let action = match result.action.as_deref().unwrap_or("") {
                    "ToggleWifi" => {
                        SystemAction::ToggleWifi(result.value.as_bool().unwrap_or(true))
                    }
                    "ToggleBluetooth" => {
                        SystemAction::ToggleBluetooth(result.value.as_bool().unwrap_or(true))
                    }
                    "SetBrightness" => {
                        SystemAction::SetBrightness(result.value.as_f64().unwrap_or(0.5) as f32)
                    }
                    "SetVolume" => {
                        SystemAction::SetVolume(result.value.as_f64().unwrap_or(0.5) as f32)
                    }
                    "ToggleDnd" => SystemAction::ToggleDnd(result.value.as_bool().unwrap_or(false)),
                    _ => SystemAction::LaunchSettings,
                };
                Intent::SystemControl(action)
            }
            _ => Intent::Unknown,
        };

        Ok(intent)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_request_serializes_correctly() {
        let req = ChatRequest {
            model: "gpt-4o-mini".to_string(),
            messages: vec![
                WireMessage {
                    role: "system".to_string(),
                    content: "You are helpful.".to_string(),
                },
                WireMessage {
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                },
            ],
            max_tokens: Some(100),
            temperature: Some(0.7),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\":\"gpt-4o-mini\""));
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("\"max_tokens\":100"));
    }

    #[test]
    fn chat_request_omits_none_fields() {
        let req = ChatRequest {
            model: "llama3".to_string(),
            messages: vec![WireMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            max_tokens: None,
            temperature: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("temperature"));
    }

    #[test]
    fn chat_response_deserializes_correctly() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"Hello!"}}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices.len(), 1);
        assert_eq!(resp.choices[0].message.content, "Hello!");
    }

    #[test]
    fn chat_response_empty_choices_handled() {
        let json = r#"{"choices":[]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.choices.is_empty());
    }

    #[test]
    fn wire_message_round_trip() {
        let msg = WireMessage {
            role: "user".to_string(),
            content: "test message".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: WireMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "user");
        assert_eq!(back.content, "test message");
    }

    #[test]
    fn classify_result_parses() {
        let json = r#"{"category":"email","confidence":0.92}"#;
        #[derive(Deserialize)]
        struct ClassifyResult {
            category: String,
            confidence: f32,
        }
        let r: ClassifyResult = serde_json::from_str(json).unwrap();
        assert_eq!(r.category, "email");
        assert!((r.confidence - 0.92).abs() < 1e-4);
    }
}
