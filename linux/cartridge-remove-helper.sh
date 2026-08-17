#!/bin/bash
# PC Cartridge System - Removal Helper
# Called by game-cartridge-remove@.service when a partition is unplugged.
# Terminates any running launch.sh process that was started from the cartridge.

set -e

DEVICE="$1"

if [ -z "$DEVICE" ]; then
    echo "Usage: pc-cartridge-system-remove <device>"
    exit 1
fi

TRUST_DIR="$HOME/.config/pc-cartridge-system"
LOG_FILE="$TRUST_DIR/helper_script.log"

mkdir -p "$TRUST_DIR"
exec >> "$LOG_FILE" 2>&1

echo "==== Cartridge removed: $DEVICE ($(date)) ===="

# Find and terminate any launch.sh processes started for this device.
# The launch helper sets the process title / args to include the mount point.
# We look for bash processes whose argv contains "launch.sh" and whose working
# directory or open files are under the former mount point.
PIDS=$(pgrep -f "launch.sh" 2>/dev/null || true)

if [ -n "$PIDS" ]; then
    echo "Sending SIGTERM to launch.sh processes: $PIDS"
    echo "$PIDS" | xargs -r kill -SIGTERM 2>/dev/null || true
    sleep 2
    # Force-kill any survivors
    REMAINING=$(pgrep -f "launch.sh" 2>/dev/null || true)
    if [ -n "$REMAINING" ]; then
        echo "Force-killing remaining processes: $REMAINING"
        echo "$REMAINING" | xargs -r kill -SIGKILL 2>/dev/null || true
    fi
fi

echo "Removal handler finished for $DEVICE"
