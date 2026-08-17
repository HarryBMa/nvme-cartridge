#!/bin/bash

set -e

DEVICE="$1"

TRUST_DIR="$HOME/.config/pc-cartridge-system"
TRUST_FILE="$TRUST_DIR/trusted_scripts.sha256"
CONFIG_FILE="$TRUST_DIR/settings.conf"

mkdir -p "$TRUST_DIR"
LOG_FILE="$TRUST_DIR/helper_script.log"
# Log all stdout and stderr to file while keeping terminal output
exec > >(tee "$LOG_FILE") 2>&1
echo "==== Cartridge helper started: $(date) ===="

echo "Game cartridge detected: $DEVICE"


# Wait for desktop automounter
MOUNT_POINT=""

for i in {1..60}; do

    MOUNT_POINT=$(findmnt -n -o TARGET "/dev/$DEVICE" 2>/dev/null || true)

    if [ -n "$MOUNT_POINT" ]; then
        break
    fi

    sleep 0.5

done


if [ -z "$MOUNT_POINT" ]; then
    echo "No mount point found for /dev/$DEVICE"
    exit 0
fi


echo "Mounted at: $MOUNT_POINT"


SCRIPT="$MOUNT_POINT/launch.sh"


if [ ! -f "$SCRIPT" ]; then
    echo "No launch.sh found on cartridge"
    exit 0
fi

# Check auto-launch mode
if [ -f "$CONFIG_FILE" ]; then

    MODE=$(grep "^MODE=" "$CONFIG_FILE" | cut -d '=' -f2)

    if [ "$MODE" != "running" ]; then

        echo "Cartridge execution is disabled."
        echo "Current mode: $MODE"
        echo "Skipping launch."

        exit 0

    fi

else

    echo "No settings file found."
    echo "Defaulting to blocked mode."

    exit 0

fi


echo "Found launch.sh"


# Check trusted scripts database exists
if [ ! -f "$TRUST_FILE" ]; then
    echo "No trusted scripts database found."
    echo "Cartridge blocked."
    exit 0
fi


# Calculate hash
SCRIPT_HASH=$(sha256sum "$SCRIPT" | awk '{print $1}')

echo "Script SHA256:"
echo "$SCRIPT_HASH"


# Check trust database
if grep -qx "$SCRIPT_HASH" "$TRUST_FILE"; then

    echo "Script is trusted."
    echo "Launching cartridge..."

    chmod +x "$SCRIPT"
    bash "$SCRIPT"

# After the launch script exits, safely unmount and power off the drive
echo "Launch script finished. Ejecting cartridge /dev/$DEVICE ..."

# Unmount all partitions belonging to the parent device
PARENT_DEVICE=$(lsblk -no PKNAME "/dev/$DEVICE" 2>/dev/null || echo "$DEVICE")

for PART in $(lsblk -ln -o NAME "/dev/$PARENT_DEVICE" 2>/dev/null | tail -n +2); do
    PART_MOUNT=$(findmnt -n -o TARGET "/dev/$PART" 2>/dev/null || true)
    if [ -n "$PART_MOUNT" ]; then
        echo "Unmounting /dev/$PART ..."
        udisksctl unmount -b "/dev/$PART" --no-user-interaction 2>/dev/null || true
    fi
done

# Power off the parent drive so it can be safely removed
udisksctl power-off -b "/dev/$PARENT_DEVICE" --no-user-interaction 2>/dev/null || true

echo "Cartridge ejected safely."

else

echo "Script is NOT trusted."
echo "Cartridge blocked."

fi