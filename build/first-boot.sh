#!/bin/bash
# Slate OS first-boot setup
#
# Runs once as the `slate` user on the first boot of a new device.
# Detects hardware, installs the correct configs, generates an initial
# theme palette, enables arkhe services, and writes default settings.
#
# Config source: /usr/share/slate/ (populated by build-rootfs.sh)
# Exit codes:
#   0  — success (non-critical warnings are logged but do not fail)
#   1  — critical failure (missing home dir, no config source, etc.)
set -uo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

SLATE_SHARE="/usr/share/slate"
NIRI_DEVICES_DIR="${SLATE_SHARE}/niri/devices"
WALLPAPER_SRC="/usr/share/backgrounds/slate-default.jpg"
ARKHE_SV_DIR="/etc/sv"
ARKHE_ENABLED_DIR="/run/arkhe/enabled"
FIRST_BOOT_MARKER="${HOME}/.config/slate/.first-boot-done"

CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}"
SLATE_CONFIG="${CONFIG_DIR}/slate"

# Counters for the summary
STEP_OK=0
STEP_WARN=0
STEP_FAIL=0

# ---------------------------------------------------------------------------
# Logging helpers
# ---------------------------------------------------------------------------

log_info()  { echo "[INFO]  $*"; }
log_ok()    { echo "[  OK]  $*"; STEP_OK=$((STEP_OK + 1)); }
log_warn()  { echo "[WARN]  $*"; STEP_WARN=$((STEP_WARN + 1)); }
log_fail()  { echo "[FAIL]  $*"; STEP_FAIL=$((STEP_FAIL + 1)); }

die() {
    echo ""
    echo "[FATAL] $*"
    echo "First-boot setup cannot continue. Please report this issue."
    exit 1
}

# ---------------------------------------------------------------------------
# Pre-flight checks
# ---------------------------------------------------------------------------

preflight() {
    log_info "Running pre-flight checks..."

    if [ -z "${HOME:-}" ]; then
        die "HOME is not set"
    fi

    if [ ! -d "${HOME}" ]; then
        die "Home directory ${HOME} does not exist"
    fi

    if [ ! -d "${SLATE_SHARE}" ]; then
        die "Config source ${SLATE_SHARE} does not exist -- is the rootfs built correctly?"
    fi

    # Skip if first-boot already completed
    if [ -f "${FIRST_BOOT_MARKER}" ]; then
        log_info "First-boot already completed (marker exists at ${FIRST_BOOT_MARKER})"
        log_info "To re-run, remove that file and run this script again."
        exit 0
    fi

    log_ok "Pre-flight checks passed"
}

# ---------------------------------------------------------------------------
# Step 1: Detect device
# ---------------------------------------------------------------------------

detect_device() {
    log_info "Detecting device..."

    local model=""
    local device="generic-x86"

    # ARM devices: read the devicetree model string
    if [ -f /sys/firmware/devicetree/base/model ]; then
        model=$(tr -d '\0' < /sys/firmware/devicetree/base/model 2>/dev/null || true)
    fi

    # x86 devices: read DMI product name
    if [ -z "${model}" ] && [ -f /sys/class/dmi/id/product_name ]; then
        model=$(cat /sys/class/dmi/id/product_name 2>/dev/null || true)
    fi

    if [ -n "${model}" ]; then
        log_info "Hardware model: ${model}"
    else
        log_warn "Could not read hardware model, assuming generic-x86"
    fi

    # Map model string to a SlateOS device name
    case "${model}" in
        *"Pixel Tablet"*|*"tangorpro"*)
            device="pixel-tablet"
            ;;
        *"Pixel 9"*|*"Pixel 8"*|*"Pixel 7"*|*"Pixel 6"*)
            device="pixel-phone"
            ;;
        *"Pixel Fold"*|*"felix"*)
            device="pixel-phone"
            ;;
        *"ONN"*|*"onn"*|*"SM8250"*|*"SM6225"*)
            device="onn-tablet"
            ;;
        *)
            # Check architecture as final fallback
            local arch
            arch=$(uname -m 2>/dev/null || echo "unknown")
            case "${arch}" in
                x86_64|i686)
                    device="generic-x86"
                    ;;
                aarch64|armv7l)
                    # Unknown ARM device -- default to pixel-tablet profile
                    # since it has the most complete touch/tablet settings
                    device="pixel-tablet"
                    log_warn "Unknown ARM device, defaulting to pixel-tablet profile"
                    ;;
                *)
                    device="generic-x86"
                    log_warn "Unknown architecture ${arch}, defaulting to generic-x86"
                    ;;
            esac
            ;;
    esac

    log_ok "Device detected: ${device}"
    echo "${device}"
}

