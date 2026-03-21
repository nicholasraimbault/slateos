# Building SlateOS

## Prerequisites

- Rust stable toolchain
- For rootfs builds: x86_64 host with `qemu-user-static` (for aarch64 cross-build), or native aarch64
- For Pixel devices: `fastboot` (from Android SDK platform-tools)

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
# Build rootfs for Pixel Tablet (default)
doas bash build/build-rootfs.sh /path/to/rootfs /path/to/output.tar.gz pixel-tablet

# Build rootfs for generic x86
doas bash build/build-rootfs.sh /path/to/rootfs /path/to/output.tar.gz generic-x86
```

This script:
1. Bootstraps a Chimera Linux rootfs for the target architecture
2. Installs system packages (niri, waybar, pipewire, etc.)
3. Builds and installs SlateOS shell binaries
4. Installs arkhe init system
5. Installs device-specific niri config and services
6. Creates a user and packages as a tarball

## Device-Specific Build

### Pixel Tablet

```bash
# Build rootfs
doas bash build/build-rootfs.sh /root/slate-rootfs /root/slate-rootfs.tar.gz pixel-tablet

# Flash (requires device in fastboot mode)
# TBD — flash script will be added as device support matures
```

### Generic x86

```bash
# Build rootfs
doas bash build/build-rootfs.sh /root/slate-rootfs /root/slate-rootfs.tar.gz generic-x86

# Write to USB
# TBD — will produce a bootable ISO or USB image
```

## arkhe Binaries

arkhe (the init system) is built separately from ~/Projects/arkhe:

```bash
cd ~/Projects/arkhe
# Cross-compile for aarch64
cargo build --release --target aarch64-unknown-linux-musl
```

The resulting binaries (pid1, arkhd, ark) are copied into the rootfs by the build script.
