// OpenClaw WebSocket client.
//
// Connects to the local OpenClaw AI server over WebSocket and handles the
// streaming response protocol. The client is designed to be used from an iced
// subscription so chunks flow directly into the UI message loop.

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::context::WindowContext;

/// Default WebSocket endpoint for the local OpenClaw server.
pub const DEFAULT_WS_URL: &str = "ws://127.0.0.1:18789/ws";

/// Reconnect interval when the connection drops.
const RECONNECT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Wire protocol types
// ---------------------------------------------------------------------------

/// Outgoing message sent to OpenClaw.
#[derive(Debug, Clone, Serialize)]
pub struct QueryMessage {
    pub r#type: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<WindowContext>,
}

impl QueryMessage {
    pub fn new(content: String, context: Option<WindowContext>) -> Self {
        Self {
            r#type: "query".to_string(),
            content,
            context,
        }
    }
}

/// Incoming streaming response chunk from OpenClaw.
#[derive(Debug, Clone, Deserialize)]
pub struct ResponseChunk {
    pub r#type: String,
    pub content: Option<String>,
}

impl ResponseChunk {
    /// Whether this chunk signals the end of a response.
    pub fn is_done(&self) -> bool {
        self.r#type == "done"
    }

    /// Whether this chunk signals an error.
    pub fn is_error(&self) -> bool {
        self.r#type == "error"
    }
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Events emitted by the WebSocket client to the UI layer.
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Connection established.
    Connected,
    /// A streaming text chunk arrived.
    Chunk(String),
    /// The assistant finished its response.
    Done,
    /// An error from the server or connection layer.
    Error(String),
    /// Connection was lost.
    Disconnected,
}

/// A handle for sending queries to the background WebSocket task.
#[derive(Debug, Clone)]
pub struct OpenClawClient {
    query_tx: mpsc::Sender<QueryMessage>,
}

impl OpenClawClient {
    /// Send a query to OpenClaw. Returns an error if the background task has
    /// stopped.
    pub async fn send_query(&self, query: QueryMessage) -> Result<(), anyhow::Error> {
        self.query_tx
            .send(query)
            .await
            .map_err(|_| anyhow::anyhow!("OpenClaw background task has stopped"))
    }
}

/// Spawn the persistent WebSocket background loop.
///
/// Returns a client handle (for sending queries) and a receiver (for
/// streaming events back to the UI). The background task will reconnect
/// automatically when the connection drops.
pub fn spawn_client() -> (OpenClawClient, mpsc::UnboundedReceiver<ClientEvent>) {
    let (query_tx, query_rx) = mpsc::channel::<QueryMessage>(32);
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ClientEvent>();

    tokio::spawn(connection_loop(query_rx, event_tx));

    let client = OpenClawClient { query_tx };
    (client, event_rx)
}

/// Persistent connection loop with automatic reconnection.
async fn connection_loop(
    mut query_rx: mpsc::Receiver<QueryMessage>,
    event_tx: mpsc::UnboundedSender<ClientEvent>,
) {
    loop {
        match try_connect_and_run(&mut query_rx, &event_tx).await {
            Ok(()) => {
                // Clean shutdown (query sender dropped).
                break;
            }
            Err(e) => {
                tracing::warn!("OpenClaw connection error: {e}");
                let _ = event_tx.send(ClientEvent::Disconnected);
                tokio::time::sleep(RECONNECT_INTERVAL).await;
            }
        }
    }
}

