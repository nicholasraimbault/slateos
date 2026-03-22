# Lock Screen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a lock screen daemon that secures the device on idle/sleep with PIN or PAM authentication, adaptive layout, and notification previews.

**Architecture:** New iced layer-shell crate `shell/slate-lock/` that runs as a persistent hidden daemon. On receiving a `Lock` D-Bus signal (from slate-power or swayidle), it becomes a fullscreen overlay with exclusive keyboard capture. Authentication via hybrid PAM (Linux) / PIN (argon2 hash in `~/.config/slate/lock.toml`). Adaptive layout: phone (tap-to-unlock → PIN pad) vs tablet (split clock+PIN).

**Tech Stack:** Rust 2021, iced 0.13 + iced_layershell 0.7, zbus 5, argon2 (PIN hashing), pam crate (Linux-only), slate-common (physics, theme, settings, dbus), swayidle (idle detection)

**Spec:** `docs/superpowers/specs/2026-03-22-lock-screen-design.md`

---

## File Structure Overview

### New crate: shell/slate-lock/

| File | Responsibility |
|------|---------------|
| `shell/slate-lock/Cargo.toml` | Crate manifest |
| `shell/slate-lock/src/main.rs` | iced app, layer-shell setup, Message enum, state machine |
| `shell/slate-lock/src/auth.rs` | PAM + PIN fallback authentication, credential file I/O |
| `shell/slate-lock/src/pin_pad.rs` | PIN pad view (digit grid, dots, backspace, shake animation) |
| `shell/slate-lock/src/clock.rs` | Clock/date display view |
| `shell/slate-lock/src/notifications.rs` | Lock screen notification previews (max 3, privacy-filtered) |
| `shell/slate-lock/src/dbus.rs` | D-Bus listener for Lock signal, Locked/Unlocked signal emission |

### Modified files

| File | Change |
|------|--------|
| `shell/slate-common/src/dbus.rs` | Add LOCKSCREEN constants, update test arrays |
| `shell/slate-common/src/settings.rs` | Add LockSettings struct, wire into Settings |
| `shell/slate-power/src/main.rs` | Add lock_screen() call before suspend |
| `shell/slate-power/Cargo.toml` | Add zbus (blocking), slate-common deps |
| `shell/slate-settings/src/pages/security.rs` | New security settings page |
| `shell/slate-settings/src/navigation.rs` | Add Page::Security |
| `shell/slate-settings/src/main.rs` | Wire security page |
| `Cargo.toml` | Add slate-lock to workspace members |

### New service/config files

| File | Content |
|------|---------|
| `services/base/slate-lock/run` | `exec /usr/lib/slate/slate-lock` |
| `services/base/slate-lock/depends` | `dbus`, `niri` |
| `services/base/swayidle/run` | swayidle with 300s timeout → Lock |
| `services/base/swayidle/depends` | `niri` |

---

## Task 1: slate-common Foundation (D-Bus + Settings)

**Must complete before Tasks 2-5.**

**Files:**
- Modify: `shell/slate-common/src/dbus.rs`
- Modify: `shell/slate-common/src/settings.rs`

- [ ] **Step 1: Add D-Bus constants**

In `shell/slate-common/src/dbus.rs`, add after the existing SHADE constants:

```rust
pub const LOCKSCREEN_BUS_NAME: &str = "org.slate.LockScreen";
pub const LOCKSCREEN_PATH: &str = "/org/slate/LockScreen";
pub const LOCKSCREEN_INTERFACE: &str = "org.slate.LockScreen";
```

- [ ] **Step 2: Update constants_are_well_formed test**

Add `LOCKSCREEN_INTERFACE` to the interface array and `LOCKSCREEN_PATH` to the path array in the existing `constants_are_well_formed()` test. Add `LOCKSCREEN_BUS_NAME` to the bus name array if one exists.

- [ ] **Step 3: Add LockSettings to settings.rs**

After `NotificationSettings`, add:

```rust
/// Lock screen preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockSettings {
    /// Seconds of idle before auto-lock. 0 = never auto-lock.
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
    /// Lock the screen when the device suspends.
    #[serde(default = "default_true")]
    pub lock_on_suspend: bool,
}

fn default_idle_timeout() -> u64 { 300 }

impl Default for LockSettings {
    fn default() -> Self {
        Self {
            idle_timeout_secs: 300,
            lock_on_suspend: true,
        }
    }
}
```

Add `pub lock: LockSettings` to the top-level `Settings` struct.

- [ ] **Step 4: Write tests**

```rust
#[test]
fn lock_settings_default_values() {
    let s = LockSettings::default();
    assert_eq!(s.idle_timeout_secs, 300);
    assert!(s.lock_on_suspend);
}

#[test]
fn lock_settings_deserialize_without_section() {
    // Existing settings TOML without [lock] should deserialize with defaults.
    let toml_str = "";
    let s: LockSettings = toml::from_str(toml_str).unwrap_or_default();
    assert_eq!(s.idle_timeout_secs, 300);
}
```

Update the `default_settings_produce_valid_toml()` test if it checks for section names.

- [ ] **Step 5: Run tests, commit**

```bash
cargo test -p slate-common
git commit -m "feat(slate-common): add lock screen D-Bus constants and settings"
```

---

## Task 2: Authentication Module

**Depends on: Task 1. Can be parallelized with Tasks 3-4.**

**Files:**
- Create: `shell/slate-lock/Cargo.toml`
- Create: `shell/slate-lock/src/auth.rs`
- Modify: `Cargo.toml` (workspace members)

### Task 2a: Crate scaffold

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "slate-lock"
version = "0.1.0"
edition = "2021"

[dependencies]
slate-common = { path = "../slate-common" }
iced = { workspace = true }
iced_layershell = { workspace = true }
zbus = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
argon2 = "0.5"
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }

[target.'cfg(target_os = "linux")'.dependencies]
pam = "0.8"

[dev-dependencies]
tempfile = "3"
```

Add `"shell/slate-lock"` to workspace members in root `Cargo.toml`.

- [ ] **Step 2: Create minimal main.rs stub**

```rust
#![allow(dead_code)]
mod auth;
fn main() { println!("slate-lock stub"); }
```

- [ ] **Step 3: Verify compilation**

```bash
cargo check -p slate-lock
```

### Task 2b: PIN credential file

- [ ] **Step 1: Write tests for PIN hash/verify**

In `shell/slate-lock/src/auth.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_pin() {
        let hash = hash_pin("1234");
        assert!(verify_pin("1234", &hash));
        assert!(!verify_pin("5678", &hash));
    }

    #[test]
    fn hash_is_different_each_time() {
        let h1 = hash_pin("1234");
        let h2 = hash_pin("1234");
        assert_ne!(h1, h2); // argon2 uses random salt
    }

    #[test]
    fn empty_pin_hashes_and_verifies() {
        let hash = hash_pin("");
        assert!(verify_pin("", &hash));
    }

    #[test]
    fn load_credential_from_missing_file() {
        let result = load_credential(std::path::Path::new("/nonexistent/lock.toml"));
        assert!(result.is_none());
    }

    #[test]
    fn save_and_load_credential_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock.toml");
        let hash = hash_pin("9876");
        save_credential(&path, &hash).unwrap();
        let loaded = load_credential(&path).unwrap();
        assert!(verify_pin("9876", &loaded));
    }
}
```

- [ ] **Step 2: Implement PIN functions**

```rust
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use std::path::Path;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LockCredential {
    pin_hash: String,
}

pub fn hash_pin(pin: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(pin.as_bytes(), &salt)
        .expect("argon2 hash should not fail")
        .to_string()
}

pub fn verify_pin(pin: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else { return false };
    Argon2::default().verify_password(pin.as_bytes(), &parsed).is_ok()
}

