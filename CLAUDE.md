# SlateOS

## What This Is
SlateOS is a Linux distribution for tablets, phones, and desktops.
Based on Chimera Linux (musl, LLVM/Clang, apk) with a custom touch-first shell built in Rust.

## Target Devices
- **Pixel Tablet** (Tensor G2) — primary target
- **Pixel phones** (Tensor) — secondary target
- **Pixel Fold** (Tensor G2) — secondary target
- **Framework Laptop 12** (x86, touchscreen) — dev machine / tertiary target
- **Generic x86 desktop/laptop** — tertiary target

## Base System
- Distro: Chimera Linux (musl libc, LLVM/Clang toolchain)
- Package manager: apk (Alpine-compatible)
- Init: arkhe (custom, separate repo at ~/Projects/arkhe)
- Compositor: niri (Wayland, Smithay-based, scrollable tiling)
- TLS: rustls (no OpenSSL dependency)
- Privilege: doas (not sudo)

## Init System
arkhe is the init system. Service configs live in `services/`.
- `services/base/` — services for ALL devices
- `services/devices/<name>/` — device-specific service overrides

arkhe source lives at ~/Projects/arkhe (separate repo). Three static binaries:
- `/usr/sbin/pid1` — PID 1 stub (C)
- `/usr/lib/arkhe/arkhd` — supervisor (Rust, io_uring event loop)
- `/usr/bin/ark` — CLI tool

Boot chain: bootloader → kernel → initramfs → pid1 → arkhd → services

## Repository Structure
```
slateos/
├── shell/                    ← Rust crates (the slate shell)
│   ├── slate-common/         ← shared types: palette, D-Bus, settings, iced theme
│   ├── touchflow/            ← multitouch gesture daemon (evdev → Niri IPC + D-Bus)
│   ├── shoal/                ← Wayland dock (iced + layer-shell, magnification)
│   ├── slate-launcher/       ← fullscreen app launcher (iced + layer-shell)
│   ├── claw-panel/           ← OpenClaw AI sidebar (iced + layer-shell, WebSocket)
│   ├── slate-palette/        ← dynamic theming (wallpaper → Material You → D-Bus)
│   ├── slate-suggest/        ← keyboard suggestion bar (iced + layer-shell)
│   ├── slate-settings/       ← settings app (iced, settings.toml)
│   └── slate-power/          ← power button monitor (suspend/poweroff)
├── tools/
│   └── slate/                ← slate CLI (build, status, flash, config, services)
├── services/                 ← arkhe service configs
│   ├── base/                 ← services for all devices
│   └── devices/              ← device-specific overrides
├── config/                   ← system configs (niri, waybar, wvkbd, nftables, sysctl)
│   └── niri/devices/         ← per-device niri configs
├── build/                    ← build scripts
│   ├── build-rootfs.sh       ← Chimera rootfs builder (device-aware)
│   └── first-boot.sh         ← first boot setup
└── docs/                     ← documentation
```

## Tech Stack
- Rust 2021 edition, stable toolchain
- GUI: iced 0.13 + iced_layershell 0.7 + iced_anim 0.2 (spring physics)
- D-Bus: zbus 5
- Input: evdev crate (raw multitouch)
- Config: TOML (serde + toml crate)
- Async: tokio
- HTTP/WS: reqwest + tokio-tungstenite (Claw Panel ↔ OpenClaw)

## Coding Conventions
- No unsafe unless absolutely required + documented
- Error handling: thiserror for libraries, anyhow for binaries
- All errors typed. No .unwrap() in library code.
- Async for all I/O. spawn_blocking for CPU work.
- Logging: tracing crate (ERROR/WARN/INFO/DEBUG/TRACE)
- Testing: every public function has a unit test
- Max 500 lines per file (split into submodules)
- Comments explain WHY, not WHAT

## Architecture Rules
1. slate-common is the ONLY crate that defines D-Bus interfaces and palette types
2. All components read palette from slate-common types, not raw files
3. All components use zbus for D-Bus communication
4. All iced apps share theming through slate-common::theme module
5. TouchFlow is the ONLY component that reads /dev/input directly
6. Claw Panel is the ONLY component that talks to OpenClaw API

## Cross-Crate Dependency Rules
slate-common ← (all other crates depend on this)
touchflow    ← standalone daemon, depends on slate-common
slate-palette ← standalone daemon, depends on slate-common
shoal        ← iced app, depends on slate-common
slate-launcher ← iced app, depends on slate-common
claw-panel   ← iced app, depends on slate-common
slate-suggest ← iced app, depends on slate-common
slate-settings ← iced app, depends on slate-common

## Build
```bash
# Build all shell crates
cargo build --workspace

# Build Chimera rootfs (run on aarch64 or x86_64 with qemu-user-static)
bash build/build-rootfs.sh /path/to/rootfs /path/to/output.tar.gz pixel-tablet

# Use the slate CLI
cargo run -p slate -- build
cargo run -p slate -- status
cargo run -p slate -- services
```

arkhe binaries come from ~/Projects/arkhe cross-compile.

## Verification Commands
- cargo run -p slate -- check (runs check + clippy + test in one command)
- cargo check --workspace
- cargo clippy --workspace -- -D warnings
- cargo test --workspace
- cargo fmt --check
