# Wiring & Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire real hardware control into quick settings, polish the notification system (expiry, badges, sound), migrate slate-suggest to Rhea D-Bus, and add settings pages for Rhea and notifications.

**Architecture:** Four independent workstreams that can be parallelized in pairs. Part 1 replaces stubs in slate-common::system with real D-Bus/sysfs/subprocess calls and wires them into slate-shade. Part 2 adds notification expiry, dock badges, and sound. Part 3 replaces direct LLM HTTP calls with Rhea D-Bus. Part 4 expands settings pages.

**Tech Stack:** Rust 2021, zbus 5 (D-Bus), tokio (async), iced 0.13 (GUI), sysfs (brightness), wpctl (volume), pw-play (sound)

**Spec:** `docs/superpowers/specs/2026-03-22-wiring-and-polish-design.md`

---

## File Structure Overview

### Part 1: Quick Settings D-Bus Wiring

| File | Responsibility |
|------|---------------|
| `shell/slate-common/src/system.rs` | `SystemConnection` struct with real D-Bus/sysfs/subprocess methods |
| `shell/slate-shade/src/main.rs` | Add `system_conn` to app state, `Message::SystemResult` variant |
| `shell/slate-shade/src/update.rs` | Wire `QsAction` handlers to real system calls via `Task::perform` |

### Part 2: Notification Polish

| File | Responsibility |
|------|---------------|
| `shell/slate-common/src/notifications.rs` | Add `expire_timeout_ms`, `suppress_sound` fields |
| `shell/slate-common/src/settings.rs` | Add `sound_enabled` to `NotificationSettings` |
| `shell/slate-notifyd/src/dbus/freedesktop.rs` | Store `expire_timeout` parameter |
| `shell/slate-notifyd/src/dbus/mod.rs` | Extract `suppress-sound` hint |
| `shell/slate-notifyd/src/main.rs` | Expiry ticker task, notification sound playback |
| `shell/shoal/src/main.rs` | Notification badge state, initial count fetch |
| `shell/shoal/src/dock.rs` | Render badge circle overlay on icons |
| `shell/shoal/src/dbus_listener.rs` | Subscribe to `GroupChanged` signal |

### Part 3: slate-suggest → Rhea

| File | Responsibility |
|------|---------------|
| `shell/slate-suggest/src/llm.rs` | Replace HTTP calls with Rhea D-Bus `Complete` method |
| `shell/slate-suggest/Cargo.toml` | Swap reqwest/serde_json for zbus/slate-common |

### Part 4: Settings Pages

| File | Responsibility |
|------|---------------|
| `shell/slate-settings/src/pages/ai.rs` | Expand to full Rhea backend configuration |
| `shell/slate-settings/src/pages/notifications.rs` | New page: DND, duration, sound toggles |
| `shell/slate-settings/src/navigation.rs` | Add `Page::Notifications` variant |
| `shell/slate-settings/src/main.rs` | Wire new page into view routing |

---

## Task 1: slate-common::system — Real Hardware Calls

**Must complete before Task 5 (slate-shade wiring).**

**Files:**
- Modify: `shell/slate-common/src/system.rs`
- Modify: `shell/slate-common/Cargo.toml` (if `tokio` process feature needed)

### Task 1a: SystemConnection struct and WiFi

- [ ] **Step 1: Write tests for WiFi and SystemConnection**

Replace the existing stub tests in `shell/slate-common/src/system.rs` with tests for the real implementation. Keep the module structure but test new behavior:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn system_connection_new_fails_gracefully_without_dbus() {
        // On macOS / CI without D-Bus, this should return an error, not panic.
        let result = SystemConnection::new().await;
        // We can't assert success or failure — depends on environment.
        // Just verify it doesn't panic.
        let _ = result;
    }

    #[tokio::test]
    async fn wifi_enabled_returns_error_without_networkmanager() {
        // When NetworkManager isn't running, should get a clean error.
        if let Ok(conn) = SystemConnection::new().await {
            let result = conn.wifi_enabled().await;
            // Either Ok(bool) or Err — both are acceptable. No panic.
            let _ = result;
        }
    }
}
```

- [ ] **Step 2: Implement SystemConnection and WiFi methods**

Replace the module content. Keep `SystemError` as-is. Add `SystemConnection`:

```rust
pub struct SystemConnection {
    system_bus: zbus::Connection,
    session_bus: zbus::Connection,
}