/// Attempt a single WebSocket session. Returns `Ok(())` when the query
/// sender is dropped (clean shutdown), or `Err` when the connection fails.
async fn try_connect_and_run(
    query_rx: &mut mpsc::Receiver<QueryMessage>,
    event_tx: &mpsc::UnboundedSender<ClientEvent>,
) -> Result<(), anyhow::Error> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(DEFAULT_WS_URL).await?;
    let (mut ws_sink, mut ws_source) = ws_stream.split();

    let _ = event_tx.send(ClientEvent::Connected);

    loop {
        tokio::select! {
            // Forward outgoing queries to the WebSocket.
            query = query_rx.recv() => {
                match query {
                    Some(q) => {
                        let json = serde_json::to_string(&q)?;
                        ws_sink.send(tungstenite::Message::Text(json)).await?;
                    }
                    None => {
                        // Sender dropped — clean shutdown.
                        return Ok(());
                    }
                }
            }
            // Forward incoming WebSocket messages to the UI.
            msg = ws_source.next() => {
                match msg {
                    Some(Ok(tungstenite::Message::Text(text))) => {
                        handle_server_message(&text, event_tx);
                    }
                    Some(Ok(tungstenite::Message::Close(_))) | None => {
                        return Err(anyhow::anyhow!("WebSocket closed by server"));
                    }
                    Some(Err(e)) => {
                        return Err(e.into());
                    }
                    // Ignore ping/pong/binary frames.
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

/// Parse a server message and emit the corresponding client event.
fn handle_server_message(text: &str, event_tx: &mpsc::UnboundedSender<ClientEvent>) {
    match serde_json::from_str::<ResponseChunk>(text) {
        Ok(chunk) => {
            if chunk.is_error() {
                let msg = chunk.content.unwrap_or_else(|| "Unknown error".to_string());
                let _ = event_tx.send(ClientEvent::Error(msg));
            } else if chunk.is_done() {
                let _ = event_tx.send(ClientEvent::Done);
            } else if let Some(content) = chunk.content {
                let _ = event_tx.send(ClientEvent::Chunk(content));
            }
        }
        Err(e) => {
            tracing::warn!("Failed to parse OpenClaw message: {e}");
            let _ = event_tx.send(ClientEvent::Error(format!("Parse error: {e}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_message_serializes_correctly() {
        let ctx = WindowContext {
            app_id: "firefox".to_string(),
            title: "GitHub".to_string(),
        };
        let msg = QueryMessage::new("How do I rebase?".to_string(), Some(ctx));
        let json = serde_json::to_string(&msg).expect("serialize");

        assert!(json.contains(r#""type":"query""#));
        assert!(json.contains(r#""content":"How do I rebase?""#));
        assert!(json.contains(r#""app_id":"firefox""#));
    }

    #[test]
    fn query_message_without_context_omits_field() {
        let msg = QueryMessage::new("Hello".to_string(), None);
        let json = serde_json::to_string(&msg).expect("serialize");

        assert!(!json.contains("context"));
    }

    #[test]
    fn response_chunk_deserializes_text_chunk() {
        let json = r#"{"type": "chunk", "content": "Hello "}"#;
        let chunk: ResponseChunk = serde_json::from_str(json).expect("deserialize");

        assert_eq!(chunk.r#type, "chunk");
        assert_eq!(chunk.content.as_deref(), Some("Hello "));
        assert!(!chunk.is_done());
        assert!(!chunk.is_error());
    }

    #[test]
    fn response_chunk_deserializes_done() {
        let json = r#"{"type": "done"}"#;
        let chunk: ResponseChunk = serde_json::from_str(json).expect("deserialize");

        assert!(chunk.is_done());
        assert!(!chunk.is_error());
        assert!(chunk.content.is_none());
    }

    #[test]
    fn response_chunk_deserializes_error() {
        let json = r#"{"type": "error", "content": "Rate limited"}"#;
        let chunk: ResponseChunk = serde_json::from_str(json).expect("deserialize");

        assert!(chunk.is_error());
        assert!(!chunk.is_done());
        assert_eq!(chunk.content.as_deref(), Some("Rate limited"));
    }

    #[test]
    fn handle_server_message_emits_chunk() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_server_message(r#"{"type":"chunk","content":"Hi"}"#, &tx);

        let event = rx.try_recv().expect("should have event");
        match event {
            ClientEvent::Chunk(c) => assert_eq!(c, "Hi"),
            other => panic!("expected Chunk, got {other:?}"),
        }
    }

    #[test]
    fn handle_server_message_emits_done() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_server_message(r#"{"type":"done"}"#, &tx);

        let event = rx.try_recv().expect("should have event");
        assert!(matches!(event, ClientEvent::Done));
    }

    #[test]
    fn handle_server_message_emits_error_on_bad_json() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_server_message("not json at all", &tx);

        let event = rx.try_recv().expect("should have event");
        assert!(matches!(event, ClientEvent::Error(_)));
    }
}
