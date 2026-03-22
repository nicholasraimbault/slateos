/// Local AI backend using llama-server (llama.cpp HTTP server).
///
/// Manages the llama-server subprocess lifecycle:
/// - Cold: process not running, model not loaded
/// - Warm: process running, model loaded in RAM
///
/// An idle timer resets on each request. When it fires, `unload()` kills the
/// subprocess to free RAM. On the next request `ensure_warm()` restarts it.
///
/// Signal senders (optional): when provided, `ensure_warm()` sends the model path
/// on `model_loaded_tx` after a Cold→Warm transition, and `unload()` sends on
/// `model_unloaded_tx` after a Warm→Cold transition. The main entry point wires
/// these channels to D-Bus signal emission.
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, Mutex};
use tokio::time::Instant;
use tracing::{debug, info};

use slate_common::ai::{
    AiBackend, AiError, ChatMessage, Classification, CompletionRequest, CompletionResponse, Intent,
};

use crate::config::RheaConfig;

// ---------------------------------------------------------------------------
// Model lifecycle state
// ---------------------------------------------------------------------------

/// State of the llama-server subprocess.
#[derive(Debug, PartialEq)]
pub enum ModelState {
    Cold,
    Warm,
}

/// Mutable inner state guarded by a Mutex.
struct Inner {
    state: ModelState,
    process: Option<Child>,
    last_request: Option<Instant>,
}

// ---------------------------------------------------------------------------
// LocalBackend
// ---------------------------------------------------------------------------

const LLAMA_PORT: u16 = 8181;

/// Backend that delegates to a llama-server child process.
pub struct LocalBackend {
    model_path: String,
    idle_timeout: Duration,
    inner: Arc<Mutex<Inner>>,
    client: reqwest::Client,
    endpoint: String,
    /// Fires when Cold→Warm transition completes; payload is the model path.
    model_loaded_tx: Option<mpsc::Sender<String>>,
    /// Fires when Warm→Cold transition completes; payload is the model path.
    model_unloaded_tx: Option<mpsc::Sender<String>>,
}

impl LocalBackend {
    /// Create a new `LocalBackend` from `RheaConfig` with no signal senders.
    ///
    /// Used in tests and as a convenience for call-sites that don't need signals.
    #[allow(dead_code)]
    pub fn from_config(config: &RheaConfig) -> Self {
        Self::from_config_with_signals(config, None, None)
    }

    /// Create a new `LocalBackend` from `RheaConfig`, wiring optional signal senders.
    ///
    /// When `model_loaded_tx` is `Some`, the model path is sent on Cold→Warm.
    /// When `model_unloaded_tx` is `Some`, the model path is sent on Warm→Cold.
    pub fn from_config_with_signals(
        config: &RheaConfig,
        model_loaded_tx: Option<mpsc::Sender<String>>,
        model_unloaded_tx: Option<mpsc::Sender<String>>,
    ) -> Self {
        let endpoint = format!("http://127.0.0.1:{LLAMA_PORT}/v1/chat/completions");
        Self {
            model_path: config.local_model_path.to_string_lossy().into_owned(),
            idle_timeout: config.local_idle_timeout,
            inner: Arc::new(Mutex::new(Inner {
                state: ModelState::Cold,
                process: None,
                last_request: None,
            })),
            client: reqwest::Client::new(),
            endpoint,
            model_loaded_tx,
            model_unloaded_tx,
        }
    }