impl SystemConnection {
    pub async fn new() -> Result<Self, SystemError> {
        let system_bus = zbus::Connection::system().await
            .map_err(|e| SystemError::Dbus(format!("system bus: {e}")))?;
        let session_bus = zbus::Connection::session().await
            .map_err(|e| SystemError::Dbus(format!("session bus: {e}")))?;
        Ok(Self { system_bus, session_bus })
    }

    pub async fn wifi_enabled(&self) -> Result<bool, SystemError> {
        let proxy = zbus::Proxy::builder(&self.system_bus)
            .destination("org.freedesktop.NetworkManager")
            .map_err(|e| SystemError::Dbus(e.to_string()))?
            .path("/org/freedesktop/NetworkManager")
            .map_err(|e| SystemError::Dbus(e.to_string()))?
            .interface("org.freedesktop.NetworkManager")
            .map_err(|e| SystemError::Dbus(e.to_string()))?
            .build().await
            .map_err(|e| SystemError::Dbus(e.to_string()))?;
        proxy.get_property("WirelessEnabled").await
            .map_err(|e| SystemError::Dbus(e.to_string()))
    }

    pub async fn set_wifi_enabled(&self, enabled: bool) -> Result<(), SystemError> {
        let proxy = zbus::Proxy::builder(&self.system_bus)
            .destination("org.freedesktop.NetworkManager")
            .map_err(|e| SystemError::Dbus(e.to_string()))?
            .path("/org/freedesktop/NetworkManager")
            .map_err(|e| SystemError::Dbus(e.to_string()))?
            .interface("org.freedesktop.NetworkManager")
            .map_err(|e| SystemError::Dbus(e.to_string()))?
            .build().await
            .map_err(|e| SystemError::Dbus(e.to_string()))?;
        proxy.set_property("WirelessEnabled", enabled).await
            .map_err(|e| SystemError::Dbus(e.to_string()))
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p slate-common -- system::tests`
Expected: PASS (tests handle missing D-Bus gracefully)

- [ ] **Step 4: Commit**

```bash
git add shell/slate-common/src/system.rs
git commit -m "feat(slate-common): add SystemConnection with WiFi D-Bus calls"
```

### Task 1b: Bluetooth

- [ ] **Step 1: Write test for Bluetooth**

```rust
#[tokio::test]
async fn bluetooth_returns_error_without_bluez() {
    if let Ok(conn) = SystemConnection::new().await {
        let _ = conn.bluetooth_enabled().await;
    }
}
```

- [ ] **Step 2: Implement Bluetooth methods**

```rust
pub async fn bluetooth_enabled(&self) -> Result<bool, SystemError> {
    let proxy = zbus::Proxy::builder(&self.system_bus)
        .destination("org.bluez")
        .map_err(|e| SystemError::Dbus(e.to_string()))?
        .path("/org/bluez/hci0")
        .map_err(|e| SystemError::Dbus(e.to_string()))?
        .interface("org.bluez.Adapter1")
        .map_err(|e| SystemError::Dbus(e.to_string()))?
        .build().await
        .map_err(|e| SystemError::Unavailable("no Bluetooth adapter".into()))?;
    proxy.get_property("Powered").await
        .map_err(|e| SystemError::Dbus(e.to_string()))
}

pub async fn set_bluetooth_enabled(&self, enabled: bool) -> Result<(), SystemError> {
    // Same proxy pattern, set_property("Powered", enabled)
}
```

- [ ] **Step 3: Run tests, commit**

### Task 1c: Volume (wpctl subprocess)

- [ ] **Step 1: Write tests for volume parsing**

```rust
#[test]
fn parse_wpctl_volume_normal() {
    assert!((parse_wpctl_volume("Volume: 0.50").unwrap() - 0.5).abs() < 1e-6);
}

#[test]
fn parse_wpctl_volume_muted() {
    assert!((parse_wpctl_volume("Volume: 0.75 [MUTED]").unwrap() - 0.75).abs() < 1e-6);
}

#[test]
fn parse_wpctl_volume_invalid() {
    assert!(parse_wpctl_volume("garbage").is_none());
}

#[test]
fn parse_wpctl_volume_empty() {
    assert!(parse_wpctl_volume("").is_none());
}
```

- [ ] **Step 2: Implement volume methods**

```rust
fn parse_wpctl_volume(output: &str) -> Option<f32> {
    // Parse "Volume: 0.50" or "Volume: 0.50 [MUTED]"
    let line = output.lines().find(|l| l.starts_with("Volume:"))?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    parts.get(1)?.parse::<f32>().ok()
}

impl SystemConnection {
    pub async fn get_volume(&self) -> Result<f32, SystemError> {
        let output = tokio::process::Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output().await
            .map_err(|e| SystemError::Unavailable(format!("wpctl: {e}")))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_wpctl_volume(&stdout)
            .ok_or_else(|| SystemError::Unavailable("could not parse wpctl output".into()))
    }

    pub async fn set_volume(&self, level: f32) -> Result<(), SystemError> {
        let level_str = format!("{:.2}", level.clamp(0.0, 1.0));
        let status = tokio::process::Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &level_str])
            .status().await
            .map_err(|e| SystemError::Unavailable(format!("wpctl: {e}")))?;
        if status.success() { Ok(()) }
        else { Err(SystemError::Dbus(format!("wpctl exited {status}"))) }
    }
}
```

- [ ] **Step 3: Run tests, commit**

### Task 1d: Brightness (sysfs)

- [ ] **Step 1: Write tests for brightness parsing**

```rust
#[test]
fn brightness_fraction_calculates_correctly() {
    assert!((brightness_fraction(128, 255) - 0.502).abs() < 0.01);
}

