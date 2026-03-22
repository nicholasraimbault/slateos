# Rhea AI Engine + Notification System — Design Spec

## Overview

This spec covers the foundation layer of SlateOS's AI-integrated notification system:

1. **Rhea** — OS-level AI engine (daemon + shared trait in slate-common)
2. **slate-notifyd** — freedesktop-compliant notification daemon with persistent history
3. **slate-shade** — pull-down notification shade with quick settings and AI features
4. **Supporting changes** — touchflow gesture, slate-common modules, claw-panel migration

## Design Principles

- **Cleanest long-term architecture** — each component has one job, communicates via D-Bus
- **Pronoia** — the system conspires in the user's favor. No telemetry, no required accounts, works offline
- **Reactive AI by default** — local inference only runs when the user interacts. No background model loading, no battery drain
- **Pluggable backends** — local 3B model, paid APIs, self-hosted servers. User chooses
- **Hardware respect** — model loads on demand, unloads after idle. The user's RAM and thermals are sacred

---

## Part 1: Rhea — AI Engine

### What It Is

Rhea is the OS-level AI daemon. It provides AI capabilities to every shell component via a single D-Bus interface. No shell crate does its own inference — all AI goes through Rhea.

### Architecture

**`slate-common::ai`** — trait and shared types (no inference code):

```rust
/// The backend-agnostic AI interface. All shell components use this
/// via Rhea's D-Bus interface, never directly.
#[async_trait]
pub trait AiBackend: Send + Sync {
    /// Free-form text completion with streaming support.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;

    /// Classify text into one of the provided categories.
    async fn classify(&self, text: &str, categories: &[&str]) -> Result<Classification>;

    /// Summarize text to approximately max_words.
    async fn summarize(&self, text: &str, max_words: u32) -> Result<String>;

    /// Generate short reply suggestions for a conversation.
    async fn suggest_replies(&self, messages: &[ChatMessage]) -> Result<Vec<String>>;

    /// Detect user intent from natural language (for system control).
    async fn detect_intent(&self, text: &str) -> Result<Intent>;
}

pub struct CompletionRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub context: Option<AiContext>,
    pub max_tokens: u32,
    pub temperature: f32,
}

pub struct AiContext {
    pub focused_window: Option<WindowInfo>,
    pub clipboard: Option<String>,
    pub recent_notifications: Vec<NotificationSummary>,
}

pub enum Intent {
    SystemControl(SystemAction),  // "turn off wifi" → ToggleWifi(false)
    AppLaunch(String),            // "open firefox"
    Query(String),                // general question → route to chat
    Unknown,
}

pub struct Classification {
    pub category: String,
    pub confidence: f32,
}
```

**`shell/rhea/`** — the daemon (new crate):

```
rhea/
├── src/
│   ├── main.rs          — entry point, arkhe service setup
│   ├── dbus.rs          — org.slate.Rhea D-Bus interface
│   ├── router.rs        — routes requests to local or cloud backend
│   ├── config.rs        — backend configuration (from settings.toml)
│   ├── local/
│   │   ├── mod.rs
│   │   ├── llama.rs     — llama.cpp process manager (load/unload/query)
│   │   └── whisper.rs   — whisper.cpp process manager (voice input)
│   ├── cloud/
│   │   ├── mod.rs
│   │   ├── claude.rs    — Anthropic Claude API backend
│   │   ├── openai.rs    — OpenAI-compatible API backend (works with Ollama too)
│   │   └── openclaw.rs  — OpenClaw WebSocket backend (legacy compat)
│   └── context.rs       — context aggregation (focused window, clipboard, etc.)
```

### D-Bus Interface: org.slate.Rhea

**Methods:**

| Method | Signature | Purpose |
|--------|-----------|---------|
| `Summarize(text, max_words)` | `su → s` | Summarize text |
| `SuggestReplies(messages)` | `a(ss) → as` | Generate reply chips. Input: array of (role, content) pairs |
| `Complete(prompt, system, max_tokens, temperature)` | `ssud → s` | Free-form completion (blocking, returns full response) |
| `DetectIntent(text)` | `s → (sv)` | Parse natural language to system action |
| `Classify(text, categories)` | `sas → (sd)` | Classify text with confidence |
| `GetStatus()` | `→ (sb)` | Backend name + model loaded |

