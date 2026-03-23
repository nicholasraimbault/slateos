#!/bin/bash
# Build Slate OS rootfs
# Base: Chimera Linux (musl, LLVM/clang, apk)
# Usage: build-rootfs.sh [rootfs_dir] [output.tar.gz] [device]
# Devices: pixel-tablet (default), pixel-phone, generic-x86
set -euo pipefail

ROOTFS_DIR="${1:-/root/slate-rootfs}"
OUTPUT="${2:-/root/slate-rootfs.tar.gz}"
DEVICE="${3:-pixel-tablet}"
SLATE_SRC="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Slate OS Rootfs Builder ==="
echo "Device: $DEVICE"
echo "Output: $OUTPUT"
echo "Source: $SLATE_SRC"

# Determine architecture from device
case "$DEVICE" in
    pixel-tablet|pixel-phone)
        TARGET_ARCH="aarch64"
        ;;
    generic-x86)
        TARGET_ARCH="x86_64"
        ;;
    *)
        echo "ERROR: Unknown device '$DEVICE'"
        echo "  Supported: pixel-tablet, pixel-phone, generic-x86"
        exit 1
        ;;
esac

# Check for qemu-user-static on x86_64 hosts building for aarch64
HOST_ARCH=$(uname -m)
if [ "$HOST_ARCH" = "x86_64" ] && [ "$TARGET_ARCH" = "aarch64" ]; then
    if [ ! -f /proc/sys/fs/binfmt_misc/qemu-aarch64 ]; then
        echo "ERROR: qemu-user-static not configured for aarch64"
        echo "  apt install qemu-user-static binfmt-support"
        echo "  update-binfmts --enable qemu-aarch64"
        exit 1
    fi
    echo "Host: x86_64, using qemu-user-static for aarch64 chroot"
fi

# Step 1: Bootstrap Chimera Linux rootfs
echo "[1/7] Bootstrapping Chimera Linux rootfs..."
mkdir -p "$ROOTFS_DIR"
CHIMERA_URL="https://repo.chimera-linux.org/live/latest/chimera-linux-${TARGET_ARCH}-ROOTFS-latest.tar.gz"
CHIMERA_CACHE="/root/chimera-base-${TARGET_ARCH}.tar.gz"
if [ ! -f "$CHIMERA_CACHE" ]; then
    curl -L "$CHIMERA_URL" -o "$CHIMERA_CACHE"
fi
tar xpf "$CHIMERA_CACHE" -C "$ROOTFS_DIR"

# Step 2: Install packages
echo "[2/7] Installing system packages..."

mount --bind /proc "$ROOTFS_DIR/proc"
mount --bind /sys "$ROOTFS_DIR/sys"
mount --bind /dev "$ROOTFS_DIR/dev"

echo "nameserver 1.1.1.1" > "$ROOTFS_DIR/etc/resolv.conf"

if [ "$HOST_ARCH" = "x86_64" ] && [ "$TARGET_ARCH" = "aarch64" ] && [ -f /usr/bin/qemu-aarch64-static ]; then
    cp /usr/bin/qemu-aarch64-static "$ROOTFS_DIR/usr/bin/"
fi

cat > "$ROOTFS_DIR/tmp/install-packages.sh" << 'PKGEOF'
#!/bin/sh
set -eu
apk update

# Core system
apk add base-devel git rust cargo

# Wayland + compositor
apk add wayland wayland-devel wayland-protocols wlroots-devel mesa mesa-dri

# Desktop components
apk add pipewire wireplumber
apk add networkmanager bluez
apk add alacritty firefox neovim

# Input + keyboard
apk add libinput libinput-devel wtype || echo "wtype not in repos, will build from source"

# Status bar + launcher
apk add waybar fuzzel swaybg

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

# Authentication (PAM for lock screen)
apk add linux-pam shadow

# Cleanup
apk cache clean || true
PKGEOF
chmod +x "$ROOTFS_DIR/tmp/install-packages.sh"
chroot "$ROOTFS_DIR" /tmp/install-packages.sh

# Step 3: Build Niri compositor
echo "[3/7] Building Niri compositor..."
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

# Step 4: Build Slate OS Rust components
echo "[4/7] Building Slate OS components..."
cp -r "$SLATE_SRC" "$ROOTFS_DIR/tmp/slate-os"
cat > "$ROOTFS_DIR/tmp/build-slate.sh" << 'SLATEEOF'
#!/bin/sh
set -eu
cd /tmp/slate-os
cargo build --release --workspace

# Install shell binaries
for bin in touchflow shoal slate-launcher claw-panel slate-palette slate-suggest slate-settings slate-power-monitor slate-notifyd slate-shade slate-lock rhea; do
    if [ -f "target/release/$bin" ]; then
        install -Dm755 "target/release/$bin" "/usr/bin/$bin"
    fi
done

# Install slate CLI
if [ -f "target/release/slate" ]; then
    install -Dm755 "target/release/slate" "/usr/bin/slate"
fi

cd /tmp && rm -rf slate-os
SLATEEOF
chmod +x "$ROOTFS_DIR/tmp/build-slate.sh"
chroot "$ROOTFS_DIR" /tmp/build-slate.sh

# Step 5: Install configs and arkhe services
echo "[5/7] Installing configuration files..."

# Niri config — pick the right device-specific config
NIRI_CONFIG="$SLATE_SRC/config/niri/devices/${DEVICE}.kdl"
if [ ! -f "$NIRI_CONFIG" ]; then
    NIRI_CONFIG="$SLATE_SRC/config/niri/devices/generic-x86.kdl"
    echo "  No device-specific niri config for $DEVICE, using generic-x86"
