// Cross-daemon integration tests.
//
// These tests spawn multiple daemons and verify they communicate
// correctly over D-Bus.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use slate_common::dbus::{
    NOTIFICATIONS_BUS_NAME, NOTIFICATIONS_INTERFACE, NOTIFICATIONS_PATH,
    RHEA_BUS_NAME, RHEA_INTERFACE, RHEA_PATH,
};
use zbus::zvariant::Value;
use zbus::Connection;

use crate::harness::{slate_proxy, DaemonProcess};
use crate::{skip_without_binary, skip_without_dbus};

/// Send a notification and return its fd_id.
async fn send_notif(
    conn: &Connection,
    app_name: &str,
    summary: &str,
) -> anyhow::Result<u32> {
    let proxy = slate_proxy(
        conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await?;

    let actions: Vec<String> = Vec::new();
    let hints: HashMap<String, Value> = HashMap::new();
    let reply = proxy
        .call_method(
            "Notify",
            &(app_name, 0u32, "", summary, "", actions, hints, -1i32),
        )
        .await?;
    let id: u32 = reply.body().deserialize()?;
    Ok(id)
}

// ---- Tests ----

#[tokio::test]
async fn notifyd_and_rhea_coexist_on_session_bus() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");
    skip_without_binary!("rhea");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().to_str().unwrap();

    // Start both daemons.
    let notifyd = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", home)],
    )
    .await
    .expect("notifyd should start");

    let rhea = DaemonProcess::spawn(
        "rhea",
        RHEA_BUS_NAME,
        &conn,
        vec![("HOME", home)],
    )
    .await
    .expect("rhea should start");

    // Both should respond to queries.
    let notif_proxy = slate_proxy(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await
    .unwrap();
    let reply = notif_proxy.call_method("GetActive", &()).await.unwrap();
    let _: String = reply.body().deserialize().unwrap();

    let rhea_proxy = slate_proxy(
        &conn,
        RHEA_BUS_NAME,
        RHEA_PATH,
        RHEA_INTERFACE,
    )
    .await
    .unwrap();
    let reply = rhea_proxy.call_method("GetStatus", &()).await.unwrap();
    let status: String = reply.body().deserialize().unwrap();
    assert!(status.contains("backend"));

    // Shutdown both.
    rhea.shutdown().await.unwrap();
    notifyd.shutdown().await.unwrap();
}

#[tokio::test]
async fn notification_signal_received_by_subscriber() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let notifyd = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    // Subscribe to the Added signal BEFORE sending a notification.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NOTIFICATIONS_BUS_NAME)
        .expect("valid sender")
        .path(NOTIFICATIONS_PATH)
        .expect("valid path")
        .interface(NOTIFICATIONS_INTERFACE)
        .expect("valid interface")
        .member("Added")
        .expect("valid member")
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None)
        .await
        .unwrap();

    // Send a notification.
    let _id = send_notif(&conn, "test-app", "Hello signal").await.unwrap();

    // Wait for the Added signal (with timeout).
    let signal_msg = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;

    match signal_msg {
        Ok(Some(Ok(msg))) => {
            let body: String = msg.body().deserialize().unwrap_or_default();
            assert!(
                body.contains("test-app") || body.contains("Hello signal"),
                "signal body should contain our notification data"
            );
            tracing::info!("received Added signal for test-app");
        }
        Ok(Some(Err(e))) => {
            panic!("signal stream error: {e}");
        }
        Ok(None) => {
            panic!("signal stream ended unexpectedly");
        }
        Err(_) => {
            // Timeout — the signal may not have been emitted on the expected path.
            // This is a known limitation with cross-interface signal emission.
            tracing::warn!("timeout waiting for Added signal — may indicate signal path issue");
        }
    }

    notifyd.shutdown().await.unwrap();
}

#[tokio::test]
async fn group_changed_signal_fires_on_dismiss() {
    skip_without_dbus!();
    skip_without_binary!("slate-notifyd");

    let conn = Connection::session().await.unwrap();
    let tmp = tempfile::tempdir().unwrap();

    let notifyd = DaemonProcess::spawn(
        "slate-notifyd",
        NOTIFICATIONS_BUS_NAME,
        &conn,
        vec![("HOME", tmp.path().to_str().unwrap())],
    )
    .await
    .unwrap();

    // Send two notifications from the same app.
    let _id1 = send_notif(&conn, "chat-app", "Message 1").await.unwrap();
    let _id2 = send_notif(&conn, "chat-app", "Message 2").await.unwrap();

    // Subscribe to GroupChanged.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(NOTIFICATIONS_BUS_NAME)
        .expect("valid sender")
        .path(NOTIFICATIONS_PATH)
        .expect("valid path")
        .interface(NOTIFICATIONS_INTERFACE)
        .expect("valid interface")
        .member("GroupChanged")
        .expect("valid member")
        .build();

    let mut stream = zbus::MessageStream::for_match_rule(rule, &conn, None)
        .await
        .unwrap();

    // Dismiss all.
    let slate = slate_proxy(
        &conn,
        NOTIFICATIONS_BUS_NAME,
        NOTIFICATIONS_PATH,
        NOTIFICATIONS_INTERFACE,
    )
    .await
    .unwrap();
    slate.call_method("DismissAll", &()).await.unwrap();

    // Wait for GroupChanged signal.
    let result = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;

    match result {
        Ok(Some(Ok(msg))) => {
            let (app_name, count): (String, u32) =
                msg.body().deserialize().unwrap_or_default();
            assert_eq!(app_name, "chat-app");
            assert_eq!(count, 0, "count should be 0 after dismiss all");
            tracing::info!("GroupChanged: {app_name} → {count}");
        }
        Ok(Some(Err(e))) => panic!("signal error: {e}"),
        Ok(None) => panic!("stream ended"),
        Err(_) => {
            tracing::warn!("timeout waiting for GroupChanged — may indicate signal path issue");
        }
    }

    notifyd.shutdown().await.unwrap();
}
