/// Router: selects and holds the active AI backend.
///
/// `Router::from_config` instantiates the correct backend based on the
/// config's `BackendKind`. A `StubBackend` is provided for tests.
use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::info;

use slate_common::ai::{
    AiBackend, AiError, ChatMessage, Classification, CompletionRequest, CompletionResponse, Intent,
};

use crate::config::{BackendKind, RheaConfig};

// ---------------------------------------------------------------------------
// StubBackend
// ---------------------------------------------------------------------------

/// A backend that returns deterministic canned responses. For testing only.
#[allow(dead_code)]
pub struct StubBackend {
    pub completion_text: String,
    pub classification: Classification,
    pub summary: String,
    pub replies: Vec<String>,
    pub intent: Intent,
}

impl Default for StubBackend {
    fn default() -> Self {
        Self {
            completion_text: "stub response".to_string(),
            classification: Classification::new("stub_category", 1.0),
            summary: "stub summary".to_string(),
            replies: vec!["OK".to_string(), "Sure".to_string(), "No".to_string()],
            intent: Intent::Unknown,
        }
    }
}

#[async_trait]
impl AiBackend for StubBackend {
    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, AiError> {
        Ok(CompletionResponse {
            text: self.completion_text.clone(),
        })
    }

    async fn classify(&self, _text: &str, _categories: &[&str]) -> Result<Classification, AiError> {
        Ok(self.classification.clone())
    }

    async fn summarize(&self, _text: &str, _max_words: u32) -> Result<String, AiError> {
        Ok(self.summary.clone())
    }

    async fn suggest_replies(&self, _messages: &[ChatMessage]) -> Result<Vec<String>, AiError> {
        Ok(self.replies.clone())
    }

    async fn detect_intent(&self, _text: &str) -> Result<Intent, AiError> {
        Ok(self.intent.clone())
    }
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Owns one backend and delegates every AI call to it.
pub struct Router {
    backend: Box<dyn AiBackend>,
}

impl Router {
    /// Create a `Router` with a stub backend for testing.
    #[allow(dead_code)]
    pub fn with_stub(stub: StubBackend) -> Self {
        Self {
            backend: Box::new(stub),
        }
    }

    /// Create a `Router` from the given config, instantiating the appropriate backend.
    ///
    /// For `BackendKind::Local`, a `LocalBackend` is created and its idle timer is spawned.
    /// For `BackendKind::Cloud`, a `CloudBackend` is created.
    ///
    /// Used in tests and as a convenience for call-sites that don't need signals.
    #[allow(dead_code)]
    pub async fn from_config(config: &RheaConfig) -> anyhow::Result<Self> {
        Self::from_config_with_signals(config, None, None).await
    }

    /// Like `from_config` but wires optional signal senders into the local backend.
    ///
    /// `model_loaded_tx` fires (with the model path) when Cold→Warm transitions.
    /// `model_unloaded_tx` fires (with the model path) when Warm→Cold transitions.
    /// Both channels are ignored for the cloud backend.
    pub async fn from_config_with_signals(
        config: &RheaConfig,
        model_loaded_tx: Option<mpsc::Sender<String>>,
        model_unloaded_tx: Option<mpsc::Sender<String>>,
    ) -> anyhow::Result<Self> {
        match config.backend {
            BackendKind::Local => {
                use crate::local::LocalBackend;
                use std::sync::Arc;
                info!("using local llama.cpp backend");
                let backend = Arc::new(LocalBackend::from_config_with_signals(
                    config,
                    model_loaded_tx,
                    model_unloaded_tx,
                ));
                // Spawn idle timer before boxing so the Arc can be cloned.
                Arc::clone(&backend).spawn_idle_timer();
                // Arc<LocalBackend> implements AiBackend via the blanket impl below.
                Ok(Self {
                    backend: Box::new(ArcBackend(backend)),
                })
            }
            BackendKind::Cloud => {
                use crate::cloud::CloudBackend;
                info!(endpoint = %config.cloud_endpoint, "using cloud backend");
                let backend = CloudBackend::from_config(config).await?;
                Ok(Self {
                    backend: Box::new(backend),
                })
            }
        }
    }