pub fn load_credential(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let cred: LockCredential = toml::from_str(&content).ok()?;
    Some(cred.pin_hash)
}

pub fn save_credential(path: &Path, pin_hash: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cred = LockCredential { pin_hash: pin_hash.to_string() };
    let content = toml::to_string_pretty(&cred)?;
    std::fs::write(path, content)?;
    // Set permissions to 0600 (owner-only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
```

- [ ] **Step 3: Run tests, commit**

### Task 2c: Hybrid authenticate function

- [ ] **Step 1: Write test**

```rust
#[test]
fn authenticate_with_pin_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lock.toml");
    let hash = hash_pin("4321");
    save_credential(&path, &hash).unwrap();
    assert!(matches!(authenticate_sync("4321", "testuser", &path), AuthResult::Success));
    assert!(matches!(authenticate_sync("0000", "testuser", &path), AuthResult::WrongCredential));
}

#[test]
fn authenticate_not_configured() {
    let path = std::path::Path::new("/nonexistent/lock.toml");
    assert!(matches!(authenticate_sync("1234", "testuser", path), AuthResult::NotConfigured));
}
```

- [ ] **Step 2: Implement authenticate**

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum AuthResult {
    Success,
    WrongCredential,
    NotConfigured,
}

/// Authenticate on a background thread (safe to call from iced Task::perform).
/// Wraps the blocking PAM call so it doesn't block the UI thread.
pub fn authenticate_in_thread(pin: &str, username: &str, credential_path: &Path) -> AuthResult {
    // This runs on a tokio blocking thread via Task::perform,
    // so blocking PAM calls are safe here.
    authenticate_sync(pin, username, credential_path)
}

/// Synchronous authenticate — safe to call from any thread context.
pub fn authenticate_sync(pin: &str, username: &str, credential_path: &Path) -> AuthResult {
    // Try PAM first (Linux only)
    #[cfg(target_os = "linux")]
    {
        if let Some(result) = try_pam(pin, username) {
            return result;
        }
    }

    // Fall back to PIN file
    match load_credential(credential_path) {
        Some(hash) => {
            if verify_pin(pin, &hash) {
                AuthResult::Success
            } else {
                AuthResult::WrongCredential
            }
        }
        None => AuthResult::NotConfigured,
    }
}

#[cfg(target_os = "linux")]
fn try_pam(password: &str, username: &str) -> Option<AuthResult> {
    use pam::Authenticator;
    let mut auth = Authenticator::with_password("slate-lock").ok()?;
    auth.get_handler().set_credentials(username, password);
    match auth.authenticate() {
        Ok(()) => Some(AuthResult::Success),
        Err(_) => Some(AuthResult::WrongCredential),
    }
}
```

- [ ] **Step 3: Run tests, commit**

```bash
git commit -m "feat(slate-lock): add hybrid PAM/PIN authentication module"
```

---

## Task 3: PIN Pad and Clock Views

**Depends on: Task 2a (crate exists). Can be parallelized with Task 2b-c.**

**Files:**
- Create: `shell/slate-lock/src/pin_pad.rs`
- Create: `shell/slate-lock/src/clock.rs`

### Task 3a: PIN pad view

- [ ] **Step 1: Define PinPadState and PinPadAction**

```rust
use iced::widget::{button, column, container, row, text};
use iced::{Element, Length};
use slate_common::Palette;

pub const MAX_PIN_LENGTH: usize = 8;

#[derive(Debug, Clone)]
pub enum PinPadAction {
    Digit(char),
    Backspace,
}

pub struct PinPadState {
    pub entered: String,
    pub max_length: usize,
    pub shake_offset: f32,  // for wrong-PIN animation
    pub error_message: Option<String>,
}

impl Default for PinPadState {
    fn default() -> Self {
        Self {
            entered: String::new(),
            max_length: MAX_PIN_LENGTH,
            shake_offset: 0.0,
            error_message: None,
        }
    }
}

impl PinPadState {
    pub fn push_digit(&mut self, d: char) {
        if self.entered.len() < self.max_length && d.is_ascii_digit() {
            self.entered.push(d);
        }
    }
    pub fn backspace(&mut self) { self.entered.pop(); }
    pub fn clear(&mut self) {
        self.entered.clear();
        self.error_message = None;
    }
    pub fn set_error(&mut self, msg: &str) {
        self.entered.clear();
        self.error_message = Some(msg.to_string());
    }
}
```

