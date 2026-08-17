#!/bin/bash
# PC Cartridge System - URI-Aware Launcher (Linux)
# Put this script on your SSD at root level and name it "launch.sh" (without quotes).
#
# Reads a cartridge.conf file from the same directory.
# cartridge.conf can contain either:
#   - A URI like steam://rungameid/1091500  (opened via xdg-open)
#   - A relative path like Game/bin/game.sh  (run directly from the cartridge)
#
# cartridge.conf format (plain text, one value per key):
#   executable=steam://rungameid/1091500
#   title=My Awesome Game

set -e

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG="$ROOT/cartridge.conf"
LOG_FILE="$ROOT/launch.log"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG_FILE"
}

log "Cartridge launch started."

if [ ! -f "$CONFIG" ]; then
    log "cartridge.conf not found. Exiting."
    exit 1
fi

# Parse cartridge.conf: read the 'executable' key
EXECUTABLE=$(grep -i '^executable=' "$CONFIG" | head -n1 | cut -d'=' -f2- | tr -d '\r' | xargs)

if [ -z "$EXECUTABLE" ]; then
    log "No 'executable' key found in cartridge.conf. Exiting."
    exit 1
fi

log "Executable: $EXECUTABLE"

# Detect whether the value is a URI or a local file path.
case "$EXECUTABLE" in
    steam://*|heroic://*|gog://*|epic://*|playnite://*|lutris://*|http://*|https://*)
        # Open through xdg-open — routes to the registered protocol/MIME handler.
        log "Detected URI. Opening via xdg-open: $EXECUTABLE"
        xdg-open "$EXECUTABLE"
        ;;
    *)
        # Treat as a relative path to an executable on the cartridge.
        FULL_PATH="$ROOT/$EXECUTABLE"
        if [ ! -f "$FULL_PATH" ]; then
            log "Executable not found on cartridge: $FULL_PATH"
            exit 1
        fi
        log "Launching local executable: $FULL_PATH"
        chmod +x "$FULL_PATH"
        cd "$(dirname "$FULL_PATH")"
        bash "$FULL_PATH"
        ;;
esac

log "Launch completed."
