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
echo "        │   1) Install (recommended)     │"
echo "        │   2) Install without root      │"
echo "        │   3) Create a cartridge        │"
echo "        │   4) Uninstall                 │"
echo "        │   5) Exit                      │"
echo "        ╰────────────────────────────────╯"
echo ""
echo "     1 uses udev: nothing runs in the background."
echo "     2 runs a small watcher instead (~2 MB), and needs no password."
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
        echo "Installing for this user only..."
        bash "$SCRIPT_DIR/linux/install-user.sh"
        ;;
    3)
        clear 2>/dev/null || printf "\033c"
        LAUNCHER="/usr/local/bin/pc-gamepak"
        # Prefer a local build so the wizard can be used before installing,
        # then a user install, then the system one.
        for CANDIDATE in \
            "$SCRIPT_DIR/tauri-ui/src-tauri/target/release/pc-gamepak" \
            "$HOME/.local/bin/pc-gamepak"
        do
            [ -x "$CANDIDATE" ] && LAUNCHER="$CANDIDATE" && break
        done

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
    4)
        clear 2>/dev/null || printf "\033c"
        echo "Which install is this?"
        echo "  1) The system one (udev rule, installed with sudo)"
        echo "  2) The user one (watcher in your home directory)"
        read -rp "  Select: " WHICH
        case "$WHICH" in
            1) sudo bash "$SCRIPT_DIR/linux/uninstall.sh" ;;
            2) bash "$SCRIPT_DIR/linux/uninstall-user.sh" ;;
            *) echo "Invalid option." ; exit 1 ;;
        esac
        ;;
    5)
        exit 0
        ;;
    *)
        echo "Invalid option."
        exit 1
        ;;
esac