- [ ] **Step 2: Implement PIN pad view function**

Render a 3x3+1 grid of circular digit buttons (1-9, then 0 centered with backspace). PIN dots row above. Use palette colors for theming.

- [ ] **Step 3: Write tests for PinPadState**

```rust
#[test]
fn push_digit_appends() {
    let mut s = PinPadState::default();
    s.push_digit('1');
    s.push_digit('2');
    assert_eq!(s.entered, "12");
}

#[test]
fn push_digit_respects_max_length() {
    let mut s = PinPadState::default();
    for c in "12345678".chars() { s.push_digit(c); }
    s.push_digit('9');
    assert_eq!(s.entered.len(), 8); // MAX_PIN_LENGTH
}

#[test]
fn backspace_removes_last() {
    let mut s = PinPadState::default();
    s.push_digit('1');
    s.push_digit('2');
    s.backspace();
    assert_eq!(s.entered, "1");
}

#[test]
fn clear_resets_state() {
    let mut s = PinPadState::default();
    s.push_digit('1');
    s.set_error("wrong");
    s.clear();
    assert!(s.entered.is_empty());
    assert!(s.error_message.is_none());
}
```

- [ ] **Step 4: Run tests, commit**

### Task 3b: Clock view

- [ ] **Step 1: Implement clock view**

In `shell/slate-lock/src/clock.rs`:

```rust
use chrono::Local;
use iced::widget::{column, text};
use iced::{Element, Length};

pub fn view_clock<'a, M: 'a>(large: bool) -> Element<'a, M> {
    let now = Local::now();
    let time_str = now.format("%H:%M").to_string();
    let date_str = now.format("%A, %B %-d").to_string();

    let time_size = if large { 72 } else { 36 };
    let date_size = if large { 16 } else { 12 };

    column![
        text(time_str).size(time_size),
        text(date_str).size(date_size),
    ]
    .align_x(iced::Alignment::Center)
    .spacing(4)
    .into()
}
```

- [ ] **Step 2: Write test**

```rust
#[test]
fn clock_format_is_24h() {
    let now = chrono::Local::now();
    let formatted = now.format("%H:%M").to_string();
    assert_eq!(formatted.len(), 5); // "HH:MM"
}
```

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(slate-lock): add PIN pad and clock views"
```

---

## Task 4: Lock Screen Notifications

**Depends on: Task 2a.**

**Files:**
- Create: `shell/slate-lock/src/notifications.rs`

- [ ] **Step 1: Implement notification preview**

```rust
use iced::widget::{column, container, row, text};
use iced::{Element, Length};
use slate_common::notifications::Notification;

const MAX_LOCK_NOTIFICATIONS: usize = 3;

pub fn view_lock_notifications<'a, M: 'a>(
    notifications: &'a [Notification],
) -> Element<'a, M> {
    let items: Vec<Element<'a, M>> = notifications
        .iter()
        .take(MAX_LOCK_NOTIFICATIONS)
        .map(|n| {
            // Privacy: show app_name + summary only, no body
            container(
                row![
                    text(&n.app_name).size(12),
                    text(" — ").size(12),
                    text(&n.summary).size(12),
                ]
                .spacing(4)
            )
            .padding(8)
            .into()
        })
        .collect();

    column(items).spacing(4).width(Length::Fill).into()
}
```

- [ ] **Step 2: Write test**

```rust
#[test]
fn lock_notifications_limited_to_max() {
    let notifs: Vec<Notification> = (0..5)
        .map(|i| Notification::new(i, &format!("app{i}"), &format!("summary{i}"), "secret body"))
        .collect();
    // view_lock_notifications takes max 3
    assert!(notifs.len() > MAX_LOCK_NOTIFICATIONS);
}
```

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(slate-lock): add lock screen notification previews"
```

