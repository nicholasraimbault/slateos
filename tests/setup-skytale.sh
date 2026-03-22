#!/bin/sh
# Set up Skytale server (root@162.216.112.162) for integration testing.
#
# Run from your Mac:
#   ssh root@162.216.112.162 'bash -s' < tests/setup-skytale.sh
#
# Or directly on Skytale:
#   bash tests/setup-skytale.sh

set -eu

echo "=== SlateOS Integration Test Setup ==="

# Install system dependencies
echo "Installing system packages..."
if command -v apt >/dev/null 2>&1; then
    apt update -qq
    apt install -y -qq dbus pipewire wireplumber pkg-config \
        clang lld libdbus-1-dev curl git
elif command -v apk >/dev/null 2>&1; then
    apk add dbus pipewire wireplumber pkgconf clang lld dbus-dev curl git
else
    echo "WARN: Unknown package manager — install manually: dbus, dbus-dev, pkg-config, clang, curl, git"
fi

# Install Rust if not present
if ! command -v cargo >/dev/null 2>&1; then
    echo "Installing Rust toolchain..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
else
    echo "Rust already installed: $(rustc --version)"
fi

# Clone or update the repo
REPO_DIR="/root/slateos"
if [ -d "$REPO_DIR" ]; then
    echo "Updating existing repo at $REPO_DIR..."
    cd "$REPO_DIR"
    git pull --ff-only || echo "WARN: git pull failed — may have local changes"
else
    echo "Cloning repo to $REPO_DIR..."
    echo "NOTE: You'll need to set up the remote manually."
    mkdir -p "$REPO_DIR"
    echo "Clone your repo to $REPO_DIR, then re-run this script."
    exit 1
fi

# Verify dbus-run-session works
echo ""
echo "Verifying dbus-run-session..."
if dbus-run-session -- echo "D-Bus session works"; then
    echo "dbus-run-session: OK"
else
    echo "ERROR: dbus-run-session failed. Check dbus-daemon installation."
    exit 1
fi

echo ""
echo "=== Setup complete ==="
echo ""
echo "To run integration tests:"
echo "  cd $REPO_DIR"
echo "  ./tests/run-integration.sh"
echo ""
echo "Or to build + test in one step:"
echo "  cd $REPO_DIR && cargo build --workspace && ./tests/run-integration.sh"