For streaming completions (used by claw-panel chat), Rhea emits `CompletionChunk` signals after a `CompleteStream` call:

| Method | Signature | Purpose |
|--------|-----------|---------|
| `CompleteStream(prompt, system, max_tokens, temperature)` | `ssud → u` | Start streaming completion, returns request_id |

| Signal | Data | Purpose |
|--------|------|---------|
| `CompletionChunk(request_id, text)` | Chunk of streamed response | Emitted per-chunk during streaming |
| `CompletionDone(request_id)` | — | Streaming complete |
| `CompletionError(request_id, error)` | Error message | Streaming failed |
| `BackendChanged(name)` | Backend name | User switched backends |
| `ModelLoaded()` | — | Local model warm |
| `ModelUnloaded()` | — | Local model freed |

**Error handling:** All methods return a D-Bus error (`org.slate.Rhea.Error`) on failure with a human-readable message. Callers should handle errors gracefully — if Rhea is unavailable, features degrade to non-AI behavior.

### Model Lifecycle (Local Backend)

```
Cold (default)          Warm                    Cold
  │                       │                       │
  ├── AI request ────────►│ loaded in ~2-3s       │
  │                       ├── serving requests    │
  │                       ├── 30s idle timer      │
  │                       ├── timer expires ─────►│ model unloaded
  │                       ├── new request resets   │
  │                       │   the 30s timer        │
```

- **Cold:** 0 RAM used by Rhea for inference. Default state.
- **Warm:** Model loaded (~2.5GB RAM for Gemma 2 2B Q4_K_M). Serves requests at 10-16 tok/s on Tensor G2.
- **Unload after 30s idle:** Configurable via settings.toml. Frees RAM for user apps.
- **Cold start penalty:** ~2-3 seconds to load model. Acceptable since it only happens when the user explicitly interacts.

### Backend Configuration

In `~/.config/slateos/settings.toml`:

```toml
[rhea]
# "local", "claude", "openai", "openclaw", "ollama"
backend = "local"

# Local backend settings
[rhea.local]
model = "gemma-2-2b-Q4_K_M"       # model file in ~/.local/share/rhea/models/ (~1.5GB)
idle_timeout_secs = 30
whisper_model = "base"              # tiny, base, small

# Cloud backend settings (only used when backend != "local")
[rhea.claude]
api_key_file = "~/.config/slateos/rhea-claude-key"  # file, not inline
model = "claude-sonnet-4-6"

[rhea.openai]
api_key_file = "~/.config/slateos/rhea-openai-key"
base_url = "https://api.openai.com/v1"              # or Ollama URL
model = "gpt-4o-mini"

[rhea.openclaw]
gateway_url = "ws://localhost:3000"

[rhea.ollama]
base_url = "http://homeserver:11434"
model = "llama3.2:3b"
```

API keys stored in separate files (not inline in config) so settings.toml can be backed up without leaking secrets.

### Proactive vs Reactive

**Default: everything is reactive.** AI only runs when the user interacts. This is the pronoia default — don't spend the user's resources (battery, thermals, RAM, API credits) without being asked.

Users who want proactive behavior can opt in per-backend in settings:

```toml
[rhea]
proactive = false   # default: reactive. Set true to enable proactive AI.
```

When proactive is enabled, Rhea subscribes to slate-notifyd signals and pre-generates summaries and smart replies in the background. When disabled (default), slate-shade requests them on demand when the user opens the shade.

This means a user with a powerful home server running Ollama might enable proactive mode — summaries ready before they open the shade, no local cost. A user on a paid API might keep it reactive to avoid surprise bills. A user on local-only keeps it reactive to save battery. **The user decides, we set safe defaults.**

### Three User Profiles