---

## Task 5: D-Bus Listener

**Depends on: Task 1.**

**Files:**
- Create: `shell/slate-lock/src/dbus.rs`

- [ ] **Step 1: Implement D-Bus listener**

Follow the `iced::stream::channel` pattern from claw-panel/slate-shade:

```rust
pub enum LockDbusEvent {
    LockRequested,
    NotificationAdded(Notification),
    PaletteChanged(Palette),
}

pub fn dbus_subscription() -> iced::Subscription<LockDbusEvent> {
    iced::stream::channel(
        std::any::TypeId::of::<LockDbusEvent>(),
        32,
        |sender| async move {
            #[cfg(target_os = "linux")]
            { run(sender).await; }
            #[cfg(not(target_os = "linux"))]
            { futures_util::future::pending::<()>().await; }
        },
    )
}
```

**Server side (zbus::interface):** Register the `org.slate.LockScreen` D-Bus interface with a `Lock()` method handler and `IsLocked` read-only property. The `Lock()` method is what slate-power and swayidle CALL — slate-lock SERVES it. Use `#[zbus::interface]` macro on a struct:

```rust
struct LockScreenService {
    sender: tokio::sync::mpsc::UnboundedSender<LockDbusEvent>,
    is_locked: bool,
}

#[zbus::interface(name = "org.slate.LockScreen")]
impl LockScreenService {
    async fn lock(&mut self) {
        let _ = self.sender.send(LockDbusEvent::LockRequested);
        self.is_locked = true;
    }

    #[zbus(property)]
    fn is_locked(&self) -> bool {
        self.is_locked
    }

    #[zbus(signal)]
    async fn locked(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn unlocked(emitter: &zbus::object_server::SignalEmitter<'_>) -> zbus::Result<()>;
}
```

Request bus name `org.slate.LockScreen` and register at path `/org/slate/LockScreen`.

**Client side (signal subscriptions):** Also subscribe to:
- `org.slate.Notifications.Added` signal (update preview while locked)
- `org.slate.Palette.Changed` signal (theme updates)

- [ ] **Step 2: Write tests**

```rust
#[test]
fn lock_dbus_event_variants_constructible() {
    let _ = LockDbusEvent::LockRequested;
}
```

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(slate-lock): add D-Bus listener for Lock signal"
```

---

## Task 6: Main App — State Machine and Wiring

**Depends on: Tasks 2-5.**

**Files:**
- Modify: `shell/slate-lock/src/main.rs`

- [ ] **Step 1: Define state and messages**

```rust
enum LockState {
    Hidden,         // Daemon running, surface invisible
    ShowingClock,   // Phone only: clock + notifs, tap to unlock
    ShowingPinPad,  // PIN pad visible (always on tablet, after tap on phone)
    Authenticating, // Waiting for auth result
    Cooldown(Instant), // Rate-limited after failures
}

enum Message {
    DbusEvent(LockDbusEvent),
    Digit(char),
    Backspace,
    TapToUnlock,  // Phone: transition from clock to PIN pad
    AuthResult(AuthResult),
    ClockTick,
    ShakeStep,    // Spring animation tick for wrong-PIN shake
    LayoutDetected(LayoutMode),
}
```

- [ ] **Step 2: Implement iced Application**

Follow the slate-shade pattern:
- `#[cfg(target_os = "linux")]` layer-shell with `Layer::Overlay`
- macOS fallback as windowed app
- Start with `size: Some((0, 1))` (hidden)
- On `LockRequested`: resize to fullscreen, set `KeyboardInteractivity::Exclusive`
- On successful auth: resize to `(0, 1)`, set `KeyboardInteractivity::None`
- Subscriptions: D-Bus, clock tick (1s interval when locked), shake animation tick

