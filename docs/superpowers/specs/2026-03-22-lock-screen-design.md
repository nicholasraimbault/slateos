# Lock Screen Design Spec

## Goal

A lock screen for SlateOS that secures the device on idle/sleep, requires PIN or password authentication, and shows clock/date/notifications while locked.

---

## Architecture

New crate `shell/slate-lock/` — an iced layer-shell app that runs as a persistent daemon. It stays hidden until receiving a `Lock` D-Bus signal, then covers the screen with a fullscreen overlay.

**Components:**
1. **slate-lock** (iced app) — lock screen UI, always running, hidden until locked
2. **slate-power modification** — sends D-Bus `Lock` signal before suspend on power button press
3. **niri idle config** — triggers lock after configurable idle timeout

No separate idle daemon. niri handles idle detection natively.

---

## Authentication — Hybrid PAM/PIN

### Strategy

Try PAM first; fall back to PIN file if PAM is unavailable. This works in production (Chimera with PAM) and dev (macOS without PAM).

### PAM Path

- Use the `pam` crate (Rust bindings for `libpam`)
- PAM service name: `slate-lock`
- PAM config file: `/etc/pam.d/slate-lock` (installed by rootfs builder)
- Authenticates against the system user's password
- Add `linux-pam` and `shadow` packages to `build/build-rootfs.sh`

### PIN Fallback Path

- PIN stored in `~/.config/slate/lock.toml` as an argon2 hash
- Use `argon2` crate for hashing/verification
- PIN length: 4-8 digits
- If no PIN is set and PAM isn't available, the lock screen unlocks without auth (first-boot grace) and shows a warning banner recommending PIN setup in settings

### Auth Module API

```rust
pub enum AuthResult {
    Success,
    WrongCredential,
    NotConfigured,  // no PAM and no PIN set
}

pub async fn authenticate(pin: &str, username: &str) -> AuthResult {
    // Try PAM first (Linux only, spawn_blocking for the blocking PAM call)
    // Fall back to PIN hash from lock.toml
}

pub fn hash_pin(pin: &str) -> String {
    // argon2 hash for storage
}

pub fn verify_pin(pin: &str, hash: &str) -> bool {
    // argon2 verify
}
```

PAM calls are blocking — wrap in `tokio::task::spawn_blocking` or use `std::thread::spawn` since PAM may not be called from a tokio context (zbus handler thread, same lesson as the sound/history bug).

---

## Layout — Adaptive

Uses `slate_common::layout::FormFactor` / `LayoutMode` detection (same as slate-shade).

### Phone (<600dp)

Two states:

**Idle state** (default on lock):
- Clock (large, centered) + date below
- Notification previews (max 3, from notifyd `GetActive`)
- Bottom hint: "Tap to unlock" + swipe bar

**PIN entry state** (on tap):
- Clock shrinks to top (smaller)
- PIN pad centered: 3x3+1 grid of circular digit buttons
- PIN dots (filled/empty) showing entry progress
- Backspace button
- Spring animation on transition (slate-common::physics)

### Tablet/Desktop (>=600dp)

Single state (no intermediate):
- **Left half:** Clock (large) + date + notification previews
- **Right half:** PIN pad always visible + PIN dots
- Vertical divider line between halves

### Common Elements

- Background: solid dark color from palette (or blurred wallpaper if feasible)
- Palette-aware theming via `slate-common::theme`
- PIN dots: 4-8 circles, filled on entry, all clear on wrong PIN
- Wrong PIN: shake animation on dots row (spring physics), 500ms delay after 3 failures, 5s after 5 failures
- Status icons: battery %, WiFi indicator, time (top bar, small)

---

## Lock/Unlock Flow

