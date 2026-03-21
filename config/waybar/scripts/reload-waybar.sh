#!/bin/sh
# Called by slate-palette after theme change to reload Waybar styles.
# SIGUSR2 tells Waybar to re-read its CSS without restarting.
# The -q flag suppresses "no process found" noise during early boot.

if command -v waybar >/dev/null 2>&1; then
    pkill -USR2 waybar 2>/dev/null || true
fi
