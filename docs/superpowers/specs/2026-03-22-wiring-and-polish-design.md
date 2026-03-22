# Wiring & Polish Design Spec

Four independent workstreams to make the Slate shell functional for daily use. Each can be built and shipped independently.

---

## Part 1: Quick Settings D-Bus Wiring

### Goal
Replace stub implementations in `slate-common::system` with real D-Bus/sysfs calls so slate-shade quick settings tiles actually control hardware.

### System Services on Chimera Linux

| Feature | Service | D-Bus Interface | Method/Property |
|---------|---------|----------------|-----------------|
| WiFi toggle | NetworkManager | `org.freedesktop.NetworkManager` on system bus | Property `WirelessEnabled` (bool, read/write) |
| WiFi status | NetworkManager | same | Property `WirelessEnabled` |
| Bluetooth toggle | BlueZ | `org.bluez` on system bus | Property `Powered` on adapter `/org/bluez/hci0` |
| Bluetooth status | BlueZ | same | Property `Powered` |
| Volume get/set | WirePlumber | CLI subprocess | `wpctl get-volume @DEFAULT_AUDIO_SINK@` / `wpctl set-volume @DEFAULT_AUDIO_SINK@ <val>` |
| Brightness get/set | sysfs | filesystem | `/sys/class/backlight/*/brightness` and `max_brightness` |
| Battery | UPower | `org.freedesktop.UPower` on system bus | Properties on `/org/freedesktop/UPower/devices/battery_BAT0` |
| AC power | UPower | same | Property `OnBattery` on `/org/freedesktop/UPower` |
| Connectivity | NetworkManager | same as WiFi | Property `Connectivity` (enum: 1=none, 4=full) |
| DND toggle | slate-notifyd | `org.slate.Notifications` on session bus | Property `dnd` (bool, already implemented) |

### Architecture

```
slate-shade quick settings
  → calls slate_common::system::{wifi_enabled, set_wifi_enabled, ...}
  → each function makes the appropriate D-Bus call or subprocess/sysfs read
  → results flow back via iced::Task::perform
```

### Changes to `shell/slate-common/src/system.rs`

Replace every stub function with a real implementation. Each function takes a `zbus::Connection` parameter (system bus) to avoid creating a new connection per call. Add a `SystemConnection` wrapper:

```rust
pub struct SystemConnection {
    system_bus: zbus::Connection,
}

impl SystemConnection {
    pub async fn new() -> Result<Self, SystemError> {
        let system_bus = zbus::Connection::system().await
            .map_err(|e| SystemError::Dbus(e.to_string()))?;
        Ok(Self { system_bus })
    }
}
```

**WiFi** — `org.freedesktop.NetworkManager` on system bus, path `/org/freedesktop/NetworkManager`:
```rust
impl SystemConnection {
    pub async fn wifi_enabled(&self) -> Result<bool, SystemError> {
        let proxy = zbus::Proxy::builder(&self.system_bus)
            .destination("org.freedesktop.NetworkManager")?
            .path("/org/freedesktop/NetworkManager")?
            .interface("org.freedesktop.NetworkManager")?
            .build().await?;
        let val: bool = proxy.get_property("WirelessEnabled").await?;
        Ok(val)
    }

    pub async fn set_wifi_enabled(&self, enabled: bool) -> Result<(), SystemError> {
        // Same proxy, set_property("WirelessEnabled", enabled)
    }
}
```

**Bluetooth** — `org.bluez` on system bus, adapter path `/org/bluez/hci0`:
- Get/set `Powered` property on `org.bluez.Adapter1` interface.
- If adapter path doesn't exist, return `SystemError::Unavailable`.

**Volume** — subprocess `wpctl`:
- Get: `wpctl get-volume @DEFAULT_AUDIO_SINK@` → parse "Volume: 0.50" → f32
- Set: `wpctl set-volume @DEFAULT_AUDIO_SINK@ {level}` where level is 0.0-1.0
- Use `tokio::process::Command` (already async).

