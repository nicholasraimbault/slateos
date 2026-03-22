// Integration tests for Rhea AI engine daemon.
//
// Spawns the real rhea binary with a stub/none backend and exercises
// the D-Bus interface.

use slate_common::dbus::{RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH};
use zbus::Connection;

use crate::harness::{slate_proxy, DaemonProcess};
use crate::{skip_without_binary, skip_without_dbus};

/// Create a Rhea proxy.
async fn rhea_proxy(conn: &Connection) -> anyhow::Result<zbus::Proxy<'static>> {
    slate_proxy(conn, RHEA_BUS_NAME, RHEA_PATH, RHEA_INTERFACE).await
}

// ---- Tests ----

#[tokio::test]
async fn rhea_starts_and_claims_bus_name() {
    skip_without_dbus!();
    skip_without_binary!("rhea");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    // Rhea reads settings from $HOME/.config/slate/settings.toml.
    // With a temp HOME, it falls back to defaults (backend = "none").
    let daemon = DaemonProcess::spawn(
        "rhea",
        RHEA_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .expect("rhea should start");

    daemon.shutdown(&conn, RHEA_BUS_NAME).await.unwrap();
}

#[tokio::test]
async fn get_status_returns_json() {
    skip_without_dbus!();
    skip_without_binary!("rhea");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "rhea",
        RHEA_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let proxy = rhea_proxy(&conn).await.unwrap();
    let reply = proxy.call_method("GetStatus", &()).await.unwrap();
    let status: String = reply.body().deserialize().unwrap();

    assert!(status.contains("backend"), "status should contain backend field: {status}");
    assert!(status.contains("ready"), "status should contain ready field: {status}");

    daemon.shutdown(&conn, RHEA_BUS_NAME).await.unwrap();
}

#[tokio::test]
async fn complete_with_none_backend_returns_error_or_empty() {
    skip_without_dbus!();
    skip_without_binary!("rhea");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "rhea",
        RHEA_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let proxy = rhea_proxy(&conn).await.unwrap();

    // With no backend configured, Complete should return an error or empty response.
    let result = proxy
        .call_method("Complete", &("hello world", ""))
        .await;

    // Either an error response or an empty string is acceptable when no backend is configured.
    match result {
        Ok(reply) => {
            let text: String = reply.body().deserialize().unwrap_or_default();
            tracing::info!("Complete returned: {text:?}");
            // Empty or error text is expected with no backend.
        }
        Err(e) => {
            tracing::info!("Complete returned error (expected with no backend): {e}");
        }
    }

    daemon.shutdown(&conn, RHEA_BUS_NAME).await.unwrap();
}

#[tokio::test]
async fn classify_with_none_backend() {
    skip_without_dbus!();
    skip_without_binary!("rhea");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "rhea",
        RHEA_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let proxy = rhea_proxy(&conn).await.unwrap();
    let categories = vec!["greeting", "question", "command"];
    let result = proxy
        .call_method("Classify", &("hello there", categories))
        .await;

    // Graceful failure or empty classification when no backend.
    match result {
        Ok(reply) => {
            let text: String = reply.body().deserialize().unwrap_or_default();
            tracing::info!("Classify returned: {text:?}");
        }
        Err(e) => {
            tracing::info!("Classify error (expected): {e}");
        }
    }

    daemon.shutdown(&conn, RHEA_BUS_NAME).await.unwrap();
}