- [ ] **Step 3: Wire update logic**

```rust
fn update(&mut self, message: Message) -> Task<Message> {
    match message {
        Message::DbusEvent(LockDbusEvent::LockRequested) => {
            self.state = LockState::ShowingClock; // or ShowingPinPad for tablet
            // Fetch notifications, resize surface
        }
        Message::TapToUnlock => {
            self.state = LockState::ShowingPinPad;
        }
        Message::Digit(d) => {
            self.pin_pad.push_digit(d);
            if self.pin_pad.entered.len() >= 4 {
                // Auto-submit when PIN reaches minimum length
                self.state = LockState::Authenticating;
                let pin = self.pin_pad.entered.clone();
                return Task::perform(
                    async move { authenticate_in_thread(&pin) },
                    Message::AuthResult,
                );
            }
        }
        Message::AuthResult(AuthResult::Success) => {
            self.pin_pad.clear();
            self.state = LockState::Hidden;
            self.failure_count = 0;
            // Resize surface to hidden, emit Unlocked signal
        }
        Message::AuthResult(AuthResult::WrongCredential) => {
            self.failure_count += 1;
            self.pin_pad.set_error("Wrong PIN");
            // Start shake animation on PIN dots
            // Apply rate-limiting cooldown based on failure count:
            let cooldown = match self.failure_count {
                0..=2 => Duration::ZERO,
                3..=4 => Duration::from_millis(500),
                5..=9 => Duration::from_secs(5),
                _ => Duration::from_secs(30),
            };
            if !cooldown.is_zero() {
                self.state = LockState::Cooldown(Instant::now() + cooldown);
            }
        }
        // ...
    }
}
```

- [ ] **Step 4: Implement adaptive view**

Phone layout: two-state (clock → PIN pad with spring transition).
Tablet layout: split view (clock left, PIN pad right).

- [ ] **Step 5: Run all tests**

```bash
cargo test -p slate-lock
cargo clippy -p slate-lock -- -D warnings
cargo fmt -p slate-lock --check
cargo check --workspace
```

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(slate-lock): complete lock screen app with adaptive layout"
```

---

## Task 7: slate-power Lock-Before-Suspend

**Independent — can be done in parallel with Task 6.**

**Files:**
- Modify: `shell/slate-power/src/main.rs`
- Modify: `shell/slate-power/Cargo.toml`

- [ ] **Step 1: Add dependencies**

In `shell/slate-power/Cargo.toml`, add:
```toml
zbus = { workspace = true }
slate-common = { path = "../slate-common" }
toml = { workspace = true }
serde = { workspace = true }
```

- [ ] **Step 2: Add lock_screen function**

In `shell/slate-power/src/main.rs`:

```rust
/// Send a Lock signal to the lock screen daemon before suspending.
fn lock_screen() {
    let Ok(conn) = zbus::blocking::Connection::session() else {
        eprintln!("[slate-power] failed to connect to session bus for lock");
        return;
    };
    match conn.call_method(
        Some(slate_common::dbus::LOCKSCREEN_BUS_NAME),
        slate_common::dbus::LOCKSCREEN_PATH,
        Some(slate_common::dbus::LOCKSCREEN_INTERFACE),
        "Lock",
        &(),
    ) {
        Ok(_) => eprintln!("[slate-power] lock screen activated"),
        Err(e) => eprintln!("[slate-power] lock signal failed: {e}"),
    }
    // Brief delay for lock screen to render before suspend
    std::thread::sleep(Duration::from_millis(200));
}
```

- [ ] **Step 3: Wire into suspend path**

Modify the `suspend()` function:

```rust
fn suspend(lock_on_suspend: bool) {
    if lock_on_suspend {
        lock_screen();
    }
    eprintln!("[slate-power] suspending");
    match std::fs::write(POWER_STATE_PATH, "mem") {
        Ok(()) => eprintln!("[slate-power] resumed from suspend"),
        Err(e) => eprintln!("[slate-power] suspend failed: {e}"),
    }
}
```

Load `lock_on_suspend` from settings at startup:

```rust
fn load_lock_on_suspend() -> bool {
    let Ok(home) = std::env::var("HOME") else { return true };
    let path = std::path::Path::new(&home).join(".config/slate/settings.toml");
    let Ok(content) = std::fs::read_to_string(path) else { return true };
    let Ok(settings) = toml::from_str::<slate_common::settings::Settings>(&content) else { return true };
    settings.lock.lock_on_suspend
}
```

- [ ] **Step 4: Update main() and run() to load setting and pass to suspend**

In `main()`, call `load_lock_on_suspend()` once at startup and store the result. In `run()`, update the call site at line 159 from `suspend()` to `suspend(lock_on_suspend)`. The `lock_on_suspend` bool must be threaded through `run()` as a parameter.

- [ ] **Step 5: Write test**

```rust
#[test]
fn load_lock_on_suspend_defaults_true_when_no_config() {
    // With a non-existent HOME, should default to true
    std::env::set_var("HOME", "/nonexistent");
    assert!(load_lock_on_suspend());
}
```

- [ ] **Step 6: Run tests, commit**

```bash
cargo test -p slate-power
git commit -m "feat(slate-power): lock screen before suspend"
```

---

## Task 8: Settings Page and Service Files

**Depends on: Task 1 (settings model).**

**Files:**
- Create: `shell/slate-settings/src/pages/security.rs`
- Modify: `shell/slate-settings/src/navigation.rs`
- Modify: `shell/slate-settings/src/main.rs`
- Create: `services/base/slate-lock/run`, `services/base/slate-lock/depends`
- Create: `services/base/swayidle/run`, `services/base/swayidle/depends`

### Task 8a: Security settings page

- [ ] **Step 1: Create security.rs**

```rust
use iced::widget::{button, column, container, row, slider, text, text_input, toggler};
use iced::Element;
use slate_common::settings::LockSettings;

