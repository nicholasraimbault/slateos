# SlateOS

A touch-first Linux distribution for laptops, tablets, and phones. Built on Chimera Linux with a custom shell written in Rust.

## What Is This

SlateOS is a complete operating system — custom init, custom shell, scrollable tiling compositor — designed from the ground up for touchscreens. It runs on open hardware with mainline Linux kernels, no vendor blobs, no compromises.

## Why

Most mobile Linux projects bolt a desktop UI onto a phone. SlateOS starts from touch and works outward. Multitouch gestures drive spatial navigation. Spring-animated physics make interactions feel physical. Material You theming adapts the system to your wallpaper. An AI sidebar provides context-aware assistance. No telemetry, no ads, no dark patterns.

## Design Philosophy

SlateOS is built on **pronoia** — the belief that the system is conspiring in your favor. Every default helps, never extracts. Settings are opinionated so things work immediately, but everything is exposed for users who want control. The system assumes you are competent and treats you accordingly.

## Stack

| Layer | Choice |
|-------|--------|
| Base distro | Chimera Linux (musl, LLVM/Clang, apk) |
| Init | [arkhe](https://github.com/nicholasraimbault/arkhe) (custom, Rust, io_uring, Landlock sandboxing) |
| Compositor | niri (Wayland, Smithay, scrollable tiling) |
| Shell | 13 Rust crates (iced 0.13, layer-shell, D-Bus) |
| Gestures | TouchFlow (evdev, sub-frame gesture recognition) |
| Theming | slate-palette (Material You from wallpaper, D-Bus broadcast) |
| AI | Rhea engine + Claw Panel sidebar |
| TLS | rustls (no OpenSSL) |

## Shell Components

| Component | Description |
|-----------|-------------|
| **shoal** | Dock with magnification, context menus, pin/unpin |
| **slate-launcher** | Fullscreen app grid with search and launch feedback |
| **slate-lock** | Lock screen with PIN/PAM auth, adaptive layout, shake animation |
| **slate-shade** | Notification shade with quick settings |
| **slate-notifyd** | Notification daemon (freedesktop + slate D-Bus, history) |
| **claw-panel** | AI sidebar with streaming responses |
| **slate-suggest** | Keyboard suggestion bar with history and LLM completions |
| **slate-palette** | Dynamic theming daemon with Material You color extraction |
| **touchflow** | Multitouch gesture daemon (tap, swipe, pinch, edge gestures) |
| **slate-settings** | Settings app (display, WiFi, gestures, AI, security, keyboard) |
| **slate-power** | Power button monitor (suspend, poweroff, lock-before-suspend) |
| **rhea** | AI engine daemon (routes to local/cloud backends via D-Bus) |
| **slate-common** | Shared library (palette, D-Bus, settings, theme, toasts, icons) |

## Target Hardware

SlateOS only targets devices with full mainline Linux kernel support. No out-of-tree drivers, no vendor blobs.

| Device | SoC | GPU driver | Priority |
|--------|-----|-----------|----------|
| Framework Laptop 12 | Intel/AMD | i915/amdgpu | Primary — dev machine + first target |
| Generic x86 | any | mainline | Ships alongside Framework |
| Pinebook Pro | RK3399 | Panfrost | First ARM target |
| PineTab 2 | RK3566 | Panfrost | ARM tablet |
| PinePhone Pro | RK3399S | Panfrost | Phone |

Future ecosystem integration: PineTime (notifications), PineBuds Pro (audio).

## Quick Start

```bash
git clone https://github.com/nicholasraimbault/slateos
cd slateos
cargo build --workspace
cargo test --workspace     # 947 tests
cargo run -p slate -- check  # check + clippy + test
```

## Developer Tool

```bash
slate build                    # Build all shell crates
slate build -c shoal           # Build a single crate
slate check                    # Run check + clippy + test
slate dev                      # Live rebuild on file changes
slate status                   # Show build state for all crates
slate services                 # List arkhe service definitions
slate config                   # View/edit CLI configuration
slate info                     # System and project diagnostics
```

## Status

Early alpha. 13 crates compile, 947 tests pass, CI green.

## License

TBD