#[test]
fn brightness_fraction_max_is_one() {
    assert!((brightness_fraction(255, 255) - 1.0).abs() < 1e-6);
}

#[test]
fn brightness_fraction_zero_max_returns_zero() {
    assert!((brightness_fraction(0, 0) - 0.0).abs() < 1e-6);
}

#[test]
fn brightness_to_raw_scales_correctly() {
    assert_eq!(brightness_to_raw(0.5, 200), 100);
}
```

- [ ] **Step 2: Implement brightness methods**

Helper functions + `get_brightness`/`set_brightness` on `SystemConnection`. Use `tokio::fs::read_to_string` for sysfs reads and `tokio::fs::write` for writes. Find backlight dir with `tokio::fs::read_dir("/sys/class/backlight/")`.

- [ ] **Step 3: Run tests, commit**

### Task 1e: Battery, AC, Connectivity, DND

- [ ] **Step 1: Implement remaining methods**

- `on_ac_power()`: UPower `OnBattery` property (inverted)
- `battery_percent()`: UPower `Percentage` property on battery device, return `Ok(None)` if no battery
- `is_connected()`: NetworkManager `Connectivity` property >= 3
- `dnd_enabled()` / `set_dnd()`: read/write `dnd` property on `org.slate.Notifications` via session bus

- [ ] **Step 2: Write tests for each, run, commit**

```bash
git commit -m "feat(slate-common): complete SystemConnection with all hardware methods"
```

---

## Task 2: Notification Expiry and Sound

**Independent of Task 1. Can be parallelized.**

**Files:**
- Modify: `shell/slate-common/src/notifications.rs`
- Modify: `shell/slate-common/src/settings.rs`
- Modify: `shell/slate-notifyd/src/dbus/freedesktop.rs`
- Modify: `shell/slate-notifyd/src/dbus/mod.rs`
- Modify: `shell/slate-notifyd/src/main.rs`

### Task 2a: Add expire_timeout_ms and suppress_sound to Notification

- [ ] **Step 1: Write test for new fields**

In `shell/slate-common/src/notifications.rs`:

```rust
#[test]
fn notification_default_expire_timeout_is_server_default() {
    let n = Notification::new("app", "summary", "body");
    assert_eq!(n.expire_timeout_ms, -1);
}

#[test]
fn notification_suppress_sound_defaults_false() {
    let n = Notification::new("app", "summary", "body");
    assert!(!n.suppress_sound);
}

