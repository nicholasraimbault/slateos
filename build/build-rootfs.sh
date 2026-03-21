#!/bin/bash
# Build Slate OS rootfs for ONN 11 Tablet Pro 2024
# Base: Chimera Linux (musl, LLVM/clang, apk)
# Run this on an x86_64 host with qemu-user-static, or natively on aarch64
set -euo pipefail

ROOTFS_DIR="${1:-/root/slate-rootfs}"
OUTPUT="${2:-/root/slate-rootfs.tar.gz}"
SLATE_SRC="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Slate OS Rootfs Builder (Chimera Linux) ==="
echo "Output: $OUTPUT"
echo "Source: $SLATE_SRC"

# Check for qemu-user-static on x86_64 hosts
ARCH=$(uname -m)
if [ "$ARCH" = "x86_64" ]; then
    if [ ! -f /proc/sys/fs/binfmt_misc/qemu-aarch64 ]; then
        echo "ERROR: qemu-user-static not configured for aarch64"
        echo "  apt install qemu-user-static binfmt-support"
        echo "  update-binfmts --enable qemu-aarch64"
        exit 1
    fi
    echo "Host: x86_64, using qemu-user-static for aarch64 chroot"
fi

# Step 1: Bootstrap Chimera Linux rootfs
echo "[1/8] Bootstrapping Chimera Linux rootfs..."
mkdir -p "$ROOTFS_DIR"
CHIMERA_URL="https://repo.chimera-linux.org/live/latest/chimera-linux-aarch64-ROOTFS-latest.tar.gz"
if [ ! -f /root/chimera-base.tar.gz ]; then
    curl -L "$CHIMERA_URL" -o /root/chimera-base.tar.gz
fi
tar xpf /root/chimera-base.tar.gz -C "$ROOTFS_DIR"

# Step 2: Install packages
echo "[2/8] Installing system packages..."

# Set up chroot mounts
mount --bind /proc "$ROOTFS_DIR/proc"
mount --bind /sys "$ROOTFS_DIR/sys"
mount --bind /dev "$ROOTFS_DIR/dev"

# DNS resolution inside chroot
echo "nameserver 1.1.1.1" > "$ROOTFS_DIR/etc/resolv.conf"

# Copy qemu-user-static into chroot (needed for binfmt to work inside chroot)
if [ "$ARCH" = "x86_64" ] && [ -f /usr/bin/qemu-aarch64-static ]; then
    cp /usr/bin/qemu-aarch64-static "$ROOTFS_DIR/usr/bin/"
fi

cat > "$ROOTFS_DIR/tmp/install-packages.sh" << 'PKGEOF'
#!/bin/sh
set -eu
apk update

# Core system
apk add base-devel git rust cargo

# Wayland + compositor
apk add wayland wayland-devel wayland-protocols wlroots-devel

# Desktop components
apk add pipewire wireplumber
apk add networkmanager bluez
apk add alacritty firefox neovim

# Input + keyboard
apk add libinput libinput-devel wtype || echo "wtype not in repos, will build from source"

# Status bar + launcher
apk add waybar fuzzel swaybg

# AI stack
apk add nodejs

# Fonts
apk add fonts-dejavu fonts-noto fonts-noto-emoji

# Image/media
apk add imv mpv

# Seat management
apk add seatd

# D-Bus
apk add dbus dbus-devel

# Firewall
apk add nftables

# Privilege escalation
apk add doas

# Cleanup
apk cache clean || true
PKGEOF
chmod +x "$ROOTFS_DIR/tmp/install-packages.sh"
chroot "$ROOTFS_DIR" /tmp/install-packages.sh

# Step 3: Compile Mesa with KGSL backend
echo "[3/8] Building Mesa with KGSL backend for Adreno 610..."
cat > "$ROOTFS_DIR/tmp/build-mesa.sh" << 'MESAEOF'
#!/bin/sh
set -eu
cd /tmp
apk add meson samurai python-mako libdrm-devel libxml2-devel libxslt-devel \
    llvm-devel clang lm-sensors-devel libglvnd-devel vulkan-loader-devel \
    wayland-protocols

