#!/bin/bash
# Build wvkbd with Slate OS custom layout
# Prerequisites: wvkbd source, wayland-protocols, pango, cairo
#
# Usage:
#   WVKBD_SRC=/path/to/wvkbd ./build.sh
#
# The script copies layout.slate.h and config.slate.h into the wvkbd source
# tree, symlinks keymap.slate.h to keymap.mobintl.h, then builds with LAYOUT=slate.
# Output binary: wvkbd-slate

set -euo pipefail

WVKBD_SRC="${WVKBD_SRC:-/opt/wvkbd}"
LAYOUT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Building wvkbd with Slate layout..."

if [ ! -d "$WVKBD_SRC" ]; then
	echo "Error: wvkbd source not found at $WVKBD_SRC"
	echo "Clone it first:"
	echo "  git clone https://github.com/jjsullivan5196/wvkbd.git $WVKBD_SRC"
	exit 1
fi

# Verify required build dependencies
for cmd in make pkg-config; do
	if ! command -v "$cmd" &>/dev/null; then
		echo "Error: $cmd not found. Install build dependencies first."
		exit 1
	fi
done

# Copy custom layout and config into wvkbd source tree
cp "$LAYOUT_DIR/layout.slate.h" "$WVKBD_SRC/"
cp "$LAYOUT_DIR/config.slate.h" "$WVKBD_SRC/"

# wvkbd expects a keymap.<layout>.h file; our layout only uses Latin,
# so we reuse the stock mobintl keymap which includes Latin as its first entry
if [ ! -f "$WVKBD_SRC/keymap.slate.h" ]; then
	if [ -f "$WVKBD_SRC/keymap.mobintl.h" ]; then
		echo "Symlinking keymap.slate.h -> keymap.mobintl.h (reusing Latin keymap)"
		ln -s keymap.mobintl.h "$WVKBD_SRC/keymap.slate.h"
	else
		echo "Warning: keymap.mobintl.h not found in $WVKBD_SRC"
		echo "  You may need to create keymap.slate.h manually."
	fi
fi

# Build with our layout
cd "$WVKBD_SRC"
make clean || true
make LAYOUT=slate

BINARY="$WVKBD_SRC/wvkbd-slate"
if [ ! -f "$BINARY" ]; then
	# Some wvkbd versions place output in build-<layout>/
	BINARY="$WVKBD_SRC/build-slate/wvkbd-slate"
fi

if [ -f "$BINARY" ]; then
	echo ""
	echo "Build successful: $BINARY"
	echo ""
	echo "Install with:"
	echo "  sudo install -Dm755 $BINARY /usr/bin/wvkbd-slate"
	echo ""
	echo "Run with:"
	echo "  wvkbd-slate"
else
	echo "Error: build completed but binary not found."
	exit 1
fi