# ---------------------------------------------------------------------------
# Step 2: Create directory structure
# ---------------------------------------------------------------------------

create_directories() {
    log_info "Creating directory structure..."

    local dirs=(
        "${CONFIG_DIR}/niri"
        "${CONFIG_DIR}/waybar/scripts"
        "${SLATE_CONFIG}"
        "${SLATE_CONFIG}/models"
        "${SLATE_CONFIG}/wallpapers"
    )

    for dir in "${dirs[@]}"; do
        if ! mkdir -p "${dir}" 2>/dev/null; then
            log_fail "Could not create ${dir}"
            return 1
        fi
    done

    log_ok "Directory structure created"
}

# ---------------------------------------------------------------------------
# Step 3: Install niri config for this device
# ---------------------------------------------------------------------------

install_niri_config() {
    local device="$1"
    log_info "Installing niri config for ${device}..."

    local niri_src="${NIRI_DEVICES_DIR}/${device}.kdl"
    local niri_dst="${CONFIG_DIR}/niri/config.kdl"

    # Fall back through: device-specific -> generic-x86 -> any available config
    if [ ! -f "${niri_src}" ]; then
        log_warn "No niri config for ${device}, trying generic-x86"
        niri_src="${NIRI_DEVICES_DIR}/generic-x86.kdl"
    fi

    if [ ! -f "${niri_src}" ]; then
        # Last resort: pick any .kdl file in the devices dir
        local fallback
        fallback=$(find "${NIRI_DEVICES_DIR}" -name '*.kdl' -type f 2>/dev/null | head -1)
        if [ -n "${fallback}" ]; then
            niri_src="${fallback}"
            log_warn "Using fallback niri config: ${niri_src}"
        else
            log_fail "No niri config found in ${NIRI_DEVICES_DIR}"
            return 1
        fi
    fi

    if cp "${niri_src}" "${niri_dst}"; then
        log_ok "Installed niri config from ${niri_src}"
    else
        log_fail "Failed to copy niri config to ${niri_dst}"
        return 1
    fi

    # Install the palette.kdl stub alongside the main config
    local palette_src="${SLATE_SHARE}/niri/palette.kdl"
    if [ -f "${palette_src}" ]; then
        if cp "${palette_src}" "${CONFIG_DIR}/niri/palette.kdl"; then
            log_ok "Installed niri palette.kdl"
        else
            log_warn "Failed to copy palette.kdl"
        fi
    fi
}

# ---------------------------------------------------------------------------
# Step 4: Install waybar config
# ---------------------------------------------------------------------------

install_waybar_config() {
    log_info "Installing waybar config..."

    local waybar_src="${SLATE_SHARE}/waybar"
    local waybar_dst="${CONFIG_DIR}/waybar"

    if [ ! -d "${waybar_src}" ]; then
        log_warn "Waybar config source not found at ${waybar_src}"
        return 0
    fi

    local ok=true

    # Main config files
    for f in config.jsonc style.css; do
        if [ -f "${waybar_src}/${f}" ]; then
            if ! cp "${waybar_src}/${f}" "${waybar_dst}/${f}"; then
                log_warn "Failed to copy waybar/${f}"
                ok=false
            fi
        fi
    done

    # Scripts
    if [ -d "${waybar_src}/scripts" ]; then
        for script in "${waybar_src}/scripts/"*; do
            [ -f "${script}" ] || continue
            local name
            name=$(basename "${script}")
            if cp "${script}" "${waybar_dst}/scripts/${name}"; then
                chmod +x "${waybar_dst}/scripts/${name}"
            else
                log_warn "Failed to copy waybar script: ${name}"
                ok=false
            fi
        done
    fi

    if [ "${ok}" = true ]; then
        log_ok "Installed waybar config"
    else
        log_warn "Waybar config installed with warnings"
    fi
}

