#!/bin/bash
# Network speed monitor for Waybar
# Reads wlan0 interface stats and outputs download/upload speed in KB/s

RX1=$(cat /sys/class/net/wlan0/statistics/rx_bytes 2>/dev/null || echo 0)
TX1=$(cat /sys/class/net/wlan0/statistics/tx_bytes 2>/dev/null || echo 0)
sleep 1
RX2=$(cat /sys/class/net/wlan0/statistics/rx_bytes 2>/dev/null || echo 0)
TX2=$(cat /sys/class/net/wlan0/statistics/tx_bytes 2>/dev/null || echo 0)

RX=$(( (RX2 - RX1) / 1024 ))
TX=$(( (TX2 - TX1) / 1024 ))

echo "↓${RX}K ↑${TX}K"
