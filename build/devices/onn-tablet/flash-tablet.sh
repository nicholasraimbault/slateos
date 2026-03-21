#!/bin/bash
# Flash Slate OS native boot images to ONN 11 Tablet Pro 2024
#
# Boot image v4 layout (matches unpack of working images):
#   boot:        kernel only (no ramdisk), header_version=4
#   init_boot:   initramfs with busybox+forcemod+modules, header_version=4
#   vbmeta:      --flags 2 (verification disabled)
#
# Ramdisk is LZ4 compressed (matching stock vendor_boot format).
#
# Usage:
#   ./flash-tablet.sh              # build + flash slot A
#   ./flash-tablet.sh --build-only # build images only, no flash
#   ./flash-tablet.sh --slot b     # flash slot B instead
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SLATE_SRC="$(cd "$SCRIPT_DIR/.." && pwd)"
BUILD_DIR="$SCRIPT_DIR/build"
OUT_DIR="$SCRIPT_DIR/out"

# Partition sizes (from avbtool footer offsets in working images)
BOOT_PARTITION_SIZE=100663296    # 96 MB
INIT_BOOT_PARTITION_SIZE=8388608 # 8 MB

# Parse args
BUILD_ONLY=0
SLOT="a"
while [ $# -gt 0 ]; do
    case "$1" in
        --build-only) BUILD_ONLY=1; shift ;;
        --slot) SLOT="$2"; shift 2 ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
done

# -----------------------------------------------------------------------
# 1. Verify prerequisites
# -----------------------------------------------------------------------
echo "=== Slate OS Flash Tool (ONN 11 Tablet Pro) ==="
echo "Target: slot $SLOT"
echo ""

KERNEL_IMG="${KERNEL_IMG:-$SCRIPT_DIR/kernel/Image}"
INITRAMFS_DIR="${INITRAMFS_DIR:-$SCRIPT_DIR/initramfs-complete}"

# Find mkbootimg and avbtool (check ~/bin for AOSP scripts)
export PATH="$HOME/bin:$PATH"

missing=0
for tool in mkbootimg avbtool; do
    if ! command -v "$tool" > /dev/null 2>&1; then
        # Try as python3 script
        if [ -f "$HOME/bin/$tool" ]; then
            continue
        fi
        echo "ERROR: $tool not found in PATH or ~/bin"
        missing=1
    fi
done

if [ ! -f "$KERNEL_IMG" ]; then
    echo "ERROR: kernel Image not found at $KERNEL_IMG"
    missing=1
fi

if [ ! -f "$INITRAMFS_DIR/init" ]; then
    echo "ERROR: initramfs/init not found at $INITRAMFS_DIR"
    echo "  Run: unpack working init_boot and copy to rom/initramfs-complete/"
    missing=1
fi

if [ ! -f "$INITRAMFS_DIR/bin/busybox" ]; then
    echo "ERROR: busybox not found in $INITRAMFS_DIR/bin/"
    echo "  The initramfs must include the static aarch64 busybox"
    missing=1
fi

[ "$missing" -eq 1 ] && exit 1

mkdir -p "$BUILD_DIR" "$OUT_DIR"

# -----------------------------------------------------------------------
# 2. Build initramfs CPIO (LZ4 compressed, matching stock format)
# -----------------------------------------------------------------------
echo "[1/4] Building initramfs CPIO (LZ4)..."
RAMDISK="$BUILD_DIR/ramdisk.lz4"

(cd "$INITRAMFS_DIR" && find . | cpio -o -H newc 2>/dev/null | lz4 -l -9 > "$RAMDISK")
echo "  ramdisk: $(du -h "$RAMDISK" | cut -f1)"

# -----------------------------------------------------------------------
# 3. Create boot.img (kernel only, v4 header)
# -----------------------------------------------------------------------
echo "[2/4] Creating boot.img (kernel, v4 header)..."
BOOT_IMG="$BUILD_DIR/boot.img"

python3 "$HOME/bin/mkbootimg" \
    --header_version 4 \
    --kernel "$KERNEL_IMG" \
    --output "$BOOT_IMG"