#[test]
fn notification_deserializes_without_expire_field() {
    // Existing persisted notifications without the new field should deserialize.
    let toml_str = r#"
        uuid = "550e8400-e29b-41d4-a716-446655440000"
        fd_id = 1
        app_name = "test"
        summary = "hello"
        body = ""
        timestamp = "2026-03-22T00:00:00Z"
    "#;
    let n: Notification = toml::from_str(toml_str).expect("should deserialize");
    assert_eq!(n.expire_timeout_ms, -1);
    assert!(!n.suppress_sound);
}
```

- [ ] **Step 2: Add fields to Notification struct**

```rust
#[serde(default = "default_expire_timeout")]
pub expire_timeout_ms: i32,

#[serde(default)]
pub suppress_sound: bool,
```

Add `fn default_expire_timeout() -> i32 { -1 }` helper.

- [ ] **Step 3: Run tests, verify pass, commit**

### Task 2b: Store expire_timeout in freedesktop Notify handler

- [ ] **Step 1: In `dbus/freedesktop.rs`, rename `_expire_timeout` to `expire_timeout`**

Store it on the notification being constructed. Set `notification.expire_timeout_ms = expire_timeout;`

- [ ] **Step 2: Extract suppress-sound hint in `dbus/mod.rs`**

Add `extract_bool_hint(hints, "suppress-sound")` alongside existing hint extractors. Set `notification.suppress_sound`.

- [ ] **Step 3: Run tests, commit**

### Task 2c: Add sound_enabled to NotificationSettings

- [ ] **Step 1: Add field to `shell/slate-common/src/settings.rs`**

```rust
pub struct NotificationSettings {
    pub dnd: bool,
    pub heads_up_duration_secs: u32,
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
}
```

Add `fn default_true() -> bool { true }`.

- [ ] **Step 2: Test, commit**

### Task 2d: Expiry ticker in slate-notifyd main

- [ ] **Step 1: Add expiry ticker task**

In `shell/slate-notifyd/src/main.rs`, spawn a task alongside the persistence loop:

```rust
async fn expiry_loop(
    store: SharedStore,
    conn: zbus::Connection,
    default_timeout_ms: i64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        let now = chrono::Utc::now();

        // Phase 1: collect expired UUIDs under read lock
        let expired: Vec<(Uuid, u32, String)> = {
            let s = store.read().await;
            s.get_active().iter().filter_map(|n| {
                if n.persistent { return None; }
                let timeout = if n.expire_timeout_ms == -1 {
                    default_timeout_ms
                } else if n.expire_timeout_ms == 0 {
                    return None; // never expires
                } else {
                    n.expire_timeout_ms as i64
                };
                let elapsed = (now - n.timestamp).num_milliseconds();
                if elapsed >= timeout {
                    Some((n.uuid, n.fd_id, n.app_name.clone()))
                } else {
                    None
                }
            }).collect()
        };

        if expired.is_empty() { continue; }

        // Phase 2: dismiss under write lock
        {
            let mut s = store.write().await;
            for (uuid, _, _) in &expired {
                s.dismiss(uuid);
            }
        }

        // Phase 3: emit signals (no lock held)
        for (uuid, fd_id, _app_name) in &expired {
            // Emit NotificationClosed(fd_id, reason=1) on freedesktop path
            // Emit Dismissed(uuid) on slate path
            // (use SignalEmitter::new like the existing dismiss handlers)
        }
    }
}
```

- [ ] **Step 2: Wire expiry loop into main's tokio::select!**

- [ ] **Step 3: Test, commit**

### Task 2e: Notification sound playback

- [ ] **Step 1: Add sound playback helper to slate-notifyd**

In `shell/slate-notifyd/src/main.rs` (or a new `sound.rs` if main gets too big):

```rust
const NOTIFICATION_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/message-new-instant.ogg";

async fn play_notification_sound() {
    if !tokio::fs::try_exists(NOTIFICATION_SOUND).await.unwrap_or(false) {
        return;
    }
    match tokio::process::Command::new("pw-play")
        .arg(NOTIFICATION_SOUND)
        .spawn()
    {
        Ok(_) => {} // fire and forget
        Err(e) => tracing::debug!("notification sound failed: {e}"),
    }
}
```

- [ ] **Step 2: Call from Notify handler**

After adding a new notification, if `!dnd && urgency != Low && !suppress_sound && sound_enabled`:
```rust
tokio::spawn(play_notification_sound());
```

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(slate-notifyd): add notification expiry and sound playback"
```

