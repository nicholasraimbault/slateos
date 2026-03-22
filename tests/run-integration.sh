#!/bin/sh
# Run SlateOS integration tests inside an isolated D-Bus session.
#
# Usage:
#   ./tests/run-integration.sh          # build + test
#   ./tests/run-integration.sh --no-build  # test only (assumes binaries exist)
#
# Requirements:
#   - Linux host with dbus-daemon installed
#   - Rust toolchain (cargo)
#   - D-Bus session bus (provided by dbus-run-session)

set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$WORKSPACE_ROOT"

# Check for dbus-run-session
if ! command -v dbus-run-session >/dev/null 2>&1; then
    echo "ERROR: dbus-run-session not found."
    echo "Install: apt install dbus (Debian/Ubuntu) or apk add dbus (Alpine/Chimera)"
    exit 1
fi

# Build unless --no-build is passed
if [ "${1:-}" != "--no-build" ]; then
    echo "Building daemons and integration test crate..."
    cargo build -p slate-notifyd -p rhea -p slate-integration-tests
    echo "Build complete."
fi

echo ""
echo "Running integration tests in isolated D-Bus session..."
echo "------------------------------------------------------"

# dbus-run-session starts a fresh dbus-daemon, sets DBUS_SESSION_BUS_ADDRESS,
# runs the command, then kills the daemon when the command exits.
dbus-run-session -- cargo test -p slate-integration-tests -- --test-threads=1 "$@"

echo "------------------------------------------------------"
echo "Integration tests complete."
