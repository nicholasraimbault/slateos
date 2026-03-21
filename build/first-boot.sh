#!/bin/sh
# Slate OS first-boot setup — run inside the Chimera rootfs
set -eu

echo "=== Slate OS First Boot Setup ==="

CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}"

# Create directory structure
mkdir -p "$CONFIG_DIR/slate/models"
mkdir -p "$CONFIG_DIR/slate/wallpapers"
mkdir -p "$CONFIG_DIR/niri"
mkdir -p "$CONFIG_DIR/waybar/scripts"

# Copy configs from skeleton
cp -n /etc/skel/.config/niri/* "$CONFIG_DIR/niri/" 2>/dev/null || true
cp -n /etc/skel/.config/waybar/* "$CONFIG_DIR/waybar/" 2>/dev/null || true
cp -rn /etc/skel/.config/waybar/scripts/* "$CONFIG_DIR/waybar/scripts/" 2>/dev/null || true

# Set default wallpaper
ln -sf /usr/share/backgrounds/slate-default.jpg "$CONFIG_DIR/slate/wallpaper"

# Generate initial palette from default wallpaper
if command -v slate-palette > /dev/null 2>&1; then
    echo "Generating initial color palette..."
    timeout 5 slate-palette --oneshot 2>/dev/null || echo "Palette generation will happen on first login"
fi

# Write minimal default settings.toml if it doesn't exist
if [ ! -f "$CONFIG_DIR/slate/settings.toml" ]; then
    cat > "$CONFIG_DIR/slate/settings.toml" << 'SETTINGS'
[display]
scale_factor = 1.5
rotation_lock = true

[wallpaper]
path = "/usr/share/backgrounds/slate-default.jpg"

[dock]
auto_hide = false
icon_size = 44

[gestures]
enabled = true
sensitivity = 1.0
edge_size = 50

[keyboard]
split_mode = false
suggestions = true

[ai]
enabled = true
SETTINGS
fi

# Create XDG runtime directory (native boot runs as root)
mkdir -p /run/user/0

echo ""
echo "=== First boot setup complete ==="
echo "Reboot to start Slate OS desktop"