---

## Task 3: Dock Notification Badges

**Depends on: Task 2a (expire_timeout field). Independent otherwise.**

**Files:**
- Modify: `shell/shoal/src/main.rs`
- Modify: `shell/shoal/src/dock.rs`
- Modify: `shell/shoal/src/dbus_listener.rs`

### Task 3a: Subscribe to GroupChanged signal

- [ ] **Step 1: Add NotificationCountChanged to DockDbusEvent**

In `shell/shoal/src/dbus_listener.rs`:

```rust
pub enum DockDbusEvent {
    PaletteChanged(Palette),
    Show,
    Hide,
    NotificationCountChanged(String, u32), // (app_name, count)
}
```

- [ ] **Step 2: Add listen_notification_counts function**

Subscribe to `org.slate.Notifications.GroupChanged(app_name: String, count: u32)` using the same `MatchRule` pattern as existing listeners. Use `NOTIFICATIONS_BUS_NAME`, `NOTIFICATIONS_PATH`, `NOTIFICATIONS_INTERFACE` from `slate-common::dbus`.

- [ ] **Step 3: Call from the D-Bus subscription in main.rs**

Spawn `listen_notification_counts(sender)` alongside `listen_palette` and `listen_dock_signals`.

- [ ] **Step 4: Test, commit**

### Task 3b: Badge state and initial count fetch

- [ ] **Step 1: Add notification_counts to Shoal state**

```rust
notification_counts: std::collections::HashMap<String, u32>,
```

- [ ] **Step 2: Handle NotificationCountChanged message**

In the update function, on `NotificationCountChanged(app_name, count)`:
- If count > 0, insert into map
- If count == 0, remove from map

- [ ] **Step 3: Fetch initial counts on startup**

Call `org.slate.Notifications.GetActive()` via D-Bus, parse the TOML response, aggregate counts per `app_name`. Send each as a `NotificationCountChanged` message.

- [ ] **Step 4: Commit**

### Task 3c: Render badge overlay on dock icons

- [ ] **Step 1: In `shell/shoal/src/dock.rs`, add badge rendering**

When rendering each `DockIcon`, check `notification_counts` for a match. If count > 0, overlay a small circle in the top-right corner with the count text. Use accent color from palette.

Matching logic:
```rust
fn get_badge_count(icon: &DockIcon, counts: &HashMap<String, u32>) -> u32 {
    // Try exact match on desktop_entry app_id
    if let Some(&count) = counts.get(&icon.app_id) {
        return count;
    }
    // Fallback: case-insensitive stem match
    let stem = icon.app_id.rsplit('.').next().unwrap_or(&icon.app_id);
    for (app_name, &count) in counts {
        if app_name.eq_ignore_ascii_case(stem) {
            return count;
        }
    }
    0
}
```

- [ ] **Step 2: Write tests for matching logic, run, commit**

```bash
git commit -m "feat(shoal): add notification count badges on dock icons"
```

---

## Task 4: slate-suggest → Rhea Migration

**Independent. Can be parallelized with Tasks 1-3.**

**Files:**
- Modify: `shell/slate-suggest/src/llm.rs`
- Modify: `shell/slate-suggest/Cargo.toml`

- [ ] **Step 1: Update Cargo.toml**

Remove `reqwest` and `serde_json` from `[dependencies]`. Add `zbus = { workspace = true }` and `slate-common = { path = "../slate-common" }` if not already present.

- [ ] **Step 2: Verify the build fails**

Run: `cargo check -p slate-suggest`
Expected: FAIL (llm.rs uses reqwest/serde types that no longer exist)

- [ ] **Step 3: Replace llm.rs implementation**

Replace the entire file content:

```rust
/// LLM suggestion client via Rhea D-Bus.
///
/// Queries the Rhea AI engine for command completions. Falls back
/// gracefully to None if Rhea is unavailable or times out.
use std::time::Duration;

/// Hard timeout — suggestions must feel instant.
const RHEA_TIMEOUT: Duration = Duration::from_millis(500);

/// Query Rhea for a command completion.
///
/// Returns `Some(completion)` on success, `None` on any error.
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
        .destination(RHEA_BUS_NAME).ok()?
        .path(RHEA_PATH).ok()?
        .interface(RHEA_INTERFACE).ok()?
        .build().await.ok()?;

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
    if trimmed.is_empty() { None } else { Some(trimmed) }
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
        // Rhea is not running in test environments.
        let result = query_llm("git").await;
        assert!(result.is_none());
    }
}
```