#[derive(Debug, Clone)]
pub enum SecurityMsg {
    IdleTimeoutChanged(f32),
    LockOnSuspendToggled(bool),
    CurrentPinInput(String),
    NewPinInput(String),
    ConfirmPinInput(String),
    ChangePin,
    RemovePin,
    PinResult(Result<String, String>),
}

pub fn update(settings: &mut LockSettings, msg: SecurityMsg) -> Option<iced::Task<SecurityMsg>> {
    match msg {
        SecurityMsg::IdleTimeoutChanged(v) => {
            settings.idle_timeout_secs = (v * 60.0) as u64; // slider is in minutes
            None
        }
        SecurityMsg::LockOnSuspendToggled(v) => {
            settings.lock_on_suspend = v;
            None
        }
        SecurityMsg::CurrentPinInput(v) => { /* store in page state, not settings */ None }
        SecurityMsg::NewPinInput(v) => { None }
        SecurityMsg::ConfirmPinInput(v) => { None }
        SecurityMsg::ChangePin => {
            // Validate: new_pin == confirm_pin, length 4-8, current PIN correct
            // Hash with auth::hash_pin, save with auth::save_credential
            // Return Task::perform for async file write
            None
        }
        SecurityMsg::RemovePin => {
            // Validate current PIN, then delete lock.toml
            None
        }
        SecurityMsg::PinResult(_) => { None }
    }
}

pub struct SecurityPageState {
    pub current_pin: String,
    pub new_pin: String,
    pub confirm_pin: String,
    pub status_message: Option<String>,
}