| Profile | Setup | Experience |
|---------|-------|------------|
| **Privacy-first** | Zero config, ships with local model | AI summaries when you open the shade, smart replies when you tap a notification. Nothing leaves the device. |
| **Power user (paid)** | Add API key in settings | Same + optional proactive summaries, longer conversations, complex tasks (code, reasoning) |
| **Self-hoster** | Point Rhea at home Ollama server | Same as power user but on your own infrastructure. No API costs. |

---

## Part 2: slate-notifyd — Notification Daemon

### What It Is

A standalone daemon that implements the freedesktop notification D-Bus spec. Receives notifications from all apps, stores them persistently, and relays them to UI consumers. Knows nothing about AI.

### D-Bus Interfaces

**Incoming (apps → slate-notifyd): `org.freedesktop.Notifications`**

Standard freedesktop spec implementation:

| Method | Purpose |
|--------|---------|
| `Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout)` | Receive notification |
| `CloseNotification(id)` | App requests dismissal |
| `GetCapabilities()` | Returns: actions, body, body-markup, body-hyperlinks, icon-static, persistence |
| `GetServerInformation()` | "slate-notifyd", "slateos", version, spec "1.2" |

Standard signals: `NotificationClosed(id, reason)`, `ActionInvoked(id, action_key)`

**Outgoing (slate-notifyd → shell components): `org.slate.Notifications`**

| Signal | Data | Purpose |
|--------|------|---------|
| `Added(uuid, notification)` | Full notification data | New notification received |
| `Updated(uuid, notification)` | Full notification data | Notification replaced/updated |
| `Dismissed(uuid, reason)` | Reason enum | Notification removed |
| `GroupChanged(app_name, count)` | Active count for app | Group membership changed |

| Method | Signature | Purpose |
|--------|-----------|---------|
| `GetActive()` | `→ a(notification)` | All active notifications |
| `GetHistory(since_timestamp, limit)` | `→ a(notification)` | Historical notifications |
| `GetGroupSummary(app_name)` | `→ a(notification)` | All active for one app |
| `Dismiss(uuid)` | `→ b` | User dismissed from UI |
| `DismissAll()` | `→ b` | Clear all dismissible |
| `DismissGroup(app_name)` | `→ b` | Clear all for one app |
| `InvokeAction(uuid, action_key)` | `→ b` | User clicked action button |
| `MarkRead(uuid)` | `→ b` | Mark as read |

### Notification Model

```rust
pub struct Notification {
    /// Stable UUID for cross-reboot reference and history.
    pub uuid: Uuid,
    /// Freedesktop spec u32 ID (ephemeral, for D-Bus compat).
    pub fd_id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<NotificationAction>,
    pub urgency: Urgency,
    pub category: Option<String>,      // freedesktop category hint
    pub timestamp: DateTime<Utc>,
    pub read: bool,
    pub persistent: bool,              // ongoing/foreground service
    pub desktop_entry: Option<String>, // .desktop file for launching app
    pub heads_up: bool,                // should display as heads-up banner
    pub group_key: Option<String>,     // custom hint for sub-grouping (e.g., conversation thread)
}

pub struct NotificationAction {
    pub key: String,
    pub label: String,
}

pub enum Urgency {
    Low,
    Normal,
    Critical,
}
```

### Storage

**Location:** `~/.local/state/slate-notifyd/`

```
~/.local/state/slate-notifyd/
├── active.toml           ← currently active notifications (loaded into memory at startup)
└── history/
    ├── 2026-03-22.toml   ← one file per day
    ├── 2026-03-21.toml
    └── ...               ← never auto-deleted
```

- **Active notifications:** In-memory HashMap, flushed to `active.toml` on every change (debounced, max once per second).
- **History:** When a notification is dismissed, it's appended to today's history file. One TOML file per day — human-readable, greppable, no database.
- **Never auto-deleted:** Pronoia. The user's data is theirs. Settings app surfaces storage usage with a manual clear button.
- **Survives reboots:** active.toml is loaded on startup.

### Grouping

Notifications are grouped by `app_name`. The grouping is done in slate-notifyd (not the UI) so all consumers see consistent groups:

- Groups sorted by most recent notification timestamp (newest group first)
- Within a group, notifications sorted by timestamp (newest first)
- If an app provides a `group_key` hint (custom `x-slate-group-key` in notification hints dict), sub-groups within the app are supported (e.g., separate conversation threads in a messaging app). Falls back to grouping by `app_name` only if no hint is provided.

### Heads-Up Decisions

slate-notifyd decides whether a notification deserves a heads-up banner based on urgency:

- `Critical` → always heads-up
- `Normal` → heads-up (unless DND active — DND state is a boolean property on `org.slate.Notifications`, toggled via the DND quick settings tile, persisted in settings.toml)
- `Low` → shade only, no heads-up

This decision is broadcast as part of the `Added` signal so slate-shade doesn't have to re-implement the logic.

---

## Part 3: slate-shade — Notification Shade UI

### What It Is

An iced + layer-shell application that renders the pull-down notification shade, heads-up notification banners, and quick settings. Knows nothing about AI inference — just displays data from slate-notifyd and Rhea.

### Crate Structure

```
shell/slate-shade/
├── src/
│   ├── main.rs              — entry point, layer-shell setup
│   ├── shade.rs             — main shade panel (pull-down)
│   ├── heads_up.rs          — heads-up banner surface
│   ├── notifications/
│   │   ├── mod.rs
│   │   ├── group.rs         — notification group view (app header + items)
│   │   ├── item.rs          — single notification card
│   │   ├── smart_reply.rs   — reply chip bar (from Rhea)
│   │   └── history.rs       — history view
│   ├── quick_settings/
│   │   ├── mod.rs
│   │   ├── tile.rs          — individual QS tile
│   │   ├── grid.rs          — tile grid layout
│   │   ├── brightness.rs    — brightness slider
│   │   ├── volume.rs        — volume slider
│   │   └── edit.rs          — tile edit/reorder mode
│   ├── layout.rs            — phone vs tablet/desktop layout selection
│   ├── animation.rs         — spring physics for pull-down
│   └── dbus_listener.rs     — subscribes to notifyd + Rhea
```

### Layer-Shell Surfaces

