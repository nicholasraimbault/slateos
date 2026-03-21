#!/bin/sh
# Network speed monitor for Waybar (generic-x86 config)
# Reads the first active network interface and outputs download/upload speed.
# Falls back gracefully if no interface is available.

# Find the default route interface, or fall back to first non-lo interface
IFACE=$(ip route show default 2>/dev/null | awk '/default/{print $5; exit}')
if [ -z "$IFACE" ]; then
    # No default route — try the first interface that is UP and not loopback
    IFACE=$(ip -o link show up 2>/dev/null | awk -F': ' '!/lo/{print $2; exit}')
fi

if [ -z "$IFACE" ] || [ ! -d "/sys/class/net/${IFACE}" ]; then
    echo "-- --"
    exit 0
fi

RX_FILE="/sys/class/net/${IFACE}/statistics/rx_bytes"
TX_FILE="/sys/class/net/${IFACE}/statistics/tx_bytes"

if [ ! -r "$RX_FILE" ] || [ ! -r "$TX_FILE" ]; then
    echo "-- --"
    exit 0
fi

RX1=$(cat "$RX_FILE")
TX1=$(cat "$TX_FILE")
sleep 1
RX2=$(cat "$RX_FILE")
TX2=$(cat "$TX_FILE")

RX_SPEED=$(( (RX2 - RX1) / 1024 ))
TX_SPEED=$(( (TX2 - TX1) / 1024 ))

# Show in MB/s when speed exceeds 1024 KB/s
if [ "$RX_SPEED" -ge 1024 ]; then
    RX_FMT="$(( RX_SPEED / 1024 )).$(( (RX_SPEED % 1024) * 10 / 1024 ))M"
else
    RX_FMT="${RX_SPEED}K"
fi

if [ "$TX_SPEED" -ge 1024 ]; then
    TX_FMT="$(( TX_SPEED / 1024 )).$(( (TX_SPEED % 1024) * 10 / 1024 ))M"
else
    TX_FMT="${TX_SPEED}K"
fi

echo "↓${RX_FMT} ↑${TX_FMT}"
