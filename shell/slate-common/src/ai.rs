/// AI backend trait and shared types for the Rhea AI engine.
///
/// Rhea supports multiple backends (local llama, Claude, OpenAI, Ollama).
/// This module defines the trait that each backend implements, plus the
/// request/response types that flow between the engine and the rest of the
/// shell.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("backend request failed: {0}")]
    Request(String),

    #[error("backend returned invalid response: {0}")]
    InvalidResponse(String),

    #[error("backend not configured: {0}")]
    NotConfigured(String),
}

// ---------------------------------------------------------------------------
// Backend trait
// ---------------------------------------------------------------------------

/// Trait that every AI backend (local, Claude, OpenAI, Ollama) implements.
///
/// All methods take owned/borrowed data and return `Result` so callers
/// can handle errors uniformly regardless of the backend.
#[async_trait]
pub trait AiBackend: Send + Sync {
    /// Generate a text completion from the given request.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError>;

    /// Classify text into one of the provided categories.
    async fn classify(&self, text: &str, categories: &[&str]) -> Result<Classification, AiError>;

    /// Summarize text to at most `max_words` words.
    async fn summarize(&self, text: &str, max_words: u32) -> Result<String, AiError>;

    /// Suggest reply options given a conversation history.
    async fn suggest_replies(&self, messages: &[ChatMessage]) -> Result<Vec<String>, AiError>;

    /// Detect the user's intent from free-form text.
    async fn detect_intent(&self, text: &str) -> Result<Intent, AiError>;
}

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// A completion request sent to an AI backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    /// The user prompt.
    pub prompt: String,
    /// Optional system prompt that sets the assistant's behaviour.
    pub system: Option<String>,
    /// Ambient context from the shell (focused window, clipboard, etc.).
    pub context: Option<AiContext>,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Sampling temperature (0.0 = deterministic, 1.0 = creative).
    pub temperature: Option<f32>,
}

impl CompletionRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            system: None,
            context: None,
            max_tokens: None,
            temperature: None,
        }
    }
}

/// A completion response from an AI backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    /// The generated text.
    pub text: String,
}

/// Ambient context gathered from the shell to augment AI requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiContext {
    /// Title/app-id of the currently focused window.
    pub focused_window: Option<String>,
    /// Current clipboard text (truncated for safety).
    pub clipboard: Option<String>,
    /// Summaries of recent notifications.
    pub recent_notifications: Vec<String>,
}

// ---------------------------------------------------------------------------
// Chat / Intent types
// ---------------------------------------------------------------------------

/// A single message in a conversation (for multi-turn context).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Role of the speaker (e.g. "user", "assistant", "system").
    pub role: String,
    /// Text content of the message.
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

/// Detected user intent from free-form text input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Intent {
    /// The user wants to control a system setting.
    SystemControl(SystemAction),
    /// The user wants to launch an application by name.
    AppLaunch(String),
    /// The user is asking a question or making a request.
    Query(String),
    /// Intent could not be determined.
    Unknown,
}

/// A system-level action that Rhea can execute on the user's behalf.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemAction {
    ToggleWifi(bool),
    ToggleBluetooth(bool),
    SetBrightness(f32),
    SetVolume(f32),
    ToggleDnd(bool),
    LaunchSettings,
}

/// Result of classifying text into a category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    /// The winning category.
    pub category: String,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f32,
}

impl Classification {
    pub fn new(category: impl Into<String>, confidence: f32) -> Self {
        Self {
            category: category.into(),
            confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_request_new() {
        let req = CompletionRequest::new("Hello");
        assert_eq!(req.prompt, "Hello");
        assert!(req.system.is_none());
        assert!(req.context.is_none());
        assert!(req.max_tokens.is_none());
        assert!(req.temperature.is_none());
    }

    #[test]
    fn completion_request_serialization_round_trip() {
        let req = CompletionRequest {
            prompt: "test".to_string(),
            system: Some("You are helpful.".to_string()),
            context: Some(AiContext {
                focused_window: Some("Firefox".to_string()),
                clipboard: None,
                recent_notifications: vec!["New email".to_string()],
            }),
            max_tokens: Some(100),
            temperature: Some(0.7),
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let back: CompletionRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(req.prompt, back.prompt);
        assert_eq!(req.system, back.system);
        assert_eq!(req.max_tokens, back.max_tokens);
    }

    #[test]
    fn completion_response_serialization_round_trip() {
        let resp = CompletionResponse {
            text: "Hello!".to_string(),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: CompletionResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(resp.text, back.text);
    }

    #[test]
    fn chat_message_new() {
        let msg = ChatMessage::new("user", "Hi there");
        assert_eq!(msg.role, "user");
        assert_eq!(msg.content, "Hi there");
    }

    #[test]
    fn classification_new() {
        let c = Classification::new("email", 0.95);
        assert_eq!(c.category, "email");
        assert!((c.confidence - 0.95).abs() < 1e-6);
    }

    #[test]
    fn intent_system_control_serialization() {
        let intent = Intent::SystemControl(SystemAction::SetBrightness(0.8));
        let json = serde_json::to_string(&intent).expect("serialize");
        let back: Intent = serde_json::from_str(&json).expect("deserialize");
        match back {
            Intent::SystemControl(SystemAction::SetBrightness(v)) => {
                assert!((v - 0.8).abs() < 1e-6);
            }
            other => panic!("expected SystemControl(SetBrightness), got {other:?}"),
        }
    }

    #[test]
    fn intent_app_launch_serialization() {
        let intent = Intent::AppLaunch("Firefox".to_string());
        let json = serde_json::to_string(&intent).expect("serialize");
        let back: Intent = serde_json::from_str(&json).expect("deserialize");
        match back {
            Intent::AppLaunch(name) => assert_eq!(name, "Firefox"),
            other => panic!("expected AppLaunch, got {other:?}"),
        }
    }

    #[test]
    fn intent_unknown_serialization() {
        let intent = Intent::Unknown;
        let json = serde_json::to_string(&intent).expect("serialize");
        let back: Intent = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, Intent::Unknown));
    }

    #[test]
    fn ai_context_default() {
        let ctx = AiContext::default();
        assert!(ctx.focused_window.is_none());
        assert!(ctx.clipboard.is_none());
        assert!(ctx.recent_notifications.is_empty());
    }

    #[test]
    fn system_action_variants_serialize() {
        // Verify all variants can round-trip through serde.
        let actions: Vec<SystemAction> = vec![
            SystemAction::ToggleWifi(true),
            SystemAction::ToggleBluetooth(false),
            SystemAction::SetBrightness(0.5),
            SystemAction::SetVolume(0.75),
            SystemAction::ToggleDnd(true),
            SystemAction::LaunchSettings,
        ];
        for action in actions {
            let json = serde_json::to_string(&action).expect("serialize");
            let _back: SystemAction = serde_json::from_str(&json).expect("deserialize");
        }
    }
}