pub fn view<'a>(
    settings: &'a LockSettings,
    page_state: &'a SecurityPageState,
) -> Element<'a, SecurityMsg> {
    let timeout_minutes = settings.idle_timeout_secs as f32 / 60.0;
    let mut items: Vec<Element<'a, SecurityMsg>> = vec![
        text("Security").size(24).into(),
        toggler(settings.lock_on_suspend)
            .label("Lock on suspend")
            .on_toggle(SecurityMsg::LockOnSuspendToggled)
            .into(),
        text(format!("Auto-lock after {} minutes", timeout_minutes as u32)).size(14).into(),
        slider(1.0..=30.0, timeout_minutes, SecurityMsg::IdleTimeoutChanged).into(),
        text("Change PIN").size(18).into(),
        text_input("Current PIN", &page_state.current_pin)
            .on_input(SecurityMsg::CurrentPinInput)
            .secure(true)
            .padding(8)
            .into(),
        text_input("New PIN (4-8 digits)", &page_state.new_pin)
            .on_input(SecurityMsg::NewPinInput)
            .secure(true)
            .padding(8)
            .into(),
        text_input("Confirm PIN", &page_state.confirm_pin)
            .on_input(SecurityMsg::ConfirmPinInput)
            .secure(true)
            .padding(8)
            .into(),
        button(text("Set PIN")).on_press(SecurityMsg::ChangePin).into(),
        button(text("Remove PIN")).on_press(SecurityMsg::RemovePin).into(),
    ];
    if let Some(msg) = &page_state.status_message {
        items.push(text(msg).size(12).into());
    }
    column(items).spacing(12).padding(20).into()
}
```

- [ ] **Step 2: Add to navigation**

In `navigation.rs`, add `Page::Security` between `Network` and `About`:
- `Page::Security` variant
- `Page::all()` updated (11 entries)
- `Page::label()`: `"Security"`
- `Page::icon()`: `"\u{1f512}"`

Update test: `page_list_has_all_expected_pages` asserts `len() == 11`.

- [ ] **Step 3: Wire into main.rs**

Add `Message::Security(SecurityMsg)` variant, handle in update, render in view.

- [ ] **Step 4: Run tests, commit**

### Task 8b: Service files

- [ ] **Step 1: Create slate-lock service**

```bash
mkdir -p services/base/slate-lock
```

`services/base/slate-lock/run`:
```sh
#!/bin/sh
exec /usr/lib/slate/slate-lock
```

`services/base/slate-lock/depends`:
```
dbus
niri
```

- [ ] **Step 2: Create swayidle service**

```bash
mkdir -p services/base/swayidle
```

`services/base/swayidle/run`:
```sh
#!/bin/sh
exec swayidle -w \
  timeout 300 'dbus-send --session --dest=org.slate.LockScreen --type=method_call /org/slate/LockScreen org.slate.LockScreen.Lock'
```

`services/base/swayidle/depends`:
```
niri
```

### Task 8c: Build and niri config changes

- [ ] **Step 1: Update build-rootfs.sh**

Add `linux-pam`, `shadow`, and `swayidle` to the package install line in `build/build-rootfs.sh`.

- [ ] **Step 2: Add spawn-at-startup to niri configs**

In each device niri config under `config/niri/devices/`, add:
```kdl
spawn-at-startup "slate-lock"
```

This ensures slate-lock starts when the compositor starts, so it's ready to receive Lock signals.

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(slate-lock): add arkhe service configs, security settings, and build changes"
```

---

## Task 9: Final Verification

- [ ] **Step 1: Run full workspace verification**

```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo fmt --check
```

- [ ] **Step 2: Verify all files under 500 lines**

- [ ] **Step 3: Verify workspace compiles**

- [ ] **Step 4: Commit any fixes**

---

## Parallelization Guide

```
Task 1 (slate-common) ──→ Task 2 (auth) ──→ Task 6 (main app) ──→ Task 9
                     ├──→ Task 3 (views)  ──┘
                     ├──→ Task 4 (notifs) ──┘
                     ├──→ Task 5 (dbus)   ──┘
                     ├──→ Task 7 (power)
                     └──→ Task 8 (settings + services)
```

**Parallel batch 1:** Task 1 alone (foundation)
**Parallel batch 2:** Tasks 2, 3, 4, 5, 7, 8 (all depend only on Task 1)
**Sequential:** Task 6 (wires everything together), then Task 9 (verification)