# ---------------------------------------------------------------------------
# Step 5: Set up default wallpaper
# ---------------------------------------------------------------------------

setup_wallpaper() {
    log_info "Setting up default wallpaper..."

    local wallpaper_link="${SLATE_CONFIG}/wallpaper"

    if [ -f "${WALLPAPER_SRC}" ]; then
        if ln -sf "${WALLPAPER_SRC}" "${wallpaper_link}"; then
            log_ok "Default wallpaper symlinked: ${wallpaper_link} -> ${WALLPAPER_SRC}"
        else
            log_fail "Failed to create wallpaper symlink"
            return 1
        fi
    else
        # No default wallpaper shipped -- create a placeholder symlink anyway
        # so slate-palette has something to watch
        ln -sf "${WALLPAPER_SRC}" "${wallpaper_link}" 2>/dev/null || true
        log_warn "Default wallpaper not found at ${WALLPAPER_SRC} (will use fallback colors)"
    fi
}

# ---------------------------------------------------------------------------
# Step 6: Write default settings.toml
# ---------------------------------------------------------------------------

write_default_settings() {
    local device="$1"
    log_info "Writing default settings for ${device}..."

    local settings_path="${SLATE_CONFIG}/settings.toml"

    # Do not overwrite existing settings
    if [ -f "${settings_path}" ]; then
        log_info "Settings file already exists, skipping"
        return 0
    fi

    # Device-specific defaults
    local scale_factor="1.5"
    local rotation_lock="true"
    local auto_hide="false"
    local icon_size="44"
    local gestures_enabled="true"
    local gesture_sensitivity="1.0"
    local edge_size="50"
    local split_keyboard="false"
    local suggestions="true"

    case "${device}" in
        pixel-tablet)
            scale_factor="2.0"
            rotation_lock="true"
            auto_hide="false"
            icon_size="48"
            gestures_enabled="true"
            edge_size="50"
            split_keyboard="false"
            suggestions="true"
            ;;
        pixel-phone)
            scale_factor="2.5"
            rotation_lock="true"
            auto_hide="true"
            icon_size="40"
            gestures_enabled="true"
            edge_size="40"
            split_keyboard="false"
            suggestions="true"
            ;;
        onn-tablet)
            scale_factor="1.5"
            rotation_lock="true"
            auto_hide="false"
            icon_size="44"
            gestures_enabled="true"
            edge_size="50"
            split_keyboard="false"
            suggestions="true"
            ;;
        generic-x86)
            scale_factor="1.0"
            rotation_lock="false"
            auto_hide="false"
            icon_size="44"
            gestures_enabled="true"
            gesture_sensitivity="1.0"
            edge_size="50"
            split_keyboard="false"
            suggestions="false"
            ;;
    esac

    cat > "${settings_path}" << SETTINGS
[display]
scale_factor = ${scale_factor}
rotation_lock = ${rotation_lock}

[wallpaper]
path = "${WALLPAPER_SRC}"

[dock]
auto_hide = ${auto_hide}
icon_size = ${icon_size}
pinned_apps = ["Alacritty", "firefox", "org.gnome.Nautilus"]

[gestures]
enabled = ${gestures_enabled}
sensitivity = ${gesture_sensitivity}
edge_size = ${edge_size}

[keyboard]
split_mode = ${split_keyboard}
suggestions = ${suggestions}

[ai]
enabled = true
SETTINGS

    if [ -f "${settings_path}" ]; then
        log_ok "Default settings written for ${device}"
    else
        log_fail "Failed to write settings.toml"
        return 1
    fi
}