```
Lock triggers:
  1. Power button short press:
     slate-power → D-Bus org.slate.LockScreen.Lock() → slate-lock activates
     slate-power → writes "mem" to /sys/power/state (suspend)
     On resume: lock screen is already visible

  2. Idle timeout:
     niri detects idle (Wayland idle-inhibit protocol)
     → runs configured command: dbus-send Lock
     → slate-lock activates

Lock activation:
  → slate-lock receives Lock D-Bus method call
  → Sets layer-shell surface to visible (fullscreen, Layer::Overlay, KeyboardInteractivity::Exclusive)
  → Fetches current notifications from org.slate.Notifications.GetActive()
  → Emits Locked() signal
  → Starts clock tick subscription

Unlock:
  → User enters correct PIN/password
  → authenticate() returns AuthResult::Success
  → Hides layer-shell surface
  → Emits Unlocked() signal
  → Clears PIN buffer

Wrong PIN:
  → Shake animation on PIN dots (spring physics)
  → Increment failure counter
  → After 3 failures: 500ms delay before next attempt accepted
  → After 5 failures: 5s delay
  → After 10 failures: 30s delay
  → Counter resets on successful unlock
```

---

## D-Bus Interface

Add to `shell/slate-common/src/dbus.rs`:
```rust
pub const LOCKSCREEN_BUS_NAME: &str = "org.slate.LockScreen";
pub const LOCKSCREEN_PATH: &str = "/org/slate/LockScreen";
pub const LOCKSCREEN_INTERFACE: &str = "org.slate.LockScreen";
```

Update the `constants_are_well_formed()` test arrays in `dbus.rs` to include the new constants.

Interface definition (`shell/slate-lock/src/dbus.rs`):
```
org.slate.LockScreen at /org/slate/LockScreen:
  Methods:
    Lock()       — activate the lock screen
  Signals:
    Locked()     — emitted when lock screen activates
    Unlocked()   — emitted when user successfully authenticates
  Properties:
    IsLocked: bool (read-only)
```

No `Unlock()` method — unlocking is only via user authentication through the UI. This prevents any D-Bus caller from bypassing the lock.

---

## Settings

### Settings Model

Add to `shell/slate-common/src/settings.rs`. Wire into the top-level `Settings` struct as `pub lock: LockSettings` (alongside existing `display`, `dock`, `rhea`, `notifications` fields). Update the `default_settings_produce_valid_toml()` test to include the `[lock]` section:
```rust
pub struct LockSettings {
    /// Seconds of idle before auto-lock. 0 = never auto-lock.
    pub idle_timeout_secs: u64,
    /// Lock the screen when the device suspends.
    pub lock_on_suspend: bool,
}
```

**PIN hash storage** is NOT in `settings.toml`. The PIN hash lives in a separate file `~/.config/slate/lock.toml` with `0600` permissions (owner-only read/write). This prevents other components or processes from reading the hash. The `auth.rs` module reads/writes this file directly:

```rust
// ~/.config/slate/lock.toml
// Permissions: 0600
pub struct LockCredential {
    pub pin_hash: String,
}

impl Default for LockSettings {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300,
            lock_on_suspend: true,
            pin_hash: None,
        }
    }
}
```

### Settings Page

New page `shell/slate-settings/src/pages/security.rs`:
- **Idle timeout** — slider (1-30 minutes, or "Never"), maps to `idle_timeout_secs`
- **Lock on suspend** — toggle, maps to `lock_on_suspend`
- **Change PIN** — form: enter current PIN (if set), enter new PIN, confirm new PIN. Stores argon2 hash.
- **Remove PIN** — button (requires current PIN to confirm)

Add `Page::Security` to navigation with lock icon ("\u{1f512}"). Update `Page::all()` to include it. Update the existing `page_list_has_all_expected_pages` test assertion count (currently 10 → 11).

---

## slate-power Modification

In `shell/slate-power/src/main.rs`, before the suspend write:

```rust
// Lock screen before suspend
fn lock_screen() {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        tracing::warn!("failed to connect to session bus for lock");
        return;
    };
    let _ = conn.call_method(
        Some("org.slate.LockScreen"),
        "/org/slate/LockScreen",
        Some("org.slate.LockScreen"),
        "Lock",
        &(),
    );
    // Brief sleep to let the lock screen activate before suspend
    std::thread::sleep(std::time::Duration::from_millis(200));
}
```

Add `zbus` dependency to `shell/slate-power/Cargo.toml` (blocking feature only — no tokio required). The `lock_on_suspend` setting is read via `slate_common::settings::Settings::load()` at startup and cached. Add `slate-common` as a dependency to slate-power.

---

## niri Configuration

niri does not have a built-in idle timeout config block. Use `swayidle` — the standard Wayland idle daemon — as a separate arkhe service.

