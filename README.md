# SlateOS

A Linux distribution for tablets, phones, and desktops. Built on Chimera Linux with a custom touch-first shell written in Rust.

## What Is This

SlateOS replaces stock Android or generic Linux desktops with a purpose-built environment: the Niri scrollable tiling Wayland compositor for window management, and a suite of Rust applications for gestures, app launching, theming, and AI assistance. It targets the Pixel Tablet as its primary device, with support for Pixel phones, x86 desktops, and the ONN tablet as a legacy target.

## Why

Most mobile Linux projects bolt a desktop UI onto a phone. SlateOS goes the other direction — it starts from touch and works outward. Multitouch gestures drive spatial navigation. Spring-animated physics make every interaction feel physical. Material You theming adapts the entire system to your wallpaper. An AI sidebar provides context-aware assistance. No telemetry, no ads, no dark patterns.

## Design Philosophy

SlateOS is built on **pronoia** — the belief that the system is conspiring in your favor. Every default is chosen to help, not to extract. The software respects your attention, your hardware, and your time. Settings are opinionated so things work immediately, but everything is exposed for users who want control. The system assumes you are competent and treats you accordingly.

## Stack

| Layer | Choice |
|-------|--------|
| Base distro | Chimera Linux (musl, LLVM/Clang, apk) |
| Init | arkhe (custom PID 1 + supervisor) |
| Compositor | niri (Wayland, Smithay, scrollable tiling) |
| Shell | slate-shell crates (Rust, iced, layer-shell) |
| Gestures | TouchFlow (evdev, sub-frame response) |
| Theming | slate-palette (Material You, D-Bus broadcast) |
| AI | Claw Panel (OpenClaw WebSocket sidebar) |

## Building

See [docs/BUILDING.md](docs/BUILDING.md) for build instructions.

## License

TBD