# ---------------------------------------------------------------------------
# Step 7: Generate initial theme palette
# ---------------------------------------------------------------------------

generate_palette() {
    log_info "Generating initial color palette from wallpaper..."

    if ! command -v slate-palette > /dev/null 2>&1; then
        log_warn "slate-palette not found in PATH, skipping palette generation"
        return 0
    fi

    # slate-palette --oneshot extracts colors from the current wallpaper,
    # writes palette files, and exits immediately (no daemon loop)
    if timeout 10 slate-palette --oneshot 2>/dev/null; then
        log_ok "Initial color palette generated"
    else
        log_warn "Palette generation failed or timed out (will regenerate on first login)"
    fi
}

# ---------------------------------------------------------------------------
# Step 8: Enable arkhe services
# ---------------------------------------------------------------------------

enable_services() {
    local device="$1"
    log_info "Enabling arkhe services..."

    if [ ! -d "${ARKHE_SV_DIR}" ]; then
        log_warn "Service directory ${ARKHE_SV_DIR} does not exist, skipping"
        return 0
    fi

    # Base services that every device needs.
    # Order matters for dependencies -- arkhe resolves via `depends` files,
    # but we list them logically for clarity.
    local base_services=(
        # System-level (no Wayland dependency)
        dbus-system
        seatd
        networkmanager
        network-online
        pipewire
        wireplumber

        # Session-level (need D-Bus session bus)
        dbus-session

        # Compositor
        niri

        # Shell components (need Wayland + D-Bus)
        swaybg
        waybar
        slate-palette
        shoal
        slate-launcher
        slate-suggest
        slate-settings
        touchflow
        wvkbd
        claw-panel
        openclaw
    )

    # Device-specific extra services
    local device_services=()
    case "${device}" in
        pixel-tablet|pixel-phone)
            # Pixel devices may have hardware-setup and power services
            device_services=(slate-hwsetup slate-modules slate-power)
            ;;
        onn-tablet)
            device_services=(slate-hwsetup slate-modules slate-power)
            ;;
        generic-x86)
            # Desktop/laptop: power service, no device-specific hardware setup
            device_services=(slate-power)
            ;;
    esac

    local enabled=0
    local skipped=0

    # Enable base services
    for svc in "${base_services[@]}"; do
        if [ -d "${ARKHE_SV_DIR}/${svc}" ]; then
            # Remove any `disabled` marker to ensure the service starts
            rm -f "${ARKHE_SV_DIR}/${svc}/disabled" 2>/dev/null
            enabled=$((enabled + 1))
        else
            skipped=$((skipped + 1))
        fi
    done

    # Enable device-specific services
    for svc in "${device_services[@]}"; do
        if [ -d "${ARKHE_SV_DIR}/${svc}" ]; then
            rm -f "${ARKHE_SV_DIR}/${svc}/disabled" 2>/dev/null
            enabled=$((enabled + 1))
        fi
    done

    # Disable services that should not run on certain devices
    case "${device}" in
        generic-x86)
            # Desktop does not need on-screen keyboard or touchflow by default
            for svc in wvkbd touchflow; do
                if [ -d "${ARKHE_SV_DIR}/${svc}" ]; then
                    touch "${ARKHE_SV_DIR}/${svc}/disabled" 2>/dev/null || true
                fi
            done
            ;;
    esac

    log_ok "Enabled ${enabled} services (${skipped} not found, non-critical)"
}

# ---------------------------------------------------------------------------
# Step 9: System-level setup (requires doas)
# ---------------------------------------------------------------------------

