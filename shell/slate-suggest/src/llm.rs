/// Local LLM client for suggestion completions.
///
/// Queries a local llama.cpp server for command completions. This is
/// strictly optional — if the server is unavailable or times out, the
/// suggestion bar falls back to history-only suggestions.
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default endpoint for the local llama.cpp completion API.
const LLM_ENDPOINT: &str = "http://localhost:8081/completion";

/// Hard timeout for LLM queries — suggestions must feel instant.
const LLM_TIMEOUT: Duration = Duration::from_millis(500);

/// Request body for the llama.cpp `/completion` endpoint.
#[derive(Debug, Serialize)]
struct CompletionRequest {
    prompt: String,
    n_predict: u32,
    temperature: f32,
    stop: Vec<String>,
}

/// Response from the llama.cpp `/completion` endpoint.
#[derive(Debug, Deserialize)]
struct CompletionResponse {
    content: String,
}

/// Query the local LLM for a command completion.
///
/// Returns `Some(completion)` on success, `None` on any error (timeout,
/// connection refused, bad response, etc.). This function never panics
/// and never blocks for more than [`LLM_TIMEOUT`].
pub async fn query_llm(input: &str) -> Option<String> {
    if input.trim().is_empty() {
        return None;
    }

    let result = query_llm_inner(input).await;

    match result {
        Ok(Some(text)) => {
            let trimmed = text.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Ok(None) => None,
        Err(err) => {
            tracing::debug!("LLM query failed (graceful): {err}");
            None
        }
    }
}

/// Inner implementation that returns a Result for clean error propagation.
async fn query_llm_inner(input: &str) -> Result<Option<String>, reqwest::Error> {
    let client = reqwest::Client::builder().timeout(LLM_TIMEOUT).build()?;

    let request = CompletionRequest {
        prompt: format!("Complete this shell command: {input}"),
        n_predict: 64,
        temperature: 0.3,
        stop: vec!["\n".to_string(), "\r".to_string()],
    };

    let response = client.post(LLM_ENDPOINT).json(&request).send().await?;

    let body: CompletionResponse = response.json().await?;

    if body.content.is_empty() {
        return Ok(None);
    }

    // Build the full suggestion: user input + LLM completion
    let full_command = format!("{input}{}", body.content);
    Ok(Some(full_command))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timeout_returns_none_not_error() {
        // The LLM server is not running in test environments.
        // query_llm should return None gracefully, never panic.
        let result = query_llm("git").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn empty_input_returns_none() {
        let result = query_llm("").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn whitespace_input_returns_none() {
        let result = query_llm("   ").await;
        assert!(result.is_none());
    }

    #[test]
    fn completion_request_serializes() {
        let req = CompletionRequest {
            prompt: "Complete this shell command: git".to_string(),
            n_predict: 64,
            temperature: 0.3,
            stop: vec!["\n".to_string()],
        };
        let json = serde_json::to_string(&req).expect("serialize request");
        assert!(json.contains("git"));
        assert!(json.contains("n_predict"));
    }

    #[test]
    fn completion_response_deserializes() {
        let json = r#"{"content": " push origin main"}"#;
        let resp: CompletionResponse = serde_json::from_str(json).expect("deserialize response");
        assert_eq!(resp.content, " push origin main");
    }

    #[test]
    fn completion_response_empty_content() {
        let json = r#"{"content": ""}"#;
        let resp: CompletionResponse = serde_json::from_str(json).expect("deserialize response");
        assert!(resp.content.is_empty());
    }
}
