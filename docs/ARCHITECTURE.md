# SlateOS Architecture

## Overview

SlateOS is a purpose-built Linux distribution: Chimera Linux base, arkhe init system, niri Wayland compositor, and a suite of Rust shell applications communicating over D-Bus.

## The Stack

```
Kernel (device-specific)
  │
  ├── pid1 (C, PID 1 stub)
  │     └── Spawns arkhd
  │
  ├── arkhd (Rust, io_uring supervisor)
  │     ├── Starts system services (D-Bus, seatd, networking)
  │     ├── Starts compositor (niri)
  │     ├── Starts shell components (shoal, launcher, palette, etc.)
  │     └── Supervises all processes (restart on crash)
  │
  ├── niri (upstream Wayland compositor, unchanged)
  │     ├── DRM/KMS output
  │     ├── Scrollable infinite workspace tiling
  │     ├── Wayland server for third-party apps
  │     └── IPC socket for shell communication
  │
  └── Shell components (Rust, iced + layer-shell)
        ├── shoal — dock (bottom bar, magnification, context menus, pin/unpin)
        ├── slate-launcher — app launcher (fullscreen grid, recent apps, search)
        ├── rhea — AI engine daemon (routes to local llama.cpp or cloud backends)
        ├── slate-notifyd — notification daemon (freedesktop + slate D-Bus, history, grouping)
        ├── slate-shade — notification shade + quick settings (pull-down, heads-up banners)
        ├── claw-panel — AI sidebar (streaming responses via Rhea D-Bus)
        ├── slate-suggest — keyboard suggestion bar (history + LLM suggestions)
        ├── slate-palette — theming daemon (wallpaper → Material You colors)
        ├── touchflow — gesture daemon (evdev → niri IPC + D-Bus, continuous edge gestures)
        ├── slate-settings — settings app (WiFi, display, gestures, AI, FEX)
        └── slate-power — power button monitor (suspend/poweroff)

Developer tools:
  └── slate CLI (tools/slate/)
        ├── slate build — compile shell crates (workspace or single crate)
        ├── slate check — run check + clippy + test in one command
        ├── slate status — show build state for all crates
        ├── slate services — list/inspect arkhe service definitions
        ├── slate config — read/write persistent CLI configuration
        └── slate flash — flash device (scaffold)
```

## Boot Chain

```
bootloader → kernel → initramfs → pid1 → arkhd → services → desktop
```

## Service Architecture

arkhe services are organized in two layers:

- `services/base/` — services that run on ALL devices (D-Bus, seatd, niri, networking, audio, shell components)
- `services/devices/<name>/` — device-specific overrides (hardware setup, kernel modules, device-specific power management)

Each service directory contains:
- `run` — executable script that starts the service
- `depends` — dependency list (one service name per line)
- `sandbox` — sandbox/isolation configuration
- `env/` — environment variables (one file per variable)

## Communication

- **D-Bus** — inter-component communication (palette changes, settings updates, gesture events, notifications, AI requests)
- **Niri IPC** — TouchFlow ↔ niri (workspace switching, window management), Rhea context (focused window)
- **HTTP** — Rhea ↔ AI backends (OpenAI-compatible API for cloud providers and local llama-server)

## AI Architecture

```
User input (Claw Panel, slate-shade)
  → D-Bus call to org.slate.Rhea
  → Rhea routes to configured backend
    ├── Cloud: reqwest POST to OpenAI-compatible endpoint (Claude, OpenAI, Ollama)
    └── Local: llama-server subprocess (HTTP, model lifecycle management)
  → Response streamed back via D-Bus signals (CompletionChunk, CompletionDone)
```

## Notification Architecture

```
App sends notification
  → org.freedesktop.Notifications.Notify (standard protocol)
  → slate-notifyd stores, groups, persists, emits D-Bus signals
  → slate-shade receives signals, displays:
    ├── Heads-up banner (urgent/normal, auto-dismiss)
    └── Pull-down shade (grouped list, quick settings, AI summaries)
```

## Theming Flow

```
Wallpaper changed
  → slate-palette extracts dominant colors
  → Generates Material You palette
  → Broadcasts palette over D-Bus
  → All iced apps receive update
  → All apps re-theme via slate-common::theme
  → niri config reloaded (border colors)
  → waybar CSS regenerated
```