    /// Ensure the llama-server is running.  Starts it if in the Cold state.
    ///
    /// On a Cold→Warm transition, sends the model path on `model_loaded_tx` (if set).
    pub async fn ensure_warm(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if inner.state == ModelState::Warm {
            return Ok(());
        }

        info!(model = %self.model_path, "starting llama-server");

        let child = Command::new("llama-server")
            .args([
                "--model",
                &self.model_path,
                "--port",
                &LLAMA_PORT.to_string(),
                "--log-disable",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn llama-server: {e}"))?;

        inner.process = Some(child);
        inner.state = ModelState::Warm;
        inner.last_request = Some(Instant::now());

        // Give the server a moment to bind the port before the first request.
        drop(inner);
        tokio::time::sleep(Duration::from_millis(500)).await;

        info!("llama-server ready");

        // Notify the D-Bus layer that the model is now loaded.
        if let Some(tx) = &self.model_loaded_tx {
            let _ = tx.send(self.model_path.clone()).await;
        }

        Ok(())
    }

    /// Kill the llama-server subprocess and free RAM.
    ///
    /// On a Warm→Cold transition, sends the model path on `model_unloaded_tx` (if set).
    pub async fn unload(&self) {
        let mut inner = self.inner.lock().await;
        if inner.state == ModelState::Cold {
            return;
        }
        if let Some(mut child) = inner.process.take() {
            let _ = child.kill().await;
        }
        inner.state = ModelState::Cold;
        inner.last_request = None;
        // Drop the lock before sending so the channel send doesn't hold the mutex.
        drop(inner);

        info!("llama-server unloaded");

        // Notify the D-Bus layer that the model has been unloaded.
        if let Some(tx) = &self.model_unloaded_tx {
            let _ = tx.send(self.model_path.clone()).await;
        }
    }

    /// Spawn the idle timer task. Calls `unload()` after `idle_timeout` of inactivity.
    ///
    /// The task runs in the background and checks the last request time on each tick.
    pub fn spawn_idle_timer(self: Arc<Self>) {
        let backend = Arc::clone(&self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let last = {
                    let inner = backend.inner.lock().await;
                    if inner.state == ModelState::Cold {
                        continue;
                    }
                    inner.last_request
                };
                if let Some(t) = last {
                    if t.elapsed() >= backend.idle_timeout {
                        debug!("idle timeout exceeded, unloading model");
                        backend.unload().await;
                    }
                }
            }
        });
    }

    /// Touch the last-request timestamp so the idle timer resets.
    async fn touch(&self) {
        let mut inner = self.inner.lock().await;
        inner.last_request = Some(Instant::now());
    }

    /// Send a chat request to the running llama-server.
    async fn chat(
        &self,
        messages: Vec<WireMessage>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<String, AiError> {
        let body = ChatRequest {
            model: "local".to_string(),
            messages,
            max_tokens,
            temperature,
        };

        let resp = self
            .client
            .post(&self.endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AiError::Request(format!(
                "llama-server HTTP {status}: {text}"
            )));
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
            .ok_or_else(|| AiError::InvalidResponse("empty choices from llama-server".to_string()))
    }
}

// ---------------------------------------------------------------------------
// Wire types (mirrors cloud.rs — same OpenAI format)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Serialize, Deserialize)]
struct WireMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: WireMessage,
}