    /// Delegate a completion request to the active backend.
    pub async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, AiError> {
        self.backend.complete(request).await
    }

    /// Delegate a classification request to the active backend.
    pub async fn classify(
        &self,
        text: &str,
        categories: &[&str],
    ) -> Result<Classification, AiError> {
        self.backend.classify(text, categories).await
    }

    /// Delegate a summarization request to the active backend.
    pub async fn summarize(&self, text: &str, max_words: u32) -> Result<String, AiError> {
        self.backend.summarize(text, max_words).await
    }

    /// Delegate a suggest-replies request to the active backend.
    pub async fn suggest_replies(&self, messages: &[ChatMessage]) -> Result<Vec<String>, AiError> {
        self.backend.suggest_replies(messages).await
    }

    /// Delegate an intent detection request to the active backend.
    pub async fn detect_intent(&self, text: &str) -> Result<Intent, AiError> {
        self.backend.detect_intent(text).await
    }
}

// ---------------------------------------------------------------------------
// Arc<LocalBackend> wrapper so it can be boxed as dyn AiBackend
// ---------------------------------------------------------------------------

/// Newtype wrapper allowing `Arc<LocalBackend>` to be boxed as `dyn AiBackend`.
struct ArcBackend<B: AiBackend>(std::sync::Arc<B>);

#[async_trait]
impl<B: AiBackend + Send + Sync> AiBackend for ArcBackend<B> {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError> {
        self.0.complete(request).await
    }
    async fn classify(&self, text: &str, categories: &[&str]) -> Result<Classification, AiError> {
        self.0.classify(text, categories).await
    }
    async fn summarize(&self, text: &str, max_words: u32) -> Result<String, AiError> {
        self.0.summarize(text, max_words).await
    }
    async fn suggest_replies(&self, messages: &[ChatMessage]) -> Result<Vec<String>, AiError> {
        self.0.suggest_replies(messages).await
    }
    async fn detect_intent(&self, text: &str) -> Result<Intent, AiError> {
        self.0.detect_intent(text).await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use slate_common::ai::CompletionRequest;

    #[tokio::test]
    async fn stub_backend_returns_canned_completion() {
        let stub = StubBackend {
            completion_text: "hello from stub".to_string(),
            ..StubBackend::default()
        };
        let router = Router::with_stub(stub);
        let resp = router
            .complete(CompletionRequest::new("test"))
            .await
            .unwrap();
        assert_eq!(resp.text, "hello from stub");
    }

    #[tokio::test]
    async fn stub_backend_classify() {
        let stub = StubBackend {
            classification: Classification::new("work", 0.88),
            ..StubBackend::default()
        };
        let router = Router::with_stub(stub);
        let c = router
            .classify("urgent meeting tomorrow", &["work", "personal"])
            .await
            .unwrap();
        assert_eq!(c.category, "work");
        assert!((c.confidence - 0.88).abs() < 1e-4);
    }

    #[tokio::test]
    async fn stub_backend_summarize() {
        let router = Router::with_stub(StubBackend::default());
        let s = router.summarize("long text here", 10).await.unwrap();
        assert_eq!(s, "stub summary");
    }

    #[tokio::test]
    async fn stub_backend_suggest_replies() {
        let router = Router::with_stub(StubBackend::default());
        let replies = router
            .suggest_replies(&[ChatMessage::new("user", "Hey, are you coming?")])
            .await
            .unwrap();
        assert_eq!(replies.len(), 3);
    }

    #[tokio::test]
    async fn stub_backend_detect_intent_unknown() {
        let router = Router::with_stub(StubBackend::default());
        let intent = router.detect_intent("lorem ipsum").await.unwrap();
        assert!(matches!(intent, Intent::Unknown));
    }

    #[tokio::test]
    async fn stub_backend_detect_intent_app_launch() {
        let stub = StubBackend {
            intent: Intent::AppLaunch("Firefox".to_string()),
            ..StubBackend::default()
        };
        let router = Router::with_stub(stub);
        let intent = router.detect_intent("open firefox").await.unwrap();
        match intent {
            Intent::AppLaunch(app) => assert_eq!(app, "Firefox"),
            other => panic!("expected AppLaunch, got {other:?}"),
        }
    }
}
