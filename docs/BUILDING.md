# Building SlateOS

## Prerequisites

- Rust stable toolchain
- For rootfs builds: x86_64 host with `qemu-user-static` (for aarch64 cross-build), or native aarch64
- For ONN tablet: `mkbootimg`, `avbtool` (from AOSP), `fastboot`

## Build Shell Crates

```bash
# Check all crates compile
cargo check --workspace

# Build all crates (debug)
cargo build --workspace

# Build release binaries
cargo build --workspace --release

# Run tests
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings
```

## Build Chimera Rootfs

The rootfs builder creates a complete Chimera Linux root filesystem with all SlateOS components installed.

```bash
# Build rootfs (requires root, or run in container)
sudo bash build/build-rootfs.sh /path/to/rootfs /path/to/output.tar.gz
```

This script:
1. Bootstraps a Chimera Linux aarch64 rootfs
2. Installs system packages (niri, waybar, pipewire, etc.)
3. Copies SlateOS shell binaries
4. Installs arkhe init system
5. Configures services and configs
6. Packages as a tarball

## Device-Specific Build

### ONN Tablet

```bash
# Flash boot images (requires device in fastboot mode)
bash build/devices/onn-tablet/flash-tablet.sh

# Build images only (no flash)
bash build/devices/onn-tablet/flash-tablet.sh --build-only
```

### Pixel Tablet

TBD — build/flash scripts will be added as device support matures.

### Generic x86

TBD — will produce a bootable ISO or USB image.

## arkhe Binaries

arkhe (the init system) is built separately from ~/Projects/arkhe:

```bash
cd ~/Projects/arkhe
# Cross-compile for aarch64
cargo build --release --target aarch64-unknown-linux-musl
```

The resulting binaries (pid1, arkhd, ark) are copied into the rootfs by the build script.