**Surface 1: Shade panel**
- Anchor: Top
- Layer: Overlay
- Exclusive zone: 0 (doesn't push other windows)
- Initially hidden (zero height)
- Expands downward following finger/pointer during pull gesture
- Phone: full screen width. Tablet/desktop: full screen width.

**Surface 2: Heads-up banner**
- Anchor: Top + Left + Right (centered, with horizontal margin)
- Layer: Overlay
- Exclusive zone: 0
- Appears/disappears independently of the shade
- Max width: 400dp (phone) or 500dp (tablet/desktop)
- Auto-dismiss after 5 seconds (configurable)

### Layout Modes

Selected at startup based on screen width from niri IPC:

**Phone (< 600dp):** Combined single-pane
```
┌─────────────────────┐
│ ☀️ Brightness slider │
│ [WiFi][BT][DND][🔦] │  ← collapsed QS (4 tiles, first pull)
├─────────────────────┤
│ ┌─────────────────┐ │
│ │ Signal (3)      │ │  ← notification group
│ │  AI: "Mom asked │ │  ← Rhea summary (requested on open)
│ │   about dinner" │ │
│ │  [sounds good]  │ │  ← smart reply chips
│ │  [give me 10]   │ │
│ └─────────────────┘ │
│ ┌─────────────────┐ │
│ │ Firefox (2)     │ │
│ └─────────────────┘ │
│                     │
│ [History] [Clear]   │
└─────────────────────┘

Second pull: full QS grid expands above notifications
```

**Tablet/Desktop (≥ 600dp):** Dual-pane
```
┌──────────────────┬──────────────────────┐
│ Quick Settings   │ Notifications        │
│                  │                      │
│ [WiFi]  [BT]    │ ┌──────────────────┐ │
│ [DND]   [Dark]  │ │ Signal (3)       │ │
│ [Night] [Cast]  │ │  AI: "Mom asked  │ │
│                  │ │   about dinner"  │ │
│ ☀️ Brightness    │ │  [sounds good]   │ │
│ 🔊 Volume        │ │  [give me 10]    │ │
│                  │ └──────────────────┘ │
│ [Rotation][Save] │ ┌──────────────────┐ │
│                  │ │ Firefox (2)      │ │
│ [✏️ Edit tiles]  │ └──────────────────┘ │
│                  │                      │
│                  │ [History] [Clear]    │
└──────────────────┴──────────────────────┘
```

### Interaction Model

**Opening the shade:**

| Input mode | Trigger | Behavior |
|------------|---------|----------|
| Touch | TopEdgeSwipe from touchflow | Shade follows finger position (direct manipulation with spring physics). Fast swipe snaps to full open. |
| Pointer | Click status area in bar | Shade animates open with spring physics. |
| Keyboard | Super+N (configurable) | Shade animates open with spring physics. |

**Closing the shade:**

| Action | Result |
|--------|--------|
| Swipe up (touch) | Shade follows finger, snaps closed |
| Click outside shade (pointer) | Spring-close animation |
| Press Escape or Super+N | Spring-close animation |

**Notification interactions:**

| Action | Result |
|--------|--------|
| Swipe left/right | Dismiss notification |
| Tap notification | Open source app, dismiss notification |
| Tap action button | Invoke action via slate-notifyd, dismiss |
| Tap smart reply chip | Invoke the notification's "inline-reply" action with the reply text (freedesktop `ActionInvoked` with the reply action key). If the app does not support inline reply, copy the reply to clipboard and open the app. |
| Expand group (tap header) | Show all notifications in group |
| Tap "History" | Switch to scrollable history view |
| Tap "Clear all" | Dismiss all non-persistent notifications |

**Quick settings interactions:**

| Action | Result |
|--------|--------|
| Tap tile | Toggle on/off |
| Long-press tile | Open relevant slate-settings page |
| Tap edit icon | Enter tile reorder mode |

### AI Integration (Reactive)

When the shade opens:
1. slate-shade calls `org.slate.Rhea.Summarize()` for each notification group with 3+ unread items
2. Rhea loads the model (if cold), generates summaries, returns them
3. slate-shade renders summaries as italic subtext on the group header
4. While waiting for Rhea, shade shows a subtle loading indicator (not a spinner — just slightly dimmed text area)

When the user taps a messaging notification:
1. slate-shade calls `org.slate.Rhea.SuggestReplies()` with the message content
2. Rhea generates 2-3 short replies
3. slate-shade renders them as tappable chips below the notification

If Rhea is unavailable or the model fails to load, everything works without AI — no summaries, no smart replies, just raw notifications. Graceful degradation.

---

## Part 4: Quick Settings System Wrappers

### Location: slate-common::system

Shared async D-Bus wrappers for system services. Used by both slate-shade (tiles) and slate-settings (pages).

```rust
// Each wrapper is a thin async struct — D-Bus calls only, no business logic.

pub struct WifiManager { connection: zbus::Connection }
impl WifiManager {
    pub async fn is_enabled(&self) -> Result<bool>;
    pub async fn set_enabled(&self, on: bool) -> Result<()>;
    pub async fn scan(&self) -> Result<Vec<AccessPoint>>;
    pub async fn connect(&self, ssid: &str, password: Option<&str>) -> Result<()>;
    pub async fn active_connection(&self) -> Result<Option<ConnectionInfo>>;
}

pub struct BluetoothManager { connection: zbus::Connection }
impl BluetoothManager {
    pub async fn is_enabled(&self) -> Result<bool>;
    pub async fn set_enabled(&self, on: bool) -> Result<()>;
    pub async fn paired_devices(&self) -> Result<Vec<BtDevice>>;
    pub async fn connect_device(&self, address: &str) -> Result<()>;
}

pub struct AudioManager { connection: zbus::Connection }
impl AudioManager {
    pub async fn get_volume(&self) -> Result<f32>;          // 0.0-1.0
    pub async fn set_volume(&self, level: f32) -> Result<()>;
    pub async fn is_muted(&self) -> Result<bool>;
    pub async fn set_muted(&self, mute: bool) -> Result<()>;
}

pub struct DisplayManager;
impl DisplayManager {
    pub async fn get_brightness(&self) -> Result<f32>;      // 0.0-1.0
    pub async fn set_brightness(&self, level: f32) -> Result<()>;
    pub async fn is_night_light(&self) -> Result<bool>;
    pub async fn set_night_light(&self, on: bool) -> Result<()>;
    pub async fn is_rotation_locked(&self) -> Result<bool>;
    pub async fn set_rotation_locked(&self, locked: bool) -> Result<()>;
}

pub struct PowerManager { connection: zbus::Connection }
impl PowerManager {
    pub async fn battery_level(&self) -> Result<Option<f32>>;  // None if no battery
    pub async fn is_charging(&self) -> Result<bool>;
    pub async fn power_profile(&self) -> Result<PowerProfile>;
    pub async fn set_power_profile(&self, profile: PowerProfile) -> Result<()>;
}

pub struct ConnectivityManager { connection: zbus::Connection }
impl ConnectivityManager {
    pub async fn has_modem(&self) -> Result<bool>;
    pub async fn is_mobile_data_enabled(&self) -> Result<bool>;
    pub async fn set_mobile_data_enabled(&self, on: bool) -> Result<()>;
    pub async fn is_airplane_mode(&self) -> Result<bool>;
    pub async fn set_airplane_mode(&self, on: bool) -> Result<()>;
    pub async fn is_hotspot_enabled(&self) -> Result<bool>;
    pub async fn set_hotspot_enabled(&self, on: bool) -> Result<()>;
}
```

**D-Bus services wrapped:**
- NetworkManager (`org.freedesktop.NetworkManager`)
- BlueZ (`org.bluez`)
- PipeWire/WirePlumber (PipeWire D-Bus or `wp` CLI)
- UPower (`org.freedesktop.UPower`)
- ModemManager (`org.freedesktop.ModemManager1`)
- Brightness via sysfs (`/sys/class/backlight/`) — DisplayManager reads/writes sysfs directly, no D-Bus
- Night light via gammastep D-Bus interface (`org.freedesktop.ColorManager` or direct gammastep control)

### Tile Auto-Hide

Quick settings tiles query the system wrappers at shade open to determine hardware presence:

```rust
// Tile is hidden when hardware is absent
ConnectivityManager::has_modem() == false  → hide Mobile Data, Hotspot, Airplane tiles
PowerManager::battery_level() == None      → hide Battery Saver tile
// Keyboard backlight: check /sys/class/leds/*kbd_backlight*
```

---

## Part 5: Shared Spring Physics

### Location: slate-common::physics

touchflow currently has its own `Spring` struct in `touchflow/src/physics.rs`. This spec moves spring physics to `slate-common::physics` so all components share the same implementation. touchflow's existing `Spring` struct is replaced with an import from slate-common. The API may differ slightly (touchflow uses `f64`, the shared version should use `f64` for consistency with touchflow's existing consumers).