system_setup() {
    log_info "Applying system-level configuration..."

    # Create XDG runtime directory for the slate user
    local uid
    uid=$(id -u 2>/dev/null || echo "1000")
    local runtime_dir="/run/user/${uid}"

    if [ ! -d "${runtime_dir}" ]; then
        if doas mkdir -p "${runtime_dir}" 2>/dev/null && \
           doas chown "${USER:-slate}:${USER:-slate}" "${runtime_dir}" 2>/dev/null && \
           doas chmod 700 "${runtime_dir}" 2>/dev/null; then
            log_ok "Created XDG runtime directory at ${runtime_dir}"
        else
            log_warn "Could not create ${runtime_dir} (may already be handled by login)"
        fi
    else
        log_info "XDG runtime directory already exists"
    fi

    # Install sysctl hardening rules
    local sysctl_src="${SLATE_SHARE}/sysctl/99-slate-hardening.conf"
    local sysctl_dst="/etc/sysctl.d/99-slate-hardening.conf"
    if [ -f "${sysctl_src}" ] && [ ! -f "${sysctl_dst}" ]; then
        if doas cp "${sysctl_src}" "${sysctl_dst}" 2>/dev/null; then
            log_ok "Installed sysctl hardening rules"
        else
            log_warn "Could not install sysctl rules (requires doas)"
        fi
    fi

    # Install firewall rules
    local fw_src="${SLATE_SHARE}/nftables/slate-firewall.conf"
    local fw_dst="/etc/nftables.d/slate-firewall.conf"
    if [ -f "${fw_src}" ] && [ ! -f "${fw_dst}" ]; then
        if doas mkdir -p /etc/nftables.d 2>/dev/null && \
           doas cp "${fw_src}" "${fw_dst}" 2>/dev/null; then
            log_ok "Installed firewall rules"
        else
            log_warn "Could not install firewall rules (requires doas)"
        fi
    fi
}

# ---------------------------------------------------------------------------
# Step 10: Write first-boot marker
# ---------------------------------------------------------------------------

write_marker() {
    local device="$1"
    log_info "Writing first-boot completion marker..."

    mkdir -p "$(dirname "${FIRST_BOOT_MARKER}")" 2>/dev/null || true

    cat > "${FIRST_BOOT_MARKER}" << MARKER
# Slate OS first-boot completed
device=${device}
date=$(date -u '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null || echo "unknown")
version=1
MARKER

    if [ -f "${FIRST_BOOT_MARKER}" ]; then
        log_ok "First-boot marker written"
    else
        log_warn "Could not write first-boot marker"
    fi
}

# ---------------------------------------------------------------------------
# Welcome message
# ---------------------------------------------------------------------------

print_welcome() {
    local device="$1"
    echo ""
    echo "================================================================"
    echo ""
    echo "  Welcome to Slate OS"
    echo ""
    echo "  Device profile : ${device}"
    echo "  Config directory: ${CONFIG_DIR}"
    echo "  Settings file   : ${SLATE_CONFIG}/settings.toml"
    echo ""
    echo "  Setup results: ${STEP_OK} ok, ${STEP_WARN} warnings, ${STEP_FAIL} failures"
    echo ""
    if [ "${STEP_FAIL}" -gt 0 ]; then
        echo "  Some steps failed. Check the output above for details."
        echo "  You can re-run this script after fixing the issues:"
        echo "    rm ${FIRST_BOOT_MARKER}"
        echo "    bash /usr/share/slate/first-boot.sh"
    else
        echo "  First-boot setup complete. Reboot to start the desktop."
    fi
    echo ""
    echo "================================================================"
    echo ""
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
    echo ""
    echo "=== Slate OS First Boot Setup ==="
    echo ""

    preflight

    # Step 1: detect device (capture the device name from the last line)
    local device
    device=$(detect_device | tail -1)

    # Step 2: create directory structure
    create_directories

    # Step 3: install niri config
    install_niri_config "${device}"

    # Step 4: install waybar config
    install_waybar_config

    # Step 5: set up wallpaper
    setup_wallpaper

    # Step 6: write default settings
    write_default_settings "${device}"

    # Step 7: generate initial palette
    generate_palette

    # Step 8: enable arkhe services
    enable_services "${device}"

    # Step 9: system-level setup
    system_setup

    # Step 10: write marker
    write_marker "${device}"

    # Done
    print_welcome "${device}"
}

main "$@"
