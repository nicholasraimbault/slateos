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