- [ ] **Step 4: Verify build succeeds and tests pass**

Run: `cargo test -p slate-suggest`
Expected: PASS

- [ ] **Step 5: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add shell/slate-suggest/src/llm.rs shell/slate-suggest/Cargo.toml
git commit -m "feat(slate-suggest): replace direct LLM HTTP with Rhea D-Bus"
```

---

## Task 5: Wire QsAction in slate-shade

**Depends on: Task 1 (SystemConnection).**

**Files:**
- Modify: `shell/slate-shade/src/main.rs`
- Modify: `shell/slate-shade/src/update.rs`

- [ ] **Step 1: Add SystemConnection to SlateShade state**

In `shell/slate-shade/src/main.rs`, add:
```rust
use std::sync::Arc;
use slate_common::system::SystemConnection;

struct SlateShade {
    // ... existing fields ...
    system_conn: Option<Arc<SystemConnection>>,
    last_brightness_write: std::time::Instant,
    last_volume_write: std::time::Instant,
}
```

Initialize `system_conn` via `Task::perform` at startup (like the layout detection). On success, send `Message::SystemConnReady(Arc<SystemConnection>)`.

- [ ] **Step 2: Add Message::SystemResult and Message::SystemConnReady variants**

- [ ] **Step 3: Wire QsAction handlers in update.rs**

Change `handle_qs_action` to return `Task<Message>`:

```rust
fn handle_qs_action(app: &mut SlateShade, action: QsAction) -> Task<Message> {
    match action {
        QsAction::TileToggled(kind) => {
            app.qs.toggle_tile(kind); // optimistic UI update
            if let Some(conn) = &app.system_conn {
                let conn = conn.clone();
                let active = app.qs.tile_active(kind);
                return match kind {
                    TileKind::WiFi => Task::perform(
                        async move { conn.set_wifi_enabled(active).await
                            .map_err(|e| e.to_string()) },
                        Message::SystemResult,
                    ),
                    TileKind::Bluetooth => Task::perform(
                        async move { conn.set_bluetooth_enabled(active).await
                            .map_err(|e| e.to_string()) },
                        Message::SystemResult,
                    ),
                    TileKind::DoNotDisturb => Task::perform(
                        async move { conn.set_dnd(active).await
                            .map_err(|e| e.to_string()) },
                        Message::SystemResult,
                    ),
                    _ => Task::none(), // Other tiles are local-only
                };
            }
            Task::none()
        }
        QsAction::BrightnessChanged(v) => {
            app.qs.set_brightness(v);
            // Debounce: only write if >= 100ms since last write
            let now = std::time::Instant::now();
            if now.duration_since(app.last_brightness_write) >= Duration::from_millis(100) {
                app.last_brightness_write = now;
                if let Some(conn) = &app.system_conn {
                    let conn = conn.clone();
                    return Task::perform(
                        async move { conn.set_brightness(v).await.map_err(|e| e.to_string()) },
                        Message::SystemResult,
                    );
                }
            }
            Task::none()
        }
        QsAction::VolumeChanged(v) => {
            app.qs.set_volume(v);
            let now = std::time::Instant::now();
            if now.duration_since(app.last_volume_write) >= Duration::from_millis(100) {
                app.last_volume_write = now;
                if let Some(conn) = &app.system_conn {
                    let conn = conn.clone();
                    return Task::perform(
                        async move { conn.set_volume(v).await.map_err(|e| e.to_string()) },
                        Message::SystemResult,
                    );
                }
            }
            Task::none()
        }
    }
}
```

- [ ] **Step 4: Handle Message::SystemResult and Message::SystemConnReady in update_app**

**IMPORTANT:** Also update the `QsAction` match arm in `update_app` to use `return`:
```rust
Message::QsAction(action) => return handle_qs_action(app, action),
```
(Previously it was `handle_qs_action(app, action)` without `return`, which would silently drop the Task.)

Add new message handlers:
```rust
Message::SystemResult(Err(e)) => {
    tracing::warn!("quick settings: {e}");
}
Message::SystemResult(Ok(())) => {}
Message::SystemConnReady(conn) => {
    app.system_conn = Some(conn);
}
```

- [ ] **Step 5: Run all slate-shade tests**

Run: `cargo test -p slate-shade`

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(slate-shade): wire quick settings tiles to real system calls"
```

