/// `org.slate.Rhea` D-Bus interface.
///
/// All AI methods are routed through the `Router`. Signals are emitted from
/// each method call so callers can track streaming completions and backend
/// lifecycle events.
use std::sync::Arc;

use zbus::object_server::SignalEmitter;

use slate_common::ai::{ChatMessage, CompletionRequest};

use crate::context;

use crate::router::Router;

// ---------------------------------------------------------------------------
// Shared router type
// ---------------------------------------------------------------------------

/// Router has no `&mut self` methods, so a plain `Arc` is sufficient.
pub type SharedRouter = Arc<Router>;

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Serialisable status returned by `GetStatus`.
#[derive(serde::Serialize)]
struct RheaStatus {
    backend: String,
    ready: bool,
}

// ---------------------------------------------------------------------------
// D-Bus interface
// ---------------------------------------------------------------------------

pub struct RheaInterface {
    pub router: SharedRouter,
    /// Which backend name is active (for status reporting).
    pub backend_name: String,
}

#[zbus::interface(name = "org.slate.Rhea")]
impl RheaInterface {
    // -----------------------------------------------------------------------
    // Methods
    // -----------------------------------------------------------------------

    /// Summarise `text` to at most `max_words` words.
    async fn summarize(
        &self,
        text: &str,
        max_words: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> String {
        match self.router.summarize(text, max_words).await {
            Ok(summary) => {
                let _ = Self::completion_done(&emitter, &summary).await;
                summary
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = Self::completion_error(&emitter, &msg).await;
                String::new()
            }
        }
    }

    /// Suggest up to 3 reply options for a TOML-encoded conversation.
    ///
    /// `messages_toml` is a TOML array of `{role, content}` tables.
    async fn suggest_replies(
        &self,
        messages_toml: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Vec<String> {
        let messages = parse_messages(messages_toml);
        match self.router.suggest_replies(&messages).await {
            Ok(replies) => {
                let joined = replies.join("\n");
                let _ = Self::completion_done(&emitter, &joined).await;
                replies
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = Self::completion_error(&emitter, &msg).await;
                Vec::new()
            }
        }
    }

    /// Generate a completion for the given prompt, with optional system prompt.
    ///
    /// Shell context (focused window, clipboard, recent notifications) is
    /// gathered automatically and injected into the request.
    async fn complete(
        &self,
        prompt: &str,
        system: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> String {
        // Gather ambient shell context to augment the request.
        let ai_context = context::gather().await;
        let request = CompletionRequest {
            prompt: prompt.to_string(),
            system: if system.is_empty() {
                None
            } else {
                Some(system.to_string())
            },
            context: Some(ai_context),
            max_tokens: None,
            temperature: None,
        };
        match self.router.complete(request).await {
            Ok(resp) => {
                let _ = Self::completion_done(&emitter, &resp.text).await;
                resp.text
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = Self::completion_error(&emitter, &msg).await;
                String::new()
            }
        }
    }

    /// Stream a completion — emits `CompletionChunk` signals then `CompletionDone`.
    ///
    /// Currently non-streaming: emits a single chunk then done.
    /// Streaming requires SSE support (future work).
    async fn complete_stream(
        &self,
        prompt: &str,
        system: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let ai_context = context::gather().await;
        let request = CompletionRequest {
            prompt: prompt.to_string(),
            system: if system.is_empty() {
                None
            } else {
                Some(system.to_string())
            },
            context: Some(ai_context),
            max_tokens: None,
            temperature: None,
        };
        match self.router.complete(request).await {
            Ok(resp) => {
                // Emit the full text as a single chunk, then signal done.
                let _ = Self::completion_chunk(&emitter, &resp.text).await;
                let _ = Self::completion_done(&emitter, &resp.text).await;
            }
            Err(e) => {
                let _ = Self::completion_error(&emitter, &e.to_string()).await;
            }
        }
    }

    /// Detect the user's intent from free-form text.
    ///
    /// Returns a JSON-encoded `Intent` (using `serde_json`).
    async fn detect_intent(&self, text: &str) -> String {
        match self.router.detect_intent(text).await {
            Ok(intent) => serde_json::to_string(&intent).unwrap_or_default(),
            Err(e) => {
                tracing::warn!("detect_intent error: {e}");
                String::new()
            }
        }
    }

    /// Classify `text` into one of the provided categories (comma-separated).
    ///
    /// Returns a JSON-encoded `Classification`.
    async fn classify(&self, text: &str, categories_csv: &str) -> String {
        let cats: Vec<&str> = categories_csv.split(',').map(str::trim).collect();
        match self.router.classify(text, &cats).await {
            Ok(c) => serde_json::to_string(&c).unwrap_or_default(),
            Err(e) => {
                tracing::warn!("classify error: {e}");
                String::new()
            }
        }
    }

    /// Return a JSON status object with backend name and readiness.
    async fn get_status(&self) -> String {
        let status = RheaStatus {
            backend: self.backend_name.clone(),
            ready: true,
        };
        serde_json::to_string(&status).unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Signals
    // -----------------------------------------------------------------------

    /// Emitted with each streamed text chunk during `CompleteStream`.
    #[zbus(signal)]
    pub async fn completion_chunk(emitter: &SignalEmitter<'_>, chunk: &str) -> zbus::Result<()>;

    /// Emitted when a completion finishes (streaming or non-streaming).
    #[zbus(signal)]
    pub async fn completion_done(emitter: &SignalEmitter<'_>, full_text: &str) -> zbus::Result<()>;

    /// Emitted when a completion fails.
    #[zbus(signal)]
    pub async fn completion_error(emitter: &SignalEmitter<'_>, error: &str) -> zbus::Result<()>;

    /// Emitted when the active backend changes.
    #[zbus(signal)]
    pub async fn backend_changed(
        emitter: &SignalEmitter<'_>,
        backend_name: &str,
    ) -> zbus::Result<()>;

    /// Emitted when a local model finishes loading.
    #[zbus(signal)]
    pub async fn model_loaded(emitter: &SignalEmitter<'_>, model_path: &str) -> zbus::Result<()>;

    /// Emitted when a local model is unloaded to free RAM.
    #[zbus(signal)]
    pub async fn model_unloaded(emitter: &SignalEmitter<'_>, model_path: &str) -> zbus::Result<()>;
}

// ---------------------------------------------------------------------------
// Helper — parse TOML-encoded messages
// ---------------------------------------------------------------------------

/// Deserialise a TOML array of `{role, content}` tables into `ChatMessage`s.
///
/// Returns an empty vec on parse error so callers degrade gracefully.
fn parse_messages(toml_str: &str) -> Vec<ChatMessage> {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        messages: Vec<MsgRecord>,
    }
    #[derive(serde::Deserialize)]
    struct MsgRecord {
        role: String,
        content: String,
    }

    match toml::from_str::<Wrapper>(toml_str) {
        Ok(w) => w
            .messages
            .into_iter()
            .map(|m| ChatMessage::new(m.role, m.content))
            .collect(),
        Err(e) => {
            tracing::warn!("failed to parse messages TOML: {e}");
            Vec::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::StubBackend;

    fn make_router(stub: StubBackend) -> SharedRouter {
        Arc::new(Router::with_stub(stub))
    }

    #[test]
    fn parse_messages_valid_toml() {
        let toml_str = r#"
[[messages]]
role = "user"
content = "Hello"

[[messages]]
role = "assistant"
content = "Hi there"
"#;
        let msgs = parse_messages(toml_str);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].content, "Hi there");
    }

    #[test]
    fn parse_messages_empty_string() {
        let msgs = parse_messages("");
        assert!(msgs.is_empty());
    }

    #[test]
    fn parse_messages_invalid_toml() {
        let msgs = parse_messages("not { valid }}}");
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn get_status_returns_json() {
        let router = make_router(StubBackend::default());
        let iface = RheaInterface {
            router,
            backend_name: "stub".to_string(),
        };
        let status = iface.get_status().await;
        assert!(status.contains("\"backend\""));
        assert!(status.contains("stub"));
    }

    #[tokio::test]
    async fn detect_intent_returns_json() {
        let router = make_router(StubBackend::default());
        let iface = RheaInterface {
            router,
            backend_name: "stub".to_string(),
        };
        let result = iface.detect_intent("some text").await;
        // Should produce valid JSON for the Unknown intent.
        let _: slate_common::ai::Intent = serde_json::from_str(&result).unwrap();
    }

    #[tokio::test]
    async fn classify_returns_json() {
        let stub = StubBackend {
            classification: slate_common::ai::Classification::new("email", 0.9),
            ..StubBackend::default()
        };
        let router = make_router(stub);
        let iface = RheaInterface {
            router,
            backend_name: "stub".to_string(),
        };
        let result = iface.classify("urgent message", "email,spam,other").await;
        let c: slate_common::ai::Classification = serde_json::from_str(&result).unwrap();
        assert_eq!(c.category, "email");
    }

    // `complete` and `suggest_replies` D-Bus methods accept a `SignalEmitter` which
    // cannot be constructed without a live connection. We test the underlying router
    // routing directly — this exercises the full call path minus the D-Bus signal.

    #[tokio::test]
    async fn complete_routes_through_stub() {
        let stub = StubBackend {
            completion_text: "hello from complete".to_string(),
            ..StubBackend::default()
        };
        let router = make_router(stub);
        let req = slate_common::ai::CompletionRequest::new("what time is it?");
        let resp = router.complete(req).await.unwrap();
        assert_eq!(resp.text, "hello from complete");
    }

    #[tokio::test]
    async fn suggest_replies_routes_through_stub() {
        let stub = StubBackend {
            replies: vec!["Yes".to_string(), "No".to_string(), "Maybe".to_string()],
            ..StubBackend::default()
        };
        let router = make_router(stub);
        let msgs = vec![slate_common::ai::ChatMessage::new("user", "Are you free?")];
        let replies = router.suggest_replies(&msgs).await.unwrap();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0], "Yes");
    }
}