**Brightness** — sysfs:
- Find first dir in `/sys/class/backlight/`
- Read `max_brightness` and `brightness` files
- Get: `brightness / max_brightness` as f32
- Set: write `(level * max_brightness) as u32` to `brightness` file
- Use `tokio::fs` for async I/O, or `spawn_blocking` for the sysfs reads.
- If no backlight dir exists, return `SystemError::Unavailable`.

**Battery/AC** — `org.freedesktop.UPower`:
- `battery_percent()`: read `Percentage` property on first battery device
- `on_ac_power()`: read `OnBattery` property (inverted) on UPower root object
- If UPower not running, return `Ok(None)` / `Ok(false)`.

**Connectivity** — NetworkManager:
- Read `Connectivity` property. Values: 0=unknown, 1=none, 2=portal, 3=limited, 4=full.
- Return `true` if >= 3 (limited or full).

**DND** — already implemented in slate-notifyd. Quick settings just needs to call:
```rust
pub async fn dnd_enabled(session_bus: &zbus::Connection) -> Result<bool, SystemError>
pub async fn set_dnd(session_bus: &zbus::Connection, enabled: bool) -> Result<(), SystemError>
```
These read/write the `dnd` property on `org.slate.Notifications`.

### Changes to `SystemConnection`

The current free functions become methods on `SystemConnection`. This avoids creating a D-Bus connection per call. `SystemConnection` stores both a system bus and session bus connection.

```rust
pub struct SystemConnection {
    system_bus: zbus::Connection,
    session_bus: zbus::Connection,
}
```

### Changes to `shell/slate-shade/src/quick_settings.rs`

Currently `QsAction` is emitted but never acted on. In `slate-shade/src/update.rs`, add handlers:

```rust
Message::QsAction(QsAction::TileToggled(TileKind::WiFi)) => {
    Task::perform(toggle_wifi(app.system_conn.clone()), Message::SystemResult)
}
Message::QsAction(QsAction::BrightnessChanged(val)) => {
    Task::perform(set_brightness(app.system_conn.clone(), val), Message::SystemResult)
}
// etc.
```

Add `system_conn: Arc<SystemConnection>` to `SlateShade` app state, created at startup.

### What stays stubbed

- AirplaneMode, AutoRotate, FlashLight, NightLight, Hotspot — these need device-specific sysfs paths that vary per hardware. They stay as local-only toggles for now. Mark tiles as `available: false` unless running on a known device.

### Testing

- Each `SystemConnection` method gets a unit test that exercises error handling (D-Bus not available → graceful fallback)
- Integration tests require actual D-Bus services, so they're `#[ignore]` tagged
- `wpctl` and sysfs functions tested with mock output parsing

---

## Part 2: Notification Polish

### 2a: Auto-Expiry

**Problem:** The freedesktop `Notify` method receives `expire_timeout` (in ms) but it's currently ignored (parameter named `_expire_timeout`).

**Design:**
1. Add `expire_timeout_ms: i32` field to `Notification` struct in `slate-common::notifications`. Values: `-1` = server default, `0` = never expire, `>0` = dismiss after N milliseconds.
2. In `slate-notifyd/src/dbus/freedesktop.rs`, store the value instead of ignoring it.
3. In `slate-notifyd/src/main.rs`, add an expiry ticker task (runs every 1 second):
   - Iterate active notifications
   - If `expire_timeout_ms > 0` and `(now - timestamp) > expire_timeout_ms`, auto-dismiss
   - If `expire_timeout_ms == -1`, use `heads_up_duration_secs * 1000` from settings
   - Emit `Dismissed` + `NotificationClosed(reason=1)` signals for expired notifications
4. Persistent notifications (`persistent: true`) are exempt from auto-expiry regardless of timeout.

### 2b: Dock Notification Badges

**Problem:** shoal doesn't show notification counts on app icons.

