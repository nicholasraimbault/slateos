# Rhea AI Engine + Notification System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build three new crates (rhea, slate-notifyd, slate-shade) and supporting changes to deliver an AI-integrated notification system for SlateOS.

**Architecture:** Bottom-up build. Task 1 (slate-common foundation) must complete first. Then Tasks 2 (slate-notifyd), 3 (rhea), and 4 (touchflow) can be parallelized — they depend on slate-common but not each other. Task 5 (slate-shade) depends on all of 1-4. Task 6 (claw-panel migration) depends only on Task 3.

**Tech Stack:** Rust 2021, iced 0.13 + iced_layershell 0.7, zbus 5, tokio, serde + toml, thiserror (lib) / anyhow (bin), tracing, uuid, chrono

**Spec:** `docs/superpowers/specs/2026-03-22-rhea-and-notifications-design.md`

---

## File Structure Overview

### New files in slate-common

| File | Responsibility |
|------|---------------|
| `shell/slate-common/src/ai.rs` | `AiBackend` trait, request/response types, `Intent`, `Classification`, `ChatMessage` |
| `shell/slate-common/src/system.rs` | D-Bus wrappers for system services (WiFi, Bluetooth, Audio, Display, Power, Connectivity) |
| `shell/slate-common/src/physics.rs` | `Spring` struct, `SpringConfig` presets, `MomentumTracker` (moved from touchflow) |
| `shell/slate-common/src/notifications.rs` | `Notification` struct, `Urgency`, `NotificationAction` — shared types for notifyd/shade |

### New crate: shell/rhea/

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point, tokio runtime, arkhe service setup |
| `src/dbus.rs` | `org.slate.Rhea` zbus interface implementation |
| `src/router.rs` | Route AI requests to configured backend |
| `src/config.rs` | `RheaConfig` deserialization from settings.toml `[rhea]` section |
| `src/local.rs` | llama.cpp subprocess manager (load/unload/query via HTTP to llama-server) |
| `src/cloud.rs` | OpenAI-compatible HTTP backend (works with Claude API, OpenAI, Ollama, and OpenClaw if exposed via OpenAI-compat endpoint) |
| `src/context.rs` | Context aggregation — queries niri IPC for focused window, reads clipboard, fetches recent notifications from slate-notifyd |

### New crate: shell/slate-notifyd/

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point, tokio runtime |
| `src/dbus.rs` | `org.freedesktop.Notifications` + `org.slate.Notifications` zbus interfaces |
| `src/store.rs` | In-memory notification store + TOML persistence |
| `src/grouping.rs` | Group notifications by app_name / group_key |
| `src/history.rs` | Daily history file management |

### New crate: shell/slate-shade/

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point, layer-shell setup, iced application |
| `src/shade.rs` | Main shade panel view (pull-down) |
| `src/heads_up.rs` | Heads-up banner view |
| `src/notifications.rs` | Notification group and item views |
| `src/quick_settings.rs` | QS tile grid, sliders, tile views |
| `src/layout.rs` | Phone vs tablet/desktop layout selection |
| `src/dbus_listener.rs` | Subscribe to notifyd + Rhea D-Bus signals |

### Modified files

| File | Change |
|------|--------|
| `Cargo.toml` | Add rhea, slate-notifyd, slate-shade to workspace members |
| `shell/slate-common/Cargo.toml` | Add uuid, chrono, async-trait dependencies |
| `shell/slate-common/src/lib.rs` | Add `pub mod ai; pub mod system; pub mod physics; pub mod notifications;` |
| `shell/slate-common/src/settings.rs` | Replace `AiSettings` with `RheaSettings` |
| `shell/slate-common/src/dbus.rs` | Add Rhea, Notifications, Shade D-Bus constants |
| `shell/touchflow/src/edge.rs` | Add continuous edge gesture emission |
| `shell/touchflow/src/physics.rs` | Replace with re-export from slate-common::physics |
| `shell/claw-panel/src/main.rs` | Replace OpenClaw WebSocket with Rhea D-Bus |
| `shell/claw-panel/src/openclaw.rs` | Remove (replaced by Rhea) |

---

## Task 1: slate-common Foundation Modules

**Must complete before Tasks 2, 3, 4 can start.**

**Files:**
- Create: `shell/slate-common/src/ai.rs`
- Create: `shell/slate-common/src/physics.rs`
- Create: `shell/slate-common/src/notifications.rs`
- Create: `shell/slate-common/src/system.rs`
- Modify: `shell/slate-common/src/lib.rs`
- Modify: `shell/slate-common/src/dbus.rs`
- Modify: `shell/slate-common/src/settings.rs`
- Modify: `shell/slate-common/Cargo.toml`

### Task 1a: AI trait and types

- [ ] **Step 1: Add dependencies to slate-common Cargo.toml**

Add `async-trait`, `uuid`, and `chrono` to `shell/slate-common/Cargo.toml`:

```toml
async-trait = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 2: Write tests for AI types**

Create `shell/slate-common/src/ai.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_request_defaults() {
        let req = CompletionRequest::new("hello".to_string());
        assert_eq!(req.prompt, "hello");
        assert!(req.system.is_none());
        assert!(req.context.is_none());
        assert_eq!(req.max_tokens, 256);
        assert!((req.temperature - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn chat_message_serialization() {
        let msg = ChatMessage { role: "user".to_string(), content: "hi".to_string() };
        assert_eq!(msg.role, "user");
    }

    #[test]
    fn intent_variants() {
        let intent = Intent::Unknown;
        assert!(matches!(intent, Intent::Unknown));
        let intent = Intent::AppLaunch("firefox".to_string());
        assert!(matches!(intent, Intent::AppLaunch(_)));
    }

    #[test]
    fn classification_confidence_bounds() {
        let c = Classification { category: "greeting".to_string(), confidence: 0.95 };
        assert!(c.confidence >= 0.0 && c.confidence <= 1.0);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p slate-common -- ai::tests`
Expected: FAIL — types not defined yet.

- [ ] **Step 4: Implement AI types and trait**

Write the full `ai.rs` with `AiBackend` trait, `CompletionRequest`, `CompletionResponse`, `AiContext`, `ChatMessage`, `Intent`, `SystemAction`, `Classification`. See spec Part 1 for all type definitions.

Key implementation detail: `CompletionRequest::new(prompt)` should set sensible defaults (`max_tokens: 256`, `temperature: 0.7`, everything else `None`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p slate-common -- ai::tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add shell/slate-common/src/ai.rs shell/slate-common/Cargo.toml
git commit -m "feat(slate-common): add AI backend trait and types"
```

### Task 1b: Shared spring physics

- [ ] **Step 1: Write tests for physics module**

Create `shell/slate-common/src/physics.rs`. Port and extend the tests from `shell/touchflow/src/physics.rs`. Add tests for `SpringConfig` presets and the `spring_step` convenience function.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_config_responsive_is_stiff() {
        assert!(SpringConfig::RESPONSIVE.stiffness > SpringConfig::GENTLE.stiffness);
    }

    #[test]
    fn spring_convergence_from_displacement() {
        let s = Spring::new(300.0, 15.0);
        let dt = 1.0 / 60.0;
        let mut pos = 100.0;
        let mut vel = 0.0;
        for _ in 0..600 {
            let (p, v) = s.step(pos, vel, dt);
            pos = p;
            vel = v;
        }
        assert!(s.is_settled(pos, vel, 0.1));
    }

    // ... port all existing touchflow physics tests
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p slate-common -- physics::tests`
Expected: FAIL

- [ ] **Step 3: Implement physics module**

Move `Spring`, `MomentumTracker`, `decelerate` from touchflow into `slate-common::physics`. Keep `f64` to match touchflow's existing API. `SpringConfig` is just named `Spring` constants — not a separate type:

```rust
impl Spring {
    pub const RESPONSIVE: Self = Self { stiffness: 300.0, damping: 25.0 };
    pub const GENTLE: Self = Self { stiffness: 150.0, damping: 20.0 };
    pub const SNAPPY: Self = Self { stiffness: 400.0, damping: 30.0 };
}
```

This keeps the existing `Spring` API intact while adding named presets. Rename references to `SpringConfig` in the spec to just `Spring`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p slate-common -- physics::tests`
Expected: PASS

- [ ] **Step 5: Update touchflow to re-export from slate-common**

Replace the contents of `shell/touchflow/src/physics.rs` with:
```rust
pub use slate_common::physics::*;
```

Add `slate-common` dependency to touchflow's `Cargo.toml` if not already present.

- [ ] **Step 6: Verify touchflow tests still pass**

Run: `cargo test -p touchflow -- physics::tests`
Expected: PASS (same tests, now exercising slate-common code)

- [ ] **Step 7: Commit**

```bash
git add shell/slate-common/src/physics.rs shell/touchflow/src/physics.rs shell/touchflow/Cargo.toml
git commit -m "feat(slate-common): move spring physics to shared module"
```

### Task 1c: Shared notification types

- [ ] **Step 1: Write tests for notification types**

Create `shell/slate-common/src/notifications.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_new_has_uuid() {
        let n = Notification::new("firefox", "Download complete", "file.zip downloaded");
        assert!(!n.uuid.is_nil());
        assert_eq!(n.app_name, "firefox");
        assert!(!n.read);
        assert!(!n.heads_up);
    }

    #[test]
    fn urgency_default_is_normal() {
        assert_eq!(Urgency::default(), Urgency::Normal);
    }

    #[test]
    fn notification_action_stores_key_and_label() {
        let a = NotificationAction { key: "reply".to_string(), label: "Reply".to_string() };
        assert_eq!(a.key, "reply");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p slate-common -- notifications::tests`
Expected: FAIL

- [ ] **Step 3: Implement notification types**

Write `Notification`, `NotificationAction`, `Urgency` structs with `Serialize`/`Deserialize`. `Notification::new()` auto-generates UUID and timestamp. See spec Part 2 for all fields.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p slate-common -- notifications::tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add shell/slate-common/src/notifications.rs
git commit -m "feat(slate-common): add shared notification types"
```

### Task 1d: D-Bus constants and settings migration

- [ ] **Step 1: Add D-Bus constants for new services**

Add to `shell/slate-common/src/dbus.rs`:

```rust
// Rhea AI engine
pub const RHEA_INTERFACE: &str = "org.slate.Rhea";
pub const RHEA_PATH: &str = "/org/slate/Rhea";
pub const RHEA_BUS_NAME: &str = "org.slate.Rhea";

// Notification daemon
pub const NOTIFICATIONS_INTERFACE: &str = "org.slate.Notifications";
pub const NOTIFICATIONS_PATH: &str = "/org/slate/Notifications";
pub const NOTIFICATIONS_BUS_NAME: &str = "org.slate.Notifications";

// Notification shade
pub const SHADE_INTERFACE: &str = "org.slate.Shade";
pub const SHADE_PATH: &str = "/org/slate/Shade";
```

Update the existing test arrays to include the new constants.

- [ ] **Step 2: Replace AiSettings with RheaSettings in settings.rs**

Replace the existing `AiSettings` struct and its `[ai]` section with `RheaSettings` and `[rhea]` section. Add nested config structs for local/cloud backends. See spec Part 1 "Backend Configuration" for the TOML schema.

Keep the `ai` field in `Settings` struct but rename it to `rhea` with type `RheaSettings`. Update all tests.

- [ ] **Step 3: Run all slate-common tests**

Run: `cargo test -p slate-common`
Expected: PASS

- [ ] **Step 4: Update lib.rs exports**

Add to `shell/slate-common/src/lib.rs`:
```rust
pub mod ai;
pub mod physics;
pub mod notifications;
pub mod system;
```

- [ ] **Step 5: Add NotificationSettings to settings schema**

Add to `Settings` struct in `settings.rs`:
```rust
pub notifications: NotificationSettings,
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationSettings {
    pub dnd: bool,
    pub heads_up_duration_secs: u32,
}
impl Default for NotificationSettings {
    fn default() -> Self { Self { dnd: false, heads_up_duration_secs: 5 } }
}
```

- [ ] **Step 6: Create stub system.rs**

Create `shell/slate-common/src/system.rs` with empty structs and stub implementations that return `Ok(())` or `Ok(false)`. The real D-Bus wrappers need a live system bus — stubs allow compilation and placeholder tests. **Quick settings tiles will be non-functional until a future plan implements real system wrappers against actual hardware.** The auto-hide logic will work (returns "hardware absent" by default), but toggling WiFi/Bluetooth/etc. will be no-ops.

- [ ] **Step 6: Run workspace check**

Run: `cargo check --workspace`
Expected: PASS (existing crates may need minor fixes for settings rename)

- [ ] **Step 7: Fix any compile errors in other crates from settings rename**

The `ai` → `rhea` rename in `Settings` may break `shell/slate-settings/src/pages/ai.rs` and `shell/claw-panel`. Fix references.

- [ ] **Step 8: Run full workspace tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add shell/slate-common/ shell/slate-settings/ shell/claw-panel/
git commit -m "feat(slate-common): add D-Bus constants, rhea settings, system stubs"
```

---

## Task 2: slate-notifyd — Notification Daemon

**Depends on: Task 1 (slate-common). Can be parallelized with Tasks 3 and 4.**

**Files:**
- Create: `shell/slate-notifyd/Cargo.toml`
- Create: `shell/slate-notifyd/src/main.rs`
- Create: `shell/slate-notifyd/src/dbus.rs`
- Create: `shell/slate-notifyd/src/store.rs`
- Create: `shell/slate-notifyd/src/grouping.rs`
- Create: `shell/slate-notifyd/src/history.rs`
- Modify: `Cargo.toml` (workspace members)

### Task 2a: Crate scaffolding and store

- [ ] **Step 1: Create crate directory and Cargo.toml**

```bash
mkdir -p shell/slate-notifyd/src
```

Create `shell/slate-notifyd/Cargo.toml`:
```toml
[package]
name = "slate-notifyd"
version = "0.1.0"
edition = "2021"

[dependencies]
slate-common = { path = "../slate-common" }
zbus = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
toml = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
tempfile = "3"
```

Add `"shell/slate-notifyd"` to workspace members in root `Cargo.toml`.

- [ ] **Step 2: Write tests for notification store**

Create `shell/slate-notifyd/src/store.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_notification_assigns_fd_id() {
        let mut store = NotificationStore::new();
        let n = store.add("firefox", "Download done", "file.zip");
        assert_eq!(n.fd_id, 1);
        let n2 = store.add("firefox", "Another", "");
        assert_eq!(n2.fd_id, 2);
    }

    #[test]
    fn get_active_returns_all() {
        let mut store = NotificationStore::new();
        store.add("firefox", "A", "");
        store.add("signal", "B", "");
        assert_eq!(store.get_active().len(), 2);
    }

    #[test]
    fn dismiss_removes_notification() {
        let mut store = NotificationStore::new();
        let n = store.add("firefox", "A", "");
        let uuid = n.uuid;
        store.dismiss(&uuid);
        assert_eq!(store.get_active().len(), 0);
    }

    #[test]
    fn dismiss_returns_dismissed_notification() {
        let mut store = NotificationStore::new();
        let n = store.add("firefox", "A", "");
        let uuid = n.uuid;
        let dismissed = store.dismiss(&uuid);
        assert!(dismissed.is_some());
        assert_eq!(dismissed.unwrap().app_name, "firefox");
    }

    #[test]
    fn dismiss_all_clears_non_persistent() {
        let mut store = NotificationStore::new();
        store.add("firefox", "A", "");
        let mut n = store.add("music", "Now Playing", "");
        n.persistent = true;
        store.update(n);
        let dismissed = store.dismiss_all();
        assert_eq!(dismissed.len(), 1);
        assert_eq!(store.get_active().len(), 1); // persistent remains
    }

    #[test]
    fn fd_id_counter_never_resets() {
        let mut store = NotificationStore::new();
        store.add("a", "1", "");
        store.add("a", "2", "");
        store.add("a", "3", "");
        // fd_id should be 1, 2, 3 even after dismissals
        store.dismiss_all();
        let n = store.add("a", "4", "");
        assert_eq!(n.fd_id, 4);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p slate-notifyd -- store::tests`
Expected: FAIL

- [ ] **Step 4: Implement NotificationStore**

In-memory `HashMap<Uuid, Notification>` with monotonic `fd_id` counter. Methods: `add()`, `get_active()`, `get_by_uuid()`, `get_by_fd_id()`, `dismiss()`, `dismiss_all()`, `dismiss_group()`, `update()`, `mark_read()`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p slate-notifyd -- store::tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add shell/slate-notifyd/ Cargo.toml
git commit -m "feat(slate-notifyd): add notification store with tests"
```

### Task 2b: Grouping logic

- [ ] **Step 1: Write tests for grouping**

Create `shell/slate-notifyd/src/grouping.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_app_name() {
        // Add 3 firefox, 2 signal notifications
        // group_notifications() returns groups sorted by newest
        // Each group contains notifications sorted by newest first
    }

    #[test]
    fn group_key_creates_subgroups() {
        // Two signal notifications with different group_keys
        // Should create two groups under signal
    }

    #[test]
    fn empty_store_returns_no_groups() {
        let groups = group_notifications(&[]);
        assert!(groups.is_empty());
    }
}
```

- [ ] **Step 2: Run tests, verify fail, implement, verify pass**

Implement `group_notifications(notifications: &[Notification]) -> Vec<NotificationGroup>` that groups by `(app_name, group_key)` and sorts by newest-first.

- [ ] **Step 3: Commit**

```bash
git add shell/slate-notifyd/src/grouping.rs
git commit -m "feat(slate-notifyd): add notification grouping logic"
```

### Task 2c: History persistence

- [ ] **Step 1: Write tests for history**

Create `shell/slate-notifyd/src/history.rs` with tests for writing/reading daily TOML files. Use `tempdir` for test isolation.

- [ ] **Step 2: Implement history module**

`HistoryWriter::append(notification)` — appends to `~/.local/state/slate-notifyd/history/YYYY-MM-DD.toml`. `HistoryReader::read(since, limit)` — reads history files, deserializes, filters, returns.

- [ ] **Step 3: Run tests, verify pass**

Run: `cargo test -p slate-notifyd -- history::tests`

- [ ] **Step 4: Commit**

```bash
git add shell/slate-notifyd/src/history.rs
git commit -m "feat(slate-notifyd): add history persistence"
```

### Task 2d: TOML active-state persistence

- [ ] **Step 1: Add persistence to store.rs**

Add `save_active(path)` and `load_active(path)` to `NotificationStore`. Serializes/deserializes the active notifications HashMap to TOML. Add debounce logic (track `dirty` flag, flush max once per second).

- [ ] **Step 2: Write tests for persistence round-trip**

```rust
#[test]
fn save_and_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("active.toml");
    let mut store = NotificationStore::new();
    store.add("firefox", "Test", "body");
    store.save_active(&path).unwrap();

    let loaded = NotificationStore::load_active(&path).unwrap();
    assert_eq!(loaded.get_active().len(), 1);
}
```

- [ ] **Step 3: Implement and verify**

- [ ] **Step 4: Commit**

```bash
git add shell/slate-notifyd/src/store.rs
git commit -m "feat(slate-notifyd): add active notification persistence"
```

### Task 2e: D-Bus interface

- [ ] **Step 1: Write the D-Bus interface structs**

Create `shell/slate-notifyd/src/dbus.rs`. Implement two zbus interfaces:

1. `org.freedesktop.Notifications` — standard spec methods (`Notify`, `CloseNotification`, `GetCapabilities`, `GetServerInformation`) and signals (`NotificationClosed`, `ActionInvoked`).

2. `org.slate.Notifications` — custom methods (`GetActive`, `GetHistory`, `GetGroupSummary`, `Dismiss`, `DismissAll`, `DismissGroup`, `InvokeAction`, `MarkRead`) and signals (`Added`, `Updated`, `Dismissed`, `GroupChanged`). Include DND as a boolean D-Bus property, backed by `NotificationSettings.dnd` in settings.toml.

Use `Arc<Mutex<NotificationStore>>` shared between both interfaces.

- [ ] **Step 2: Write unit tests for D-Bus message handling**

Test the conversion from freedesktop `Notify` parameters to internal `Notification` struct (urgency hint extraction, action parsing, heads-up decision).

- [ ] **Step 3: Implement and verify**

- [ ] **Step 4: Commit**

```bash
git add shell/slate-notifyd/src/dbus.rs
git commit -m "feat(slate-notifyd): implement freedesktop and slate notification D-Bus interfaces"
```

### Task 2f: Main entry point and DND

- [ ] **Step 1: Write main.rs**

Create `shell/slate-notifyd/src/main.rs`:
- Initialize tracing
- Load active notifications from disk
- Set up D-Bus session bus with both interfaces
- Run tokio event loop
- Flush active state on SIGTERM

Add DND as a boolean D-Bus property on `org.slate.Notifications`, persisted to settings.toml.

- [ ] **Step 2: Add mod declarations**

```rust
mod dbus;
mod grouping;
mod history;
mod store;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p slate-notifyd`
Expected: compiles (won't run without D-Bus session bus)

- [ ] **Step 4: Run all slate-notifyd tests**

Run: `cargo test -p slate-notifyd`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add shell/slate-notifyd/src/main.rs
git commit -m "feat(slate-notifyd): add main entry point with D-Bus setup"
```

### Task 2g: arkhe service definition

- [ ] **Step 1: Create service directory**

```bash
mkdir -p services/base/slate-notifyd/env
```

Create `services/base/slate-notifyd/run`:
```bash
#!/bin/sh
exec /usr/lib/slate/slate-notifyd
```

Create `services/base/slate-notifyd/depends`:
```
dbus
```

- [ ] **Step 2: Commit**

```bash
git add services/base/slate-notifyd/
git commit -m "feat(slate-notifyd): add arkhe service definition"
```

---

## Task 3: Rhea — AI Engine Daemon

**Depends on: Task 1 (slate-common). Can be parallelized with Tasks 2 and 4.**

**Spec deviations (intentional simplifications):**
- The spec shows separate `cloud/claude.rs`, `cloud/openai.rs`, `cloud/openclaw.rs` files. This plan uses a single `cloud.rs` with one OpenAI-compatible HTTP backend that works with all providers (Claude, OpenAI, Ollama all support the OpenAI chat completions format). The separate OpenClaw WebSocket backend is dropped — users who run OpenClaw can expose it via its OpenAI-compatible API endpoint instead.
- The spec shows `local/llama.rs` and `local/whisper.rs`. This plan implements only `local.rs` (llama.cpp). **Voice input via whisper.cpp is descoped to a future plan** — it requires audio pipeline integration (PipeWire), wake word detection, and streaming audio handling that is a separate project. The spec's whisper_model config field remains in settings for forward compatibility.

**Files:**
- Create: `shell/rhea/Cargo.toml`
- Create: `shell/rhea/src/main.rs`
- Create: `shell/rhea/src/dbus.rs`
- Create: `shell/rhea/src/router.rs`
- Create: `shell/rhea/src/config.rs`
- Create: `shell/rhea/src/local.rs`
- Create: `shell/rhea/src/cloud.rs`
- Modify: `Cargo.toml` (workspace members)

### Task 3a: Crate scaffolding and config

- [ ] **Step 1: Create crate directory and Cargo.toml**

```bash
mkdir -p shell/rhea/src
```

Create `shell/rhea/Cargo.toml`:
```toml
[package]
name = "rhea"
version = "0.1.0"
edition = "2021"

[dependencies]
slate-common = { path = "../slate-common" }
zbus = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
toml = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
reqwest = { workspace = true }
serde_json = "1"
```

Add `"shell/rhea"` to workspace members.

- [ ] **Step 2: Write config tests and implement**

Create `shell/rhea/src/config.rs`. Test that `RheaConfig` deserializes from the TOML schema defined in the spec (backend selection, local model path, cloud API key file paths, idle timeout, proactive flag).

- [ ] **Step 3: Commit**

```bash
git add shell/rhea/ Cargo.toml
git commit -m "feat(rhea): scaffold crate with config parsing"
```

### Task 3b: Router and backend abstraction

- [ ] **Step 1: Write tests for router**

Create `shell/rhea/src/router.rs`. The router holds the active backend and delegates `AiBackend` trait methods. Test that it routes based on config.

- [ ] **Step 2: Implement router**

`Router` struct holds a `Box<dyn AiBackend>`. `Router::from_config(config)` creates the appropriate backend. Implement a `StubBackend` for testing that returns canned responses.

- [ ] **Step 3: Verify and commit**

```bash
git add shell/rhea/src/router.rs
git commit -m "feat(rhea): add backend router with stub backend"
```

### Task 3c: Cloud backend (OpenAI-compatible HTTP)

- [ ] **Step 1: Write tests for cloud backend**

Create `shell/rhea/src/cloud.rs`. Test request serialization to OpenAI chat completions format. Test response deserialization. Use mock/canned JSON for tests (no real HTTP calls).

- [ ] **Step 2: Implement cloud backend**

`CloudBackend` implements `AiBackend`. Uses `reqwest` to POST to an OpenAI-compatible `/v1/chat/completions` endpoint. Works with Claude API (via their OpenAI-compatible endpoint), OpenAI, Ollama, and any compatible server.

API key read from file path specified in config (not inline).

- [ ] **Step 3: Verify and commit**

```bash
git add shell/rhea/src/cloud.rs
git commit -m "feat(rhea): add OpenAI-compatible cloud backend"
```

### Task 3d: Local backend (llama.cpp subprocess)

- [ ] **Step 1: Write tests for local backend**

Create `shell/rhea/src/local.rs`. Test model lifecycle state machine (Cold → Warm → Cold). Test request formatting for llama-server HTTP API. Use mock responses.

- [ ] **Step 2: Implement local backend**

`LocalBackend` implements `AiBackend`. Manages a `llama-server` subprocess:
- `ensure_warm()` — starts `llama-server` with the configured model if not running
- `query()` — HTTP POST to `localhost:<port>/v1/chat/completions` (llama-server is OpenAI-compatible)
- `unload()` — kills the subprocess, frees RAM
- Idle timer: `tokio::time::sleep(idle_timeout)` restarted on each request. When it fires, calls `unload()`.

The local backend reuses the same OpenAI-compatible HTTP format as the cloud backend — only the lifecycle management differs.

- [ ] **Step 3: Verify and commit**

```bash
git add shell/rhea/src/local.rs
git commit -m "feat(rhea): add local llama.cpp backend with model lifecycle"
```

### Task 3e: D-Bus interface

- [ ] **Step 1: Write the D-Bus interface**

Create `shell/rhea/src/dbus.rs`. Implement `org.slate.Rhea` with all methods from the spec:
- `Summarize`, `SuggestReplies`, `Complete`, `CompleteStream`, `DetectIntent`, `Classify`, `GetStatus`
- Signals: `CompletionChunk`, `CompletionDone`, `CompletionError`, `BackendChanged`, `ModelLoaded`, `ModelUnloaded`

Route all method calls through the `Router`.

- [ ] **Step 2: Write unit tests**

Test D-Bus method parameter parsing and error handling (what happens when backend is unavailable).

- [ ] **Step 3: Verify and commit**

```bash
git add shell/rhea/src/dbus.rs
git commit -m "feat(rhea): implement org.slate.Rhea D-Bus interface"
```

### Task 3f: Context aggregation

- [ ] **Step 1: Write context.rs**

Create `shell/rhea/src/context.rs`. Populates `AiContext` by:
- Querying niri IPC (`niri msg focused-window`) for focused window info
- Reading clipboard via `wl-paste` subprocess
- Fetching recent notification summaries from `org.slate.Notifications.GetActive()` D-Bus call

- [ ] **Step 2: Write tests for context parsing**

Test niri IPC output parsing and clipboard handling (mock subprocess output).

- [ ] **Step 3: Commit**

```bash
git add shell/rhea/src/context.rs
git commit -m "feat(rhea): add context aggregation for focused window and clipboard"
```

### Task 3g: Main entry point

- [ ] **Step 1: Write main.rs**

```rust
mod cloud;
mod config;
mod context;
mod dbus;
mod local;
mod router;
```

Initialize tracing, load config from settings.toml, create router, set up D-Bus, run event loop.

- [ ] **Step 2: Verify compilation**

Run: `cargo build -p rhea`

- [ ] **Step 3: Run all rhea tests**

Run: `cargo test -p rhea`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add shell/rhea/src/main.rs
git commit -m "feat(rhea): add main entry point"
```

### Task 3h: arkhe service definition

- [ ] **Step 1: Create service files**

```bash
mkdir -p services/base/rhea
echo '#!/bin/sh
exec /usr/lib/slate/rhea' > services/base/rhea/run
chmod +x services/base/rhea/run
echo 'dbus' > services/base/rhea/depends
```

Rhea reads config from the standard settings.toml path — no custom env var needed.

- [ ] **Step 2: Commit**

```bash
git add services/base/rhea/
git commit -m "feat(rhea): add arkhe service definition"
```

---

## Task 4: TouchFlow — Continuous Edge Gestures

**Depends on: Task 1b (shared physics). Can be parallelized with Tasks 2 and 3.**

**Files:**
- Modify: `shell/touchflow/src/edge.rs`
- Modify: `shell/touchflow/src/dbus_emitter.rs`
- Modify: `shell/touchflow/src/gesture.rs`

### Task 4a: Add EdgeGesture signal type

- [ ] **Step 1: Write tests for edge gesture phases**

Add tests to `shell/touchflow/src/edge.rs` for a new `EdgeGesture` struct with `edge`, `phase`, `progress`, `velocity` fields.

```rust
#[test]
fn edge_gesture_progress_clamped() {
    let g = EdgeGesture::new(Edge::Top, GesturePhase::Update, 0.5, 100.0);
    assert!((0.0..=1.0).contains(&g.progress));
}
```

- [ ] **Step 2: Implement EdgeGesture struct and GesturePhase enum**

```rust
pub enum GesturePhase { Start, Update, End, Cancel }

pub struct EdgeGesture {
    pub edge: Edge,
    pub phase: GesturePhase,
    pub progress: f64,
    pub velocity: f64,
}
```

- [ ] **Step 3: Add D-Bus signal for EdgeGesture**

In `shell/touchflow/src/dbus_emitter.rs`, add an `EdgeGesture` signal to the existing `org.slate.TouchFlow` interface. The signal carries `(edge: String, phase: String, progress: f64, velocity: f64)`.

- [ ] **Step 4: Modify gesture recognizer for continuous emission**

In `shell/touchflow/src/gesture.rs`, modify the edge swipe recognizer to emit `Start` when a top-edge touch is detected, `Update` on each move event (with progress = finger_travel / screen_height), and `End` on finger lift (with velocity from MomentumTracker). Emit `Cancel` if the gesture is aborted.

**This is the most significant refactor in this plan.** The existing `Recognizer::on_event()` returns `Option<GestureType>` — one event per call, only at finger-up. For continuous edge gesture emission:

1. Change the return type to `Vec<GestureType>` (or `SmallVec`) so multiple events can be emitted per input event.
2. Add a new `GestureType::EdgeGesture(EdgeGesture)` variant alongside the existing `GestureType::EdgeSwipe`.
3. When a touch starts in the top edge zone, immediately emit `EdgeGesture { phase: Start, ... }`. On each subsequent move event, emit `EdgeGesture { phase: Update, progress, velocity }`. On finger-up, emit `EdgeGesture { phase: End, ... }`.
4. Existing `EdgeSwipe` behavior for non-top edges is preserved — they still emit a single event at finger-up.
5. The D-Bus emitter receives the `Vec` and emits all events.

- [ ] **Step 5: Run all touchflow tests**

Run: `cargo test -p touchflow`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add shell/touchflow/src/
git commit -m "feat(touchflow): add continuous edge gesture emission"
```

---

## Task 5: slate-shade — Notification Shade UI

**Depends on: Tasks 1, 2, 3, 4 (all must be complete).**

**Files:**
- Create: `shell/slate-shade/Cargo.toml`
- Create: `shell/slate-shade/src/main.rs`
- Create: `shell/slate-shade/src/shade.rs`
- Create: `shell/slate-shade/src/heads_up.rs`
- Create: `shell/slate-shade/src/notifications.rs`
- Create: `shell/slate-shade/src/quick_settings.rs`
- Create: `shell/slate-shade/src/layout.rs`
- Create: `shell/slate-shade/src/dbus_listener.rs`
- Modify: `Cargo.toml` (workspace members)

### Task 5a: Crate scaffolding

- [ ] **Step 1: Create crate and Cargo.toml**

```toml
[package]
name = "slate-shade"
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
iced_anim = { workspace = true }
```

Add to workspace members.

- [ ] **Step 2: Create main.rs with iced app skeleton**

Follow the same pattern as `shell/claw-panel/src/main.rs` and `shell/shoal/src/main.rs`:
- `#[cfg(target_os = "linux")]` for layer-shell, fallback for macOS dev
- Layer: Overlay, Anchor: Top, exclusive_zone: 0
- Message enum, update/view/subscription functions

- [ ] **Step 3: Verify it compiles and runs (empty window)**

Run: `cargo build -p slate-shade`

- [ ] **Step 4: Commit**

```bash
git add shell/slate-shade/ Cargo.toml
git commit -m "feat(slate-shade): scaffold iced + layer-shell crate"
```

### Task 5b: D-Bus listener

- [ ] **Step 1: Write dbus_listener.rs**

Subscribe to:
- `org.slate.Notifications` signals (Added, Updated, Dismissed, GroupChanged)
- `org.slate.Rhea` signals (CompletionChunk, CompletionDone, CompletionError)
- `org.slate.TouchFlow` signals (EdgeGesture)
- `org.slate.Palette` property changes

Follow the same `iced::stream::channel` pattern used in `shell/claw-panel/src/dbus_listener.rs`.

- [ ] **Step 2: Define DbusEvent enum**

```rust
pub enum DbusEvent {
    NotificationAdded(Notification),
    NotificationUpdated(Notification),
    NotificationDismissed(Uuid),
    GroupChanged(String, u32),
    AiSummaryReady(String, String),   // (app_name, summary)
    SmartRepliesReady(Uuid, Vec<String>),
    EdgeGesture { phase: String, progress: f64, velocity: f64 },
    PaletteChanged(Palette),
}
```

- [ ] **Step 3: Commit**

```bash
git add shell/slate-shade/src/dbus_listener.rs
git commit -m "feat(slate-shade): add D-Bus listener for notifyd, rhea, touchflow"
```

### Task 5c: Layout selection

- [ ] **Step 1: Write layout.rs**

Query screen width via niri IPC (or fallback to a config value). If < 600dp, use phone layout. Otherwise tablet/desktop.

```rust
pub enum LayoutMode { Phone, TabletDesktop }

pub fn detect_layout() -> LayoutMode {
    // Query niri IPC for output width, or read from config
}
```

- [ ] **Step 2: Commit**

```bash
git add shell/slate-shade/src/layout.rs
git commit -m "feat(slate-shade): add phone vs tablet/desktop layout detection"
```

### Task 5d: Notification views

- [ ] **Step 1: Write notifications.rs**

Notification group view (app header + expandable list of items), individual notification card, AI summary subtext, smart reply chips. Follow the iced view pattern from `shell/claw-panel/src/panel.rs`.

- [ ] **Step 2: Write tests for view message mapping**

Test that tapping a notification emits the right message, that group expansion works, etc.

- [ ] **Step 3: Commit**

```bash
git add shell/slate-shade/src/notifications.rs
git commit -m "feat(slate-shade): add notification group and item views"
```

### Task 5e: Quick settings views

- [ ] **Step 1: Write quick_settings.rs**

Tile grid, individual tile view (icon + label, active/inactive state), brightness and volume sliders. Tile data model with auto-hide based on `slate-common::system` hardware queries.

- [ ] **Step 2: Commit**

```bash
git add shell/slate-shade/src/quick_settings.rs
git commit -m "feat(slate-shade): add quick settings tile grid and sliders"
```

### Task 5f: Shade panel with animation

- [ ] **Step 1: Write shade.rs**

Main shade view combining notifications and quick settings in the layout-appropriate arrangement. Pull-down animation using `slate-common::physics::Spring` driven by touchflow EdgeGesture progress.

- [ ] **Step 2: Write heads_up.rs**

Heads-up banner view. Auto-dismiss timer. Swipe-up to dismiss.

- [ ] **Step 3: Wire everything into main.rs**

Connect all views, D-Bus listener, and animation state into the main iced update/view loop.

- [ ] **Step 4: Run all slate-shade tests**

Run: `cargo test -p slate-shade`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add shell/slate-shade/src/
git commit -m "feat(slate-shade): complete shade panel with animation and heads-up banners"
```

### Task 5g: arkhe service definition

- [ ] **Step 1: Create service files**

```bash
mkdir -p services/base/slate-shade
echo '#!/bin/sh
exec /usr/lib/slate/slate-shade' > services/base/slate-shade/run
chmod +x services/base/slate-shade/run
printf 'dbus\nniri\nslate-notifyd\nrhea' > services/base/slate-shade/depends
```

WAYLAND_DISPLAY is inherited from the compositor session — no env file needed (same as shoal and other iced apps).

- [ ] **Step 2: Commit**

```bash
git add services/base/slate-shade/
git commit -m "feat(slate-shade): add arkhe service definition"
```

---

## Task 6: claw-panel Migration

**Depends on: Task 3 (Rhea must exist). Independent of Tasks 2, 4, 5.**

**Files:**
- Modify: `shell/claw-panel/src/main.rs`
- Modify: `shell/claw-panel/src/dbus_listener.rs`
- Delete: `shell/claw-panel/src/openclaw.rs`
- Modify: `shell/claw-panel/Cargo.toml`

### Task 6a: Replace OpenClaw with Rhea D-Bus

- [ ] **Step 1: Update dbus_listener.rs**

Add Rhea D-Bus subscription: listen for `CompletionChunk`, `CompletionDone`, `CompletionError` signals. Map them to the existing `Message::ResponseChunk`, `Message::ResponseDone`, `Message::OpenClawError` variants (or rename those variants to be backend-agnostic).

- [ ] **Step 2: Update main.rs**

Replace the `Message::Send` handler: instead of sending via `OpenClawClient`, make a D-Bus call to `org.slate.Rhea.CompleteStream()`. Remove `openclaw_client` field from `ClawPanel` state. Remove `Message::OpenClawClientReady`.

- [ ] **Step 3: Remove openclaw.rs**

Delete `shell/claw-panel/src/openclaw.rs` and remove the `mod openclaw;` declaration.

- [ ] **Step 4: Remove tokio-tungstenite dependency**

Remove `tokio-tungstenite` from `shell/claw-panel/Cargo.toml` if no other module uses it.

- [ ] **Step 5: Add backend indicator to panel header**

In `shell/claw-panel/src/panel.rs`, add a small label showing the active Rhea backend name (queried via `org.slate.Rhea.GetStatus()`).

- [ ] **Step 6: Update tests**

Update existing tests in main.rs that reference `OpenClawClient`, `OpenClawEvent`, etc. Replace with Rhea D-Bus equivalents.

- [ ] **Step 7: Run all claw-panel tests**

Run: `cargo test -p claw-panel`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git add shell/claw-panel/
git commit -m "feat(claw-panel): replace OpenClaw WebSocket with Rhea D-Bus"
```

---

## Task 7: CLAUDE.md and Final Verification

- [ ] **Step 1: Update CLAUDE.md architecture rules**

- Replace rule 6: "Rhea is the ONLY component that does AI inference. All components talk to Rhea via D-Bus."
- Add rhea, slate-notifyd, slate-shade to cross-crate dependency rules
- Add slate-common::ai, slate-common::system, slate-common::physics, slate-common::notifications to architecture

- [ ] **Step 2: Update docs/ARCHITECTURE.md**

Add Rhea, slate-notifyd, and slate-shade to the stack diagram and service architecture section. This file exists at `docs/ARCHITECTURE.md`.

- [ ] **Step 3: Run full workspace verification**

Run: `cargo run -p slate -- check` (runs check + clippy + test)
Expected: PASS with zero warnings

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md docs/ARCHITECTURE.md
git commit -m "docs: update architecture for rhea, slate-notifyd, slate-shade"
```
