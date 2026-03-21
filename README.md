# SlateOS

A Linux distribution for tablets, phones, and desktops. Built on Chimera Linux with a custom touch-first shell written in Rust.

## What Is This

SlateOS replaces stock Android or generic Linux desktops with a purpose-built environment: the Niri scrollable tiling Wayland compositor for window management, and a suite of Rust applications for gestures, app launching, theming, and AI assistance. It targets the Pixel Tablet as its primary device, with support for Pixel phones, the Pixel Fold, Framework laptops, and generic x86 desktops.

## Why

Most mobile Linux projects bolt a desktop UI onto a phone. SlateOS goes the other direction — it starts from touch and works outward. Multitouch gestures drive spatial navigation. Spring-animated physics make every interaction feel physical. Material You theming adapts the entire system to your wallpaper. An AI sidebar provides context-aware assistance. No telemetry, no ads, no dark patterns.

## Design Philosophy

SlateOS is built on **pronoia** — the belief that the system is conspiring in your favor. Every default is chosen to help, not to extract. The software respects your attention, your hardware, and your time. Settings are opinionated so things work immediately, but everything is exposed for users who want control. The system assumes you are competent and treats you accordingly.

## Stack

| Layer | Choice |
|-------|--------|
| Base distro | Chimera Linux (musl, LLVM/Clang, apk) |
| Init | arkhe (custom PID 1 + supervisor, io_uring) |
| Compositor | niri (Wayland, Smithay, scrollable tiling) |
| Shell | 9 Rust crates (iced 0.13, layer-shell, D-Bus) |
| Gestures | TouchFlow (evdev, sub-frame gesture recognition) |
| Theming | slate-palette (Material You from wallpaper, D-Bus broadcast) |
| AI | Claw Panel (OpenClaw WebSocket sidebar, code block apply) |
| Suggestions | slate-suggest (shell history + LLM completions) |

## Shell Components

| Component | Description |
|-----------|-------------|
| **shoal** | macOS-style dock with magnification, context menus, pin/unpin |
| **slate-launcher** | Fullscreen app grid with search, recent apps, launch feedback |
| **claw-panel** | AI sidebar with streaming responses, clipboard integration |
| **slate-suggest** | Keyboard suggestion bar with history and LLM completions |
| **slate-palette** | Dynamic theming daemon with built-in color extraction |
| **touchflow** | Multitouch gesture daemon (tap, swipe, pinch, edge gestures) |
| **slate-settings** | Settings app (WiFi, display, brightness, gestures, AI, FEX) |
| **slate-power** | Power button monitor (suspend on short press, poweroff on long) |
| **slate-common** | Shared library (palette, D-Bus, settings, theme, toasts, icons) |

## Developer Tool

The `slate` CLI provides a unified development workflow:

```bash
slate build                    # Build all shell crates
slate build -c shoal           # Build a single crate
slate check                    # Run check + clippy + test
slate dev                      # Live rebuild on file changes
slate status                   # Show build state for all crates
slate services                 # List arkhe service definitions
slate services --inspect niri  # Inspect a specific service
slate config                   # View/edit CLI configuration
slate info                     # System and project diagnostics
```

## Target Devices

| Device | SoC | Status |
|--------|-----|--------|
| Pixel Tablet | Tensor G2 | Primary target |
| Pixel Phone | Tensor | Secondary |
| Pixel Fold | Tensor G2 | Secondary |
| Framework 12 | x86 (touchscreen) | Dev machine |
| Generic x86 | any | Development target |

## Quick Start

```bash
# Clone and build
git clone https://github.com/nicholasraimbault/slateos
cd slateos
cargo build --workspace

# Run verification
cargo run -p slate -- check

# See project info
cargo run -p slate -- info
```

See [docs/BUILDING.md](docs/BUILDING.md) for rootfs builds and device flashing.

## Documentation

- [Architecture](docs/ARCHITECTURE.md) — system design and boot chain
- [Building](docs/BUILDING.md) — build instructions and rootfs creation
- [Devices](docs/DEVICES.md) — supported hardware
- [Pronoia](docs/PRONOIA.md) — design philosophy

## License

TBD