**Design:**
1. In `shell/shoal/src/main.rs`, add `notification_counts: HashMap<String, u32>` to `Shoal` state.
2. Subscribe to `org.slate.Notifications.GroupChanged(app_name, count)` signal via D-Bus in shoal's existing `dbus_listener`.
3. On signal: update `notification_counts` map. Count of `0` removes the entry.
4. In `shell/shoal/src/dock.rs`, when rendering each `DockIcon`, check `notification_counts` for a matching `desktop_entry` or `app_name`. If count > 0, render a badge circle (pill shape, accent color, white text).
5. On startup, call `org.slate.Notifications.GetGroupSummary()` to populate initial counts.

**Matching logic:** The `GroupChanged` signal emits `app_name` (e.g., "firefox"). Desktop entries have an `app_id` (e.g., "org.mozilla.firefox") and a `Name` field. Match by:
- Exact `app_name == desktop_entry.app_id`
- Fallback: case-insensitive `app_name` contains desktop_entry filename stem

### 2c: Notification Sound

**Problem:** Notifications arrive silently.

**Design:**
1. In `slate-notifyd/src/main.rs`, after a new notification is added (in the `Notify` handler):
   - Skip if DND is enabled
   - Skip if urgency is `Low`
   - Skip if the notification has `suppress-sound` hint
   - Otherwise: spawn `pw-play /usr/share/sounds/freedesktop/stereo/message-new-instant.ogg` subprocess
2. Use `tokio::process::Command` (fire and forget, log errors).
3. The sound file path is the freedesktop default. If the file doesn't exist, skip silently.
4. Add `sound_enabled: bool` field to `NotificationSettings` (default `true`). Respect it.

---

## Part 3: slate-suggest → Rhea Migration

### Goal
Replace direct llama.cpp HTTP calls in `shell/slate-suggest/src/llm.rs` with Rhea D-Bus calls.

### Current Architecture
- `llm.rs` makes direct HTTP requests to `localhost:8081/completion` (llama.cpp native API)
- Has its own timeout handling, request formatting, response parsing
- Duplicates model lifecycle management that Rhea now handles

### New Architecture
- Replace `query_llm()` with a D-Bus call to `org.slate.Rhea.Complete(prompt, system_prompt)`
- Use `zbus::Proxy::call_method` with a 500ms timeout (same responsiveness guarantee)
- Rhea handles backend selection, model lifecycle, API formatting
- Remove `reqwest` dependency from slate-suggest (it only uses it for LLM)
- Remove `serde_json` dependency from slate-suggest (same reason)

### Changes

**`shell/slate-suggest/src/llm.rs`:**
Replace the entire implementation:
```rust
pub async fn query_llm(input: &str) -> Option<String> {
    if input.trim().is_empty() { return None; }
    #[cfg(target_os = "linux")]
    {
        let conn = zbus::Connection::session().await.ok()?;
        let proxy = zbus::Proxy::builder(&conn)
            .destination(slate_common::dbus::RHEA_BUS_NAME)?
            .path(slate_common::dbus::RHEA_PATH)?
            .interface(slate_common::dbus::RHEA_INTERFACE)?
            .build().await.ok()?;

        let system_prompt = "You are a shell command completion engine. Given a partial command, output ONLY the completed command. No explanation.";
        let result: Result<String, _> = tokio::time::timeout(
            Duration::from_millis(500),
            proxy.call_method("Complete", &(input, system_prompt))
        ).await.ok()?.map(|r| r.body().deserialize().ok()).flatten();
        result.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }
    #[cfg(not(target_os = "linux"))]
    { None }
}
```

**`shell/slate-suggest/Cargo.toml`:**
- Remove `reqwest` and `serde_json` dependencies
- Add `zbus = { workspace = true }` if not already present
- Add `slate-common = { path = "../slate-common" }` if not already present

**`shell/slate-suggest/src/dbus_listener.rs`:**
- Add subscription to `org.slate.Rhea.BackendChanged` signal (optional, for status awareness)

### Testing
- Existing tests still pass (LLM unavailable → `None`, empty input → `None`)
- Add test for the new D-Bus path (mock or just verify graceful failure when no session bus)

