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
        ├── shoal — dock (bottom bar, magnification)
        ├── slate-launcher — app launcher (fullscreen grid)
        ├── claw-panel — AI sidebar (WebSocket to OpenClaw)
        ├── slate-suggest — keyboard suggestion bar
        ├── slate-palette — theming daemon (wallpaper → colors)
        ├── touchflow — gesture daemon (evdev → niri IPC)
        ├── slate-settings — settings app
        └── slate-power — power button monitor
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

- **D-Bus** — inter-component communication (palette changes, settings updates, gesture events)
- **Niri IPC** — TouchFlow ↔ niri (workspace switching, window management)
- **WebSocket** — Claw Panel ↔ OpenClaw (AI queries/responses)

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