---

## Task 6: Settings Pages

**Independent. Can be parallelized with Tasks 2-3.**

**Files:**
- Modify: `shell/slate-settings/src/pages/ai.rs`
- Create: `shell/slate-settings/src/pages/notifications.rs`
- Modify: `shell/slate-settings/src/navigation.rs`
- Modify: `shell/slate-settings/src/main.rs`
- Modify: `shell/slate-common/src/settings.rs` (if sound_enabled not yet added by Task 2c)

### Task 6a: Expand AI page to Rhea settings

- [ ] **Step 1: Rename page title and replace toggle with backend selector**

Replace "AI & LLM" with "Rhea". Replace the single enable/disable toggle with radio-style buttons for backend selection: None / Local / Claude / OpenAI / Ollama. Each sets `settings.rhea.backend`.

- [ ] **Step 2: Add conditional backend configuration sections**

Show fields based on selected backend:
- **Local**: model picker (existing), idle timeout
- **Claude**: api_key_file input, model name
- **OpenAI**: base_url, api_key_file, model
- **Ollama**: base_url, model

- [ ] **Step 3: Remove flag file mechanism**

Delete `llm_flag_path()` and remove `arkhe_service_ctl` calls. Keep `scan_models()` — it's still needed for the model file picker. Remove the flag file creation/deletion in `EnabledToggled`. Rhea handles model lifecycle directly.

- [ ] **Step 4: Add GetStatus indicator**

On page load, call `org.slate.Rhea.GetStatus()` via `Task::perform`. Display result as a status label (e.g., "Backend: local, Model: phi-3, Status: warm").

- [ ] **Step 5: Test, commit**

### Task 6b: New Notifications settings page

- [ ] **Step 1: Create `shell/slate-settings/src/pages/notifications.rs`**

```rust
pub enum NotifMsg {
    DndToggled(bool),
    DurationChanged(f32),
    SoundToggled(bool),
}

pub fn update(settings: &mut NotificationSettings, msg: NotifMsg) {
    match msg {
        NotifMsg::DndToggled(v) => settings.dnd = v,
        NotifMsg::DurationChanged(v) => settings.heads_up_duration_secs = v as u32,
        NotifMsg::SoundToggled(v) => settings.sound_enabled = v,
    }
}

pub fn view(settings: &NotificationSettings) -> Element<NotifMsg> {
    // Title, DND toggler, duration slider (1-10), sound toggler
}
```

- [ ] **Step 2: Add Page::Notifications to navigation.rs**

Update `Page` enum, `Page::all()`, `Page::label()` ("Notifications"), `Page::icon()` ("\u{1f514}" — bell).

- [ ] **Step 3: Wire into main.rs page router**

Add `mod notifications` to pages module, handle in view match.

- [ ] **Step 4: Update test for page count**

The existing test `page_list_has_all_expected_pages` asserts `pages.len() == 9`. Update to `10`.

- [ ] **Step 5: Run all tests, commit**

```bash
git commit -m "feat(slate-settings): add Rhea backend config and Notifications settings page"
```

---

## Task 7: Final Verification

- [ ] **Step 1: Run full workspace verification**

```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo fmt --check
```

All must pass with zero warnings.

- [ ] **Step 2: Verify all files under 500 lines**

Check every modified file.

- [ ] **Step 3: Commit any fixes**

---

## Parallelization Guide

```
Task 1 (system.rs) ──────────→ Task 5 (shade wiring)
                                        ↓
Task 2 (notif expiry/sound) ──→ Task 3 (dock badges) ──→ Task 7 (verify)
                                        ↑
Task 4 (slate-suggest)        Task 6 (settings pages) ──┘
```

**Parallel batch 1:** Tasks 1, 2, 4, 6 (all independent)
**Parallel batch 2:** Tasks 3, 5 (depend on batch 1)
**Sequential:** Task 7 (final verification)
