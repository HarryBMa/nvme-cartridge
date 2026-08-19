#!/bin/bash
#
# Started by pc-gamepak@.service when udev sees a partition appear.
#
# Its whole job is to wait for the desktop to mount the cartridge and then open
# the launcher on it. Nothing is executed from the cartridge here: the launcher
# shows the cover art and waits for the user to press Play.
#
# That is why there is no trust list any more. The old design auto-executed
# launch.sh on insert, so it needed a SHA-256 allowlist to be safe. Now a human
# clicks Play, which is a better gate than a hash of a file the same person
# could rewrite.

set -euo pipefail

DEVICE="${1:-}"

if [ -z "$DEVICE" ]; then
    echo "usage: $0 <kernel device name, e.g. sdb1>" >&2
    exit 2
fi

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/pc-gamepak"
mkdir -p "$STATE_DIR"
LOG_FILE="$STATE_DIR/helper.log"

# Keep one short log rather than growing forever.
exec >>"$LOG_FILE" 2>&1
if [ "$(stat -c %s "$LOG_FILE" 2>/dev/null || echo 0)" -gt 262144 ]; then
    : >"$LOG_FILE"
fi

echo "==== $(date -Is) cartridge detected: $DEVICE ===="

# Wait for the desktop automounter. udev fires as soon as the kernel sees the
# partition, which is earlier than the mount we actually need.
MOUNT_POINT=""
for _ in $(seq 1 60); do
    MOUNT_POINT=$(findmnt -n -f -o TARGET "/dev/$DEVICE" 2>/dev/null || true)
    [ -n "$MOUNT_POINT" ] && break
    sleep 0.5
done

if [ -z "$MOUNT_POINT" ]; then
    echo "no mount point appeared for /dev/$DEVICE after 30s; giving up"
    exit 0
fi

echo "mounted at: $MOUNT_POINT"

# Only cartridges get a launcher. Without this every USB stick would pop a
# window.
if [ ! -f "$MOUNT_POINT/cartridge.conf" ] && [ ! -f "$MOUNT_POINT/autorun.inf" ]; then
    echo "no cartridge.conf or autorun.inf at the root; not a cartridge"
    exit 0
fi

LAUNCHER="${PC_CARTRIDGE_LAUNCHER:-/usr/local/bin/pc-gamepak}"

if [ ! -x "$LAUNCHER" ]; then
    echo "launcher not found at $LAUNCHER"
    echo "build it with: cd tauri-ui && npm run build, then install the binary there"
    exit 0
fi

echo "opening launcher for $MOUNT_POINT"

# The launcher needs the user's session to put a window on screen. The systemd
# unit already runs as the desktop user; point it at their display.
export DISPLAY="${DISPLAY:-:0}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -S "$XDG_RUNTIME_DIR/wayland-0" ]; then
    export WAYLAND_DISPLAY=wayland-0
fi

exec "$LAUNCHER" --drive "$MOUNT_POINT"