The module provides:
- A `SpringConfig` struct with named presets (responsive, gentle, snappy)
- A `spring_step` function for frame-by-frame animation
- Preset values are starting points — will be tuned on real hardware

touchflow, shoal, slate-shade, and slate-launcher all use the same physics module. One place to tune, consistent feel everywhere.

### Migration from existing `[ai]` settings

The existing `AiSettings` under `[ai]` in settings.toml (with flat `enabled`, `model_path`, `endpoint`, `api_key` fields) is replaced by the `[rhea]` section. The old `[ai]` section is ignored if present — no migration needed since SlateOS hasn't shipped yet.

---

## Part 6: TouchFlow Integration

### New Gesture: TopEdgeSwipe

**Detection:**
- Touch begins in the configurable edge zone (default 50px, matching existing `EdgeConfig`)
- Touch moves downward by at least 10px (threshold to avoid false triggers)
- Once triggered, emits continuous progress updates as the finger moves

**Architecture note:** The existing touchflow recognizer emits a single `EdgeSwipe` event at finger-up time. This spec requires continuous emission during the gesture (start/update/end phases) so the shade can follow the finger. This is a significant refactor of the edge gesture recognizer — it must emit events during the gesture, not just at completion. The implementation plan should scope this accordingly.

**D-Bus signal on `org.slate.TouchFlow`:**