git clone --depth 1 https://gitlab.freedesktop.org/mesa/mesa.git
cd mesa
meson setup build -Dbuildtype=release \
    -Dplatforms=wayland \
    -Dgallium-drivers=freedreno,zink \
    -Dvulkan-drivers=freedreno \
    -Dfreedreno-kmds=kgsl \
    -Db_lto=true \
    -Dprefix=/usr
samu -C build
samu -C build install
cd /tmp && rm -rf mesa
MESAEOF
chmod +x "$ROOTFS_DIR/tmp/build-mesa.sh"
chroot "$ROOTFS_DIR" /tmp/build-mesa.sh

# Step 4: Build Niri compositor
echo "[4/8] Building Niri compositor..."
cat > "$ROOTFS_DIR/tmp/build-niri.sh" << 'NIRIEOF'
#!/bin/sh
set -eu
cd /tmp
git clone --depth 1 https://github.com/niri-wm/niri.git
cd niri
cargo build --release
install -Dm755 target/release/niri /usr/bin/niri
cd /tmp && rm -rf niri
NIRIEOF
chmod +x "$ROOTFS_DIR/tmp/build-niri.sh"
chroot "$ROOTFS_DIR" /tmp/build-niri.sh

# Step 5: Build Slate OS Rust components
echo "[5/8] Building Slate OS components..."
cp -r "$SLATE_SRC" "$ROOTFS_DIR/tmp/slate-os"
cat > "$ROOTFS_DIR/tmp/build-slate.sh" << 'SLATEEOF'
#!/bin/sh
set -eu
cd /tmp/slate-os
cargo build --release --workspace

# Install all binaries
for bin in touchflow shoal slate-launcher claw-panel slate-palette slate-suggest slate-settings; do
    install -Dm755 "target/release/$bin" "/usr/bin/$bin"
done

# Power button monitor (standalone binary, no tokio)
install -Dm755 "target/release/slate-power-monitor" "/usr/bin/slate-power-monitor"

# Cleanup build artifacts
cd /tmp && rm -rf slate-os
SLATEEOF
chmod +x "$ROOTFS_DIR/tmp/build-slate.sh"
chroot "$ROOTFS_DIR" /tmp/build-slate.sh

# Step 6: Install configs and arkhe services
echo "[6/8] Installing configuration files..."
# Niri config
install -Dm644 "$SLATE_SRC/config/niri/config.kdl" "$ROOTFS_DIR/etc/skel/.config/niri/config.kdl"
install -Dm644 "$SLATE_SRC/config/niri/palette.kdl" "$ROOTFS_DIR/etc/skel/.config/niri/palette.kdl"

# Waybar
install -Dm644 "$SLATE_SRC/config/waybar/config.jsonc" "$ROOTFS_DIR/etc/skel/.config/waybar/config.jsonc"
install -Dm644 "$SLATE_SRC/config/waybar/style.css" "$ROOTFS_DIR/etc/skel/.config/waybar/style.css"
install -Dm755 "$SLATE_SRC/config/waybar/scripts/netspeed.sh" "$ROOTFS_DIR/etc/skel/.config/waybar/scripts/netspeed.sh"
install -Dm755 "$SLATE_SRC/config/waybar/scripts/reload-waybar.sh" "$ROOTFS_DIR/etc/skel/.config/waybar/scripts/reload-waybar.sh"

# Arkhe service definitions
if [ -d "$SLATE_SRC/slate-services" ]; then
    mkdir -p "$ROOTFS_DIR/etc/sv"
    cp -r "$SLATE_SRC/slate-services/"* "$ROOTFS_DIR/etc/sv/"
    # Ensure run and ready-check scripts are executable
    find "$ROOTFS_DIR/etc/sv" -name run -exec chmod +x {} +
    find "$ROOTFS_DIR/etc/sv" -name ready-check -exec chmod +x {} +
fi