Add to `config/niri/` device configs:
```kdl
spawn-at-startup "slate-lock"
```

Create `services/base/swayidle/run`:
```sh
#!/bin/sh
exec swayidle -w \
  timeout 300 'dbus-send --session --dest=org.slate.LockScreen --type=method_call /org/slate/LockScreen org.slate.LockScreen.Lock'
```

Create `services/base/swayidle/depends`:
```
niri
```

Add `swayidle` to `build/build-rootfs.sh` package list.

The 300-second timeout is a static default. When the user changes the idle timeout in settings, slate-lock can kill and respawn the swayidle process with the new timeout value via `ark` CLI or by writing a new swayidle config.

---

## Notification Display on Lock Screen

Lock screen shows a limited notification preview:
- Fetch from `org.slate.Notifications.GetActive()` on lock activation
- Show max 3 most recent notifications (summary + app_name only — no body text for privacy). Body stripping is done client-side in slate-lock's `notifications.rs` — notifyd's `GetActive` returns full notifications, slate-lock renders only `app_name` and `summary`.
- Subscribe to `Added` signal to update while locked
- Tapping a notification does NOT open it (device is locked) — just visual awareness

---

## Layer-Shell Setup

```rust
LayerShellSettings {
    size: Some((0, 0)),  // fullscreen
    anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
    exclusive_zone: -1,  // take full screen, ignore other surfaces
    layer: Layer::Overlay,
    keyboard_interactivity: KeyboardInteractivity::Exclusive,  // capture all input
}
```

The lock screen starts hidden using the same `size: Some((0, 1))` pattern as slate-shade (height=1 pixel, effectively invisible). On `Lock` signal, it resizes to fullscreen (`(0, 0)` = compositor-determined). On unlock, it shrinks back to `(0, 1)`. The surface stays mapped to avoid re-creation overhead. `KeyboardInteractivity` is set to `Exclusive` only when locked, `None` when hidden (so it doesn't steal keyboard focus from other apps).

---

## Dependencies

### New Crate Dependencies
- `argon2` — PIN hashing
- `pam` — PAM authentication (Linux-only, optional)
- Existing workspace deps: `iced`, `iced_layershell`, `zbus`, `tokio`, `slate-common`, `tracing`, `anyhow`

### Rootfs Changes
- Add to `build/build-rootfs.sh`: `linux-pam`, `shadow` (for `/etc/shadow` access)
- Create `/etc/pam.d/slate-lock` with standard auth config

---

## File Structure

| File | Responsibility |
|------|---------------|
| `shell/slate-lock/Cargo.toml` | Crate manifest with argon2, pam (Linux-optional) |
| `shell/slate-lock/src/main.rs` | iced app entry, layer-shell, message loop, state machine |
| `shell/slate-lock/src/auth.rs` | PAM + PIN fallback authentication |
| `shell/slate-lock/src/pin_pad.rs` | PIN pad view (digit grid, dots, backspace) |
| `shell/slate-lock/src/clock.rs` | Clock/date display |
| `shell/slate-lock/src/notifications.rs` | Lock screen notification previews |
| `shell/slate-lock/src/dbus.rs` | D-Bus interface (Lock signal listener, Locked/Unlocked signals) |
| `shell/slate-common/src/dbus.rs` | Add LOCKSCREEN_BUS_NAME/PATH/INTERFACE constants |
| `shell/slate-common/src/settings.rs` | Add LockSettings struct |
| `shell/slate-power/src/main.rs` | Add dbus-send Lock call before suspend |
| `shell/slate-settings/src/pages/security.rs` | Security settings page (PIN, timeout, lock-on-suspend) |
| `shell/slate-settings/src/navigation.rs` | Add Page::Security |
| `services/base/slate-lock/run` | `exec /usr/lib/slate/slate-lock` |
| `services/base/slate-lock/depends` | `dbus`, `niri` |
| `services/base/swayidle/run` | swayidle idle timeout → Lock |
| `services/base/swayidle/depends` | `niri` |
| `shell/slate-power/Cargo.toml` | Add zbus (blocking), slate-common deps |
| `build/build-rootfs.sh` | Add linux-pam, shadow, swayidle packages |
| `config/niri/` | Add `spawn-at-startup "slate-lock"` |