python3 "$HOME/bin/avbtool" add_hash_footer \
    --image "$BOOT_IMG" \
    --partition_name boot \
    --partition_size "$BOOT_PARTITION_SIZE"

echo "  boot.img: $(du -h "$BOOT_IMG" | cut -f1)"

# -----------------------------------------------------------------------
# 4. Create init_boot.img (initramfs, v4 header)
# -----------------------------------------------------------------------
echo "[3/4] Creating init_boot.img (initramfs, v4 header)..."
INIT_BOOT_IMG="$BUILD_DIR/init_boot.img"

python3 "$HOME/bin/mkbootimg" \
    --header_version 4 \
    --ramdisk "$RAMDISK" \
    --output "$INIT_BOOT_IMG"

python3 "$HOME/bin/avbtool" add_hash_footer \
    --image "$INIT_BOOT_IMG" \
    --partition_name init_boot \
    --partition_size "$INIT_BOOT_PARTITION_SIZE"

echo "  init_boot.img: $(du -h "$INIT_BOOT_IMG" | cut -f1)"

# -----------------------------------------------------------------------
# 5. Create vbmeta.img (verification disabled)
# -----------------------------------------------------------------------
echo "[4/4] Creating vbmeta.img..."
VBMETA_IMG="$BUILD_DIR/vbmeta.img"

python3 "$HOME/bin/avbtool" make_vbmeta_image \
    --flags 2 \
    --padding_size 65536 \
    --include_descriptors_from_image "$BOOT_IMG" \
    --include_descriptors_from_image "$INIT_BOOT_IMG" \
    --output "$VBMETA_IMG"

echo "  vbmeta.img: $(du -h "$VBMETA_IMG" | cut -f1)"

# Copy to out/ for easy scp
cp "$BOOT_IMG" "$INIT_BOOT_IMG" "$VBMETA_IMG" "$OUT_DIR/"
echo ""
echo "Images ready in $OUT_DIR/"
ls -lh "$OUT_DIR/"

if [ "$BUILD_ONLY" -eq 1 ]; then
    echo ""
    echo "Build only — skipping flash. To flash:"
    echo "  scp $OUT_DIR/{boot,init_boot,vbmeta}.img mac:/tmp/"
    echo "  fastboot flash boot_$SLOT /tmp/boot.img"
    echo "  fastboot flash init_boot_$SLOT /tmp/init_boot.img"
    echo "  fastboot flash vbmeta_$SLOT /tmp/vbmeta.img"
    echo "  fastboot set_active $SLOT && fastboot reboot"
    exit 0
fi

# -----------------------------------------------------------------------
# 6. Flash
# -----------------------------------------------------------------------
if ! command -v fastboot > /dev/null 2>&1; then
    echo "ERROR: fastboot not found — run flash commands from the Mac"
    echo "  fastboot flash boot_$SLOT $OUT_DIR/boot.img"
    echo "  fastboot flash init_boot_$SLOT $OUT_DIR/init_boot.img"
    echo "  fastboot flash vbmeta_$SLOT $OUT_DIR/vbmeta.img"
    echo "  fastboot set_active $SLOT && fastboot reboot"
    exit 1
fi

echo ""
echo "About to flash slot $SLOT:"
echo "  boot_$SLOT      <- $BOOT_IMG"
echo "  init_boot_$SLOT <- $INIT_BOOT_IMG"
echo "  vbmeta_$SLOT    <- $VBMETA_IMG"
echo ""
read -rp "Device must be in fastboot mode. Continue? [y/N] " confirm
case "$confirm" in
    [yY]*) ;;
    *) echo "Aborted."; exit 0 ;;
esac

fastboot flash "boot_$SLOT" "$BOOT_IMG"
fastboot flash "init_boot_$SLOT" "$INIT_BOOT_IMG"
fastboot flash "vbmeta_$SLOT" "$VBMETA_IMG"
fastboot set_active "$SLOT"

echo ""
echo "=== Flash complete (slot $SLOT) ==="
echo "Reboot: fastboot reboot"
echo "Recovery: fastboot set_active a && fastboot reboot"
