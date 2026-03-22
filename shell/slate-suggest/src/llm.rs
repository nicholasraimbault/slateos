/// LLM suggestion client via Rhea D-Bus.
///
/// Queries the Rhea AI engine for command completions. Falls back
/// gracefully to None if Rhea is unavailable or times out.
use std::time::Duration;

/// Hard timeout — suggestions must feel instant.
const RHEA_TIMEOUT: Duration = Duration::from_millis(500);

/// Query Rhea for a command completion.
pub async fn query_llm(input: &str) -> Option<String> {
    if input.trim().is_empty() {
        return None;
    }
    query_rhea(input).await
}

#[cfg(target_os = "linux")]
async fn query_rhea(input: &str) -> Option<String> {
    use slate_common::dbus::{RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH};

    let conn = zbus::Connection::session().await.ok()?;
    let proxy = zbus::Proxy::builder(&conn)
        .destination(RHEA_BUS_NAME)
        .ok()?
        .path(RHEA_PATH)
        .ok()?
        .interface(RHEA_INTERFACE)
        .ok()?
        .build()
        .await
        .ok()?;

    let system_prompt = "You are a shell command completion engine. \
        Given a partial command, output ONLY the completed command. No explanation.";

    let reply = tokio::time::timeout(
        RHEA_TIMEOUT,
        proxy.call_method("Complete", &(input, system_prompt)),
    )
    .await
    .ok()?
    .ok()?;

    let text: String = reply.body().deserialize().ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(not(target_os = "linux"))]
async fn query_rhea(_input: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_input_returns_none() {
        assert!(query_llm("").await.is_none());
    }

    #[tokio::test]
    async fn whitespace_input_returns_none() {
        assert!(query_llm("   ").await.is_none());
    }

    #[tokio::test]
    async fn unavailable_rhea_returns_none() {
        let result = query_llm("git").await;
        assert!(result.is_none());
    }
}