```
EdgeGesture {
    edge: String,       // "top", "bottom", "left", "right"
    phase: String,      // "start", "update", "end", "cancel"
    progress: f64,      // 0.0 → 1.0 (how far finger has traveled relative to screen height)
    velocity: f64,      // pixels per second at release (for snap decisions)
}
```

**touchflow has no knowledge of what the shade is.** It just reports edge gestures. The shade interprets them. Other components could use edge gestures in the future (e.g., bottom edge for app switcher).

---

## Part 7: claw-panel Migration

### Phase 1 (This Spec)

claw-panel switches its backend from direct OpenClaw WebSocket to Rhea's D-Bus interface:

- Remove: `openclaw.rs` (WebSocket client)
- Add: D-Bus calls to `org.slate.Rhea.Complete()` for chat
- Keep: all UI code (panel.rs, conversation.rs, clipboard.rs, context.rs)
- Keep: D-Bus listener for palette changes and show/hide
- Add: backend indicator in UI header showing which Rhea backend is active

The OpenClaw WebSocket backend still exists — in Rhea, as one of the cloud backend options. Users who run OpenClaw can configure it in settings.toml and claw-panel talks to it through Rhea.

### Phase 2 (Future Spec)

claw-panel evolves into Rhea's chat surface. Rename to `slate-rhea-panel` or integrate into a broader Rhea UI system with terminal integration, contextual tips, etc. Not in scope for this spec.

---

## Crate Map

### New Crates

| Crate | Type | Purpose |
|-------|------|---------|
| `shell/rhea/` | Daemon | AI engine — model management, routing, D-Bus |
| `shell/slate-notifyd/` | Daemon | Notification daemon — freedesktop spec, storage |
| `shell/slate-shade/` | Iced app | Notification shade + quick settings UI |

### Modified Crates

| Crate | Changes |
|-------|---------|
| `shell/slate-common/` | Add `ai.rs`, `system.rs`, `physics.rs` modules |
| `shell/touchflow/` | Add `TopEdgeSwipe` gesture detection |
| `shell/claw-panel/` | Replace OpenClaw WebSocket with Rhea D-Bus calls |

### New arkhe Services

```
services/base/rhea/
├── run           ← exec rhea binary
├── depends       ← dbus
└── env/
    └── RHEA_CONFIG  ← path to settings.toml

services/base/slate-notifyd/
├── run           ← exec slate-notifyd binary
├── depends       ← dbus
└── env/

services/base/slate-shade/
├── run           ← exec slate-shade binary
├── depends       ← dbus niri slate-notifyd rhea
└── env/
    └── WAYLAND_DISPLAY
```

---

## Dependency Graph

```
                  slate-common
                 /    |    |   \
                /     |    |    \
               /      |    |     \
          rhea    notifyd  shade  claw-panel
           |         |      |        |
           |         |      ├────────┘ (future: merge)
           |         |      |
     ┌─────┴──┐      |      ├── subscribes to notifyd (D-Bus)
     |        |      |      ├── subscribes to rhea (D-Bus)
  local    cloud     |      └── subscribes to touchflow (D-Bus)
  llama.cpp  APIs    |
  whisper           stores notifications
                    ~/.local/state/
```

---

## CLAUDE.md Updates Required

After implementation, CLAUDE.md architecture rules need updating:
- Rule 6: "Claw Panel is the ONLY component that talks to OpenClaw API" → "Rhea is the ONLY component that does AI inference. All components talk to Rhea via D-Bus."
- Add Rhea, slate-notifyd, and slate-shade to the cross-crate dependency rules
- Add `slate-common::ai`, `slate-common::system`, `slate-common::physics` to the architecture rules

---

## What Is NOT In This Spec

- Lock screen (separate spec)
- Terminal/dev mode integration for Rhea (Phase 2)
- Voice wake word ("Hey Rhea") (future — requires always-on whisper, conflicts with reactive-by-default)
- Rhea learning from user patterns (future — needs careful privacy design)
- Native status bar to replace waybar (future)
- Notification sounds/haptics (device-dependent, needs hardware)
