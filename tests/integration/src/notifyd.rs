// Integration tests for slate-notifyd.
//
// These tests spawn the real slate-notifyd binary, send notifications
// via the freedesktop D-Bus interface, and verify behavior through the
// Slate D-Bus interface.

use std::collections::HashMap;

use slate_common::dbus::{NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH};
use zbus::Connection;
use zbus::zvariant::Value;

use crate::harness::{slate_proxy, DaemonProcess};
use crate::{skip_without_binary, skip_without_dbus};

/// Create a freedesktop notifications proxy.
async fn fd_proxy(conn: &Connection) -> anyhow::Result<zbus::Proxy<'static>> {
    slate_proxy(
        conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await
}

/// Create a Slate notifications proxy.
async fn slate_notif_proxy(conn: &Connection) -> anyhow::Result<zbus::Proxy<'static>> {
    slate_proxy(conn, NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_PATH, NOTIFICATIONS_INTERFACE).await
}

/// Send a notification via freedesktop Notify and return the assigned ID.
async fn send_notification(
    proxy: &zbus::Proxy<'_>,
    app_name: &str,
    summary: &str,
    body: &str,
) -> anyhow::Result<u32> {
    let actions: Vec<String> = Vec::new();
    let hints: HashMap<String, Value> = HashMap::new();
    let reply = proxy
        .call_method(
            "Notify",
            &(
                app_name,         // app_name
                0u32,             // replaces_id
                "",               // app_icon
                summary,          // summary
                body,             // body
                actions,          // actions
                hints,            // hints
                -1i32,            // expire_timeout
            ),
        )
        .await?;
    let id: u32 = reply.body().deserialize()?;
    Ok(id)
}

// ---- Tests ----

#[tokio::test]
async fn notifyd_starts_and_claims_bus_name() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .expect("notifyd should start");

    // Verify we can query it.
    let proxy = fd_proxy(&conn).await.unwrap();
    let reply = proxy.call_method("GetServerInformation", &()).await.unwrap();
    let (name, _, _, _): (String, String, String, String) = reply.body().deserialize().unwrap();
    assert_eq!(name, "slate-notifyd");

    daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn send_and_retrieve_notification() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let fd = fd_proxy(&conn).await.unwrap();
    let id = send_notification(&fd, "firefox", "Download complete", "file.zip")
        .await
        .unwrap();
    assert!(id > 0, "should assign a positive fd_id");

    // Retrieve via Slate interface.
    let slate = slate_notif_proxy(&conn).await.unwrap();
    let reply = slate.call_method("GetActive", &()).await.unwrap();
    let active_toml: String = reply.body().deserialize().unwrap();
    assert!(
        active_toml.contains("firefox"),
        "active notifications should contain our app_name"
    );
    assert!(
        active_toml.contains("Download complete"),
        "should contain our summary"
    );

    daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn dismiss_removes_notification() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let fd = fd_proxy(&conn).await.unwrap();
    let _id = send_notification(&fd, "signal", "New message", "Hey!")
        .await
        .unwrap();

    // Get active, find the UUID.
    let slate = slate_notif_proxy(&conn).await.unwrap();
    let reply = slate.call_method("GetActive", &()).await.unwrap();
    let active_toml: String = reply.body().deserialize().unwrap();

    // Parse UUID from TOML.
    let uuid_line = active_toml
        .lines()
        .find(|l| l.starts_with("uuid"))
        .expect("should have uuid field");
    let uuid_str = uuid_line
        .split('"')
        .nth(1)
        .expect("uuid should be quoted");

    // Dismiss it.
    slate
        .call_method("Dismiss", &(uuid_str,))
        .await
        .unwrap();

    // Verify it's gone.
    let reply = slate.call_method("GetActive", &()).await.unwrap();
    let active_after: String = reply.body().deserialize().unwrap();
    assert!(
        !active_after.contains("signal"),
        "dismissed notification should be removed"
    );

    daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn fd_id_increments_monotonically() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let fd = fd_proxy(&conn).await.unwrap();
    let id1 = send_notification(&fd, "app1", "first", "").await.unwrap();
    let id2 = send_notification(&fd, "app2", "second", "").await.unwrap();
    let id3 = send_notification(&fd, "app3", "third", "").await.unwrap();

    assert!(id2 > id1, "IDs should increase: {id2} > {id1}");
    assert!(id3 > id2, "IDs should increase: {id3} > {id2}");

    daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn dnd_property_round_trip() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let slate = slate_notif_proxy(&conn).await.unwrap();

    // Read initial DND state.
    let dnd: bool = slate.get_property("dnd").await.unwrap();
    assert!(!dnd, "DND should default to false");

    // Enable DND.
    slate.set_property("dnd", true).await.unwrap();
    let dnd: bool = slate.get_property("dnd").await.unwrap();
    assert!(dnd, "DND should be true after setting");

    // Disable DND.
    slate.set_property("dnd", false).await.unwrap();
    let dnd: bool = slate.get_property("dnd").await.unwrap();
    assert!(!dnd, "DND should be false after unsetting");

    daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn get_capabilities_returns_expected_list() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let fd = fd_proxy(&conn).await.unwrap();
    let reply = fd.call_method("GetCapabilities", &()).await.unwrap();
    let caps: Vec<String> = reply.body().deserialize().unwrap();

    assert!(caps.contains(&"body".to_string()), "should support body");
    assert!(caps.contains(&"actions".to_string()), "should support actions");

    daemon.shutdown().await.unwrap();
}

#[tokio::test]
async fn dismiss_all_clears_non_persistent() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let daemon = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    let fd = fd_proxy(&conn).await.unwrap();
    send_notification(&fd, "app1", "first", "").await.unwrap();
    send_notification(&fd, "app2", "second", "").await.unwrap();

    let slate = slate_notif_proxy(&conn).await.unwrap();
    slate.call_method("DismissAll", &()).await.unwrap();

    let reply = slate.call_method("GetActive", &()).await.unwrap();
    let active: String = reply.body().deserialize().unwrap();
    // After DismissAll, non-persistent notifications should be gone.
    // The TOML may be empty or contain only persistent ones.
    assert!(
        !active.contains("app1") && !active.contains("app2"),
        "all non-persistent notifications should be dismissed"
    );

    daemon.shutdown().await.unwrap();
}