// ---------------------------------------------------------------------------
// AiBackend implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl AiBackend for LocalBackend {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, AiError> {
        self.ensure_warm()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        self.touch().await;

        let mut messages = Vec::new();
        if let Some(system) = &request.system {
            messages.push(WireMessage {
                role: "system".to_string(),
                content: system.clone(),
            });
        }
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
            content: request.prompt,
        });

        let text = self
            .chat(messages, request.max_tokens, request.temperature)
            .await?;
        Ok(CompletionResponse { text })
    }

    async fn classify(&self, text: &str, categories: &[&str]) -> Result<Classification, AiError> {
        self.ensure_warm()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        self.touch().await;

        let categories_list = categories.join(", ");
        let prompt = format!(
            "Classify the following text into one of: {categories_list}.\n\
             Respond with ONLY JSON: {{\"category\": \"<cat>\", \"confidence\": 0.9}}\n\nText: {text}"
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
        let result: ClassifyResult = serde_json::from_str(raw.trim())
            .map_err(|e| AiError::InvalidResponse(format!("classify parse: {e} — raw: {raw}")))?;
        Ok(Classification::new(result.category, result.confidence))
    }

    async fn summarize(&self, text: &str, max_words: u32) -> Result<String, AiError> {
        self.ensure_warm()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        self.touch().await;

        let prompt =
            format!("Summarize in at most {max_words} words. Only output the summary.\n\n{text}");
        let messages = vec![WireMessage {
            role: "user".to_string(),
            content: prompt,
        }];
        self.chat(messages, Some(max_words * 2), Some(0.3)).await
    }

    async fn suggest_replies(&self, messages: &[ChatMessage]) -> Result<Vec<String>, AiError> {
        self.ensure_warm()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        self.touch().await;

        let mut wire_msgs: Vec<WireMessage> = messages
            .iter()
            .map(|m| WireMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();
        wire_msgs.push(WireMessage {
            role: "user".to_string(),
            content: "Suggest 3 short reply options. Respond with only a JSON array of strings."
                .to_string(),
        });
        let raw = self.chat(wire_msgs, Some(256), Some(0.7)).await?;
        let replies: Vec<String> = serde_json::from_str(raw.trim()).map_err(|e| {
            AiError::InvalidResponse(format!("suggest_replies parse: {e} — raw: {raw}"))
        })?;
        Ok(replies)
    }

    async fn detect_intent(&self, text: &str) -> Result<Intent, AiError> {
        self.ensure_warm()
            .await
            .map_err(|e| AiError::Request(e.to_string()))?;
        self.touch().await;

        let prompt = format!(
            "Detect intent from text. Respond ONLY with JSON with a \"kind\" field: \
             SystemControl | AppLaunch | Query | Unknown.\n\nText: {text}"
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
            AiError::InvalidResponse(format!("detect_intent parse: {e} — raw: {raw}"))
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
    use crate::config::{BackendKind, RheaConfig};
    use std::path::PathBuf;

    fn test_config() -> RheaConfig {
        RheaConfig {
            backend: BackendKind::Local,
            local_model_path: PathBuf::from("/tmp/test-model.gguf"),
            local_idle_timeout: Duration::from_secs(30),
            cloud_api_key_file: PathBuf::new(),
            cloud_endpoint: String::new(),
            cloud_model: String::new(),
        }
    }

    #[test]
    fn local_backend_starts_cold() {
        let backend = LocalBackend::from_config(&test_config());
        // Without calling ensure_warm the state must be Cold.
        // We test this synchronously by peeking at the Arc<Mutex<Inner>> using try_lock.
        let inner = backend.inner.try_lock().expect("no contention in test");
        assert_eq!(inner.state, ModelState::Cold);
        assert!(inner.process.is_none());
    }

    #[test]
    fn model_state_debug() {
        assert_eq!(format!("{:?}", ModelState::Cold), "Cold");
        assert_eq!(format!("{:?}", ModelState::Warm), "Warm");
    }

    #[tokio::test]
    async fn unload_when_cold_is_noop() {
        let backend = LocalBackend::from_config(&test_config());
        // Should not panic when already cold.
        backend.unload().await;
        let inner = backend.inner.lock().await;
        assert_eq!(inner.state, ModelState::Cold);
    }

    #[test]
    fn chat_request_serialization() {
        let req = ChatRequest {
            model: "local".to_string(),
            messages: vec![WireMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            max_tokens: Some(50),
            temperature: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"model\":\"local\""));
        assert!(!json.contains("temperature"));
    }

    #[test]
    fn chat_response_deserialization() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"pong"}}]}"#;
        let resp: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.choices[0].message.content, "pong");
    }

    #[tokio::test]
    async fn touch_updates_last_request() {
        let backend = LocalBackend::from_config(&test_config());
        {
            let inner = backend.inner.lock().await;
            assert!(inner.last_request.is_none());
        }
        backend.touch().await;
        {
            let inner = backend.inner.lock().await;
            assert!(inner.last_request.is_some());
        }
    }
}
