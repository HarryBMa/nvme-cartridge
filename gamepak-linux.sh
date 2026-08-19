#!/bin/bash
#
# Installer menu. There is no trust list and no auto-launch toggle any more:
# inserting a cartridge opens the launcher, and nothing runs until Play is
# pressed, so there is nothing to allowlist or switch off.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

CYAN="\033[1;36m"
RESET="\033[0m"

clear 2>/dev/null || printf "\033c"

echo -e "${CYAN}"
cat <<'EOF'
   ██████╗ █████╗ ██████╗ ████████╗██████╗ ██╗██████╗  ██████╗ ███████╗███████╗
  ██╔════╝██╔══██╗██╔══██╗╚══██╔══╝██╔══██╗██║██╔══██╗██╔════╝ ██╔════╝██╔════╝
  ██║     ███████║██████╔╝   ██║   ██████╔╝██║██║  ██║██║  ███╗█████╗  ███████╗
  ██║     ██╔══██║██╔══██╗   ██║   ██╔══██╗██║██║  ██║██║   ██║██╔══╝  ╚════██║
  ╚██████╗██║  ██║██║  ██║   ██║   ██║  ██║██║██████╔╝╚██████╔╝███████╗███████║
   ╚═════╝╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚═╝╚═════╝  ╚═════╝ ╚══════╝╚══════╝
EOF
echo -e "${RESET}"

echo "        ╭────────────────────────────────╮"
echo "        │   1) Install                   │"
echo "        │   2) Create a cartridge        │"
echo "        │   3) Uninstall                 │"
echo "        │   4) Exit                      │"
echo "        ╰────────────────────────────────╯"
echo ""

read -rp "     Select option: " OPTION

case "$OPTION" in
    1)
        clear 2>/dev/null || printf "\033c"
        echo "Starting installation..."
        sudo bash "$SCRIPT_DIR/linux/install.sh"
        ;;
    2)
        clear 2>/dev/null || printf "\033c"
        LAUNCHER="/usr/local/bin/pc-gamepak"
        # Prefer a local build so the wizard can be used before installing.
        BUILT="$SCRIPT_DIR/tauri-ui/src-tauri/target/release/pc-gamepak"
        [ -x "$BUILT" ] && LAUNCHER="$BUILT"

        if [ ! -x "$LAUNCHER" ]; then
            echo "The launcher has not been built yet:"
            echo ""
            echo "  cd tauri-ui && npm install && npm run build"
            echo ""
            read -rp "Press enter to continue..." _
            exit 1
        fi
        echo "Opening the cartridge wizard..."
        "$LAUNCHER" --create
        ;;
    3)
        clear 2>/dev/null || printf "\033c"
        echo "Starting uninstall..."
        sudo bash "$SCRIPT_DIR/linux/uninstall.sh"
        ;;
    4)
        exit 0
        ;;
    *)
        echo "Invalid option."
        exit 1
        ;;
esac