---

## Part 4: Settings Pages

### 4a: Update AI Page → Rhea Settings

**Problem:** The AI settings page (`pages/ai.rs`) only manages the local llama-server via a flag file. It doesn't expose backend selection (local/claude/openai/ollama) or integrate with the Rhea daemon.

**Changes:**
1. Rename page title "AI & LLM" → "Rhea"
2. Replace the single "Enable local LLM" toggle with a backend selector:
   - Radio buttons or dropdown: None / Local / Claude / OpenAI / Ollama
   - Each selection sets `settings.rhea.backend` to the appropriate string
3. Show conditional sections based on selected backend:
   - **Local**: model file picker (existing), idle timeout slider
   - **Claude**: API key file path input, model name input
   - **OpenAI**: base URL, API key file path, model name
   - **Ollama**: base URL, model name
4. Add a status indicator showing Rhea's current state (query `GetStatus()` on page load)
5. Remove the flag file mechanism — Rhea manages model lifecycle directly

### 4b: New Notifications Settings Page

**New file:** `shell/slate-settings/src/pages/notifications.rs`

**Layout:**
- Page title: "Notifications"
- DND toggle (reads/writes `settings.notifications.dnd`)
- Heads-up duration slider (1-10 seconds, default 5)
- Sound toggle (reads/writes `settings.notifications.sound_enabled`)
- Future: per-app notification controls (not in this iteration)

**Changes to settings model:**
- Add `sound_enabled: bool` to `NotificationSettings` in `slate-common::settings`

**Changes to navigation:**
- Add `Page::Notifications` variant to the page enum in `slate-settings/src/navigation.rs`
- Wire into the page router

---

## Dependency Order

These four parts are independent. Recommended build order:
1. **Quick Settings** (biggest impact — makes the shade functional)
2. **Notification Polish** (completes the notification UX)
3. **slate-suggest → Rhea** (small, clean migration)
4. **Settings Pages** (ties configuration together)

Parts 1+3 can be parallelized. Parts 2+4 can be parallelized.

---

## Files Changed Summary

### Part 1: Quick Settings
| File | Change |
|------|--------|
| `shell/slate-common/src/system.rs` | Replace all stubs with real D-Bus/sysfs/subprocess calls |
| `shell/slate-shade/src/main.rs` | Add `SystemConnection` to app state |
| `shell/slate-shade/src/update.rs` | Add `QsAction` handlers with `Task::perform` |
| `shell/slate-shade/src/actions.rs` | Add system call wrappers (or use system.rs directly) |

### Part 2: Notification Polish
| File | Change |
|------|--------|
| `shell/slate-common/src/notifications.rs` | Add `expire_timeout_ms` field |
| `shell/slate-common/src/settings.rs` | Add `sound_enabled` to `NotificationSettings` |
| `shell/slate-notifyd/src/dbus/freedesktop.rs` | Store `expire_timeout` instead of ignoring |
| `shell/slate-notifyd/src/main.rs` | Add expiry ticker task, sound playback |
| `shell/shoal/src/main.rs` | Add notification badge state + D-Bus subscription |
| `shell/shoal/src/dock.rs` | Render badge overlay on icons |
| `shell/shoal/src/dbus_listener.rs` | Subscribe to GroupChanged signal |

### Part 3: slate-suggest → Rhea
| File | Change |
|------|--------|
| `shell/slate-suggest/src/llm.rs` | Replace HTTP calls with Rhea D-Bus |
| `shell/slate-suggest/Cargo.toml` | Remove reqwest/serde_json, add zbus/slate-common |

### Part 4: Settings Pages
| File | Change |
|------|--------|
| `shell/slate-settings/src/pages/ai.rs` | Expand to full Rhea backend configuration |
| `shell/slate-settings/src/pages/notifications.rs` | New page: DND, duration, sound |
| `shell/slate-settings/src/navigation.rs` | Add Notifications page variant |
| `shell/slate-settings/src/main.rs` | Wire new page |