fi
install -Dm644 "$NIRI_CONFIG" "$ROOTFS_DIR/etc/skel/.config/niri/config.kdl"
install -Dm644 "$SLATE_SRC/config/niri/palette.kdl" "$ROOTFS_DIR/etc/skel/.config/niri/palette.kdl"

# Waybar
install -Dm644 "$SLATE_SRC/config/waybar/config.jsonc" "$ROOTFS_DIR/etc/skel/.config/waybar/config.jsonc"
install -Dm644 "$SLATE_SRC/config/waybar/style.css" "$ROOTFS_DIR/etc/skel/.config/waybar/style.css"
install -Dm755 "$SLATE_SRC/config/waybar/scripts/netspeed.sh" "$ROOTFS_DIR/etc/skel/.config/waybar/scripts/netspeed.sh"
install -Dm755 "$SLATE_SRC/config/waybar/scripts/reload-waybar.sh" "$ROOTFS_DIR/etc/skel/.config/waybar/scripts/reload-waybar.sh"

# Arkhe service definitions — install base services + device overrides
if [ -d "$SLATE_SRC/services/base" ]; then
    mkdir -p "$ROOTFS_DIR/etc/sv"
    cp -r "$SLATE_SRC/services/base/"* "$ROOTFS_DIR/etc/sv/"

    # Layer device-specific overrides on top
    DEVICE_SERVICES="$SLATE_SRC/services/devices/$DEVICE"
    if [ -d "$DEVICE_SERVICES" ]; then
        cp -r "$DEVICE_SERVICES/"* "$ROOTFS_DIR/etc/sv/"
    fi

    find "$ROOTFS_DIR/etc/sv" -name run -exec chmod +x {} +
    find "$ROOTFS_DIR/etc/sv" -name ready-check -exec chmod +x {} +
fi

# Arkhe init binaries
ARKHE_BIN_DIR=""
if [ -d "$SLATE_SRC/rom/arkhe-bins" ]; then
    ARKHE_BIN_DIR="$SLATE_SRC/rom/arkhe-bins"
elif [ -d "$HOME/Projects/arkhe/target/${TARGET_ARCH}-unknown-linux-musl/release" ]; then
    ARKHE_BIN_DIR="$HOME/Projects/arkhe/target/${TARGET_ARCH}-unknown-linux-musl/release"
fi

if [ -n "$ARKHE_BIN_DIR" ]; then
    [ -f "$ARKHE_BIN_DIR/pid1" ]  && install -Dm755 "$ARKHE_BIN_DIR/pid1"  "$ROOTFS_DIR/usr/sbin/pid1"
    [ -f "$ARKHE_BIN_DIR/arkhd" ] && install -Dm755 "$ARKHE_BIN_DIR/arkhd" "$ROOTFS_DIR/usr/sbin/arkhd"
    [ -f "$ARKHE_BIN_DIR/ark" ]   && install -Dm755 "$ARKHE_BIN_DIR/ark"   "$ROOTFS_DIR/usr/bin/ark"
else
    echo "WARNING: arkhe binaries not found — rootfs will not boot without pid1 + arkhd"
    echo "  Build arkhe: cd ~/Projects/arkhe && cargo build --release --target ${TARGET_ARCH}-unknown-linux-musl"
fi

mkdir -p "$ROOTFS_DIR/run/arkhe"
mkdir -p "$ROOTFS_DIR/run/ready"

# Firewall
install -Dm644 "$SLATE_SRC/config/nftables/slate-firewall.conf" "$ROOTFS_DIR/etc/nftables.d/slate-firewall.conf"

# Default wallpaper
install -Dm644 "$SLATE_SRC/rom/wallpapers/default.jpg" "$ROOTFS_DIR/usr/share/backgrounds/slate-default.jpg" 2>/dev/null || \
    echo "Warning: default wallpaper not found"

# PAM config for lock screen authentication
cat > "$ROOTFS_DIR/etc/pam.d/slate-lock" << 'PAMEOF'
auth    required    pam_unix.so
account required    pam_unix.so
PAMEOF
chmod 644 "$ROOTFS_DIR/etc/pam.d/slate-lock"

# Default settings directory
install -dm755 "$ROOTFS_DIR/etc/skel/.config/slate"

# Step 6: Create user and configure system
echo "[6/7] Creating slate user..."
chroot "$ROOTFS_DIR" useradd -m -G wheel,input,video,audio -s /bin/sh slate
chroot "$ROOTFS_DIR" sh -c 'echo "slate:slate" | chpasswd'

cat > "$ROOTFS_DIR/etc/doas.conf" << 'DOASEOF'
permit nopass :wheel
DOASEOF
chmod 600 "$ROOTFS_DIR/etc/doas.conf"

# Step 7: Package
echo "[7/7] Packaging rootfs..."
umount "$ROOTFS_DIR/dev" 2>/dev/null || true
umount "$ROOTFS_DIR/sys" 2>/dev/null || true
umount "$ROOTFS_DIR/proc" 2>/dev/null || true

rm -f "$ROOTFS_DIR/usr/bin/qemu-aarch64-static"
rm -rf "$ROOTFS_DIR/tmp/"*

cd "$ROOTFS_DIR"
tar czf "$OUTPUT" .
echo "=== Done! Rootfs at: $OUTPUT ==="
echo "Size: $(du -h "$OUTPUT" | cut -f1)"
