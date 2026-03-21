#!/bin/bash
# Called by slate-palette after theme change to reload Waybar styles
killall -SIGUSR2 waybar 2>/dev/null || true