# Arkhe init binaries — check rom/arkhe-bins/ (pre-built) or ~/Projects/arkhe/
ARKHE_BIN_DIR=""
if [ -d "$SLATE_SRC/rom/arkhe-bins" ]; then
    ARKHE_BIN_DIR="$SLATE_SRC/rom/arkhe-bins"
elif [ -d "$HOME/Projects/arkhe/target/aarch64-unknown-linux-musl/release" ]; then
    ARKHE_BIN_DIR="$HOME/Projects/arkhe/target/aarch64-unknown-linux-musl/release"
fi

if [ -n "$ARKHE_BIN_DIR" ]; then
    [ -f "$ARKHE_BIN_DIR/pid1" ]  && install -Dm755 "$ARKHE_BIN_DIR/pid1"  "$ROOTFS_DIR/usr/sbin/pid1"
    [ -f "$ARKHE_BIN_DIR/arkhd" ] && install -Dm755 "$ARKHE_BIN_DIR/arkhd" "$ROOTFS_DIR/usr/sbin/arkhd"
    [ -f "$ARKHE_BIN_DIR/ark" ]   && install -Dm755 "$ARKHE_BIN_DIR/ark"   "$ROOTFS_DIR/usr/bin/ark"
    [ -f "$ARKHE_BIN_DIR/slate-power-monitor" ] && \
        install -Dm755 "$ARKHE_BIN_DIR/slate-power-monitor" "$ROOTFS_DIR/usr/bin/slate-power-monitor"
    [ -f "$ARKHE_BIN_DIR/qbootctl" ] && \
        install -Dm755 "$ARKHE_BIN_DIR/qbootctl" "$ROOTFS_DIR/usr/bin/qbootctl"
else
    echo "WARNING: arkhe binaries not found — rootfs will not boot without pid1 + arkhd"
    echo "  Build arkhe first: cd ~/Projects/arkhe && cargo build --release --target aarch64-unknown-linux-musl"
    echo "  Or place pre-built binaries in rom/arkhe-bins/"
fi

# Runtime directories that arkhe expects
mkdir -p "$ROOTFS_DIR/run/arkhe"
mkdir -p "$ROOTFS_DIR/run/ready"

# Firewall
install -Dm644 "$SLATE_SRC/config/nftables/slate-firewall.conf" "$ROOTFS_DIR/etc/nftables.d/slate-firewall.conf"

# Default wallpaper
install -Dm644 "$SLATE_SRC/rom/wallpapers/default.jpg" "$ROOTFS_DIR/usr/share/backgrounds/slate-default.jpg" 2>/dev/null || \
    echo "Warning: default wallpaper not found"

# Default settings
install -dm755 "$ROOTFS_DIR/etc/skel/.config/slate"

# Step 7: Create user and configure system
echo "[7/8] Creating slate user..."
chroot "$ROOTFS_DIR" useradd -m -G wheel,input,video,audio -s /bin/sh slate
chroot "$ROOTFS_DIR" sh -c 'echo "slate:slate" | chpasswd'

# doas instead of sudo
cat > "$ROOTFS_DIR/etc/doas.conf" << 'DOASEOF'
permit nopass :wheel
DOASEOF
chmod 600 "$ROOTFS_DIR/etc/doas.conf"

# Step 8: Package
echo "[8/8] Packaging rootfs..."
umount "$ROOTFS_DIR/dev" 2>/dev/null || true
umount "$ROOTFS_DIR/sys" 2>/dev/null || true
umount "$ROOTFS_DIR/proc" 2>/dev/null || true

# Remove qemu-user-static from rootfs (not needed at runtime on real hardware)
rm -f "$ROOTFS_DIR/usr/bin/qemu-aarch64-static"

# Clean up temp files
rm -rf "$ROOTFS_DIR/tmp/"*

# Create tarball
cd "$ROOTFS_DIR"
tar czf "$OUTPUT" .
echo "=== Done! Rootfs at: $OUTPUT ==="
echo "Size: $(du -h "$OUTPUT" | cut -f1)"
